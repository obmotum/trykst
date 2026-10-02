use serde::{Deserialize, Deserializer, Serialize};
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

/// Distinguishes a JSON field that is absent from one that is `null`, so PATCH
/// requests can move a node to the top level (`"parent_id": null`).
fn double_option<'de, D, T>(d: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(d).map(Some)
}

// --- Users -------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct User {
    pub id: String,
    pub username: String,
    pub email: Option<String>,
    #[serde(with = "serde_i64_bool")]
    pub is_admin: i64,
    /// External user: may only work with projects they were added to.
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

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateUserRequest {
    pub is_admin: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StorageStats {
    pub documents_size_bytes: i64,
    pub files_size_bytes: i64,
    pub total_size_bytes: i64,
}

// --- Projects ----------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub created_by: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    /// The caller's role in the project.
    #[sqlx(default)]
    pub role: Option<String>,
    #[sqlx(default)]
    pub document_count: Option<i64>,
    #[sqlx(default)]
    pub member_count: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct CreateProjectRequest {
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateProjectRequest {
    pub name: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct ProjectMember {
    pub user_id: String,
    pub username: String,
    pub email: Option<String>,
    pub avatar_url: Option<String>,
    #[serde(with = "serde_i64_bool")]
    pub is_guest: i64,
    pub role: String,
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
pub struct AddMemberRequest {
    /// IdP subject from a directory search result.
    pub subject: Option<String>,
    pub email: Option<String>,
    pub role: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateMemberRequest {
    pub role: String,
}

// --- Documents and their file trees -----------------------------------------

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Document {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub entrypoint_id: Option<String>,
    pub thumbnail_svg: Option<String>,
    pub public_role: Option<String>,
    pub created_by: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    /// The caller's effective role on the document.
    #[sqlx(default)]
    pub role: Option<String>,
    /// True when that role comes from project membership rather than link sharing.
    #[sqlx(skip)]
    #[serde(default)]
    pub is_member: bool,
}

#[derive(Debug, Deserialize)]
pub struct CreateDocumentRequest {
    pub title: String,
    /// Content of the initial main.typ; a short template when absent.
    pub content: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateDocumentRequest {
    pub title: Option<String>,
    /// "viewer", "editor" or null to turn link sharing off.
    #[serde(default, deserialize_with = "double_option")]
    pub public_role: Option<Option<String>>,
}

/// A folder or file inside a document. `path` is derived from the tree.
#[derive(Debug, Serialize, Clone)]
pub struct Node {
    pub id: String,
    pub parent_id: Option<String>,
    pub name: String,
    pub kind: String,
    pub mime_type: Option<String>,
    pub size: i64,
    pub path: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateNodeRequest {
    pub parent_id: Option<String>,
    pub name: String,
    /// "folder" or "text"; binary files are uploaded.
    pub kind: String,
    pub content: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateNodeRequest {
    pub name: Option<String>,
    /// Move target: a folder id, or null for the top level. Absent = stay.
    #[serde(default, deserialize_with = "double_option")]
    pub parent_id: Option<Option<String>>,
    /// Replaces the content of a text file.
    pub content: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SetEntrypointRequest {
    pub node_id: String,
}

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct DocumentVersion {
    pub id: String,
    pub document_id: String,
    pub user_id: Option<String>,
    pub label: Option<String>,
    pub created_at: String,
    #[sqlx(default)]
    pub author_name: Option<String>,
    #[sqlx(default)]
    pub author_avatar_url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateVersionRequest {
    pub label: Option<String>,
}

// --- Comments ----------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Comment {
    pub id: String,
    pub document_id: String,
    pub node_id: Option<String>,
    pub user_id: String,
    pub content: String,
    #[serde(with = "serde_i64_bool")]
    pub resolved: i64,
    pub created_at: String,
    pub author_name: Option<String>,
    #[sqlx(default)]
    pub author_avatar_url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateCommentRequest {
    pub content: String,
    pub node_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateCommentRequest {
    pub content: Option<String>,
    pub resolved: Option<bool>,
}

// --- Packages ----------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Package {
    pub id: String,
    /// None for instance-wide packages (@trykst/...).
    pub project_id: Option<String>,
    pub owner_id: Option<String>,
    pub name: String,
    pub description: Option<String>,
    pub created_at: String,
    #[sqlx(default)]
    pub owner_name: Option<String>,
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

#[derive(Debug, Deserialize)]
pub struct PublishPackageRequest {
    pub document_id: String,
    /// "project" (default) or "instance" (admins only).
    pub scope: Option<String>,
    pub version: Option<String>,
}

// --- API keys ------------------------------------------------------------------

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
