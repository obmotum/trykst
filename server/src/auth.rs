use axum::{
    extract::State,
    http::StatusCode,
    Json,
};
use axum_extra::extract::cookie::SignedCookieJar;

use crate::{
    models::{User, StorageStats},
    AppState,
};

pub async fn me(
    State(state): State<AppState>,
    jar: SignedCookieJar,
) -> Result<Json<User>, (StatusCode, String)> {
    let user_id = match jar.get("session_user_id").map(|c| c.value().to_string()) {
        Some(id) => id,
        None => return Err((StatusCode::UNAUTHORIZED, "Not logged in".to_string())),
    };

    let user = sqlx::query_as::<_, User>("SELECT id, username, email, is_admin FROM users WHERE id = ?")
        .bind(&user_id)
        .fetch_optional(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    match user {
        Some(u) => Ok(Json(u)),
        None => Err((StatusCode::UNAUTHORIZED, "User not found".to_string())),
    }
}

pub async fn storage_stats(
    State(state): State<AppState>,
    jar: SignedCookieJar,
) -> Result<Json<StorageStats>, (StatusCode, String)> {
    let user_id = jar.get("session_user_id").map(|c| c.value().to_string())
        .ok_or((StatusCode::UNAUTHORIZED, "Not logged in".to_string()))?;

    // LENGTH() returns byte count for both BYTEA (Postgres) and BLOB (SQLite)
    let docs_size: (i64,) = sqlx::query_as(
        "SELECT COALESCE(SUM(LENGTH(content)), 0) FROM documents WHERE owner_id = ?"
    )
    .bind(&user_id)
    .fetch_one(&state.db)
    .await
    .unwrap_or((0,));

    let files_size: (i64,) = sqlx::query_as(
        "SELECT COALESCE(SUM(LENGTH(data)), 0) FROM files WHERE owner_id = ?"
    )
    .bind(&user_id)
    .fetch_one(&state.db)
    .await
    .unwrap_or((0,));

    let stats = StorageStats {
        documents_size_bytes: docs_size.0,
        files_size_bytes: files_size.0,
        total_size_bytes: docs_size.0 + files_size.0,
    };

    Ok(Json(stats))
}
