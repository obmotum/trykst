use serde::{Deserialize, Serialize};
use sqlx::FromRow;

// sqlx::Any maps SQLite INTEGER to i64 (BIGINT), not bool.
// These helpers let us store boolean flags (is_admin, is_guest, resolved) as i64
// in DB-mapped structs while still serializing them as JSON booleans for the frontend.
mod serde_i64_bool {
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(v: &i64, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_bool(*v != 0)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<i64, D::Error> {
        Ok(if bool::deserialize(d)? { 1 } else { 0 })
    }
}

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct User {
    pub id: String,
    pub username: String,
    pub email: Option<String>,
    #[serde(with = "serde_i64_bool")]
    pub is_admin: i64,
    /// External user: may only work with documents and spaces shared with them.
    #[serde(with = "serde_i64_bool")]
    pub is_guest: i64,
    /// Profile picture URL from the IdP (`picture` claim).
    pub avatar_url: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct AdminUserView {
    pub id: String,
    pub username: String,
    pub email: Option<String>,
    #[serde(with = "serde_i64_bool")]
    pub is_admin: i64,
    #[serde(with = "serde_i64_bool")]
    pub is_guest: i64,
    pub avatar_url: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Folder {
    pub id: String,
    pub owner_id: String,
    pub parent_id: Option<String>,
    pub name: String,
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct File {
    pub id: String,
    pub owner_id: String,
    pub document_id: Option<String>,
    pub folder_id: Option<String>,
    pub name: String,
    pub mime_type: String,
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Document {
    pub id: String,
    pub owner_id: String,
    pub folder_id: Option<String>,
    pub title: String,
    #[serde(skip_serializing)]
    pub content: Option<Vec<u8>>,
    pub thumbnail_svg: Option<String>,
    pub public_role: Option<String>,
    #[serde(default)]
    #[sqlx(default)]
    pub effective_role: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Space {
    pub id: String,
    pub owner_id: String,
    pub folder_id: Option<String>,
    pub name: String,
    pub entrypoint: String,
    pub thumbnail_svg: Option<String>,
    pub public_role: Option<String>,
    #[serde(default)]
    #[sqlx(default)]
    pub effective_role: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct SpaceFile {
    pub id: String,
    pub space_id: String,
    pub path: String,
    pub kind: String,
    pub mime_type: String,
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Package {
    pub id: String,
    pub owner_id: String,
    pub namespace: String,
    pub name: String,
    pub description: Option<String>,
    pub created_at: String,
    #[serde(default)]
    #[sqlx(default)]
    pub owner_name: Option<String>,
    #[serde(default)]
    #[sqlx(default)]
    pub latest_version: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct PackageVersion {
    pub id: String,
    pub package_id: String,
    pub version: String,
    pub entrypoint: String,
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateSpaceRequest {
    pub name: String,
    pub folder_id: Option<String>,
    pub template: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateSpaceRequest {
    pub name: Option<String>,
    pub folder_id: Option<String>,
    pub entrypoint: Option<String>,
    pub public_role: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateSpaceFileRequest {
    pub path: String,
    pub kind: Option<String>,
    pub content: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateSpaceFileRequest {
    pub path: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PublishPackageRequest {
    pub space_id: String,
    pub version: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateFolderRequest {
    pub name: String,
    pub parent_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateDocumentRequest {
    pub title: String,
    pub folder_id: Option<String>,
    pub content: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateDocumentRequest {
    pub title: Option<String>,
    pub folder_id: Option<String>,
    pub public_role: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StorageStats {
    pub documents_size_bytes: i64,
    pub files_size_bytes: i64,
    pub total_size_bytes: i64,
}

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Collaborator {
    pub id: String,
    pub document_id: String,
    pub user_id: String,
    pub role: String,
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct CollaboratorView {
    pub id: String,
    pub user_id: String,
    pub username: String,
    pub email: Option<String>,
    pub avatar_url: Option<String>,
    pub role: String,
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Invitation {
    pub id: String,
    pub document_id: String,
    pub role: String,
    pub token: String,
    pub created_at: String,
    pub expires_at: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Comment {
    pub id: String,
    pub document_id: String,
    pub user_id: String,
    pub content: String,
    #[serde(with = "serde_i64_bool")]
    pub resolved: i64,
    pub created_at: String,
    pub author_name: Option<String>,
    #[sqlx(default)]
    pub author_avatar_url: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateCommentRequest {
    pub content: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateCommentRequest {
    pub content: Option<String>,
    pub resolved: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct DocumentVersion {
    pub id: String,
    pub document_id: String,
    pub user_id: String,
    pub content: String,
    pub created_at: String,
    #[sqlx(default)]
    pub author_name: Option<String>,
    #[sqlx(default)]
    pub author_avatar_url: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateVersionRequest {
    pub content: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct InviteRequest {
    /// IdP subject from a directory search result.
    pub subject: Option<String>,
    pub email: Option<String>,
    pub role: String,
}

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct UsagePoint {
    pub date: String,
    pub count: i64,
}

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct ApiKeyView {
    pub id: String,
    pub name: String,
    pub key_prefix: String,
    pub created_at: String,
    pub last_used_at: Option<String>,
    pub rate_limit: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateApiKeyRequest {
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateUserRequest {
    pub is_admin: Option<bool>,
}
