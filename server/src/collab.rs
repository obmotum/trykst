use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use axum_extra::extract::cookie::SignedCookieJar;
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    models::{Collaborator, CollaboratorView, Comment, CreateCommentRequest, Invitation, InviteRequest, UpdateCommentRequest},
    AppState,
};

pub async fn invite_collaborator(
    State(state): State<AppState>,
    Path(doc_id): Path<String>,
    jar: SignedCookieJar,
    Json(payload): Json<InviteRequest>,
) -> Result<Json<Invitation>, (StatusCode, String)> {
    let inviter_id = jar.get("session_user_id").map(|c| c.value().to_string())
        .ok_or((StatusCode::UNAUTHORIZED, "Not logged in".to_string()))?;
    crate::auth::require_member(&state, &inviter_id).await?;

    let doc_exists = sqlx::query_as::<_, (String,)>("SELECT id FROM documents WHERE id = ? AND owner_id = ?")
        .bind(&doc_id)
        .bind(&inviter_id)
        .fetch_optional(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if doc_exists.is_none() {
        return Err((StatusCode::FORBIDDEN, "Only the owner can invite collaborators".to_string()));
    }

    if payload.role != "editor" && payload.role != "viewer" {
        return Err((StatusCode::BAD_REQUEST, "Role must be editor or viewer".to_string()));
    }
    let user_id = crate::oidc::resolve_invitee(&state, payload.subject.as_deref(), payload.email.as_deref()).await?;
    if user_id == inviter_id {
        return Err((StatusCode::BAD_REQUEST, "You already own this document".to_string()));
    }

    let collab_id = Uuid::new_v4().to_string();
    let _collab = sqlx::query_as::<_, Collaborator>(
        "INSERT INTO collaborators (id, document_id, user_id, role) VALUES (?, ?, ?, ?) ON CONFLICT (document_id, user_id) DO UPDATE SET role = excluded.role RETURNING id, document_id, user_id, role, created_at"
    )
    .bind(&collab_id)
    .bind(&doc_id)
    .bind(&user_id)
    .bind(&payload.role)
    .fetch_one(&state.db)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(Invitation {
        id: Uuid::new_v4().to_string(),
        document_id: doc_id.to_string(),
        role: payload.role.clone(),
        token: "direct-added".to_string(),
        created_at: chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        expires_at: None,
    }))
}

#[derive(Deserialize)]
pub struct AcceptInviteQuery {
    pub token: String,
}

pub async fn accept_invite(
    State(state): State<AppState>,
    jar: SignedCookieJar,
    Query(query): Query<AcceptInviteQuery>,
) -> Result<Json<Collaborator>, (StatusCode, String)> {
    let user_id = jar.get("session_user_id").map(|c| c.value().to_string())
        .ok_or((StatusCode::UNAUTHORIZED, "Not logged in".to_string()))?;

    let invitation = sqlx::query_as::<_, Invitation>(
        "SELECT id, document_id, role, token, created_at, expires_at FROM invitations WHERE token = ?"
    )
    .bind(&query.token)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .ok_or((StatusCode::NOT_FOUND, "Invalid or expired invitation".to_string()))?;

    let collab_id = Uuid::new_v4().to_string();

    let collab = sqlx::query_as::<_, Collaborator>(
        "INSERT INTO collaborators (id, document_id, user_id, role) VALUES (?, ?, ?, ?) ON CONFLICT (document_id, user_id) DO UPDATE SET role = excluded.role RETURNING id, document_id, user_id, role, created_at"
    )
    .bind(&collab_id)
    .bind(&invitation.document_id)
    .bind(&user_id)
    .bind(&invitation.role)
    .fetch_one(&state.db)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(collab))
}

pub async fn list_collaborators(
    State(state): State<AppState>,
    Path(doc_id): Path<String>,
    jar: SignedCookieJar,
) -> Result<Json<Vec<CollaboratorView>>, (StatusCode, String)> {
    let user_id = jar.get("session_user_id").map(|c| c.value().to_string())
        .ok_or((StatusCode::UNAUTHORIZED, "Not logged in".to_string()))?;

    // Only owner or collaborators on the document can see the list
    let has_access = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM documents WHERE id = ? AND owner_id = ? \
         UNION ALL SELECT COUNT(*) FROM collaborators WHERE document_id = ? AND user_id = ?"
    )
    .bind(&doc_id).bind(&user_id).bind(&doc_id).bind(&user_id)
    .fetch_all(&state.db)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .into_iter().sum::<i64>() > 0;

    if !has_access {
        return Err((StatusCode::FORBIDDEN, "Access denied".to_string()));
    }

    let collaborators = sqlx::query_as::<_, CollaboratorView>(
        "SELECT c.id, c.user_id, u.username, u.email, c.role, c.created_at \
         FROM collaborators c \
         INNER JOIN users u ON u.id = c.user_id \
         WHERE c.document_id = ? \
         ORDER BY c.created_at ASC"
    )
    .bind(&doc_id)
    .fetch_all(&state.db)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(collaborators))
}

pub async fn remove_collaborator(
    State(state): State<AppState>,
    Path((doc_id, collab_id)): Path<(String, String)>,
    jar: SignedCookieJar,
) -> Result<StatusCode, (StatusCode, String)> {
    let user_id = jar.get("session_user_id").map(|c| c.value().to_string())
        .ok_or((StatusCode::UNAUTHORIZED, "Not logged in".to_string()))?;

    // Only the document owner can remove collaborators
    let is_owner = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM documents WHERE id = ? AND owner_id = ?"
    )
    .bind(&doc_id)
    .bind(&user_id)
    .fetch_one(&state.db)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))? > 0;

    if !is_owner {
        return Err((StatusCode::FORBIDDEN, "Only the document owner can remove collaborators".to_string()));
    }

    let result = sqlx::query(
        "DELETE FROM collaborators WHERE id = ? AND document_id = ?"
    )
    .bind(&collab_id)
    .bind(&doc_id)
    .execute(&state.db)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if result.rows_affected() == 0 {
        return Err((StatusCode::NOT_FOUND, "Collaborator not found".to_string()));
    }

    Ok(StatusCode::NO_CONTENT)
}

/// The caller's effective role on a document, resolved the same way as `docs::get_document`:
/// owner, then the collaborators table, then `documents.public_role`.
/// Returns `None` when the document does not exist or the caller has no access.
async fn document_role(state: &AppState, doc_id: &str, user_id: &str) -> Result<Option<String>, (StatusCode, String)> {
    let doc = sqlx::query_as::<_, (String, Option<String>)>("SELECT owner_id, public_role FROM documents WHERE id = ?")
        .bind(doc_id)
        .fetch_optional(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let Some((owner_id, public_role)) = doc else {
        return Ok(None);
    };

    if owner_id == user_id {
        return Ok(Some("owner".to_string()));
    }

    let collab_role = sqlx::query_scalar::<_, String>("SELECT role FROM collaborators WHERE document_id = ? AND user_id = ?")
        .bind(doc_id)
        .bind(user_id)
        .fetch_optional(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if collab_role.is_some() {
        return Ok(collab_role);
    }

    Ok(public_role.filter(|pr| pr == "viewer" || pr == "editor"))
}

/// Comments follow the document's role semantics: anyone who can open the document
/// (owner, editor, viewer) may read its comments, but only owners and editors may
/// write them. A viewer is read-only everywhere else (no edits, no versions), so
/// posting, editing or resolving comments is treated as a write as well.
fn can_write_comments(role: &str) -> bool {
    role == "owner" || role == "editor"
}

// resolved is BOOLEAN on Postgres and INTEGER on SQLite; the CAST makes both decode as i64.
const COMMENT_COLUMNS: &str = "c.id, c.document_id, c.user_id, c.content, \
     COALESCE(CAST(c.resolved AS INTEGER), 0) AS resolved, c.created_at, u.username as author_name";

async fn fetch_comment(state: &AppState, comment_id: &str) -> Result<Option<Comment>, (StatusCode, String)> {
    sqlx::query_as::<_, Comment>(&format!(
        "SELECT {COMMENT_COLUMNS} FROM comments c LEFT JOIN users u ON c.user_id = u.id WHERE c.id = ?"
    ))
    .bind(comment_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

pub async fn get_comments(
    State(state): State<AppState>,
    Path(doc_id): Path<String>,
    jar: SignedCookieJar,
) -> Result<Json<Vec<Comment>>, (StatusCode, String)> {
    let user_id = jar.get("session_user_id").map(|c| c.value().to_string())
        .ok_or((StatusCode::UNAUTHORIZED, "Not logged in".to_string()))?;

    if document_role(&state, &doc_id, &user_id).await?.is_none() {
        return Err((StatusCode::NOT_FOUND, "Document not found".to_string()));
    }

    let comments = sqlx::query_as::<_, Comment>(&format!(
        "SELECT {COMMENT_COLUMNS} \
         FROM comments c \
         LEFT JOIN users u ON c.user_id = u.id \
         WHERE c.document_id = ? \
         ORDER BY c.created_at ASC"
    ))
    .bind(&doc_id)
    .fetch_all(&state.db)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(comments))
}

pub async fn add_comment(
    State(state): State<AppState>,
    Path(doc_id): Path<String>,
    jar: SignedCookieJar,
    Json(payload): Json<CreateCommentRequest>,
) -> Result<Json<Comment>, (StatusCode, String)> {
    let user_id = jar.get("session_user_id").map(|c| c.value().to_string())
        .ok_or((StatusCode::UNAUTHORIZED, "Not logged in".to_string()))?;

    let role = document_role(&state, &doc_id, &user_id).await?
        .ok_or((StatusCode::NOT_FOUND, "Document not found".to_string()))?;
    if !can_write_comments(&role) {
        return Err((StatusCode::FORBIDDEN, "Viewers cannot add comments".to_string()));
    }

    let comment_id = Uuid::new_v4().to_string();

    sqlx::query("INSERT INTO comments (id, document_id, user_id, content) VALUES (?, ?, ?, ?)")
        .bind(&comment_id)
        .bind(&doc_id)
        .bind(&user_id)
        .bind(&payload.content)
        .execute(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let comment = fetch_comment(&state, &comment_id).await?
        .ok_or((StatusCode::INTERNAL_SERVER_ERROR, "Comment missing after insert".to_string()))?;

    Ok(Json(comment))
}

pub async fn create_version(
    State(state): State<AppState>,
    Path(doc_id): Path<String>,
    jar: SignedCookieJar,
    Json(payload): Json<crate::models::CreateVersionRequest>,
) -> Result<Json<crate::models::DocumentVersion>, (StatusCode, String)> {
    let user_id = jar.get("session_user_id").map(|c| c.value().to_string())
        .ok_or((StatusCode::UNAUTHORIZED, "Not logged in".to_string()))?;

    let doc = sqlx::query_as::<_, crate::models::Document>(
        "SELECT id, owner_id, folder_id, title, content, thumbnail_svg, public_role, created_at, updated_at FROM documents WHERE id = ?"
    )
    .bind(&doc_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .ok_or((StatusCode::NOT_FOUND, "Document not found".to_string()))?;

    let is_owner = doc.owner_id == user_id;
    let role = sqlx::query_scalar::<_, String>("SELECT role FROM collaborators WHERE document_id = ? AND user_id = ?")
        .bind(&doc_id)
        .bind(&user_id)
        .fetch_optional(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if !is_owner && role != Some("editor".to_string()) {
        return Err((StatusCode::FORBIDDEN, "Not authorized to create versions".to_string()));
    }

    let version_id = uuid::Uuid::new_v4().to_string();

    sqlx::query("INSERT INTO document_versions (id, document_id, user_id, content) VALUES (?, ?, ?, ?)")
        .bind(&version_id)
        .bind(&doc_id)
        .bind(&user_id)
        .bind(&payload.content)
        .execute(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let version = sqlx::query_as::<_, crate::models::DocumentVersion>(
        "SELECT v.id, v.document_id, v.user_id, v.content, v.created_at, u.username as author_name \
         FROM document_versions v \
         LEFT JOIN users u ON v.user_id = u.id \
         WHERE v.id = ?"
    )
    .bind(&version_id)
    .fetch_one(&state.db)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(version))
}

pub async fn get_versions(
    State(state): State<AppState>,
    Path(doc_id): Path<String>,
    jar: SignedCookieJar,
) -> Result<Json<Vec<crate::models::DocumentVersion>>, (StatusCode, String)> {
    let _user_id = jar.get("session_user_id").map(|c| c.value().to_string())
        .ok_or((StatusCode::UNAUTHORIZED, "Not logged in".to_string()))?;

    let versions = sqlx::query_as::<_, crate::models::DocumentVersion>(
        "SELECT v.id, v.document_id, v.user_id, v.content, v.created_at, u.username as author_name \
         FROM document_versions v \
         LEFT JOIN users u ON v.user_id = u.id \
         WHERE v.document_id = ? \
         ORDER BY v.created_at DESC"
    )
    .bind(&doc_id)
    .fetch_all(&state.db)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(versions))
}

pub async fn update_comment(
    State(state): State<AppState>,
    Path(comment_id): Path<String>,
    jar: SignedCookieJar,
    Json(payload): Json<UpdateCommentRequest>,
) -> Result<Json<Comment>, (StatusCode, String)> {
    let user_id = jar.get("session_user_id").map(|c| c.value().to_string())
        .ok_or((StatusCode::UNAUTHORIZED, "Not logged in".to_string()))?;

    // Comments on documents the caller cannot open are reported as missing.
    let not_found = || (StatusCode::NOT_FOUND, "Comment not found".to_string());
    let mut comment = fetch_comment(&state, &comment_id).await?.ok_or_else(not_found)?;
    let role = document_role(&state, &comment.document_id, &user_id).await?.ok_or_else(not_found)?;

    // The author may edit and resolve their own comment while they can still write
    // comments; the document owner may resolve/reopen any comment but not reword it.
    let is_author = comment.user_id == user_id && can_write_comments(&role);
    let is_owner = role == "owner";
    if payload.content.is_some() && !is_author {
        return Err((StatusCode::FORBIDDEN, "Only the author can edit a comment".to_string()));
    }
    if payload.resolved.is_some() && !is_author && !is_owner {
        return Err((StatusCode::FORBIDDEN, "Only the author or the document owner can resolve a comment".to_string()));
    }

    if let Some(c) = payload.content {
        comment.content = c;
    }
    if let Some(r) = payload.resolved {
        comment.resolved = r as i64;
    }

    // `(? <> 0)` is a BOOLEAN on Postgres and 0/1 on SQLite.
    sqlx::query("UPDATE comments SET content = ?, resolved = (? <> 0) WHERE id = ?")
        .bind(&comment.content)
        .bind(comment.resolved)
        .bind(&comment.id)
        .execute(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let updated_comment = fetch_comment(&state, &comment.id).await?.ok_or_else(not_found)?;

    Ok(Json(updated_comment))
}

pub async fn delete_comment(
    State(state): State<AppState>,
    Path(comment_id): Path<String>,
    jar: SignedCookieJar,
) -> Result<StatusCode, (StatusCode, String)> {
    let user_id = jar.get("session_user_id").map(|c| c.value().to_string())
        .ok_or((StatusCode::UNAUTHORIZED, "Not logged in".to_string()))?;

    let not_found = || (StatusCode::NOT_FOUND, "Comment not found".to_string());
    let comment = fetch_comment(&state, &comment_id).await?.ok_or_else(not_found)?;
    let role = document_role(&state, &comment.document_id, &user_id).await?.ok_or_else(not_found)?;

    // Authors may delete their own comments; the document owner may moderate any comment.
    let is_author = comment.user_id == user_id && can_write_comments(&role);
    if !is_author && role != "owner" {
        return Err((StatusCode::FORBIDDEN, "Only the author or the document owner can delete a comment".to_string()));
    }

    sqlx::query("DELETE FROM comments WHERE id = ?")
        .bind(&comment_id)
        .execute(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(StatusCode::NO_CONTENT)
}
