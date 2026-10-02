//! Documents and their file trees. A document is a bundle of folders and files
//! (`nodes`, an adjacency list over `parent_id`) that compiles to one output.
//! Every new document starts with a `main.typ` entrypoint.
//!
//! Text files are stored as Yjs updates. While someone edits a file, the newest
//! state lives in the collaboration room in memory (persisted every few seconds);
//! reads and writes here go through that room when it exists, so the API, the
//! compiler and open editors always agree.

use std::collections::{HashMap, HashSet};

use axum::{
    extract::{Multipart, Path, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use axum_extra::extract::cookie::SignedCookieJar;
use base64::Engine;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use yrs::updates::decoder::Decode;
use yrs::{Doc, GetString, ReadTxn, StateVector, Text, Transact, Update};

use crate::{
    access::{
        db_err, forbidden, not_found, now, require_document, require_project_role, require_user, session_user,
        ApiError, Role, DOCUMENT_COLUMNS,
    },
    compiler::ProjectInput,
    models::{
        CreateDocumentRequest, CreateNodeRequest, CreateVersionRequest, Document, DocumentVersion, Node,
        SetEntrypointRequest, UpdateDocumentRequest, UpdateNodeRequest,
    },
    AppState,
};

const TEXT_NAME: &str = "typst";
const MAX_NAME_LEN: usize = 255;
const DEFAULT_MAIN: &str = "= New document\n\nStart writing here.\n";

// --- Text blobs and live rooms ---------------------------------------------------

pub fn encode_text_blob(text: &str) -> Vec<u8> {
    let doc = Doc::new();
    let handle = doc.get_or_insert_text(TEXT_NAME);
    handle.insert(&mut doc.transact_mut(), 0, text);
    let bytes = doc.transact().encode_state_as_update_v1(&StateVector::default());
    bytes
}

pub fn decode_text_blob(blob: &[u8]) -> String {
    let doc = Doc::new();
    if let Ok(update) = Update::decode_v1(blob) {
        doc.transact_mut().apply_update(update);
    }
    let handle = doc.get_or_insert_text(TEXT_NAME);
    let text = handle.get_string(&doc.transact());
    text
}

pub fn room_key(doc_id: &str, node_id: &str) -> String {
    format!("doc:{doc_id}:{node_id}")
}

/// Current text of a file that is open in a collaboration room.
async fn live_text(state: &AppState, doc_id: &str, node_id: &str) -> Option<String> {
    let group = state.bcast_map.lock().await.get(&room_key(doc_id, node_id)).cloned()?;
    let awareness = group.awareness().read().await;
    let doc = awareness.doc();
    let text = doc.get_or_insert_text(TEXT_NAME);
    let content = text.get_string(&doc.transact());
    Some(content)
}

/// Replaces the text of an open room; connected editors receive the change.
/// Returns false when the file is not open anywhere.
async fn replace_live_text(state: &AppState, doc_id: &str, node_id: &str, content: &str) -> bool {
    let Some(group) = state.bcast_map.lock().await.get(&room_key(doc_id, node_id)).cloned() else {
        return false;
    };
    let awareness = group.awareness().write().await;
    let doc = awareness.doc();
    let text = doc.get_or_insert_text(TEXT_NAME);
    let mut txn = doc.transact_mut();
    let len = text.len(&txn);
    text.remove_range(&mut txn, 0, len);
    text.insert(&mut txn, 0, content);
    true
}

/// Closes the rooms of a document (used when its whole tree is replaced).
async fn close_rooms(state: &AppState, doc_id: &str) {
    let prefix = format!("doc:{doc_id}:");
    state.bcast_map.lock().await.retain(|key, _| !key.starts_with(&prefix));
}

fn is_text_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    [".typ", ".toml", ".bib", ".csl", ".yml", ".yaml", ".json", ".md", ".txt", ".csv", ".xml", ".svg"]
        .iter()
        .any(|ext| lower.ends_with(ext))
}

fn mime_for(name: &str) -> &'static str {
    let lower = name.to_lowercase();
    let ext = lower.rsplit('.').next().unwrap_or("");
    match ext {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "pdf" => "application/pdf",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "json" => "application/json",
        "csv" => "text/csv",
        "xml" => "application/xml",
        _ if is_text_name(&lower) => "text/plain",
        _ => "application/octet-stream",
    }
}

// --- Tree --------------------------------------------------------------------------

fn valid_node_name(name: &str) -> Result<String, ApiError> {
    let name = name.trim();
    let bad = name.is_empty()
        || name.chars().count() > MAX_NAME_LEN
        || name == "."
        || name == ".."
        || name.contains('/')
        || name.contains('\\')
        || name.chars().any(char::is_control);
    if bad {
        return Err((
            StatusCode::BAD_REQUEST,
            "Names must be 1 to 255 characters, without '/', '\\' or control characters, and not '.' or '..'".to_string(),
        ));
    }
    Ok(name.to_string())
}

#[derive(sqlx::FromRow)]
struct NodeRow {
    id: String,
    parent_id: Option<String>,
    name: String,
    kind: String,
    mime_type: Option<String>,
    size: i64,
    updated_at: String,
}

async fn load_rows(state: &AppState, doc_id: &str) -> Result<Vec<NodeRow>, ApiError> {
    sqlx::query_as(
        "SELECT id, parent_id, name, kind, mime_type, COALESCE(LENGTH(content), 0) AS size, updated_at \
         FROM nodes WHERE document_id = $1",
    )
    .bind(doc_id)
    .fetch_all(&state.db)
    .await
    .map_err(db_err)
}

/// All nodes with their paths, folders first, then by name within each folder.
async fn load_tree(state: &AppState, doc_id: &str) -> Result<Vec<Node>, ApiError> {
    let rows = load_rows(state, doc_id).await?;
    let by_id: HashMap<&str, &NodeRow> = rows.iter().map(|r| (r.id.as_str(), r)).collect();

    let path_of = |row: &NodeRow| -> String {
        let mut parts = vec![row.name.clone()];
        let mut seen = HashSet::from([row.id.as_str()]);
        let mut parent = row.parent_id.as_deref();
        while let Some(pid) = parent {
            match by_id.get(pid) {
                Some(p) if seen.insert(p.id.as_str()) => {
                    parts.push(p.name.clone());
                    parent = p.parent_id.as_deref();
                }
                _ => break, // the database prevents cycles; never loop regardless
            }
        }
        parts.reverse();
        parts.join("/")
    };

    let mut nodes: Vec<Node> = rows
        .iter()
        .map(|r| Node {
            id: r.id.clone(),
            parent_id: r.parent_id.clone(),
            name: r.name.clone(),
            kind: r.kind.clone(),
            mime_type: r.mime_type.clone(),
            size: r.size,
            path: path_of(r),
            updated_at: r.updated_at.clone(),
        })
        .collect();
    nodes.sort_by(|a, b| {
        let key = |n: &Node| (n.path.rsplit_once('/').map(|(dir, _)| dir.to_string()).unwrap_or_default(), n.kind != "folder", n.name.to_lowercase());
        key(a).cmp(&key(b))
    });
    Ok(nodes)
}

async fn node_of_document(state: &AppState, doc_id: &str, node_id: &str) -> Result<NodeRow, ApiError> {
    sqlx::query_as(
        "SELECT id, parent_id, name, kind, mime_type, COALESCE(LENGTH(content), 0) AS size, updated_at \
         FROM nodes WHERE id = $1 AND document_id = $2",
    )
    .bind(node_id)
    .bind(doc_id)
    .fetch_optional(&state.db)
    .await
    .map_err(db_err)?
    .ok_or_else(|| not_found("File"))
}

/// A parent must be a folder of the same document (or None for the top level).
async fn check_parent(state: &AppState, doc_id: &str, parent_id: Option<&str>) -> Result<(), ApiError> {
    if let Some(pid) = parent_id {
        let parent = node_of_document(state, doc_id, pid).await.map_err(|_| not_found("Folder"))?;
        if parent.kind != "folder" {
            return Err((StatusCode::BAD_REQUEST, "The target is not a folder".to_string()));
        }
    }
    Ok(())
}

fn conflict_on_duplicate(e: sqlx::Error) -> ApiError {
    match e {
        sqlx::Error::Database(err) if err.is_unique_violation() => {
            (StatusCode::CONFLICT, "A file or folder with this name already exists here".to_string())
        }
        e => db_err(e),
    }
}

async fn touch_document(state: &AppState, doc_id: &str) {
    let _ = sqlx::query("UPDATE documents SET updated_at = $1 WHERE id = $2")
        .bind(now())
        .bind(doc_id)
        .execute(&state.db)
        .await;
}

async fn node_response(state: &AppState, doc_id: &str, node_id: &str) -> Result<Json<Node>, ApiError> {
    load_tree(state, doc_id)
        .await?
        .into_iter()
        .find(|n| n.id == node_id)
        .map(Json)
        .ok_or_else(|| not_found("File"))
}

// --- Documents ---------------------------------------------------------------------

pub async fn list_documents(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
    jar: SignedCookieJar,
) -> Result<Json<Vec<Document>>, ApiError> {
    let user_id = require_user(&jar)?;
    let role = require_project_role(&state, &project_id, &user_id, Role::Viewer).await?;
    let mut docs: Vec<Document> = sqlx::query_as(&format!(
        "SELECT {DOCUMENT_COLUMNS} FROM documents WHERE project_id = $1 ORDER BY updated_at DESC"
    ))
    .bind(&project_id)
    .fetch_all(&state.db)
    .await
    .map_err(db_err)?;
    for doc in &mut docs {
        doc.role = Some(role.as_str().to_string());
        doc.is_member = true;
    }
    Ok(Json(docs))
}

pub async fn create_document(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
    jar: SignedCookieJar,
    Json(payload): Json<CreateDocumentRequest>,
) -> Result<(StatusCode, Json<Document>), ApiError> {
    let user_id = require_user(&jar)?;
    // Project editors, guests included, may add documents.
    require_project_role(&state, &project_id, &user_id, Role::Editor).await?;
    let title = payload.title.trim();
    if title.is_empty() || title.chars().count() > 200 {
        return Err((StatusCode::BAD_REQUEST, "Title must be 1 to 200 characters".to_string()));
    }

    let doc_id = Uuid::new_v4().to_string();
    let main_id = Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO documents (id, project_id, title, created_by) VALUES ($1, $2, $3, $4)")
        .bind(&doc_id)
        .bind(&project_id)
        .bind(title)
        .bind(&user_id)
        .execute(&state.db)
        .await
        .map_err(db_err)?;
    sqlx::query(
        "INSERT INTO nodes (id, document_id, parent_id, name, kind, content, mime_type) VALUES ($1, $2, NULL, 'main.typ', 'text', $3, 'text/plain')",
    )
    .bind(&main_id)
    .bind(&doc_id)
    .bind(encode_text_blob(payload.content.as_deref().unwrap_or(DEFAULT_MAIN)))
    .execute(&state.db)
    .await
    .map_err(db_err)?;
    sqlx::query("UPDATE documents SET entrypoint_id = $1 WHERE id = $2")
        .bind(&main_id)
        .bind(&doc_id)
        .execute(&state.db)
        .await
        .map_err(db_err)?;
    let _ = sqlx::query("UPDATE projects SET updated_at = $1 WHERE id = $2")
        .bind(now())
        .bind(&project_id)
        .execute(&state.db)
        .await;

    let access = require_document(&state, &doc_id, Some(&user_id), Role::Viewer).await?;
    Ok((StatusCode::CREATED, Json(access.doc)))
}

pub async fn get_document(
    State(state): State<AppState>,
    Path(doc_id): Path<String>,
    jar: SignedCookieJar,
) -> Result<Json<Document>, ApiError> {
    let user = session_user(&jar);
    Ok(Json(require_document(&state, &doc_id, user.as_deref(), Role::Viewer).await?.doc))
}

pub async fn update_document(
    State(state): State<AppState>,
    Path(doc_id): Path<String>,
    jar: SignedCookieJar,
    Json(payload): Json<UpdateDocumentRequest>,
) -> Result<Json<Document>, ApiError> {
    let user_id = require_user(&jar)?;
    let access = require_document(&state, &doc_id, Some(&user_id), Role::Editor).await?;
    // Link-sharing editors may change content, not the document's title or sharing.
    if !access.is_member {
        return Err(forbidden("Only project members can change document settings"));
    }

    if let Some(title) = &payload.title {
        let title = title.trim();
        if title.is_empty() || title.chars().count() > 200 {
            return Err((StatusCode::BAD_REQUEST, "Title must be 1 to 200 characters".to_string()));
        }
        sqlx::query("UPDATE documents SET title = $1, updated_at = $2 WHERE id = $3")
            .bind(title)
            .bind(now())
            .bind(&doc_id)
            .execute(&state.db)
            .await
            .map_err(db_err)?;
    }
    if let Some(public_role) = &payload.public_role {
        if let Some(role) = public_role {
            if role != "viewer" && role != "editor" {
                return Err((StatusCode::BAD_REQUEST, "public_role must be viewer, editor or null".to_string()));
            }
        }
        sqlx::query("UPDATE documents SET public_role = $1 WHERE id = $2")
            .bind(public_role)
            .bind(&doc_id)
            .execute(&state.db)
            .await
            .map_err(db_err)?;
    }
    Ok(Json(require_document(&state, &doc_id, Some(&user_id), Role::Viewer).await?.doc))
}

/// Project owners delete any document; editors the ones they created.
pub async fn delete_document(
    State(state): State<AppState>,
    Path(doc_id): Path<String>,
    jar: SignedCookieJar,
) -> Result<StatusCode, ApiError> {
    let user_id = require_user(&jar)?;
    let access = require_document(&state, &doc_id, Some(&user_id), Role::Editor).await?;
    let own = access.doc.created_by.as_deref() == Some(user_id.as_str());
    if !access.is_member || (access.role != Role::Owner && !own) {
        return Err(forbidden("Only project owners and the document's creator can delete it"));
    }
    close_rooms(&state, &doc_id).await;
    sqlx::query("DELETE FROM documents WHERE id = $1")
        .bind(&doc_id)
        .execute(&state.db)
        .await
        .map_err(db_err)?;
    Ok(StatusCode::NO_CONTENT)
}

// --- Files and folders -------------------------------------------------------------

#[derive(Serialize)]
pub struct TreeResponse {
    entrypoint_id: Option<String>,
    nodes: Vec<Node>,
}

pub async fn get_tree(
    State(state): State<AppState>,
    Path(doc_id): Path<String>,
    jar: SignedCookieJar,
) -> Result<Json<TreeResponse>, ApiError> {
    let user = session_user(&jar);
    let access = require_document(&state, &doc_id, user.as_deref(), Role::Viewer).await?;
    Ok(Json(TreeResponse { entrypoint_id: access.doc.entrypoint_id, nodes: load_tree(&state, &doc_id).await? }))
}

pub async fn create_node(
    State(state): State<AppState>,
    Path(doc_id): Path<String>,
    jar: SignedCookieJar,
    Json(payload): Json<CreateNodeRequest>,
) -> Result<(StatusCode, Json<Node>), ApiError> {
    let user = session_user(&jar);
    require_document(&state, &doc_id, user.as_deref(), Role::Editor).await?;
    let name = valid_node_name(&payload.name)?;
    check_parent(&state, &doc_id, payload.parent_id.as_deref()).await?;

    let (kind, content, mime) = match payload.kind.as_str() {
        "folder" => ("folder", None, None),
        "text" => ("text", Some(encode_text_blob(payload.content.as_deref().unwrap_or(""))), Some(mime_for(&name))),
        _ => return Err((StatusCode::BAD_REQUEST, "kind must be folder or text; upload binary files".to_string())),
    };

    let node_id = Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO nodes (id, document_id, parent_id, name, kind, content, mime_type) VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(&node_id)
    .bind(&doc_id)
    .bind(&payload.parent_id)
    .bind(&name)
    .bind(kind)
    .bind(content)
    .bind(mime)
    .execute(&state.db)
    .await
    .map_err(conflict_on_duplicate)?;
    touch_document(&state, &doc_id).await;

    Ok((StatusCode::CREATED, node_response(&state, &doc_id, &node_id).await?))
}

/// Multipart upload into a folder: an optional `parent_id` field followed by one or
/// more files. A file with the same name in that folder is replaced.
pub async fn upload_files(
    State(state): State<AppState>,
    Path(doc_id): Path<String>,
    jar: SignedCookieJar,
    mut multipart: Multipart,
) -> Result<Json<Vec<Node>>, ApiError> {
    let user = session_user(&jar);
    require_document(&state, &doc_id, user.as_deref(), Role::Editor).await?;

    let mut parent_id: Option<String> = None;
    let mut uploaded = Vec::new();
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?
    {
        if field.name() == Some("parent_id") {
            let value = field.text().await.map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
            parent_id = Some(value).filter(|v| !v.is_empty());
            check_parent(&state, &doc_id, parent_id.as_deref()).await?;
            continue;
        }
        let Some(file_name) = field.file_name().map(str::to_string) else { continue };
        let name = valid_node_name(&file_name)?;
        let data = field.bytes().await.map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?.to_vec();
        let (kind, content) = if is_text_name(&name) {
            match String::from_utf8(data.clone()) {
                Ok(text) => ("text", encode_text_blob(&text)),
                Err(_) => ("binary", data),
            }
        } else {
            ("binary", data)
        };

        let existing: Option<(String, String)> = sqlx::query_as(
            "SELECT id, kind FROM nodes WHERE document_id = $1 AND COALESCE(parent_id, '') = COALESCE($2, '') AND name = $3",
        )
        .bind(&doc_id)
        .bind(&parent_id)
        .bind(&name)
        .fetch_optional(&state.db)
        .await
        .map_err(db_err)?;

        let node_id = match existing {
            Some((_, existing_kind)) if existing_kind == "folder" => {
                return Err((StatusCode::CONFLICT, format!("A folder named {name} already exists here")));
            }
            Some((id, _)) => {
                close_room(&state, &doc_id, &id).await;
                sqlx::query("UPDATE nodes SET kind = $1, content = $2, mime_type = $3, updated_at = $4 WHERE id = $5")
                    .bind(kind)
                    .bind(&content)
                    .bind(mime_for(&name))
                    .bind(now())
                    .bind(&id)
                    .execute(&state.db)
                    .await
                    .map_err(db_err)?;
                id
            }
            None => {
                let id = Uuid::new_v4().to_string();
                sqlx::query(
                    "INSERT INTO nodes (id, document_id, parent_id, name, kind, content, mime_type) VALUES ($1, $2, $3, $4, $5, $6, $7)",
                )
                .bind(&id)
                .bind(&doc_id)
                .bind(&parent_id)
                .bind(&name)
                .bind(kind)
                .bind(&content)
                .bind(mime_for(&name))
                .execute(&state.db)
                .await
                .map_err(conflict_on_duplicate)?;
                id
            }
        };
        uploaded.push(node_id);
    }
    if uploaded.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "No files uploaded".to_string()));
    }
    touch_document(&state, &doc_id).await;

    let tree = load_tree(&state, &doc_id).await?;
    Ok(Json(tree.into_iter().filter(|n| uploaded.contains(&n.id)).collect()))
}

/// A replaced file must not be overwritten by its old room's autosave.
async fn close_room(state: &AppState, doc_id: &str, node_id: &str) {
    state.bcast_map.lock().await.remove(&room_key(doc_id, node_id));
}

pub async fn get_node_content(
    State(state): State<AppState>,
    Path((doc_id, node_id)): Path<(String, String)>,
    jar: SignedCookieJar,
) -> Result<Response, ApiError> {
    let user = session_user(&jar);
    require_document(&state, &doc_id, user.as_deref(), Role::Viewer).await?;
    let node = node_of_document(&state, &doc_id, &node_id).await?;
    if node.kind == "folder" {
        return Err((StatusCode::BAD_REQUEST, "Folders have no content".to_string()));
    }

    if node.kind == "text" {
        let text = match live_text(&state, &doc_id, &node_id).await {
            Some(text) => text,
            None => decode_text_blob(&stored_content(&state, &node_id).await?),
        };
        return Ok(([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], text).into_response());
    }

    let mime = node.mime_type.unwrap_or_else(|| "application/octet-stream".to_string());
    Ok(([(header::CONTENT_TYPE, mime)], stored_content(&state, &node_id).await?).into_response())
}

async fn stored_content(state: &AppState, node_id: &str) -> Result<Vec<u8>, ApiError> {
    let content: Option<Option<Vec<u8>>> = sqlx::query_scalar("SELECT content FROM nodes WHERE id = $1")
        .bind(node_id)
        .fetch_optional(&state.db)
        .await
        .map_err(db_err)?;
    Ok(content.flatten().unwrap_or_default())
}

/// Rename, move (`parent_id`, null = top level) or replace the text of a node.
pub async fn update_node(
    State(state): State<AppState>,
    Path((doc_id, node_id)): Path<(String, String)>,
    jar: SignedCookieJar,
    Json(payload): Json<UpdateNodeRequest>,
) -> Result<Json<Node>, ApiError> {
    let user = session_user(&jar);
    require_document(&state, &doc_id, user.as_deref(), Role::Editor).await?;
    let node = node_of_document(&state, &doc_id, &node_id).await?;

    let name = match &payload.name {
        Some(name) => valid_node_name(name)?,
        None => node.name.clone(),
    };
    let parent_id = match &payload.parent_id {
        Some(target) => {
            check_parent(&state, &doc_id, target.as_deref()).await?;
            if let Some(target_id) = target {
                ensure_not_inside(&state, &doc_id, &node_id, target_id).await?;
            }
            target.clone()
        }
        None => node.parent_id.clone(),
    };

    if payload.name.is_some() || payload.parent_id.is_some() {
        sqlx::query("UPDATE nodes SET name = $1, parent_id = $2, updated_at = $3 WHERE id = $4")
            .bind(&name)
            .bind(&parent_id)
            .bind(now())
            .bind(&node_id)
            .execute(&state.db)
            .await
            .map_err(conflict_on_duplicate)?;
    }

    if let Some(content) = &payload.content {
        if node.kind != "text" {
            return Err((StatusCode::BAD_REQUEST, "Only text files can be edited".to_string()));
        }
        // An open room would overwrite the database on its next save: edit it instead.
        if !replace_live_text(&state, &doc_id, &node_id, content).await {
            sqlx::query("UPDATE nodes SET content = $1, updated_at = $2 WHERE id = $3")
                .bind(encode_text_blob(content))
                .bind(now())
                .bind(&node_id)
                .execute(&state.db)
                .await
                .map_err(db_err)?;
        }
    }
    touch_document(&state, &doc_id).await;

    node_response(&state, &doc_id, &node_id).await
}

/// Moving a folder into itself or one of its descendants would create a cycle.
async fn ensure_not_inside(state: &AppState, doc_id: &str, node_id: &str, target_id: &str) -> Result<(), ApiError> {
    let rows = load_rows(state, doc_id).await?;
    let parents: HashMap<&str, Option<&str>> = rows.iter().map(|r| (r.id.as_str(), r.parent_id.as_deref())).collect();
    let mut current = Some(target_id);
    let mut steps = 0;
    while let Some(id) = current {
        if id == node_id {
            return Err((StatusCode::BAD_REQUEST, "A folder cannot be moved into itself".to_string()));
        }
        steps += 1;
        if steps > rows.len() {
            break;
        }
        current = parents.get(id).copied().flatten();
    }
    Ok(())
}

pub async fn delete_node(
    State(state): State<AppState>,
    Path((doc_id, node_id)): Path<(String, String)>,
    jar: SignedCookieJar,
) -> Result<StatusCode, ApiError> {
    let user = session_user(&jar);
    require_document(&state, &doc_id, user.as_deref(), Role::Editor).await?;
    node_of_document(&state, &doc_id, &node_id).await?;
    // Contents of a folder cascade; the entrypoint reference is cleared by the database.
    sqlx::query("DELETE FROM nodes WHERE id = $1")
        .bind(&node_id)
        .execute(&state.db)
        .await
        .map_err(db_err)?;
    close_room(&state, &doc_id, &node_id).await;
    touch_document(&state, &doc_id).await;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn set_entrypoint(
    State(state): State<AppState>,
    Path(doc_id): Path<String>,
    jar: SignedCookieJar,
    Json(payload): Json<SetEntrypointRequest>,
) -> Result<Json<Document>, ApiError> {
    let user = session_user(&jar);
    require_document(&state, &doc_id, user.as_deref(), Role::Editor).await?;
    let node = node_of_document(&state, &doc_id, &payload.node_id).await?;
    if node.kind != "text" || !node.name.to_lowercase().ends_with(".typ") {
        return Err((StatusCode::BAD_REQUEST, "The entrypoint must be a .typ file".to_string()));
    }
    sqlx::query("UPDATE documents SET entrypoint_id = $1, updated_at = $2 WHERE id = $3")
        .bind(&node.id)
        .bind(now())
        .bind(&doc_id)
        .execute(&state.db)
        .await
        .map_err(db_err)?;
    Ok(Json(require_document(&state, &doc_id, user.as_deref(), Role::Viewer).await?.doc))
}

// --- Compilation input ---------------------------------------------------------------

/// The files of a document as Typst sees them (path -> bytes), using the live state
/// of open files. `overrides` (path -> text) take precedence, e.g. unsaved edits.
pub async fn document_files(
    state: &AppState,
    doc: &Document,
    overrides: &HashMap<String, String>,
) -> Result<(Option<String>, HashMap<String, Vec<u8>>), ApiError> {
    let tree = load_tree(state, &doc.id).await?;
    let mut files = HashMap::new();
    let mut entrypoint = None;
    for node in &tree {
        if node.kind == "folder" {
            continue;
        }
        if doc.entrypoint_id.as_deref() == Some(node.id.as_str()) {
            entrypoint = Some(node.path.clone());
        }
        let bytes = if let Some(text) = overrides.get(&node.path) {
            text.clone().into_bytes()
        } else if node.kind == "text" {
            match live_text(state, &doc.id, &node.id).await {
                Some(text) => text.into_bytes(),
                None => decode_text_blob(&stored_content(state, &node.id).await?).into_bytes(),
            }
        } else {
            stored_content(state, &node.id).await?
        };
        files.insert(node.path.clone(), bytes);
    }
    Ok((entrypoint, files))
}

pub async fn assemble_document(
    state: &AppState,
    doc: &Document,
    overrides: &HashMap<String, String>,
) -> Result<ProjectInput, ApiError> {
    let (entrypoint, files) = document_files(state, doc, overrides).await?;
    let entrypoint = entrypoint.ok_or((
        StatusCode::UNPROCESSABLE_ENTITY,
        "The document has no entrypoint; mark a .typ file as main file".to_string(),
    ))?;
    Ok(ProjectInput {
        entrypoint,
        files,
        packages: crate::packages::load_packages(state, &doc.project_id).await,
    })
}

// --- Versions --------------------------------------------------------------------------

#[derive(Serialize, Deserialize)]
struct Snapshot {
    entrypoint: Option<String>,
    folders: Vec<String>,
    files: Vec<SnapshotFile>,
}

#[derive(Serialize, Deserialize)]
struct SnapshotFile {
    path: String,
    kind: String,
    mime_type: Option<String>,
    /// Plain text for text files, base64 for binary files.
    data: String,
}

const VERSION_COLUMNS: &str = "v.id, v.document_id, v.user_id, v.label, v.created_at, \
     u.username AS author_name, u.avatar_url AS author_avatar_url";

pub async fn list_versions(
    State(state): State<AppState>,
    Path(doc_id): Path<String>,
    jar: SignedCookieJar,
) -> Result<Json<Vec<DocumentVersion>>, ApiError> {
    let user = session_user(&jar);
    require_document(&state, &doc_id, user.as_deref(), Role::Viewer).await?;
    let versions = sqlx::query_as(&format!(
        "SELECT {VERSION_COLUMNS} FROM document_versions v LEFT JOIN users u ON u.id = v.user_id \
         WHERE v.document_id = $1 ORDER BY v.created_at DESC"
    ))
    .bind(&doc_id)
    .fetch_all(&state.db)
    .await
    .map_err(db_err)?;
    Ok(Json(versions))
}

pub async fn create_version(
    State(state): State<AppState>,
    Path(doc_id): Path<String>,
    jar: SignedCookieJar,
    Json(payload): Json<CreateVersionRequest>,
) -> Result<(StatusCode, Json<DocumentVersion>), ApiError> {
    let user_id = require_user(&jar)?;
    let access = require_document(&state, &doc_id, Some(&user_id), Role::Editor).await?;

    let tree = load_tree(&state, &doc_id).await?;
    let (entrypoint, files) = document_files(&state, &access.doc, &HashMap::new()).await?;
    let b64 = base64::engine::general_purpose::STANDARD;
    let snapshot = Snapshot {
        entrypoint,
        folders: tree.iter().filter(|n| n.kind == "folder").map(|n| n.path.clone()).collect(),
        files: tree
            .iter()
            .filter(|n| n.kind != "folder")
            .map(|n| {
                let bytes = files.get(&n.path).cloned().unwrap_or_default();
                SnapshotFile {
                    path: n.path.clone(),
                    kind: n.kind.clone(),
                    mime_type: n.mime_type.clone(),
                    data: if n.kind == "text" { String::from_utf8_lossy(&bytes).into_owned() } else { b64.encode(&bytes) },
                }
            })
            .collect(),
    };

    let version_id = Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO document_versions (id, document_id, user_id, label, snapshot) VALUES ($1, $2, $3, $4, $5)")
        .bind(&version_id)
        .bind(&doc_id)
        .bind(&user_id)
        .bind(payload.label.as_deref().map(str::trim).filter(|l| !l.is_empty()))
        .bind(serde_json::to_vec(&snapshot).map_err(db_err)?)
        .execute(&state.db)
        .await
        .map_err(db_err)?;

    let version = sqlx::query_as(&format!(
        "SELECT {VERSION_COLUMNS} FROM document_versions v LEFT JOIN users u ON u.id = v.user_id WHERE v.id = $1"
    ))
    .bind(&version_id)
    .fetch_one(&state.db)
    .await
    .map_err(db_err)?;
    Ok((StatusCode::CREATED, Json(version)))
}

async fn load_snapshot(state: &AppState, doc_id: &str, version_id: &str) -> Result<Snapshot, ApiError> {
    let blob: Vec<u8> = sqlx::query_scalar("SELECT snapshot FROM document_versions WHERE id = $1 AND document_id = $2")
        .bind(version_id)
        .bind(doc_id)
        .fetch_optional(&state.db)
        .await
        .map_err(db_err)?
        .ok_or_else(|| not_found("Version"))?;
    serde_json::from_slice(&blob).map_err(db_err)
}

/// The files of a version: text files in full, binary files by path only.
pub async fn get_version(
    State(state): State<AppState>,
    Path((doc_id, version_id)): Path<(String, String)>,
    jar: SignedCookieJar,
) -> Result<Json<serde_json::Value>, ApiError> {
    let user = session_user(&jar);
    require_document(&state, &doc_id, user.as_deref(), Role::Viewer).await?;
    let snapshot = load_snapshot(&state, &doc_id, &version_id).await?;
    let files: Vec<_> = snapshot
        .files
        .iter()
        .map(|f| {
            serde_json::json!({
                "path": f.path,
                "kind": f.kind,
                "content": if f.kind == "text" { Some(&f.data) } else { None },
            })
        })
        .collect();
    Ok(Json(serde_json::json!({ "entrypoint": snapshot.entrypoint, "folders": snapshot.folders, "files": files })))
}

/// Replaces the whole file tree with a version. Open editors must reload.
pub async fn restore_version(
    State(state): State<AppState>,
    Path((doc_id, version_id)): Path<(String, String)>,
    jar: SignedCookieJar,
) -> Result<Json<TreeResponse>, ApiError> {
    let user_id = require_user(&jar)?;
    let access = require_document(&state, &doc_id, Some(&user_id), Role::Editor).await?;
    if !access.is_member {
        return Err(forbidden("Only project members can restore versions"));
    }
    let snapshot = load_snapshot(&state, &doc_id, &version_id).await?;
    let b64 = base64::engine::general_purpose::STANDARD;

    close_rooms(&state, &doc_id).await;
    sqlx::query("DELETE FROM nodes WHERE document_id = $1")
        .bind(&doc_id)
        .execute(&state.db)
        .await
        .map_err(db_err)?;

    // Recreate folders parents-first, then files.
    let mut folder_ids: HashMap<String, String> = HashMap::new();
    let mut folders = snapshot.folders.clone();
    folders.sort_by_key(|p| p.matches('/').count());
    let ensure_folder = |path: &str, folder_ids: &HashMap<String, String>| -> Option<String> {
        path.rsplit_once('/').and_then(|(dir, _)| folder_ids.get(dir).cloned())
    };
    for path in &folders {
        let id = Uuid::new_v4().to_string();
        let name = path.rsplit('/').next().unwrap_or(path);
        sqlx::query("INSERT INTO nodes (id, document_id, parent_id, name, kind) VALUES ($1, $2, $3, $4, 'folder')")
            .bind(&id)
            .bind(&doc_id)
            .bind(ensure_folder(path, &folder_ids))
            .bind(name)
            .execute(&state.db)
            .await
            .map_err(db_err)?;
        folder_ids.insert(path.clone(), id);
    }
    let mut entrypoint_id = None;
    for file in &snapshot.files {
        let id = Uuid::new_v4().to_string();
        let name = file.path.rsplit('/').next().unwrap_or(&file.path);
        let content = if file.kind == "text" {
            encode_text_blob(&file.data)
        } else {
            b64.decode(&file.data).map_err(db_err)?
        };
        sqlx::query(
            "INSERT INTO nodes (id, document_id, parent_id, name, kind, content, mime_type) VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(&id)
        .bind(&doc_id)
        .bind(ensure_folder(&file.path, &folder_ids))
        .bind(name)
        .bind(&file.kind)
        .bind(&content)
        .bind(&file.mime_type)
        .execute(&state.db)
        .await
        .map_err(db_err)?;
        if snapshot.entrypoint.as_deref() == Some(file.path.as_str()) {
            entrypoint_id = Some(id);
        }
    }
    sqlx::query("UPDATE documents SET entrypoint_id = $1, updated_at = $2 WHERE id = $3")
        .bind(&entrypoint_id)
        .bind(now())
        .bind(&doc_id)
        .execute(&state.db)
        .await
        .map_err(db_err)?;

    Ok(Json(TreeResponse { entrypoint_id, nodes: load_tree(&state, &doc_id).await? }))
}

/// Font families uploaded into a document, for the editor's font picker.
pub async fn list_fonts(
    State(state): State<AppState>,
    Path(doc_id): Path<String>,
    jar: SignedCookieJar,
) -> Result<Json<Vec<String>>, ApiError> {
    let user = session_user(&jar);
    require_document(&state, &doc_id, user.as_deref(), Role::Viewer).await?;
    let fonts: Vec<(String, Option<Vec<u8>>)> = sqlx::query_as(
        "SELECT name, content FROM nodes WHERE document_id = $1 AND kind = 'binary' \
         AND (LOWER(name) LIKE '%.ttf' OR LOWER(name) LIKE '%.otf')",
    )
    .bind(&doc_id)
    .fetch_all(&state.db)
    .await
    .map_err(db_err)?;

    let mut families = std::collections::BTreeSet::new();
    for (_, data) in fonts {
        for font in typst::text::Font::iter(typst::foundations::Bytes::new(data.unwrap_or_default())) {
            families.insert(font.info().family.clone());
        }
    }
    Ok(Json(families.into_iter().collect()))
}
