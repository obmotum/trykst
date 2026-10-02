//! Comments on documents, optionally attached to one file of the document.
//! Everyone who can open a document reads its comments; writers (project
//! editors and owners, or link-sharing editors) add them. Authors edit, resolve
//! and delete their own comments; project owners may resolve or delete any.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use axum_extra::extract::cookie::SignedCookieJar;
use uuid::Uuid;

use crate::{
    access::{db_err, document_access, forbidden, not_found, require_document, require_user, ApiError, Role},
    models::{Comment, CreateCommentRequest, UpdateCommentRequest},
    AppState,
};

const COMMENT_COLUMNS: &str = "c.id, c.document_id, c.node_id, c.user_id, c.content, c.resolved, c.created_at, \
     u.username AS author_name, u.avatar_url AS author_avatar_url";

async fn fetch_comment(state: &AppState, comment_id: &str) -> Result<Option<Comment>, ApiError> {
    sqlx::query_as(&format!(
        "SELECT {COMMENT_COLUMNS} FROM comments c LEFT JOIN users u ON u.id = c.user_id WHERE c.id = $1"
    ))
    .bind(comment_id)
    .fetch_optional(&state.db)
    .await
    .map_err(db_err)
}

pub async fn list_comments(
    State(state): State<AppState>,
    Path(doc_id): Path<String>,
    jar: SignedCookieJar,
) -> Result<Json<Vec<Comment>>, ApiError> {
    let user_id = require_user(&jar)?;
    require_document(&state, &doc_id, Some(&user_id), Role::Viewer).await?;
    let comments = sqlx::query_as(&format!(
        "SELECT {COMMENT_COLUMNS} FROM comments c LEFT JOIN users u ON u.id = c.user_id \
         WHERE c.document_id = $1 ORDER BY c.created_at ASC"
    ))
    .bind(&doc_id)
    .fetch_all(&state.db)
    .await
    .map_err(db_err)?;
    Ok(Json(comments))
}

pub async fn add_comment(
    State(state): State<AppState>,
    Path(doc_id): Path<String>,
    jar: SignedCookieJar,
    Json(payload): Json<CreateCommentRequest>,
) -> Result<(StatusCode, Json<Comment>), ApiError> {
    let user_id = require_user(&jar)?;
    let access = require_document(&state, &doc_id, Some(&user_id), Role::Viewer).await?;
    if !access.role.can_write() {
        return Err(forbidden("Viewers cannot add comments"));
    }
    if payload.content.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, "Comment must not be empty".to_string()));
    }
    if let Some(node_id) = &payload.node_id {
        let belongs: Option<String> = sqlx::query_scalar("SELECT id FROM nodes WHERE id = $1 AND document_id = $2")
            .bind(node_id)
            .bind(&doc_id)
            .fetch_optional(&state.db)
            .await
            .map_err(db_err)?;
        belongs.ok_or_else(|| not_found("File"))?;
    }

    let comment_id = Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO comments (id, document_id, node_id, user_id, content) VALUES ($1, $2, $3, $4, $5)")
        .bind(&comment_id)
        .bind(&doc_id)
        .bind(&payload.node_id)
        .bind(&user_id)
        .bind(payload.content.trim())
        .execute(&state.db)
        .await
        .map_err(db_err)?;

    let comment = fetch_comment(&state, &comment_id).await?.ok_or_else(|| not_found("Comment"))?;
    Ok((StatusCode::CREATED, Json(comment)))
}

/// The comment and the caller's role on its document; 404 without access.
async fn comment_with_role(state: &AppState, comment_id: &str, user_id: &str) -> Result<(Comment, Role), ApiError> {
    let comment = fetch_comment(state, comment_id).await?.ok_or_else(|| not_found("Comment"))?;
    let access = document_access(state, &comment.document_id, Some(user_id))
        .await?
        .ok_or_else(|| not_found("Comment"))?;
    Ok((comment, access.role))
}

pub async fn update_comment(
    State(state): State<AppState>,
    Path(comment_id): Path<String>,
    jar: SignedCookieJar,
    Json(payload): Json<UpdateCommentRequest>,
) -> Result<Json<Comment>, ApiError> {
    let user_id = require_user(&jar)?;
    let (comment, role) = comment_with_role(&state, &comment_id, &user_id).await?;

    let is_author = comment.user_id == user_id && role.can_write();
    if payload.content.is_some() && !is_author {
        return Err(forbidden("Only the author can edit a comment"));
    }
    if payload.resolved.is_some() && !is_author && role != Role::Owner {
        return Err(forbidden("Only the author or a project owner can resolve a comment"));
    }

    if let Some(content) = &payload.content {
        if content.trim().is_empty() {
            return Err((StatusCode::BAD_REQUEST, "Comment must not be empty".to_string()));
        }
        sqlx::query("UPDATE comments SET content = $1 WHERE id = $2")
            .bind(content.trim())
            .bind(&comment_id)
            .execute(&state.db)
            .await
            .map_err(db_err)?;
    }
    if let Some(resolved) = payload.resolved {
        sqlx::query("UPDATE comments SET resolved = $1 WHERE id = $2")
            .bind(resolved as i64)
            .bind(&comment_id)
            .execute(&state.db)
            .await
            .map_err(db_err)?;
    }

    Ok(Json(fetch_comment(&state, &comment_id).await?.ok_or_else(|| not_found("Comment"))?))
}

pub async fn delete_comment(
    State(state): State<AppState>,
    Path(comment_id): Path<String>,
    jar: SignedCookieJar,
) -> Result<StatusCode, ApiError> {
    let user_id = require_user(&jar)?;
    let (comment, role) = comment_with_role(&state, &comment_id, &user_id).await?;
    let is_author = comment.user_id == user_id && role.can_write();
    if !is_author && role != Role::Owner {
        return Err(forbidden("Only the author or a project owner can delete a comment"));
    }
    sqlx::query("DELETE FROM comments WHERE id = $1")
        .bind(&comment_id)
        .execute(&state.db)
        .await
        .map_err(db_err)?;
    Ok(StatusCode::NO_CONTENT)
}
