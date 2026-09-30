//! OpenID Connect authentication.
//!
//! TypstDrive has no local accounts: every user signs in through the configured
//! OpenID Provider (Keycloak, Authentik, Entra ID, ...). Users are provisioned
//! just-in-time on their first login and keyed by `(issuer, subject)`.
//!
//! Sessions live in the `sessions` table. The browser holds two signed cookies:
//! `session_id` (the row) and `session_user_id` (read by the existing handlers).
//! [`session_guard`] strips `session_user_id` from every request whose session is
//! missing, expired or revoked, so handlers never see a stale identity.

use std::sync::Arc;

use axum::{
    extract::{Form, Query, Request, State},
    http::{header, HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Redirect, Response},
    Json,
};
use axum_extra::extract::cookie::{Cookie, PrivateCookieJar, SameSite, SignedCookieJar};
use base64::Engine;
use openidconnect::{
    core::{CoreAuthenticationFlow, CoreClient, CoreIdToken},
    reqwest, AuthorizationCode, ClientId, ClientSecret, CsrfToken, EndpointMaybeSet,
    EndpointNotSet, EndpointSet, IssuerUrl, LogoutRequest, Nonce, PkceCodeChallenge,
    PkceCodeVerifier, PostLogoutRedirectUrl, ProviderMetadataWithLogout, RedirectUrl, Scope,
    OAuth2TokenResponse, TokenResponse,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::AppState;

pub const SESSION_COOKIE: &str = "session_id";
pub const USER_COOKIE: &str = "session_user_id";
const FLOW_COOKIE: &str = "oidc_flow";
const CALLBACK_PATH: &str = "/api/auth/oidc/callback";
const BACKCHANNEL_LOGOUT_EVENT: &str = "http://schemas.openid.net/event/backchannel-logout";

type OidcClient = CoreClient<
    EndpointSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointMaybeSet,
    EndpointMaybeSet,
>;

type ApiError = (StatusCode, String);

fn internal<E: std::fmt::Display>(e: E) -> ApiError {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct OidcConfig {
    pub issuer: String,
    pub client_id: String,
    pub client_secret: Option<String>,
    /// Externally reachable base URL of TypstDrive, e.g. `https://typst.example.com`.
    pub public_url: String,
    pub scopes: Vec<String>,
    /// Claim holding the stable user identifier. `sub` for Keycloak/Authentik, `oid` for Entra ID.
    pub subject_claim: String,
    pub username_claim: String,
    /// Dot-separated path to the roles/groups array, e.g. `realm_access.roles` or `groups`.
    pub roles_claim: Option<String>,
    /// Members of this role are admins. When unset, admins are managed inside TypstDrive.
    pub admin_role: Option<String>,
    /// When set, only members of this role may sign in.
    pub required_role: Option<String>,
    pub session_max_age_secs: i64,
}

impl OidcConfig {
    pub fn from_env() -> Self {
        fn required(name: &str) -> String {
            std::env::var(name)
                .ok()
                .filter(|v| !v.trim().is_empty())
                .unwrap_or_else(|| panic!("{name} must be set (TypstDrive signs in exclusively via OIDC)"))
        }
        fn optional(name: &str) -> Option<String> {
            std::env::var(name).ok().filter(|v| !v.trim().is_empty())
        }

        let session_hours: i64 = optional("SESSION_MAX_AGE_HOURS")
            .map(|v| v.parse().expect("SESSION_MAX_AGE_HOURS must be a whole number"))
            .unwrap_or(10);

        OidcConfig {
            issuer: required("OIDC_ISSUER"),
            client_id: required("OIDC_CLIENT_ID"),
            client_secret: optional("OIDC_CLIENT_SECRET"),
            public_url: required("PUBLIC_URL").trim_end_matches('/').to_string(),
            scopes: optional("OIDC_SCOPES")
                .unwrap_or_else(|| "openid profile email".to_string())
                .split_whitespace()
                .filter(|s| *s != "openid")
                .map(str::to_string)
                .collect(),
            subject_claim: optional("OIDC_SUBJECT_CLAIM").unwrap_or_else(|| "sub".to_string()),
            username_claim: optional("OIDC_USERNAME_CLAIM")
                .unwrap_or_else(|| "preferred_username".to_string()),
            roles_claim: optional("OIDC_ROLES_CLAIM"),
            admin_role: optional("OIDC_ADMIN_ROLE"),
            required_role: optional("OIDC_REQUIRED_ROLE"),
            session_max_age_secs: session_hours * 3600,
        }
    }

    fn secure_cookies(&self) -> bool {
        self.public_url.starts_with("https://")
    }
}

// ---------------------------------------------------------------------------
// Provider (discovered lazily, so TypstDrive can start before the IdP is up)
// ---------------------------------------------------------------------------

struct Provider {
    client: OidcClient,
    userinfo_url: Option<openidconnect::UserInfoUrl>,
    end_session_url: Option<openidconnect::EndSessionUrl>,
}

pub struct Oidc {
    pub config: OidcConfig,
    http: reqwest::Client,
    provider: RwLock<Option<Arc<Provider>>>,
}

impl Oidc {
    pub fn new(config: OidcConfig) -> Self {
        let http = reqwest::ClientBuilder::new()
            // Following redirects opens the client up to SSRF.
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("failed to build HTTP client");
        Oidc { config, http, provider: RwLock::new(None) }
    }

    pub fn http(&self) -> &reqwest::Client {
        &self.http
    }

    async fn provider(&self) -> Result<Arc<Provider>, ApiError> {
        if let Some(p) = self.provider.read().await.as_ref() {
            return Ok(p.clone());
        }
        let mut slot = self.provider.write().await;
        if let Some(p) = slot.as_ref() {
            return Ok(p.clone());
        }

        let issuer = IssuerUrl::new(self.config.issuer.clone()).map_err(internal)?;
        let metadata = ProviderMetadataWithLogout::discover_async(issuer, &self.http)
            .await
            .map_err(|e| {
                tracing::error!("OIDC discovery for {} failed: {e:?}", self.config.issuer);
                (StatusCode::BAD_GATEWAY, "Identity provider is unreachable".to_string())
            })?;
        let end_session_url = metadata.additional_metadata().end_session_endpoint.clone();
        let userinfo_url = metadata.userinfo_endpoint().cloned();

        let redirect = format!("{}{}", self.config.public_url, CALLBACK_PATH);
        let client = CoreClient::from_provider_metadata(
            metadata,
            ClientId::new(self.config.client_id.clone()),
            self.config.client_secret.clone().map(ClientSecret::new),
        )
        .set_redirect_uri(RedirectUrl::new(redirect).map_err(internal)?);

        let provider = Arc::new(Provider { client, userinfo_url, end_session_url });
        *slot = Some(provider.clone());
        tracing::info!("OIDC provider {} discovered", self.config.issuer);
        Ok(provider)
    }

    /// Drops the cached discovery document so the next request refetches it
    /// (and with it the JWKS, in case the IdP rotated its signing keys).
    async fn invalidate(&self) {
        *self.provider.write().await = None;
    }
}

// ---------------------------------------------------------------------------
// Claims helpers
// ---------------------------------------------------------------------------

/// Decodes the payload of a JWT whose signature has already been verified.
fn jwt_payload(jwt: &str) -> Result<Value, ApiError> {
    let payload = jwt.split('.').nth(1).ok_or_else(|| internal("malformed JWT"))?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload.trim_end_matches('='))
        .map_err(internal)?;
    serde_json::from_slice(&bytes).map_err(internal)
}

/// Looks up a dot-separated path such as `realm_access.roles`.
fn claim_at<'a>(claims: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.').try_fold(claims, |v, key| v.get(key))
}

fn claim_str(claims: &Value, path: &str) -> Option<String> {
    claim_at(claims, path).and_then(Value::as_str).map(str::to_string)
}

fn claim_roles(claims: &Value, path: &str) -> Option<Vec<String>> {
    match claim_at(claims, path)? {
        Value::Array(items) => Some(items.iter().filter_map(Value::as_str).map(str::to_string).collect()),
        Value::String(s) => Some(vec![s.clone()]),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Login
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize)]
struct FlowState {
    csrf: String,
    nonce: String,
    pkce_verifier: String,
    return_to: String,
}

#[derive(Deserialize)]
pub struct LoginQuery {
    return_to: Option<String>,
}

/// Only same-origin paths are accepted, to avoid an open redirect.
fn sanitize_return_to(value: Option<String>) -> String {
    match value {
        Some(p) if p.starts_with('/') && !p.starts_with("//") && !p.contains('\\') => p,
        _ => "/".to_string(),
    }
}

pub async fn login(
    State(state): State<AppState>,
    jar: PrivateCookieJar,
    Query(query): Query<LoginQuery>,
) -> Result<(PrivateCookieJar, Redirect), ApiError> {
    let oidc = &state.oidc;
    let provider = oidc.provider().await?;
    let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();

    let mut request = provider.client.authorize_url(
        CoreAuthenticationFlow::AuthorizationCode,
        CsrfToken::new_random,
        Nonce::new_random,
    );
    for scope in &oidc.config.scopes {
        request = request.add_scope(Scope::new(scope.clone()));
    }
    let (auth_url, csrf, nonce) = request.set_pkce_challenge(pkce_challenge).url();

    let flow = FlowState {
        csrf: csrf.secret().clone(),
        nonce: nonce.secret().clone(),
        pkce_verifier: pkce_verifier.secret().clone(),
        return_to: sanitize_return_to(query.return_to),
    };
    let cookie = Cookie::build((FLOW_COOKIE, serde_json::to_string(&flow).map_err(internal)?))
        .path("/api/auth/oidc")
        .http_only(true)
        .secure(oidc.config.secure_cookies())
        .same_site(SameSite::Lax)
        .max_age(time::Duration::minutes(10));

    Ok((jar.add(cookie), Redirect::to(auth_url.as_str())))
}

#[derive(Deserialize)]
pub struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

pub async fn callback(
    State(state): State<AppState>,
    flow_jar: PrivateCookieJar,
    session_jar: SignedCookieJar,
    Query(query): Query<CallbackQuery>,
) -> Result<Response, ApiError> {
    if let Some(error) = query.error {
        let detail = query.error_description.unwrap_or_default();
        tracing::warn!("OIDC provider returned error {error}: {detail}");
        return Err((StatusCode::UNAUTHORIZED, format!("Sign-in failed: {error} {detail}")));
    }

    let flow: FlowState = flow_jar
        .get(FLOW_COOKIE)
        .and_then(|c| serde_json::from_str(c.value()).ok())
        .ok_or((StatusCode::BAD_REQUEST, "Sign-in session expired, please try again".to_string()))?;
    let flow_jar = flow_jar.remove(Cookie::build(FLOW_COOKIE).path("/api/auth/oidc"));

    let returned_state = query.state.unwrap_or_default();
    if !constant_time_eq(returned_state.as_bytes(), flow.csrf.as_bytes()) {
        return Err((StatusCode::BAD_REQUEST, "Invalid sign-in state".to_string()));
    }
    let code = query.code.ok_or((StatusCode::BAD_REQUEST, "Missing authorization code".to_string()))?;

    let oidc = &state.oidc;
    let provider = oidc.provider().await?;
    let token_response = provider
        .client
        .exchange_code(AuthorizationCode::new(code))
        .map_err(internal)?
        .set_pkce_verifier(PkceCodeVerifier::new(flow.pkce_verifier))
        .request_async(oidc.http())
        .await
        .map_err(|e| {
            tracing::error!("OIDC code exchange failed: {e:?}");
            (StatusCode::BAD_GATEWAY, "Could not complete sign-in with the identity provider".to_string())
        })?;

    let id_token = token_response
        .id_token()
        .ok_or_else(|| internal("Identity provider did not return an ID token"))?;
    if let Err(e) = id_token.claims(&provider.client.id_token_verifier(), &Nonce::new(flow.nonce)) {
        tracing::error!("ID token verification failed: {e:?}");
        oidc.invalidate().await;
        return Err((StatusCode::UNAUTHORIZED, "Invalid ID token".to_string()));
    }

    let id_token_raw = id_token.to_string();
    let mut claims = jwt_payload(&id_token_raw)?;

    // Roles (and sometimes email) are not always in the ID token; fall back to userinfo.
    let roles_missing = oidc
        .config
        .roles_claim
        .as_deref()
        .is_some_and(|path| claim_at(&claims, path).is_none());
    if roles_missing || claims.get("email").is_none() {
        merge_userinfo(oidc, &provider, token_response.access_token(), &mut claims).await;
    }

    let user_id = provision_user(&state, &claims).await?;
    let idp_session_id = claim_str(&claims, "sid");

    let session_id = Uuid::new_v4().to_string();
    let expires_at = chrono::Utc::now().timestamp() + oidc.config.session_max_age_secs;
    sqlx::query("INSERT INTO sessions (id, user_id, idp_session_id, id_token, expires_at) VALUES (?, ?, ?, ?, ?)")
        .bind(&session_id)
        .bind(&user_id)
        .bind(&idp_session_id)
        .bind(&id_token_raw)
        .bind(expires_at)
        .execute(&state.db)
        .await
        .map_err(internal)?;

    let max_age = time::Duration::seconds(oidc.config.session_max_age_secs);
    let session_jar = session_jar
        .add(session_cookie(SESSION_COOKIE, session_id, &oidc.config, max_age))
        .add(session_cookie(USER_COOKIE, user_id, &oidc.config, max_age));

    Ok((flow_jar, session_jar, Redirect::to(&flow.return_to)).into_response())
}

async fn merge_userinfo(
    oidc: &Oidc,
    provider: &Provider,
    access_token: &openidconnect::AccessToken,
    claims: &mut Value,
) {
    let Some(endpoint) = &provider.userinfo_url else { return };
    // Fetched as raw JSON so that custom claims (roles, groups) survive.
    let response = oidc
        .http()
        .get(endpoint.url().as_str())
        .bearer_auth(access_token.secret())
        .header(header::ACCEPT, "application/json")
        .send()
        .await;
    let Ok(response) = response else { return };
    let Ok(body) = response.bytes().await else { return };
    let Ok(userinfo) = serde_json::from_slice::<Value>(&body) else { return };

    // Per spec the userinfo `sub` must match the ID token; otherwise ignore it entirely.
    if userinfo.get("sub") != claims.get("sub") {
        tracing::warn!("Ignoring userinfo response: subject mismatch");
        return;
    }
    if let (Value::Object(target), Value::Object(extra)) = (claims, userinfo) {
        for (key, value) in extra {
            target.entry(key).or_insert(value);
        }
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn session_cookie(name: &'static str, value: String, config: &OidcConfig, max_age: time::Duration) -> Cookie<'static> {
    Cookie::build((name, value))
        .path("/")
        .http_only(true)
        .secure(config.secure_cookies())
        .same_site(SameSite::Lax)
        .max_age(max_age)
        .build()
}

// ---------------------------------------------------------------------------
// Just-in-time provisioning
// ---------------------------------------------------------------------------

async fn provision_user(state: &AppState, claims: &Value) -> Result<String, ApiError> {
    let config = &state.oidc.config;
    let issuer = claim_str(claims, "iss").ok_or_else(|| internal("ID token has no issuer"))?;
    let subject = claim_str(claims, &config.subject_claim).ok_or_else(|| {
        tracing::error!("Claim '{}' missing from ID token (OIDC_SUBJECT_CLAIM)", config.subject_claim);
        internal(format!("Claim '{}' missing from ID token", config.subject_claim))
    })?;
    let email = claim_str(claims, "email");
    let email_verified = claims.get("email_verified").and_then(Value::as_bool).unwrap_or(false);
    let preferred_username = claim_str(claims, &config.username_claim)
        .or_else(|| email.as_ref().and_then(|e| e.split('@').next().map(str::to_string)))
        .unwrap_or_else(|| subject.clone());

    let roles = match &config.roles_claim {
        Some(path) => {
            let roles = claim_roles(claims, path);
            if roles.is_none() {
                tracing::warn!(
                    "Roles claim '{path}' not found in ID token or userinfo. \
                     For Keycloak, enable 'Add to ID token' on the roles mapper of the 'roles' client scope."
                );
            }
            roles.unwrap_or_default()
        }
        None => Vec::new(),
    };

    if let Some(required) = &config.required_role {
        if !roles.iter().any(|r| r == required) {
            return Err((StatusCode::FORBIDDEN, "You are not permitted to use TypstDrive".to_string()));
        }
    }

    // 1. Known identity.
    let existing: Option<(String,)> =
        sqlx::query_as("SELECT id FROM users WHERE oidc_issuer = ? AND oidc_subject = ?")
            .bind(&issuer)
            .bind(&subject)
            .fetch_optional(&state.db)
            .await
            .map_err(internal)?;

    // 2. Unlinked account with the same verified email: a pre-OIDC local account
    //    or a placeholder created by an invitation.
    let existing = match existing {
        Some(row) => Some(row),
        None if email_verified && email.is_some() => {
            let linked: Option<(String,)> =
                sqlx::query_as("SELECT id FROM users WHERE email = ? AND oidc_subject IS NULL")
                    .bind(&email)
                    .fetch_optional(&state.db)
                    .await
                    .map_err(internal)?;
            if let Some((id,)) = &linked {
                sqlx::query("UPDATE users SET oidc_issuer = ?, oidc_subject = ? WHERE id = ?")
                    .bind(&issuer)
                    .bind(&subject)
                    .bind(id)
                    .execute(&state.db)
                    .await
                    .map_err(internal)?;
                tracing::info!("Linked existing account {id} to OIDC subject {subject}");
            }
            linked
        }
        None => None,
    };

    let user_id = match existing {
        Some((id,)) => id,
        None => {
            let id = Uuid::new_v4().to_string();
            let username = unique_username(state, &preferred_username, None).await?;
            // With no admin role configured, the very first user bootstraps the instance.
            let is_first: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
                .fetch_one(&state.db)
                .await
                .map_err(internal)?;
            let is_admin = config.admin_role.is_none() && is_first.0 == 0;
            sqlx::query(
                "INSERT INTO users (id, username, email, password_hash, is_admin, oidc_issuer, oidc_subject) VALUES (?, ?, ?, '', ?, ?, ?)",
            )
            .bind(&id)
            .bind(&username)
            .bind(&email)
            .bind(if is_admin { 1i64 } else { 0i64 })
            .bind(&issuer)
            .bind(&subject)
            .execute(&state.db)
            .await
            .map_err(|e| match e {
                sqlx::Error::Database(err) if err.is_unique_violation() => (
                    StatusCode::CONFLICT,
                    "Another account already uses this email address".to_string(),
                ),
                e => internal(e),
            })?;
            tracing::info!("Provisioned user {username} ({id}) for OIDC subject {subject}");
            return finish_admin_sync(state, id, &roles).await;
        }
    };

    // Keep profile data in sync with the IdP on every login.
    let username = unique_username(state, &preferred_username, Some(&user_id)).await?;
    let update = sqlx::query("UPDATE users SET username = ?, email = ? WHERE id = ?")
        .bind(&username)
        .bind(&email)
        .bind(&user_id)
        .execute(&state.db)
        .await;
    if let Err(e) = update {
        tracing::warn!("Could not sync profile for user {user_id}: {e}");
    }

    finish_admin_sync(state, user_id, &roles).await
}

async fn finish_admin_sync(state: &AppState, user_id: String, roles: &[String]) -> Result<String, ApiError> {
    if let Some(admin_role) = &state.oidc.config.admin_role {
        let is_admin = roles.iter().any(|r| r == admin_role);
        sqlx::query("UPDATE users SET is_admin = ? WHERE id = ?")
            .bind(if is_admin { 1i64 } else { 0i64 })
            .bind(&user_id)
            .execute(&state.db)
            .await
            .map_err(internal)?;
    }
    Ok(user_id)
}

/// `username` is UNIQUE; append a numeric suffix when the IdP name is taken by someone else.
async fn unique_username(state: &AppState, wanted: &str, own_id: Option<&str>) -> Result<String, ApiError> {
    let base = if wanted.trim().is_empty() { "user" } else { wanted.trim() };
    for n in 1..1000 {
        let candidate = if n == 1 { base.to_string() } else { format!("{base}-{n}") };
        let taken: Option<(String,)> = sqlx::query_as("SELECT id FROM users WHERE username = ?")
            .bind(&candidate)
            .fetch_optional(&state.db)
            .await
            .map_err(internal)?;
        match taken {
            None => return Ok(candidate),
            Some((id,)) if Some(id.as_str()) == own_id => return Ok(candidate),
            Some(_) => continue,
        }
    }
    Ok(format!("{base}-{}", &Uuid::new_v4().to_string()[..8]))
}

// ---------------------------------------------------------------------------
// Invitations
// ---------------------------------------------------------------------------

/// Finds the account to share with, creating a placeholder for people who never
/// signed in. Placeholders carry the IdP subject when the directory knows the
/// person (linked on first login by subject) or only the email address otherwise
/// (linked on first login by verified email, see `provision_user`).
pub async fn resolve_invitee(state: &AppState, subject: Option<&str>, email: Option<&str>) -> Result<String, ApiError> {
    let issuer = &state.oidc.config.issuer;
    let email = email.map(str::trim).filter(|e| !e.is_empty());

    let person = if let Some(subject) = subject {
        let known: Option<(String,)> =
            sqlx::query_as("SELECT id FROM users WHERE oidc_issuer = ? AND oidc_subject = ?")
                .bind(issuer)
                .bind(subject)
                .fetch_optional(&state.db)
                .await
                .map_err(internal)?;
        if let Some((id,)) = known {
            return Ok(id);
        }
        Some(
            state
                .directory
                .get(&state.oidc, subject)
                .await?
                .ok_or((StatusCode::NOT_FOUND, "User not found in the directory".to_string()))?,
        )
    } else if let Some(email) = email {
        let known: Option<(String,)> = sqlx::query_as("SELECT id FROM users WHERE LOWER(email) = LOWER(?)")
            .bind(email)
            .fetch_optional(&state.db)
            .await
            .map_err(internal)?;
        if let Some((id,)) = known {
            return Ok(id);
        }
        match state.directory.find_by_email(&state.oidc, email).await? {
            Some(person) => Some(person),
            None if matches!(*state.directory, crate::directory::Directory::None) => None,
            None => {
                return Err((StatusCode::NOT_FOUND, "No user with this email address in the directory".to_string()))
            }
        }
    } else {
        return Err((StatusCode::BAD_REQUEST, "Choose a person or enter an email address".to_string()));
    };

    let (subject, username, email) = match person {
        Some(p) => {
            // Someone may already hold this email without being linked yet.
            if let Some(mail) = &p.email {
                let known: Option<(String,)> = sqlx::query_as(
                    "SELECT id FROM users WHERE LOWER(email) = LOWER(?) AND oidc_subject IS NULL",
                )
                .bind(mail)
                .fetch_optional(&state.db)
                .await
                .map_err(internal)?;
                if let Some((id,)) = known {
                    sqlx::query("UPDATE users SET oidc_issuer = ?, oidc_subject = ? WHERE id = ?")
                        .bind(issuer)
                        .bind(&p.subject)
                        .bind(&id)
                        .execute(&state.db)
                        .await
                        .map_err(internal)?;
                    return Ok(id);
                }
            }
            (Some(p.subject), p.username, p.email)
        }
        None => {
            let email = email.unwrap_or_default().to_string();
            if !email.contains('@') {
                return Err((StatusCode::BAD_REQUEST, "Enter a valid email address".to_string()));
            }
            let local = email.split('@').next().unwrap_or("user").to_string();
            (None, local, Some(email))
        }
    };

    let id = Uuid::new_v4().to_string();
    let username = unique_username(state, &username, None).await?;
    sqlx::query(
        "INSERT INTO users (id, username, email, password_hash, is_admin, oidc_issuer, oidc_subject) VALUES (?, ?, ?, '', 0, ?, ?)",
    )
    .bind(&id)
    .bind(&username)
    .bind(&email)
    .bind(subject.as_ref().map(|_| issuer.clone()))
    .bind(&subject)
    .execute(&state.db)
    .await
    .map_err(internal)?;
    tracing::info!("Created placeholder account {username} ({id}) for an invitation");
    Ok(id)
}

// ---------------------------------------------------------------------------
// Logout
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct LogoutResponse {
    /// Where the browser should go next to end the IdP session as well.
    logout_url: Option<String>,
}

pub async fn logout(
    State(state): State<AppState>,
    jar: SignedCookieJar,
) -> Result<(SignedCookieJar, Json<LogoutResponse>), ApiError> {
    let mut id_token = None;
    if let Some(session_id) = jar.get(SESSION_COOKIE).map(|c| c.value().to_string()) {
        let row: Option<(Option<String>,)> = sqlx::query_as("SELECT id_token FROM sessions WHERE id = ?")
            .bind(&session_id)
            .fetch_optional(&state.db)
            .await
            .map_err(internal)?;
        id_token = row.and_then(|(t,)| t);
        sqlx::query("DELETE FROM sessions WHERE id = ?")
            .bind(&session_id)
            .execute(&state.db)
            .await
            .map_err(internal)?;
    }
    let jar = remove_session_cookies(jar);

    let logout_url = match state.oidc.provider().await {
        Ok(provider) => provider.end_session_url.clone().map(|url| {
            let mut request = LogoutRequest::from(url)
                .set_client_id(ClientId::new(state.oidc.config.client_id.clone()));
            if let Some(token) = id_token.as_deref().and_then(|t| t.parse::<CoreIdToken>().ok()) {
                request = request.set_id_token_hint(&token);
            }
            if let Ok(redirect) = PostLogoutRedirectUrl::new(format!("{}/", state.oidc.config.public_url)) {
                request = request.set_post_logout_redirect_uri(redirect);
            }
            request.http_get_url().to_string()
        }),
        Err(_) => None,
    };

    Ok((jar, Json(LogoutResponse { logout_url })))
}

fn remove_session_cookies(jar: SignedCookieJar) -> SignedCookieJar {
    jar.remove(Cookie::build(SESSION_COOKIE).path("/"))
        .remove(Cookie::build(USER_COOKIE).path("/"))
}

#[derive(Deserialize)]
pub struct BackchannelLogout {
    logout_token: String,
}

/// OpenID Connect Back-Channel Logout 1.0: the IdP tells us a session ended.
pub async fn backchannel_logout(
    State(state): State<AppState>,
    Form(form): Form<BackchannelLogout>,
) -> Result<StatusCode, ApiError> {
    let oidc = &state.oidc;
    let provider = oidc.provider().await?;

    // A logout token is a JWT with the same signature, issuer and audience rules as an
    // ID token, but it must not carry a nonce.
    let token: CoreIdToken = form
        .logout_token
        .parse()
        .map_err(|_| (StatusCode::BAD_REQUEST, "Malformed logout token".to_string()))?;
    let verifier = provider.client.id_token_verifier().allow_all_jose_types();
    let no_nonce = |nonce: Option<&Nonce>| match nonce {
        None => Ok(()),
        Some(_) => Err("logout token must not contain a nonce".to_string()),
    };
    if let Err(e) = token.claims(&verifier, no_nonce) {
        tracing::warn!("Rejected back-channel logout token: {e:?}");
        oidc.invalidate().await;
        return Err((StatusCode::BAD_REQUEST, "Invalid logout token".to_string()));
    }

    let claims = jwt_payload(&form.logout_token)?;
    if claims.get("events").and_then(|e| e.get(BACKCHANNEL_LOGOUT_EVENT)).is_none() {
        return Err((StatusCode::BAD_REQUEST, "Not a back-channel logout token".to_string()));
    }

    let deleted = if let Some(sid) = claim_str(&claims, "sid") {
        sqlx::query("DELETE FROM sessions WHERE idp_session_id = ?")
            .bind(sid)
            .execute(&state.db)
            .await
            .map_err(internal)?
            .rows_affected()
    } else if let (Some(iss), Some(sub)) = (claim_str(&claims, "iss"), claim_str(&claims, "sub")) {
        sqlx::query(
            "DELETE FROM sessions WHERE user_id IN (SELECT id FROM users WHERE oidc_issuer = ? AND oidc_subject = ?)",
        )
        .bind(iss)
        .bind(sub)
        .execute(&state.db)
        .await
        .map_err(internal)?
        .rows_affected()
    } else {
        return Err((StatusCode::BAD_REQUEST, "Logout token has neither sid nor sub".to_string()));
    };
    tracing::info!("Back-channel logout ended {deleted} session(s)");
    Ok(StatusCode::OK)
}

// ---------------------------------------------------------------------------
// Session guard
// ---------------------------------------------------------------------------

/// Validates the session behind `session_user_id` on every request. Requests with
/// an invalid session reach the handlers without any identity, and the browser is
/// told to drop the stale cookies.
pub async fn session_guard(
    State(state): State<AppState>,
    jar: SignedCookieJar,
    mut request: Request,
    next: Next,
) -> Response {
    let Some(user_id) = jar.get(USER_COOKIE).map(|c| c.value().to_string()) else {
        return next.run(request).await;
    };

    let valid = match jar.get(SESSION_COOKIE).map(|c| c.value().to_string()) {
        Some(session_id) => {
            let row: Result<Option<(String,)>, _> =
                sqlx::query_as("SELECT user_id FROM sessions WHERE id = ? AND expires_at > ?")
                    .bind(&session_id)
                    .bind(chrono::Utc::now().timestamp())
                    .fetch_optional(&state.db)
                    .await;
            matches!(row, Ok(Some((owner,))) if owner == user_id)
        }
        None => false,
    };

    if valid {
        return next.run(request).await;
    }

    strip_cookies(request.headers_mut(), &[USER_COOKIE, SESSION_COOKIE]);
    let response = next.run(request).await;
    (remove_session_cookies(jar), response).into_response()
}

fn strip_cookies(headers: &mut axum::http::HeaderMap, names: &[&str]) {
    let kept: Vec<String> = headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .map(str::trim)
        .filter(|pair| {
            let name = pair.split('=').next().unwrap_or("");
            !pair.is_empty() && !names.contains(&name)
        })
        .map(str::to_string)
        .collect();
    headers.remove(header::COOKIE);
    if !kept.is_empty() {
        if let Ok(value) = HeaderValue::from_str(&kept.join("; ")) {
            headers.insert(header::COOKIE, value);
        }
    }
}

/// Removes expired sessions once an hour.
pub fn spawn_session_cleanup(db: sqlx::AnyPool) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(3600));
        loop {
            interval.tick().await;
            let _ = sqlx::query("DELETE FROM sessions WHERE expires_at <= ?")
                .bind(chrono::Utc::now().timestamp())
                .execute(&db)
                .await;
        }
    });
}
