//! Projects and their members. A project is the unit of sharing: members get
//! their role (owner, editor, viewer) on every document in it. Every project
//! keeps at least one owner.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use axum_extra::extract::cookie::SignedCookieJar;
use uuid::Uuid;

use crate::{
    access::{db_err, forbidden, not_found, now, require_project_role, require_user, ApiError, Role},
    models::{AddMemberRequest, CreateProjectRequest, Project, ProjectMember, UpdateMemberRequest, UpdateProjectRequest},
    AppState,
};

const PROJECT_COLUMNS: &str = "p.id, p.name, p.description, p.created_by, p.created_at, p.updated_at, m.role, \
     (SELECT COUNT(*) FROM documents d WHERE d.project_id = p.id) AS document_count, \
     (SELECT COUNT(*) FROM project_members pm WHERE pm.project_id = p.id) AS member_count";

fn valid_name(name: &str) -> Result<String, ApiError> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 200 {
        return Err((StatusCode::BAD_REQUEST, "Name must be 1 to 200 characters".to_string()));
    }
    Ok(name.to_string())
}

fn member_role(role: &str) -> Result<Role, ApiError> {
    Role::parse(role).ok_or((StatusCode::BAD_REQUEST, "Role must be owner, editor or viewer".to_string()))
}

async fn fetch_project(state: &AppState, project_id: &str, user_id: &str) -> Result<Project, ApiError> {
    sqlx::query_as(&format!(
        "SELECT {PROJECT_COLUMNS} FROM projects p \
         JOIN project_members m ON m.project_id = p.id AND m.user_id = $1 \
         WHERE p.id = $2"
    ))
    .bind(user_id)
    .bind(project_id)
    .fetch_optional(&state.db)
    .await
    .map_err(db_err)?
    .ok_or_else(|| not_found("Project"))
}

pub async fn list_projects(
    State(state): State<AppState>,
    jar: SignedCookieJar,
) -> Result<Json<Vec<Project>>, ApiError> {
    let user_id = require_user(&jar)?;
    let projects = sqlx::query_as(&format!(
        "SELECT {PROJECT_COLUMNS} FROM projects p \
         JOIN project_members m ON m.project_id = p.id AND m.user_id = $1 \
         ORDER BY p.updated_at DESC"
    ))
    .bind(&user_id)
    .fetch_all(&state.db)
    .await
    .map_err(db_err)?;
    Ok(Json(projects))
}

pub async fn create_project(
    State(state): State<AppState>,
    jar: SignedCookieJar,
    Json(payload): Json<CreateProjectRequest>,
) -> Result<(StatusCode, Json<Project>), ApiError> {
    let user_id = require_user(&jar)?;
    // Guests work inside projects they were added to, but cannot open new ones.
    crate::auth::require_member(&state, &user_id).await?;
    let name = valid_name(&payload.name)?;

    let project_id = Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO projects (id, name, description, created_by) VALUES ($1, $2, $3, $4)")
        .bind(&project_id)
        .bind(&name)
        .bind(payload.description.as_deref().map(str::trim).filter(|d| !d.is_empty()))
        .bind(&user_id)
        .execute(&state.db)
        .await
        .map_err(db_err)?;
    sqlx::query("INSERT INTO project_members (id, project_id, user_id, role) VALUES ($1, $2, $3, 'owner')")
        .bind(Uuid::new_v4().to_string())
        .bind(&project_id)
        .bind(&user_id)
        .execute(&state.db)
        .await
        .map_err(db_err)?;

    Ok((StatusCode::CREATED, Json(fetch_project(&state, &project_id, &user_id).await?)))
}

pub async fn get_project(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
    jar: SignedCookieJar,
) -> Result<Json<Project>, ApiError> {
    let user_id = require_user(&jar)?;
    Ok(Json(fetch_project(&state, &project_id, &user_id).await?))
}

pub async fn update_project(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
    jar: SignedCookieJar,
    Json(payload): Json<UpdateProjectRequest>,
) -> Result<Json<Project>, ApiError> {
    let user_id = require_user(&jar)?;
    require_project_role(&state, &project_id, &user_id, Role::Owner).await?;

    if let Some(name) = &payload.name {
        sqlx::query("UPDATE projects SET name = $1, updated_at = $2 WHERE id = $3")
            .bind(valid_name(name)?)
            .bind(now())
            .bind(&project_id)
            .execute(&state.db)
            .await
            .map_err(db_err)?;
    }
    if let Some(description) = &payload.description {
        sqlx::query("UPDATE projects SET description = $1, updated_at = $2 WHERE id = $3")
            .bind(Some(description.trim()).filter(|d| !d.is_empty()))
            .bind(now())
            .bind(&project_id)
            .execute(&state.db)
            .await
            .map_err(db_err)?;
    }
    Ok(Json(fetch_project(&state, &project_id, &user_id).await?))
}

pub async fn delete_project(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
    jar: SignedCookieJar,
) -> Result<StatusCode, ApiError> {
    let user_id = require_user(&jar)?;
    require_project_role(&state, &project_id, &user_id, Role::Owner).await?;
    // Documents, their files, comments, versions and project packages cascade.
    sqlx::query("DELETE FROM projects WHERE id = $1")
        .bind(&project_id)
        .execute(&state.db)
        .await
        .map_err(db_err)?;
    Ok(StatusCode::NO_CONTENT)
}

// --- Members ---------------------------------------------------------------------

pub async fn list_members(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
    jar: SignedCookieJar,
) -> Result<Json<Vec<ProjectMember>>, ApiError> {
    let user_id = require_user(&jar)?;
    require_project_role(&state, &project_id, &user_id, Role::Viewer).await?;
    let members = sqlx::query_as(
        "SELECT u.id AS user_id, u.username, u.email, u.avatar_url, u.is_guest, m.role, m.created_at \
         FROM project_members m JOIN users u ON u.id = m.user_id \
         WHERE m.project_id = $1 \
         ORDER BY CASE m.role WHEN 'owner' THEN 0 WHEN 'editor' THEN 1 ELSE 2 END, u.username",
    )
    .bind(&project_id)
    .fetch_all(&state.db)
    .await
    .map_err(db_err)?;
    Ok(Json(members))
}

async fn owner_count(state: &AppState, project_id: &str) -> Result<i64, ApiError> {
    sqlx::query_scalar("SELECT COUNT(*) FROM project_members WHERE project_id = $1 AND role = 'owner'")
        .bind(project_id)
        .fetch_one(&state.db)
        .await
        .map_err(db_err)
}

async fn current_role(state: &AppState, project_id: &str, member_id: &str) -> Result<Role, ApiError> {
    crate::access::project_role(state, project_id, member_id)
        .await?
        .ok_or_else(|| not_found("Member"))
}

pub async fn add_member(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
    jar: SignedCookieJar,
    Json(payload): Json<AddMemberRequest>,
) -> Result<(StatusCode, Json<ProjectMember>), ApiError> {
    let user_id = require_user(&jar)?;
    require_project_role(&state, &project_id, &user_id, Role::Owner).await?;
    let role = member_role(&payload.role)?;

    // Finds the account, creating a placeholder for people who never signed in.
    let member_id = crate::oidc::resolve_invitee(&state, payload.subject.as_deref(), payload.email.as_deref()).await?;
    if member_id == user_id {
        return Err((StatusCode::BAD_REQUEST, "You are already a member of this project".to_string()));
    }
    if crate::access::project_role(&state, &project_id, &member_id).await?.is_some() {
        return Err((StatusCode::CONFLICT, "This person is already a member; change their role instead".to_string()));
    }

    sqlx::query("INSERT INTO project_members (id, project_id, user_id, role) VALUES ($1, $2, $3, $4)")
        .bind(Uuid::new_v4().to_string())
        .bind(&project_id)
        .bind(&member_id)
        .bind(role.as_str())
        .execute(&state.db)
        .await
        .map_err(db_err)?;

    let member = fetch_member(&state, &project_id, &member_id).await?;
    Ok((StatusCode::CREATED, Json(member)))
}

async fn fetch_member(state: &AppState, project_id: &str, member_id: &str) -> Result<ProjectMember, ApiError> {
    sqlx::query_as(
        "SELECT u.id AS user_id, u.username, u.email, u.avatar_url, u.is_guest, m.role, m.created_at \
         FROM project_members m JOIN users u ON u.id = m.user_id \
         WHERE m.project_id = $1 AND m.user_id = $2",
    )
    .bind(project_id)
    .bind(member_id)
    .fetch_optional(&state.db)
    .await
    .map_err(db_err)?
    .ok_or_else(|| not_found("Member"))
}

pub async fn update_member(
    State(state): State<AppState>,
    Path((project_id, member_id)): Path<(String, String)>,
    jar: SignedCookieJar,
    Json(payload): Json<UpdateMemberRequest>,
) -> Result<Json<ProjectMember>, ApiError> {
    let user_id = require_user(&jar)?;
    require_project_role(&state, &project_id, &user_id, Role::Owner).await?;
    let new_role = member_role(&payload.role)?;
    let old_role = current_role(&state, &project_id, &member_id).await?;

    if old_role == Role::Owner && new_role != Role::Owner && owner_count(&state, &project_id).await? <= 1 {
        return Err((StatusCode::CONFLICT, "A project needs at least one owner".to_string()));
    }

    sqlx::query("UPDATE project_members SET role = $1 WHERE project_id = $2 AND user_id = $3")
        .bind(new_role.as_str())
        .bind(&project_id)
        .bind(&member_id)
        .execute(&state.db)
        .await
        .map_err(db_err)?;
    Ok(Json(fetch_member(&state, &project_id, &member_id).await?))
}

/// Owners remove members; any member may leave on their own.
pub async fn remove_member(
    State(state): State<AppState>,
    Path((project_id, member_id)): Path<(String, String)>,
    jar: SignedCookieJar,
) -> Result<StatusCode, ApiError> {
    let user_id = require_user(&jar)?;
    let caller = require_project_role(&state, &project_id, &user_id, Role::Viewer).await?;
    if member_id != user_id && caller != Role::Owner {
        return Err(forbidden("Only project owners can remove members"));
    }
    let role = current_role(&state, &project_id, &member_id).await?;
    if role == Role::Owner && owner_count(&state, &project_id).await? <= 1 {
        return Err((StatusCode::CONFLICT, "A project needs at least one owner".to_string()));
    }

    sqlx::query("DELETE FROM project_members WHERE project_id = $1 AND user_id = $2")
        .bind(&project_id)
        .bind(&member_id)
        .execute(&state.db)
        .await
        .map_err(db_err)?;
    Ok(StatusCode::NO_CONTENT)
}
