use axum::{
    routing::{delete, get, patch, post, put},
    Router,
};
use axum_extra::extract::cookie::Key;
use sqlx::AnyPool;
use std::sync::Arc;
use std::collections::HashMap;
use tokio::sync::Mutex;
use yrs_axum::broadcast::BroadcastGroup;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod access;
mod admin;
mod api_keys;
mod auth;
mod comments;
mod compiler;
mod db;
mod directory;
mod documents;
mod handlers;
mod models;
mod oidc;
mod packages;
mod projects;
mod public_api;
mod world;

use compiler::TypstCompiler;
use handlers::{compile_handler, export_handler, yjs_handler};

pub type RateLimiterMap = Arc<Mutex<HashMap<String, (u32, std::time::Instant)>>>;

#[derive(Clone)]
pub struct AppState {
    pub compiler: Arc<Mutex<TypstCompiler>>,
    pub bcast_map: Arc<Mutex<HashMap<String, Arc<BroadcastGroup>>>>,
    pub db: AnyPool,
    pub key: Key,
    pub oidc: Arc<oidc::Oidc>,
    pub directory: Arc<directory::Directory>,
    pub rate_limiter: RateLimiterMap,
}

impl axum::extract::FromRef<AppState> for Key {
    fn from_ref(state: &AppState) -> Self {
        state.key.clone()
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "server=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::info!("Starting Trykst Server");

    let db = db::init_db().await;

    
    let key = match std::env::var("COOKIE_SECRET") {
        Ok(secret) => {
            let bytes = secret.as_bytes();
            if bytes.len() < 64 {
                tracing::warn!("COOKIE_SECRET is shorter than 64 bytes; sessions will not persist across restarts");
                Key::generate()
            } else {
                Key::from(bytes)
            }
        }
        Err(_) => {
            tracing::warn!("COOKIE_SECRET not set; generating a random key. Sessions will be invalidated on restart.");
            Key::generate()
        }
    };

    let oidc_config = oidc::OidcConfig::from_env();
    let directory = Arc::new(directory::Directory::from_env(&oidc_config.issuer));
    let oidc = Arc::new(oidc::Oidc::new(oidc_config));
    oidc::spawn_session_cleanup(db.clone());

    let state = AppState {
        compiler: Arc::new(Mutex::new(TypstCompiler::new())),
        bcast_map: Arc::new(Mutex::new(HashMap::new())),
        db,
        key,
        oidc,
        directory,
        rate_limiter: Arc::new(Mutex::new(HashMap::new())),
    };

    let api_routes = Router::new()
        .route("/admin/users", get(admin::list_users))
        .route("/admin/users/{id}", patch(admin::update_user).delete(admin::delete_user))
        .route("/compile", post(compile_handler))
        .route("/export/{format}", post(export_handler))
        .route("/export/pandoc/{format}", post(handlers::pandoc_export_handler))
        .route("/import/pandoc", post(handlers::pandoc_import_handler))
        .route("/lsp/{id}", get(handlers::lsp_handler))
        .route("/auth/oidc/login", get(oidc::login))
        .route("/auth/oidc/callback", get(oidc::callback))
        .route("/auth/oidc/backchannel-logout", post(oidc::backchannel_logout))
        .route("/auth/logout", post(oidc::logout))
        .route("/auth/me", get(auth::me))
        .route("/auth/storage", get(auth::storage_stats))
        .route("/directory/search", get(directory::search))
        // Projects and members
        .route("/projects", get(projects::list_projects).post(projects::create_project))
        .route("/projects/{id}", get(projects::get_project).patch(projects::update_project).delete(projects::delete_project))
        .route("/projects/{id}/members", get(projects::list_members).post(projects::add_member))
        .route("/projects/{id}/members/{user_id}", patch(projects::update_member).delete(projects::remove_member))
        .route("/projects/{id}/documents", get(documents::list_documents).post(documents::create_document))
        .route("/projects/{id}/packages", get(packages::list_project_packages))
        // Documents and their file trees
        .route("/documents/{id}", get(documents::get_document).patch(documents::update_document).delete(documents::delete_document))
        .route("/documents/{id}/tree", get(documents::get_tree))
        .route("/documents/{id}/nodes", post(documents::create_node))
        .route("/documents/{id}/nodes/{node_id}", patch(documents::update_node).delete(documents::delete_node))
        .route("/documents/{id}/nodes/{node_id}/content", get(documents::get_node_content))
        .route("/documents/{id}/upload", post(documents::upload_files))
        .route("/documents/{id}/entrypoint", put(documents::set_entrypoint))
        .route("/documents/{id}/fonts", get(documents::list_fonts))
        .route("/documents/{id}/versions", get(documents::list_versions).post(documents::create_version))
        .route("/documents/{id}/versions/{version_id}", get(documents::get_version))
        .route("/documents/{id}/versions/{version_id}/restore", post(documents::restore_version))
        .route("/documents/{id}/comments", get(comments::list_comments).post(comments::add_comment))
        .route("/comments/{id}", patch(comments::update_comment).delete(comments::delete_comment))
        // API keys for the render API
        .route("/keys", get(api_keys::list_keys).post(api_keys::create_key))
        .route("/keys/usage", get(api_keys::get_aggregate_usage))
        .route("/keys/{id}", delete(api_keys::delete_key))
        .route("/keys/{id}/regenerate", post(api_keys::regenerate_key))
        // Packages: instance-wide list; project packages are listed per project
        .route("/packages", get(packages::list_instance_packages))
        .route("/packages/publish", post(packages::publish_package))
        .route("/packages/{id}", delete(packages::delete_package))
        .route("/packages/{id}/versions", get(packages::list_versions));


    let v1_routes = Router::new()
        .route("/render", post(public_api::render_handler));

    let yjs_routes = Router::new()
        .route("/{id}", get(yjs_handler));

    let session_guard = axum::middleware::from_fn_with_state(state.clone(), oidc::session_guard);

    let static_dir = std::env::var("STATIC_DIR").unwrap_or_else(|_| "../build".to_string());

    let app = Router::new()
        .nest("/api", api_routes.layer(session_guard.clone()).layer(TraceLayer::new_for_http()))
        .nest("/v1", v1_routes.layer(TraceLayer::new_for_http()))
        .nest("/yjs", yjs_routes.layer(session_guard).layer(TraceLayer::new_for_http()))
        .fallback_service(ServeDir::new(&static_dir).fallback(ServeFile::new(format!("{}/index.html", static_dir))))
        .with_state(state);

    let port = std::env::var("PORT").unwrap_or_else(|_| "3000".to_string());
    let addr = format!("0.0.0.0:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .unwrap();
    tracing::info!("Server listening on http://{}", addr);
    axum::serve(listener, app).await.unwrap();
}

