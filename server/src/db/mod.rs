mod postgres;
mod sqlite;

use sqlx::any::AnyPoolOptions;
use sqlx::{AnyPool, Executor};

/// Version of the database layout. 2 = projects -> documents -> nodes (Trykst 0.2).
/// Databases from Trykst/TypstDrive 0.1.x have no version and are not migrated.
pub const SCHEMA_VERSION: i64 = 2;

/// Tables of the 0.1.x layout, children before parents so they can be dropped in order.
const LEGACY_TABLES: [&str; 21] = [
    "package_files",
    "package_versions",
    "packages",
    "space_collaborators",
    "space_files",
    "spaces",
    "api_key_usage_detail",
    "api_key_usage",
    "api_render_cache",
    "api_keys",
    "document_versions",
    "document_history",
    "comments",
    "invitations",
    "collaborators",
    "files",
    "documents",
    "folders",
    "sessions",
    "device_tokens",
    "users",
];

#[derive(Clone, Copy, PartialEq)]
pub enum DbKind {
    Sqlite,
    Postgres,
}

pub async fn init_db() -> AnyPool {
    sqlx::any::install_default_drivers();

    let db_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:password@127.0.0.1:5432/trykst".to_string());

    // DB_TYPE can override URL-based detection: "sqlite" or "postgres"
    let db_type = std::env::var("DB_TYPE").unwrap_or_else(|_| {
        if db_url.starts_with("sqlite") {
            "sqlite".to_string()
        } else {
            "postgres".to_string()
        }
    });
    let kind = match db_type.as_str() {
        "sqlite" => DbKind::Sqlite,
        "postgres" => DbKind::Postgres,
        other => panic!("Unknown DB_TYPE '{}'. Expected 'sqlite' or 'postgres'.", other),
    };

    let pool = AnyPoolOptions::new()
        .max_connections(5)
        // SQLite enforces foreign keys (and ON DELETE CASCADE) per connection,
        // so every pooled connection needs the pragma, not just the first one.
        .after_connect(move |conn, _meta| {
            Box::pin(async move {
                if kind == DbKind::Sqlite {
                    conn.execute("PRAGMA foreign_keys = ON").await?;
                }
                Ok(())
            })
        })
        .connect(&db_url)
        .await
        .expect("Failed to connect to database. Check DATABASE_URL.");

    guard_schema_version(&pool, kind).await;

    match kind {
        DbKind::Sqlite => sqlite::init_schema(&pool).await,
        DbKind::Postgres => postgres::init_schema(&pool).await,
    }

    sqlx::query(
        "INSERT INTO schema_meta (key, value) VALUES ('schema_version', $1) \
         ON CONFLICT (key) DO UPDATE SET value = excluded.value",
    )
    .bind(SCHEMA_VERSION.to_string())
    .execute(&pool)
    .await
    .expect("Failed to record schema version");

    pool
}

async fn table_exists(pool: &AnyPool, kind: DbKind, name: &str) -> bool {
    let sql = match kind {
        DbKind::Sqlite => "SELECT name FROM sqlite_master WHERE type = 'table' AND name = $1",
        // table_name is of type sql_identifier, which sqlx::Any cannot decode as text.
        DbKind::Postgres => {
            "SELECT table_name::text FROM information_schema.tables \
             WHERE table_schema = current_schema() AND table_name = $1"
        }
    };
    // A failing check must not read as "table missing": that would hide a legacy
    // database and run the new schema on top of it.
    match sqlx::query_scalar::<_, String>(sql).bind(name).fetch_optional(pool).await {
        Ok(row) => row.is_some(),
        Err(e) => fail(&format!("Could not inspect the database schema: {e}")),
    }
}

/// Refuses to start on a 0.1.x database (unless TRYKST_DROP_LEGACY_DATA=true) or on
/// a database written by a newer Trykst, instead of silently breaking or wiping it.
async fn guard_schema_version(pool: &AnyPool, kind: DbKind) {
    if table_exists(pool, kind, "schema_meta").await {
        let version: Option<String> =
            sqlx::query_scalar("SELECT value FROM schema_meta WHERE key = 'schema_version'")
                .fetch_optional(pool)
                .await
                .unwrap_or_else(|e| fail(&format!("Could not read the schema version: {e}")));
        let version: i64 = version.and_then(|v| v.parse().ok()).unwrap_or(0);
        if version > SCHEMA_VERSION {
            fail(&format!(
                "The database was created by a newer Trykst (schema version {version}, this build supports {SCHEMA_VERSION}). \
                 Update Trykst or restore a backup."
            ));
        }
        return;
    }

    let mut legacy = false;
    for table in ["users", "documents", "spaces", "folders"] {
        if table_exists(pool, kind, table).await {
            legacy = true;
            break;
        }
    }
    if !legacy {
        return; // empty database
    }

    let drop_requested = std::env::var("TRYKST_DROP_LEGACY_DATA")
        .map(|v| v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    if !drop_requested {
        fail(
            "This database was created by Trykst/TypstDrive 0.1.x. Trykst 0.2 introduced projects and is a \
             fresh start without data migration. Point DATABASE_URL at a new, empty database, or set \
             TRYKST_DROP_LEGACY_DATA=true to DELETE all existing data in this database.",
        );
    }

    tracing::warn!("TRYKST_DROP_LEGACY_DATA=true: deleting all data of the 0.1.x database");
    for table in LEGACY_TABLES {
        let sql = match kind {
            DbKind::Sqlite => format!("DROP TABLE IF EXISTS {table}"),
            DbKind::Postgres => format!("DROP TABLE IF EXISTS {table} CASCADE"),
        };
        if let Err(e) = sqlx::query(&sql).execute(pool).await {
            fail(&format!("Could not drop legacy table {table}: {e}"));
        }
    }
}

fn fail(message: &str) -> ! {
    tracing::error!("{message}");
    eprintln!("\n{message}\n");
    std::process::exit(1);
}
