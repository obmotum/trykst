use axum::{
    extract::{Multipart, Path, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use axum_extra::extract::cookie::SignedCookieJar;
use futures_util::stream::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use yrs::sync::Awareness;
use yrs::updates::decoder::Decode;
use yrs::{Doc, ReadTxn, Transact, Update};
use yrs_axum::broadcast::BroadcastGroup;
use yrs_axum::ws::AxumSink;

use crate::access::{db_err, not_found, require_document, require_user, session_user, ApiError, Role};
use crate::compiler::{Diagnostics, DocumentStats, ProjectInput};
use crate::documents::{assemble_document, document_files, room_key};
use crate::AppState;

/// Drops document updates sent by read-only participants of a room.
pub struct ViewerFilterStream {
    inner: futures_util::stream::SplitStream<axum::extract::ws::WebSocket>,
    is_viewer: bool,
}

impl Stream for ViewerFilterStream {
    type Item = Result<Vec<u8>, yrs::sync::Error>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        loop {
            match futures_util::ready!(std::pin::Pin::new(&mut self.inner).poll_next(cx)) {
                Some(Ok(msg)) => {
                    if let axum::extract::ws::Message::Binary(bytes) = msg {
                        if self.is_viewer && !bytes.is_empty() && bytes[0] == 0 && bytes.len() > 1 && bytes[1] == 2 {
                            continue; // Skip updates
                        }
                        return std::task::Poll::Ready(Some(Ok(bytes.to_vec())));
                    } else if let axum::extract::ws::Message::Close(_) = msg {
                        return std::task::Poll::Ready(None);
                    }
                    continue;
                }
                Some(Err(e)) => return std::task::Poll::Ready(Some(Err(yrs::sync::Error::Other(Box::new(e))))),
                None => return std::task::Poll::Ready(None),
            }
        }
    }
}


#[derive(Deserialize)]
pub struct CompileRequest {
    /// The document to compile. Without it, `text` is compiled on its own.
    pub document_id: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
    /// Unsaved text by path, taking precedence over the stored files.
    #[serde(default)]
    pub files: Option<HashMap<String, String>>,
}

fn map_diagnostics(diags: Diagnostics) -> Vec<Diagnostic> {
    diags
        .into_iter()
        .map(|(d, range)| Diagnostic {
            message: d.message.to_string(),
            severity: format!("{:?}", d.severity),
            from: range.as_ref().map(|r| r.start),
            to: range.as_ref().map(|r| r.end),
        })
        .collect()
}

#[derive(Serialize)]
pub struct CompileResponse {
    pub svgs: Option<Vec<String>>,
    pub errors: Option<Vec<Diagnostic>>,
    pub stats: Option<DocumentStats>,
}

#[derive(Serialize)]
pub struct Diagnostic {
    pub message: String,
    pub severity: String,
    pub from: Option<usize>,
    pub to: Option<usize>,
}

fn error_response(message: String) -> CompileResponse {
    CompileResponse {
        svgs: None,
        errors: Some(vec![Diagnostic { message, severity: "Error".to_string(), from: None, to: None }]),
        stats: None,
    }
}

/// Real-time collaboration on one text file of a document. Room id:
/// `doc:<document_id>:<node_id>`. Without access to the document there is no
/// room at all (404), so nobody can listen in on other people's edits.
pub async fn yjs_handler(
    ws: axum::extract::ws::WebSocketUpgrade,
    Path(id): Path<String>,
    State(state): State<AppState>,
    jar: SignedCookieJar,
) -> Result<Response, ApiError> {
    let (doc_id, node_id) = id
        .strip_prefix("doc:")
        .and_then(|rest| rest.split_once(':'))
        .map(|(d, n)| (d.to_string(), n.to_string()))
        .ok_or_else(|| not_found("Room"))?;
    let user = session_user(&jar);
    let access = require_document(&state, &doc_id, user.as_deref(), Role::Viewer).await?;
    let is_viewer = !access.role.can_write();

    let row: Option<(String, Option<Vec<u8>>)> =
        sqlx::query_as("SELECT kind, content FROM nodes WHERE id = $1 AND document_id = $2")
            .bind(&node_id)
            .bind(&doc_id)
            .fetch_optional(&state.db)
            .await
            .map_err(db_err)?;
    let initial_content = match row {
        Some((kind, content)) if kind == "text" => content,
        _ => return Err(not_found("File")),
    };

    let key = room_key(&doc_id, &node_id);
    let mut rooms = state.bcast_map.lock().await;
    let group = match rooms.get(&key) {
        Some(group) => group.clone(),
        None => {
            let ydoc = Doc::new();
            if let Some(content) = initial_content {
                if let Ok(update) = Update::decode_v1(&content) {
                    ydoc.transact_mut().apply_update(update);
                }
            }
            let awareness = Arc::new(RwLock::new(Awareness::new(ydoc)));
            let group = Arc::new(BroadcastGroup::new(awareness.clone(), 10).await);
            rooms.insert(key.clone(), group.clone());
            spawn_autosave(state.clone(), key.clone(), group.clone(), node_id.clone(), doc_id.clone());
            group
        }
    };
    drop(rooms);

    Ok(ws.on_upgrade(move |socket| async move {
        let (sink, stream) = socket.split();
        let sink = Arc::new(Mutex::new(AxumSink(sink)));
        let stream = ViewerFilterStream { inner: stream, is_viewer };
        let subscription = group.subscribe(sink, stream);
        if let Err(e) = subscription.completed().await {
            tracing::debug!("collaboration connection ended: {e}");
        }
    }))
}

/// Persists a room every few seconds. Stops once the room is closed or replaced
/// (file deleted, re-uploaded or version restored), so stale content is never
/// written over newer data.
fn spawn_autosave(state: AppState, key: String, group: Arc<BroadcastGroup>, node_id: String, doc_id: String) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
        let mut last_saved: Option<Vec<u8>> = None;
        loop {
            interval.tick().await;
            let still_open = state
                .bcast_map
                .lock()
                .await
                .get(&key)
                .is_some_and(|current| Arc::ptr_eq(current, &group));
            if !still_open {
                break;
            }
            let content = {
                let awareness = group.awareness().read().await;
                let content = awareness.doc().transact().encode_state_as_update_v1(&yrs::StateVector::default());
                content
            };
            if last_saved.as_ref() == Some(&content) {
                continue;
            }
            let now = crate::access::now();
            let saved = sqlx::query("UPDATE nodes SET content = $1, updated_at = $2 WHERE id = $3")
                .bind(&content)
                .bind(&now)
                .bind(&node_id)
                .execute(&state.db)
                .await;
            if saved.is_ok() {
                // The first save after a (re)start only persists; it is not an edit.
                if last_saved.is_some() {
                    let _ = sqlx::query("UPDATE documents SET updated_at = $1 WHERE id = $2")
                        .bind(&now)
                        .bind(&doc_id)
                        .execute(&state.db)
                        .await;
                }
                last_saved = Some(content);
            }
        }
    });
}

/// The compile input for a request, and whether the caller may store a thumbnail.
/// The inner error is a message to show instead of a compile result.
async fn compile_input(
    state: &AppState,
    jar: &SignedCookieJar,
    payload: &CompileRequest,
) -> Result<Result<(ProjectInput, bool), String>, ApiError> {
    let overrides = payload.files.clone().unwrap_or_default();
    match &payload.document_id {
        Some(doc_id) => {
            let user = session_user(jar);
            let access = require_document(state, doc_id, user.as_deref(), Role::Viewer).await?;
            match assemble_document(state, &access.doc, &overrides).await {
                Ok(input) => Ok(Ok((input, access.role.can_write()))),
                Err((StatusCode::UNPROCESSABLE_ENTITY, message)) => Ok(Err(message)),
                Err(e) => Err(e),
            }
        }
        None => {
            require_user(jar)?;
            Ok(Ok((ProjectInput::single(payload.text.clone().unwrap_or_default(), HashMap::new()), false)))
        }
    }
}

pub async fn compile_handler(
    State(state): State<AppState>,
    jar: SignedCookieJar,
    Json(payload): Json<CompileRequest>,
) -> Result<Json<CompileResponse>, ApiError> {
    let (input, can_save) = match compile_input(&state, &jar, &payload).await? {
        Ok(v) => v,
        Err(message) => return Ok(Json(error_response(message))),
    };

    let compiler = state.compiler.lock().await;
    let result = compiler.compile_svg(input);
    drop(compiler);

    Ok(Json(match result {
        Ok((svgs, thumbnail, stats)) => {
            if let (true, Some(doc_id)) = (can_save, &payload.document_id) {
                let _ = sqlx::query("UPDATE documents SET thumbnail_svg = $1 WHERE id = $2")
                    .bind(&thumbnail)
                    .bind(doc_id)
                    .execute(&state.db)
                    .await;
            }
            CompileResponse { svgs: Some(svgs), errors: None, stats: Some(stats) }
        }
        Err(diags) => CompileResponse { svgs: None, errors: Some(map_diagnostics(diags)), stats: None },
    }))
}

pub async fn export_handler(
    State(state): State<AppState>,
    jar: SignedCookieJar,
    Path(format): Path<String>,
    Json(payload): Json<CompileRequest>,
) -> Result<Response, ApiError> {
    let input = match compile_input(&state, &jar, &payload).await? {
        Ok((input, _)) => input,
        Err(message) => return Ok((StatusCode::UNPROCESSABLE_ENTITY, message).into_response()),
    };
    let compiler = state.compiler.lock().await;

    Ok(match format.as_str() {
        "pdf" => match compiler.export_pdf(input) {
            Ok(bytes) => ([(header::CONTENT_TYPE, "application/pdf")], bytes).into_response(),
            Err(_) => (StatusCode::BAD_REQUEST, "Compilation failed").into_response(),
        },
        "png" => match compiler.export_png(input) {
            Ok(bytes) => ([(header::CONTENT_TYPE, "image/png")], bytes).into_response(),
            Err(_) => (StatusCode::BAD_REQUEST, "Compilation failed").into_response(),
        },
        "svg" => match compiler.compile_svg(input) {
            Ok((svgs, _, _)) => {
                let combined: String = svgs.into_iter().map(|svg| svg + "\n").collect();
                ([(header::CONTENT_TYPE, "image/svg+xml")], combined.into_bytes()).into_response()
            }
            Err(_) => (StatusCode::BAD_REQUEST, "Compilation failed").into_response(),
        },
        _ => (StatusCode::NOT_FOUND, "Format not supported").into_response(),
    })
}

use std::process::Stdio;
use tokio::process::Command;

pub async fn pandoc_export_handler(
    Path(format): Path<String>,
    Json(payload): Json<CompileRequest>,
) -> impl IntoResponse {
    let supported_formats = ["docx", "latex", "markdown", "html"];
    if !supported_formats.contains(&format.as_str()) {
        return (StatusCode::BAD_REQUEST, "Unsupported format").into_response();
    }

    let _ext = match format.as_str() {
        "latex" => "tex",
        "markdown" => "md",
        f => f,
    };

    let mut child = match Command::new("pandoc")
        .arg("-f")
        .arg("typst")
        .arg("-t")
        .arg(&format)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to start pandoc: {}", e)).into_response(),
    };

    let mut stdin = child.stdin.take().unwrap();
    let text = payload.text.clone().unwrap_or_default();
    tokio::spawn(async move {
        use tokio::io::AsyncWriteExt;
        let _ = stdin.write_all(text.as_bytes()).await;
    });

    let output = match child.wait_with_output().await {
        Ok(o) => o,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, format!("Pandoc failed: {}", e)).into_response(),
    };

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return (StatusCode::BAD_REQUEST, format!("Pandoc error: {}", err)).into_response();
    }

    let content_type = match format.as_str() {
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "html" => "text/html",
        "latex" => "application/x-latex",
        "markdown" => "text/markdown",
        _ => "application/octet-stream",
    };

    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, content_type)],
        output.stdout,
    )
        .into_response()
}

pub async fn pandoc_import_handler(
    mut multipart: Multipart,
) -> impl IntoResponse {
    let mut file_data = Vec::new();
    let mut file_ext = String::new();

    if let Some(field) = multipart.next_field().await.unwrap_or(None) {
        if let Some(file_name) = field.file_name() {
            if file_name.ends_with(".docx") {
                file_ext = "docx".to_string();
            } else if file_name.ends_with(".tex") {
                file_ext = "latex".to_string();
            } else if file_name.ends_with(".md") {
                file_ext = "markdown".to_string();
            } else if file_name.ends_with(".html") {
                file_ext = "html".to_string();
            } else {
                file_ext = "markdown".to_string();
            }
        }
        if let Ok(bytes) = field.bytes().await {
            file_data = bytes.to_vec();
        }
    }

    if file_data.is_empty() {
        return (StatusCode::BAD_REQUEST, "No file uploaded").into_response();
    }

    let mut child = match Command::new("pandoc")
        .arg("-f")
        .arg(&file_ext)
        .arg("-t")
        .arg("typst")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to start pandoc: {}", e)).into_response(),
    };

    let mut stdin = child.stdin.take().unwrap();
    tokio::spawn(async move {
        use tokio::io::AsyncWriteExt;
        let _ = stdin.write_all(&file_data).await;
    });

    let output = match child.wait_with_output().await {
        Ok(o) => o,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, format!("Pandoc failed: {}", e)).into_response(),
    };

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return (StatusCode::BAD_REQUEST, format!("Pandoc error: {}", err)).into_response();
    }

    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        output.stdout,
    )
        .into_response()
}


/// Language server for a document: tinymist runs on a copy of all its files.
pub async fn lsp_handler(
    ws: axum::extract::ws::WebSocketUpgrade,
    Path(id): Path<String>,
    State(state): State<AppState>,
    jar: SignedCookieJar,
) -> Result<Response, ApiError> {
    let user = session_user(&jar);
    let access = require_document(&state, &id, user.as_deref(), Role::Viewer).await?;
    let (_, files_map) = document_files(&state, &access.doc, &HashMap::new()).await?;

    Ok(ws.on_upgrade(move |socket| async move {
        use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};
        use tokio::process::Command;
        use std::process::Stdio;

        let temp_dir = tempfile::tempdir().unwrap();

        for (name, data) in files_map {
            let path = temp_dir.path().join(&name);
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(&path, data);
        }

        let mut child = Command::new("tinymist")
            .arg("lsp")
            .arg("--font-path")
            .arg(temp_dir.path())
            .current_dir(temp_dir.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("Failed to start tinymist lsp");

        let mut stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let mut stdout_reader = BufReader::new(stdout);

        let (mut ws_tx, mut ws_rx) = socket.split();

        let root_uri = format!("file://{}", temp_dir.path().display());
        let init_msg = serde_json::json!({
            "type": "init",
            "rootUri": root_uri
        });
        use futures_util::SinkExt;
        let _ = ws_tx.send(axum::extract::ws::Message::Text(init_msg.to_string().into())).await;

        let ws_to_lsp = tokio::spawn(async move {
            while let Some(Ok(axum::extract::ws::Message::Text(msg))) = ws_rx.next().await {
                let content_length = format!("Content-Length: {}\r\n\r\n", msg.len());
                if stdin.write_all(content_length.as_bytes()).await.is_err() {
                    break;
                }
                if stdin.write_all(msg.as_bytes()).await.is_err() {
                    break;
                }
            }
        });

        let lsp_to_ws = tokio::spawn(async move {
            loop {
                let mut content_length = 0;
                let mut header = String::new();
                loop {
                    let mut char_buf = [0; 1];
                    if stdout_reader.read_exact(&mut char_buf).await.is_err() {
                        return;
                    }
                    header.push(char_buf[0] as char);
                    if header.ends_with("\r\n\r\n") {
                        break;
                    }
                }

                for line in header.split("\r\n") {
                    if let Some(value) = line.strip_prefix("Content-Length: ") {
                        if let Ok(len) = value.trim().parse::<usize>() {
                            content_length = len;
                        }
                    }
                }

                if content_length == 0 { continue; }

                let mut body = vec![0; content_length];
                if stdout_reader.read_exact(&mut body).await.is_err() {
                    break;
                }

                if let Ok(text) = String::from_utf8(body) {
                    use futures_util::SinkExt;
                    if ws_tx.send(axum::extract::ws::Message::Text(text.into())).await.is_err() {
                        break;
                    }
                }
            }
        });

        tokio::select! {
            _ = ws_to_lsp => {}
            _ = lsp_to_ws => {}
            _ = child.wait() => {}
        }
    }))
}
