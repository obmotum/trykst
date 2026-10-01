//! Who may do what. Permissions live on projects only: a project member has the
//! role of their membership on every document of the project. Everyone else can
//! at most use a document's link sharing (`documents.public_role`).
//!
//! Callers without access get 404, not 403, so ids do not reveal what exists.

use axum::http::StatusCode;
use axum_extra::extract::cookie::SignedCookieJar;

use crate::{models::Document, AppState};

pub type ApiError = (StatusCode, String);

pub fn db_err<E: std::fmt::Display>(e: E) -> ApiError {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

/// Timestamp in the format of the column defaults ("YYYY-MM-DD HH:MM:SS", UTC).
/// SQL's CURRENT_TIMESTAMP renders differently on SQLite and Postgres.
pub fn now() -> String {
    chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

pub fn not_found(what: &str) -> ApiError {
    (StatusCode::NOT_FOUND, format!("{what} not found"))
}

pub fn forbidden(message: &str) -> ApiError {
    (StatusCode::FORBIDDEN, message.to_string())
}

/// The signed-in user, if any (validated by the session guard).
pub fn session_user(jar: &SignedCookieJar) -> Option<String> {
    jar.get(crate::oidc::USER_COOKIE).map(|c| c.value().to_string())
}

pub fn require_user(jar: &SignedCookieJar) -> Result<String, ApiError> {
    session_user(jar).ok_or((StatusCode::UNAUTHORIZED, "Not logged in".to_string()))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Role {
    Viewer,
    Editor,
    Owner,
}

impl Role {
    pub fn parse(role: &str) -> Option<Role> {
        match role {
            "viewer" => Some(Role::Viewer),
            "editor" => Some(Role::Editor),
            "owner" => Some(Role::Owner),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Role::Viewer => "viewer",
            Role::Editor => "editor",
            Role::Owner => "owner",
        }
    }

    pub fn can_write(self) -> bool {
        self >= Role::Editor
    }
}

pub async fn project_role(state: &AppState, project_id: &str, user_id: &str) -> Result<Option<Role>, ApiError> {
    let role: Option<String> =
        sqlx::query_scalar("SELECT role FROM project_members WHERE project_id = $1 AND user_id = $2")
            .bind(project_id)
            .bind(user_id)
            .fetch_optional(&state.db)
            .await
            .map_err(db_err)?;
    Ok(role.as_deref().and_then(Role::parse))
}

/// The caller's project role if it is at least `min`. Non-members get 404.
pub async fn require_project_role(
    state: &AppState,
    project_id: &str,
    user_id: &str,
    min: Role,
) -> Result<Role, ApiError> {
    let role = project_role(state, project_id, user_id).await?.ok_or_else(|| not_found("Project"))?;
    if role < min {
        return Err(forbidden(match min {
            Role::Owner => "Only project owners can do this",
            _ => "Viewers cannot change this project",
        }));
    }
    Ok(role)
}

pub const DOCUMENT_COLUMNS: &str =
    "id, project_id, title, entrypoint_id, thumbnail_svg, public_role, created_by, created_at, updated_at";

pub struct DocAccess {
    pub doc: Document,
    pub role: Role,
    /// True when the role comes from the project membership, false for link sharing.
    pub is_member: bool,
}

/// The caller's access to a document: their project role, else the document's
/// link sharing. `None` when the document does not exist or is not accessible.
pub async fn document_access(
    state: &AppState,
    doc_id: &str,
    user_id: Option<&str>,
) -> Result<Option<DocAccess>, ApiError> {
    let doc: Option<Document> = sqlx::query_as(&format!("SELECT {DOCUMENT_COLUMNS} FROM documents WHERE id = $1"))
        .bind(doc_id)
        .fetch_optional(&state.db)
        .await
        .map_err(db_err)?;
    let Some(mut doc) = doc else { return Ok(None) };

    if let Some(uid) = user_id {
        if let Some(role) = project_role(state, &doc.project_id, uid).await? {
            doc.role = Some(role.as_str().to_string());
            return Ok(Some(DocAccess { doc, role, is_member: true }));
        }
    }

    // Link sharing never grants ownership.
    match doc.public_role.as_deref().and_then(Role::parse).filter(|r| *r != Role::Owner) {
        Some(role) => {
            doc.role = Some(role.as_str().to_string());
            Ok(Some(DocAccess { doc, role, is_member: false }))
        }
        None => Ok(None),
    }
}

/// Like [`document_access`], but requires at least `min` and turns "no access" into 404.
pub async fn require_document(
    state: &AppState,
    doc_id: &str,
    user_id: Option<&str>,
    min: Role,
) -> Result<DocAccess, ApiError> {
    let access = document_access(state, doc_id, user_id).await?.ok_or_else(|| not_found("Document"))?;
    if access.role < min {
        return Err(forbidden("Viewers cannot change this document"));
    }
    Ok(access)
}
