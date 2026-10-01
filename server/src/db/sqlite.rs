use sqlx::AnyPool;

// Keep in sync with postgres.rs. Flags are 0/1 integers on both databases
// because sqlx::Any cannot decode a Postgres BOOLEAN into the i64 fields.
pub async fn init_schema(pool: &AnyPool) {
    let statements = [
        "CREATE TABLE IF NOT EXISTS schema_meta (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        )",
        "CREATE TABLE IF NOT EXISTS users (
            id TEXT PRIMARY KEY,
            username TEXT NOT NULL UNIQUE,
            email TEXT UNIQUE,
            oidc_issuer TEXT,
            oidc_subject TEXT,
            avatar_url TEXT,
            is_admin INTEGER NOT NULL DEFAULT 0,
            is_guest INTEGER NOT NULL DEFAULT 0,
            created_at TEXT DEFAULT (strftime('%Y-%m-%d %H:%M:%S', 'now'))
        )",
        "CREATE UNIQUE INDEX IF NOT EXISTS users_oidc_identity ON users(oidc_issuer, oidc_subject)",
        "CREATE TABLE IF NOT EXISTS sessions (
            id TEXT PRIMARY KEY,
            user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
            idp_session_id TEXT,
            profile_fingerprint TEXT,
            id_token TEXT,
            expires_at BIGINT NOT NULL
        )",
        "CREATE INDEX IF NOT EXISTS sessions_idp_session ON sessions(idp_session_id)",
        // --- Projects: the unit of membership and permissions -------------------
        "CREATE TABLE IF NOT EXISTS projects (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            description TEXT,
            created_by TEXT REFERENCES users(id) ON DELETE SET NULL,
            created_at TEXT DEFAULT (strftime('%Y-%m-%d %H:%M:%S', 'now')),
            updated_at TEXT DEFAULT (strftime('%Y-%m-%d %H:%M:%S', 'now'))
        )",
        "CREATE TABLE IF NOT EXISTS project_members (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
            user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
            role TEXT NOT NULL CHECK (role IN ('owner', 'editor', 'viewer')),
            created_at TEXT DEFAULT (strftime('%Y-%m-%d %H:%M:%S', 'now')),
            UNIQUE(project_id, user_id)
        )",
        "CREATE INDEX IF NOT EXISTS project_members_user ON project_members(user_id)",
        // --- Documents: a bundle of files that compiles to one output -----------
        "CREATE TABLE IF NOT EXISTS documents (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
            title TEXT NOT NULL,
            entrypoint_id TEXT REFERENCES nodes(id) ON DELETE SET NULL,
            thumbnail_svg TEXT,
            public_role TEXT CHECK (public_role IN ('viewer', 'editor')),
            created_by TEXT REFERENCES users(id) ON DELETE SET NULL,
            created_at TEXT DEFAULT (strftime('%Y-%m-%d %H:%M:%S', 'now')),
            updated_at TEXT DEFAULT (strftime('%Y-%m-%d %H:%M:%S', 'now'))
        )",
        "CREATE INDEX IF NOT EXISTS documents_project ON documents(project_id)",
        // --- Nodes: folders and files of a document (adjacency list) ------------
        // parent_id NULL = top level of the document. Text files hold a Yjs update,
        // binary files their raw bytes, folders no content.
        "CREATE TABLE IF NOT EXISTS nodes (
            id TEXT PRIMARY KEY,
            document_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
            parent_id TEXT REFERENCES nodes(id) ON DELETE CASCADE,
            name TEXT NOT NULL,
            kind TEXT NOT NULL CHECK (kind IN ('folder', 'text', 'binary')),
            content BLOB,
            mime_type TEXT,
            created_at TEXT DEFAULT (strftime('%Y-%m-%d %H:%M:%S', 'now')),
            updated_at TEXT DEFAULT (strftime('%Y-%m-%d %H:%M:%S', 'now'))
        )",
        // NULLs never collide in a plain UNIQUE constraint, so top-level names are
        // made unique through the expression.
        "CREATE UNIQUE INDEX IF NOT EXISTS nodes_unique_name ON nodes(document_id, COALESCE(parent_id, ''), name)",
        "CREATE INDEX IF NOT EXISTS nodes_parent ON nodes(parent_id)",
        "CREATE TABLE IF NOT EXISTS comments (
            id TEXT PRIMARY KEY,
            document_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
            node_id TEXT REFERENCES nodes(id) ON DELETE SET NULL,
            user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
            content TEXT NOT NULL,
            resolved INTEGER NOT NULL DEFAULT 0,
            created_at TEXT DEFAULT (strftime('%Y-%m-%d %H:%M:%S', 'now'))
        )",
        // A version is a snapshot of the whole file tree.
        "CREATE TABLE IF NOT EXISTS document_versions (
            id TEXT PRIMARY KEY,
            document_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
            user_id TEXT REFERENCES users(id) ON DELETE SET NULL,
            label TEXT,
            snapshot BLOB NOT NULL,
            created_at TEXT DEFAULT (strftime('%Y-%m-%d %H:%M:%S', 'now'))
        )",
        // --- Packages: per project (@project/...) or instance-wide (@trykst/...) --
        "CREATE TABLE IF NOT EXISTS packages (
            id TEXT PRIMARY KEY,
            project_id TEXT REFERENCES projects(id) ON DELETE CASCADE,
            owner_id TEXT REFERENCES users(id) ON DELETE SET NULL,
            name TEXT NOT NULL,
            description TEXT,
            created_at TEXT DEFAULT (strftime('%Y-%m-%d %H:%M:%S', 'now'))
        )",
        "CREATE UNIQUE INDEX IF NOT EXISTS packages_unique_name ON packages(COALESCE(project_id, ''), name)",
        "CREATE TABLE IF NOT EXISTS package_versions (
            id TEXT PRIMARY KEY,
            package_id TEXT NOT NULL REFERENCES packages(id) ON DELETE CASCADE,
            version TEXT NOT NULL,
            entrypoint TEXT NOT NULL DEFAULT 'lib.typ',
            manifest BLOB,
            created_at TEXT DEFAULT (strftime('%Y-%m-%d %H:%M:%S', 'now')),
            UNIQUE(package_id, version)
        )",
        "CREATE TABLE IF NOT EXISTS package_files (
            id TEXT PRIMARY KEY,
            version_id TEXT NOT NULL REFERENCES package_versions(id) ON DELETE CASCADE,
            path TEXT NOT NULL,
            data BLOB NOT NULL,
            UNIQUE(version_id, path)
        )",
        // --- Render API ------------------------------------------------------------
        "CREATE TABLE IF NOT EXISTS api_keys (
            id TEXT PRIMARY KEY,
            user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
            name TEXT NOT NULL,
            key_hash TEXT NOT NULL UNIQUE,
            key_prefix TEXT NOT NULL,
            created_at TEXT NOT NULL,
            last_used_at TEXT,
            rate_limit INTEGER NOT NULL DEFAULT 60
        )",
        "CREATE TABLE IF NOT EXISTS api_render_cache (
            id TEXT PRIMARY KEY,
            content_hash TEXT NOT NULL UNIQUE,
            format TEXT NOT NULL,
            data BLOB NOT NULL,
            created_at TEXT NOT NULL
        )",
        "CREATE TABLE IF NOT EXISTS api_key_usage (
            key_id TEXT NOT NULL REFERENCES api_keys(id) ON DELETE CASCADE,
            date TEXT NOT NULL,
            count INTEGER NOT NULL DEFAULT 1,
            PRIMARY KEY(key_id, date)
        )",
        "CREATE TABLE IF NOT EXISTS api_key_usage_detail (
            key_id TEXT NOT NULL REFERENCES api_keys(id) ON DELETE CASCADE,
            minute TEXT NOT NULL,
            count INTEGER NOT NULL DEFAULT 1,
            PRIMARY KEY(key_id, minute)
        )",
    ];

    for stmt in &statements {
        sqlx::query(stmt)
            .execute(pool)
            .await
            .unwrap_or_else(|e| panic!("Failed to execute SQLite schema: {e}\n{stmt}"));
    }
}
