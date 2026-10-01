use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use axum_extra::extract::cookie::SignedCookieJar;

use crate::{
    models::{AdminUserView, UpdateUserRequest},
    AppState,
};

async fn require_admin(state: &AppState, jar: &SignedCookieJar) -> Result<String, (StatusCode, String)> {
    let user_id = jar.get("session_user_id").map(|c| c.value().to_string())
        .ok_or((StatusCode::UNAUTHORIZED, "Not logged in".to_string()))?;

    let is_admin: Option<(i64,)> = sqlx::query_as("SELECT is_admin FROM users WHERE id = $1")
        .bind(&user_id)
        .fetch_optional(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    match is_admin {
        Some((v,)) if v != 0 => Ok(user_id),
        _ => Err((StatusCode::FORBIDDEN, "Admin access required".to_string())),
    }
}

pub async fn list_users(
    State(state): State<AppState>,
    jar: SignedCookieJar,
) -> Result<Json<Vec<AdminUserView>>, (StatusCode, String)> {
    require_admin(&state, &jar).await?;

    let users = sqlx::query_as::<_, AdminUserView>(
        "SELECT id, username, email, is_admin, is_guest, avatar_url, created_at FROM users ORDER BY created_at ASC"
    )
    .fetch_all(&state.db)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(users))
}

pub async fn update_user(
    State(state): State<AppState>,
    jar: SignedCookieJar,
    Path(user_id): Path<String>,
    Json(payload): Json<UpdateUserRequest>,
) -> Result<Json<AdminUserView>, (StatusCode, String)> {
    let requester_id = require_admin(&state, &jar).await?;

    if let Some(is_admin) = payload.is_admin {
        if state.oidc.config.admin_role.is_some() {
            return Err((StatusCode::CONFLICT, "Admin rights are managed by the identity provider (OIDC_ADMIN_ROLE)".to_string()));
        }
        if is_admin {
            let guest: Option<(i64,)> = sqlx::query_as("SELECT is_guest FROM users WHERE id = $1")
                .bind(&user_id)
                .fetch_optional(&state.db)
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
            if matches!(guest, Some((g,)) if g != 0) {
                return Err((StatusCode::BAD_REQUEST, "Guests cannot be administrators".to_string()));
            }
        }
        if !is_admin && requester_id == user_id {
            return Err((StatusCode::BAD_REQUEST, "Cannot remove your own admin privileges".to_string()));
        }
        sqlx::query("UPDATE users SET is_admin = $1 WHERE id = $2")
            .bind(if is_admin { 1i64 } else { 0i64 })
            .bind(&user_id)
            .execute(&state.db)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }

    let user = sqlx::query_as::<_, AdminUserView>(
        "SELECT id, username, email, is_admin, is_guest, avatar_url, created_at FROM users WHERE id = $1"
    )
    .bind(&user_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .ok_or((StatusCode::NOT_FOUND, "User not found".to_string()))?;

    Ok(Json(user))
}

pub async fn delete_user(
    State(state): State<AppState>,
    jar: SignedCookieJar,
    Path(user_id): Path<String>,
) -> Result<StatusCode, (StatusCode, String)> {
    let requester_id = require_admin(&state, &jar).await?;

    if requester_id == user_id {
        return Err((StatusCode::BAD_REQUEST, "Cannot delete your own account via admin panel".to_string()));
    }

    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(&user_id)
        .execute(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(StatusCode::NO_CONTENT)
}
