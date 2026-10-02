//! Typst packages published from documents:
//! - project packages, importable as `@project/<name>:<version>` by the documents
//!   of the same project only; any project editor may publish them
//! - instance packages, importable as `@trykst/<name>:<version>` everywhere
//!   (`@typstdrive/...` still resolves); only admins publish them
//!
//! Versions are immutable.

use std::collections::HashMap;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use axum_extra::extract::cookie::SignedCookieJar;
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    access::{db_err, forbidden, not_found, require_document, require_project_role, require_user, ApiError, Role},
    models::{Package, PackageVersion, PublishPackageRequest},
    AppState,
};

/// Namespace for packages of the current project.
pub const PROJECT_NAMESPACE: &str = "project";
/// Namespace for instance-wide packages.
pub const INSTANCE_NAMESPACE: &str = "trykst";

#[derive(Deserialize)]
struct Manifest {
    package: PackageMeta,
}

#[derive(Deserialize)]
struct PackageMeta {
    name: String,
    version: String,
    entrypoint: Option<String>,
    description: Option<String>,
}

fn is_valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

fn is_valid_version(version: &str) -> bool {
    let parts: Vec<&str> = version.split('.').collect();
    parts.len() == 3 && parts.iter().all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
}

pub async fn is_admin(state: &AppState, user_id: &str) -> Result<bool, ApiError> {
    let flag: Option<i64> = sqlx::query_scalar("SELECT is_admin FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_optional(&state.db)
        .await
        .map_err(db_err)?;
    Ok(flag.unwrap_or(0) != 0)
}

/// Packages visible to documents of `project_id`, keyed "namespace/name:version".
pub async fn load_packages(state: &AppState, project_id: &str) -> HashMap<String, HashMap<String, Vec<u8>>> {
    let rows = sqlx::query_as::<_, (Option<String>, String, String, String, Vec<u8>)>(
        "SELECT p.project_id, p.name, v.version, f.path, f.data \
         FROM package_files f \
         JOIN package_versions v ON v.id = f.version_id \
         JOIN packages p ON p.id = v.package_id \
         WHERE p.project_id IS NULL OR p.project_id = $1",
    )
    .bind(project_id)
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();

    let mut packages: HashMap<String, HashMap<String, Vec<u8>>> = HashMap::new();
    for (owner_project, name, version, path, data) in rows {
        let namespace = if owner_project.is_some() { PROJECT_NAMESPACE } else { INSTANCE_NAMESPACE };
        packages.entry(format!("{namespace}/{name}:{version}")).or_default().insert(path, data);
    }
    packages
}

const PACKAGE_COLUMNS: &str = "p.id, p.project_id, p.owner_id, p.name, p.description, p.created_at, \
     u.username AS owner_name, \
     (SELECT v.version FROM package_versions v WHERE v.package_id = p.id ORDER BY v.created_at DESC LIMIT 1) AS latest_version";

pub async fn publish_package(
    State(state): State<AppState>,
    jar: SignedCookieJar,
    Json(payload): Json<PublishPackageRequest>,
) -> Result<(StatusCode, Json<Package>), ApiError> {
    let user_id = require_user(&jar)?;
    let access = require_document(&state, &payload.document_id, Some(&user_id), Role::Editor).await?;
    if !access.is_member {
        return Err(forbidden("Only project members can publish packages"));
    }
    let instance_wide = match payload.scope.as_deref().unwrap_or("project") {
        "project" => false,
        "instance" => true,
        _ => return Err((StatusCode::BAD_REQUEST, "scope must be project or instance".to_string())),
    };
    if instance_wide && !is_admin(&state, &user_id).await? {
        return Err(forbidden("Only admins can publish instance-wide packages"));
    }

    let (_, files) = crate::documents::document_files(&state, &access.doc, &HashMap::new()).await?;
    let manifest_text = files
        .get("typst.toml")
        .map(|b| String::from_utf8_lossy(b).into_owned())
        .ok_or((StatusCode::BAD_REQUEST, "The document has no typst.toml at its top level".to_string()))?;
    let manifest: Manifest = toml::from_str(&manifest_text)
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("Invalid typst.toml: {e}")))?;

    let name = manifest.package.name.trim().to_string();
    let version = payload.version.unwrap_or(manifest.package.version).trim().to_string();
    let entrypoint = manifest.package.entrypoint.unwrap_or_else(|| "lib.typ".to_string());
    if !is_valid_name(&name) {
        return Err((StatusCode::BAD_REQUEST, "Invalid package name (lowercase letters, digits, '-' and '_' only)".to_string()));
    }
    if !is_valid_version(&version) {
        return Err((StatusCode::BAD_REQUEST, "Version must be in the form major.minor.patch".to_string()));
    }
    if !files.contains_key(&entrypoint) {
        return Err((StatusCode::BAD_REQUEST, format!("The package entrypoint {entrypoint} does not exist")));
    }

    let project_id = if instance_wide { None } else { Some(access.doc.project_id.clone()) };
    let existing: Option<String> =
        sqlx::query_scalar("SELECT id FROM packages WHERE COALESCE(project_id, '') = COALESCE($1, '') AND name = $2")
            .bind(&project_id)
            .bind(&name)
            .fetch_optional(&state.db)
            .await
            .map_err(db_err)?;
    let package_id = match existing {
        Some(id) => id,
        None => {
            let id = Uuid::new_v4().to_string();
            sqlx::query("INSERT INTO packages (id, project_id, owner_id, name, description) VALUES ($1, $2, $3, $4, $5)")
                .bind(&id)
                .bind(&project_id)
                .bind(&user_id)
                .bind(&name)
                .bind(&manifest.package.description)
                .execute(&state.db)
                .await
                .map_err(db_err)?;
            id
        }
    };

    let version_exists: Option<String> =
        sqlx::query_scalar("SELECT id FROM package_versions WHERE package_id = $1 AND version = $2")
            .bind(&package_id)
            .bind(&version)
            .fetch_optional(&state.db)
            .await
            .map_err(db_err)?;
    if version_exists.is_some() {
        return Err((StatusCode::CONFLICT, format!("Version {version} already published; versions are immutable")));
    }

    let version_id = Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO package_versions (id, package_id, version, entrypoint, manifest) VALUES ($1, $2, $3, $4, $5)")
        .bind(&version_id)
        .bind(&package_id)
        .bind(&version)
        .bind(&entrypoint)
        .bind(manifest_text.into_bytes())
        .execute(&state.db)
        .await
        .map_err(db_err)?;
    for (path, data) in files {
        sqlx::query("INSERT INTO package_files (id, version_id, path, data) VALUES ($1, $2, $3, $4)")
            .bind(Uuid::new_v4().to_string())
            .bind(&version_id)
            .bind(&path)
            .bind(&data)
            .execute(&state.db)
            .await
            .map_err(db_err)?;
    }

    Ok((StatusCode::CREATED, Json(fetch_package(&state, &package_id).await?)))
}

async fn fetch_package(state: &AppState, package_id: &str) -> Result<Package, ApiError> {
    sqlx::query_as(&format!("SELECT {PACKAGE_COLUMNS} FROM packages p LEFT JOIN users u ON u.id = p.owner_id WHERE p.id = $1"))
        .bind(package_id)
        .fetch_optional(&state.db)
        .await
        .map_err(db_err)?
        .ok_or_else(|| not_found("Package"))
}

/// The package if the caller may see it: instance packages for everyone signed
/// in, project packages for project members.
async fn visible_package(state: &AppState, package_id: &str, user_id: &str) -> Result<Package, ApiError> {
    let package = fetch_package(state, package_id).await?;
    if let Some(project_id) = &package.project_id {
        require_project_role(state, project_id, user_id, Role::Viewer)
            .await
            .map_err(|_| not_found("Package"))?;
    }
    Ok(package)
}

pub async fn list_instance_packages(
    State(state): State<AppState>,
    jar: SignedCookieJar,
) -> Result<Json<Vec<Package>>, ApiError> {
    require_user(&jar)?;
    let packages = sqlx::query_as(&format!(
        "SELECT {PACKAGE_COLUMNS} FROM packages p LEFT JOIN users u ON u.id = p.owner_id \
         WHERE p.project_id IS NULL ORDER BY p.name"
    ))
    .fetch_all(&state.db)
    .await
    .map_err(db_err)?;
    Ok(Json(packages))
}

pub async fn list_project_packages(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
    jar: SignedCookieJar,
) -> Result<Json<Vec<Package>>, ApiError> {
    let user_id = require_user(&jar)?;
    require_project_role(&state, &project_id, &user_id, Role::Viewer).await?;
    let packages = sqlx::query_as(&format!(
        "SELECT {PACKAGE_COLUMNS} FROM packages p LEFT JOIN users u ON u.id = p.owner_id \
         WHERE p.project_id = $1 ORDER BY p.name"
    ))
    .bind(&project_id)
    .fetch_all(&state.db)
    .await
    .map_err(db_err)?;
    Ok(Json(packages))
}

pub async fn list_versions(
    State(state): State<AppState>,
    Path(package_id): Path<String>,
    jar: SignedCookieJar,
) -> Result<Json<Vec<PackageVersion>>, ApiError> {
    let user_id = require_user(&jar)?;
    visible_package(&state, &package_id, &user_id).await?;
    let versions = sqlx::query_as(
        "SELECT id, package_id, version, entrypoint, created_at FROM package_versions \
         WHERE package_id = $1 ORDER BY created_at DESC",
    )
    .bind(&package_id)
    .fetch_all(&state.db)
    .await
    .map_err(db_err)?;
    Ok(Json(versions))
}

/// Admins delete instance packages; project owners delete their project's packages.
pub async fn delete_package(
    State(state): State<AppState>,
    Path(package_id): Path<String>,
    jar: SignedCookieJar,
) -> Result<StatusCode, ApiError> {
    let user_id = require_user(&jar)?;
    let package = visible_package(&state, &package_id, &user_id).await?;
    match &package.project_id {
        Some(project_id) => {
            require_project_role(&state, project_id, &user_id, Role::Owner).await?;
        }
        None => {
            if !is_admin(&state, &user_id).await? {
                return Err(forbidden("Only admins can delete instance-wide packages"));
            }
        }
    }
    sqlx::query("DELETE FROM packages WHERE id = $1")
        .bind(&package_id)
        .execute(&state.db)
        .await
        .map_err(db_err)?;
    Ok(StatusCode::NO_CONTENT)
}
