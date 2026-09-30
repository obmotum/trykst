//! User directory lookup for invitations.
//!
//! With OIDC there is no local user list, so a colleague who never signed in is
//! unknown to TypstDrive. A directory lets us find such people at the identity
//! provider and create a placeholder account keyed by their subject, which is
//! linked automatically on their first login.
//!
//! Supported backends (`OIDC_DIRECTORY`):
//! - `none` (default): only users who already signed in can be found; inviting an
//!   unknown email creates a placeholder that is linked by verified email.
//! - `keycloak`: Keycloak Admin REST API via the client's service account
//!   (realm-management roles `view-users` and `query-users`).
//!
//! Further backends (Microsoft Graph, SCIM, ...) slot in as new enum variants.
//!
//! Keycloak user attributes shown in the people picker (optional):
//! `OIDC_DIRECTORY_ORG_ATTRIBUTE` (default `organization`) and `picture` (avatar URL).

use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use axum_extra::extract::cookie::SignedCookieJar;
use openidconnect::reqwest;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::{oidc::Oidc, AppState};

type ApiError = (StatusCode, String);

const DEFAULT_LIMIT: usize = 5;
const MAX_LIMIT: usize = 25;

#[derive(Debug, Clone, Serialize)]
pub struct DirectoryUser {
    /// Stable id at the IdP; equals the `sub` claim of their ID token.
    pub subject: String,
    pub username: String,
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub organization: Option<String>,
    pub picture: Option<String>,
}

pub enum Directory {
    None,
    Keycloak(KeycloakDirectory),
}

impl Directory {
    pub fn from_env(issuer: &str) -> Self {
        let kind = std::env::var("OIDC_DIRECTORY").unwrap_or_default().to_lowercase();
        match kind.as_str() {
            "" | "none" => Directory::None,
            "keycloak" => Directory::Keycloak(
                KeycloakDirectory::new(issuer)
                    .expect("OIDC_DIRECTORY=keycloak requires OIDC_ISSUER of the form https://host/realms/<realm>"),
            ),
            other => panic!("Unknown OIDC_DIRECTORY '{other}'. Expected 'none' or 'keycloak'."),
        }
    }

    pub async fn search(&self, oidc: &Oidc, query: &str, limit: usize) -> Result<Vec<DirectoryUser>, ApiError> {
        match self {
            Directory::None => Ok(Vec::new()),
            Directory::Keycloak(kc) => kc.search(oidc, query, limit).await,
        }
    }

    pub async fn get(&self, oidc: &Oidc, subject: &str) -> Result<Option<DirectoryUser>, ApiError> {
        match self {
            Directory::None => Ok(None),
            Directory::Keycloak(kc) => kc.get(oidc, subject).await,
        }
    }

    pub async fn find_by_email(&self, oidc: &Oidc, email: &str) -> Result<Option<DirectoryUser>, ApiError> {
        match self {
            Directory::None => Ok(None),
            Directory::Keycloak(kc) => kc.find_by_email(oidc, email).await,
        }
    }
}

// ---------------------------------------------------------------------------
// Keycloak
// ---------------------------------------------------------------------------

pub struct KeycloakDirectory {
    token_url: String,
    admin_url: String,
    org_attribute: String,
    token: Mutex<Option<(String, i64)>>,
}

#[derive(Deserialize)]
struct KeycloakUser {
    id: String,
    username: String,
    email: Option<String>,
    #[serde(rename = "firstName")]
    first_name: Option<String>,
    #[serde(rename = "lastName")]
    last_name: Option<String>,
    #[serde(default = "enabled_default")]
    enabled: bool,
    #[serde(default)]
    attributes: std::collections::HashMap<String, Vec<String>>,
}

fn enabled_default() -> bool {
    true
}

impl KeycloakUser {
    fn into_directory_user(mut self, org_attribute: &str) -> DirectoryUser {
        let mut attr = |name: &str| {
            self.attributes
                .remove(name)
                .and_then(|v| v.into_iter().next())
                .filter(|v| !v.trim().is_empty())
        };
        let organization = attr(org_attribute);
        let picture = attr("picture").filter(|url| url.starts_with("https://") || url.starts_with("http://"));
        let u = self;
        let name = [u.first_name.as_deref(), u.last_name.as_deref()]
            .into_iter()
            .flatten()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        DirectoryUser {
            subject: u.id,
            username: u.username,
            email: u.email,
            display_name: (!name.is_empty()).then_some(name),
            organization,
            picture,
        }
    }
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: i64,
}

fn upstream<E: std::fmt::Display>(e: E) -> ApiError {
    tracing::error!("Keycloak directory request failed: {e}");
    (StatusCode::BAD_GATEWAY, "User directory is unavailable".to_string())
}

impl KeycloakDirectory {
    fn new(issuer: &str) -> Option<Self> {
        // https://host/realms/<realm>  ->  https://host/admin/realms/<realm>
        let issuer = issuer.trim_end_matches('/');
        let idx = issuer.rfind("/realms/")?;
        let (base, realm_path) = issuer.split_at(idx);
        Some(KeycloakDirectory {
            token_url: format!("{issuer}/protocol/openid-connect/token"),
            admin_url: format!("{base}/admin{realm_path}"),
            org_attribute: std::env::var("OIDC_DIRECTORY_ORG_ATTRIBUTE")
                .ok()
                .filter(|v| !v.trim().is_empty())
                .unwrap_or_else(|| "organization".to_string()),
            token: Mutex::new(None),
        })
    }

    /// Service-account token via client credentials, cached until shortly before expiry.
    async fn token(&self, oidc: &Oidc) -> Result<String, ApiError> {
        let mut slot = self.token.lock().await;
        let now = chrono::Utc::now().timestamp();
        if let Some((token, expires)) = slot.as_ref() {
            if *expires > now + 30 {
                return Ok(token.clone());
            }
        }
        let secret = oidc.config.client_secret.as_deref().ok_or_else(|| {
            upstream("OIDC_DIRECTORY=keycloak needs OIDC_CLIENT_SECRET (confidential client with service account)")
        })?;
        let response = oidc
            .http()
            .post(&self.token_url)
            .form(&[
                ("grant_type", "client_credentials"),
                ("client_id", oidc.config.client_id.as_str()),
                ("client_secret", secret),
            ])
            .send()
            .await
            .map_err(upstream)?;
        if !response.status().is_success() {
            return Err(upstream(format!(
                "service account token request returned {}; is 'Service accounts roles' enabled on the client?",
                response.status()
            )));
        }
        let body = response.bytes().await.map_err(upstream)?;
        let token: TokenResponse = serde_json::from_slice(&body).map_err(upstream)?;
        *slot = Some((token.access_token.clone(), now + token.expires_in));
        Ok(token.access_token)
    }

    async fn users(&self, oidc: &Oidc, params: &[(&str, &str)]) -> Result<Vec<DirectoryUser>, ApiError> {
        let token = self.token(oidc).await?;
        let response = oidc
            .http()
            .get(format!("{}/users", self.admin_url))
            .bearer_auth(token)
            .query(params)
            .send()
            .await
            .map_err(upstream)?;
        if response.status() == reqwest::StatusCode::FORBIDDEN {
            return Err(upstream(
                "403 from Keycloak admin API; grant the service account the realm-management roles view-users and query-users",
            ));
        }
        if !response.status().is_success() {
            return Err(upstream(format!("users query returned {}", response.status())));
        }
        let body = response.bytes().await.map_err(upstream)?;
        let users: Vec<KeycloakUser> = serde_json::from_slice(&body).map_err(upstream)?;
        Ok(users
            .into_iter()
            .filter(|u| u.enabled && !u.username.starts_with("service-account-"))
            .map(|u| u.into_directory_user(&self.org_attribute))
            .collect())
    }

    async fn search(&self, oidc: &Oidc, query: &str, limit: usize) -> Result<Vec<DirectoryUser>, ApiError> {
        let max = limit.to_string();
        // Keycloak matches prefixes by default; wildcards make "example" find "@example.com".
        let infix = format!("*{}*", query.replace('*', ""));
        self.users(oidc, &[("search", &infix), ("max", &max), ("briefRepresentation", "false")])
            .await
    }

    async fn find_by_email(&self, oidc: &Oidc, email: &str) -> Result<Option<DirectoryUser>, ApiError> {
        let users = self
            .users(oidc, &[("email", email), ("exact", "true"), ("briefRepresentation", "false")])
            .await?;
        Ok(users.into_iter().next())
    }

    async fn get(&self, oidc: &Oidc, subject: &str) -> Result<Option<DirectoryUser>, ApiError> {
        // Validate before putting it into a URL path.
        if Uuid::parse_str(subject).is_err() {
            return Ok(None);
        }
        let token = self.token(oidc).await?;
        let response = oidc
            .http()
            .get(format!("{}/users/{subject}", self.admin_url))
            .bearer_auth(token)
            .send()
            .await
            .map_err(upstream)?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !response.status().is_success() {
            return Err(upstream(format!("user lookup returned {}", response.status())));
        }
        let body = response.bytes().await.map_err(upstream)?;
        let user: KeycloakUser = serde_json::from_slice(&body).map_err(upstream)?;
        Ok(user.enabled.then(|| user.into_directory_user(&self.org_attribute)))
    }
}

// ---------------------------------------------------------------------------
// HTTP handler
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct SearchQuery {
    q: String,
    limit: Option<usize>,
}

#[derive(Serialize)]
pub struct SearchResult {
    /// Set for people found at the IdP; pass it back as `subject` when inviting.
    subject: Option<String>,
    /// Set for people who already have a TypstDrive account.
    user_id: Option<String>,
    username: String,
    email: Option<String>,
    display_name: Option<String>,
    organization: Option<String>,
    picture: Option<String>,
    /// Known guest (external user); only set for people who signed in before.
    is_guest: bool,
}

/// People search for the share dialog: existing accounts plus the IdP directory.
pub async fn search(
    State(state): State<AppState>,
    jar: SignedCookieJar,
    Query(query): Query<SearchQuery>,
) -> Result<Json<Vec<SearchResult>>, ApiError> {
    let me = jar
        .get("session_user_id")
        .map(|c| c.value().to_string())
        .ok_or((StatusCode::UNAUTHORIZED, "Not logged in".to_string()))?;
    crate::auth::require_member(&state, &me).await?;

    // One extra row (plus one for the caller, filtered out below) tells the client whether
    // "show more" makes sense: it displays `limit` rows and offers more when it got more.
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT) + 1;
    let q = query.q.trim();
    if q.chars().count() < 2 {
        return Ok(Json(Vec::new()));
    }

    let pattern = format!("%{}%", q.to_lowercase().replace('%', "").replace('_', ""));
    let local: Vec<(String, String, Option<String>, Option<String>, i64)> = sqlx::query_as(
        "SELECT id, username, email, oidc_subject, is_guest FROM users \
         WHERE LOWER(username) LIKE ? OR LOWER(COALESCE(email, '')) LIKE ? \
         ORDER BY username LIMIT ?",
    )
    .bind(&pattern)
    .bind(&pattern)
    .bind(limit as i64 + 1)
    .fetch_all(&state.db)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let mut results: Vec<SearchResult> = local
        .into_iter()
        .map(|(id, username, email, subject, is_guest)| SearchResult {
            subject,
            user_id: Some(id),
            username,
            email,
            display_name: None,
            organization: None,
            picture: None,
            is_guest: is_guest != 0,
        })
        .collect();

    // A directory outage should not break the dialog; local matches still work.
    let remote = state.directory.search(&state.oidc, q, limit + 1).await.unwrap_or_default();
    for person in remote {
        if let Some(existing) = results.iter_mut().find(|r| r.subject.as_deref() == Some(person.subject.as_str())) {
            existing.display_name = person.display_name;
            existing.organization = person.organization;
            existing.picture = person.picture;
            continue;
        }
        results.push(SearchResult {
            subject: Some(person.subject),
            user_id: None,
            username: person.username,
            email: person.email,
            display_name: person.display_name,
            organization: person.organization,
            picture: person.picture,
            is_guest: false,
        });
    }
    // Nobody needs to invite themselves.
    let my_subject: Option<String> = sqlx::query_as::<_, (Option<String>,)>("SELECT oidc_subject FROM users WHERE id = ?")
        .bind(&me)
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten()
        .and_then(|(s,)| s);
    results.retain(|r| r.user_id.as_deref() != Some(me.as_str()) && (my_subject.is_none() || r.subject != my_subject));
    results.truncate(limit);
    Ok(Json(results))
}
