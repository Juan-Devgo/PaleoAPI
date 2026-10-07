//! Database migrations and startup checks (FR-003, FR-004, FR-016).

use std::collections::HashMap;
use std::fmt;
use std::time::{Duration, Instant};

use actix_web::rt::time::{sleep, timeout};
use sqlx::migrate::{Migrate, Migration, Migrator};
use sqlx::postgres::PgConnectOptions;
use sqlx::{Connection, PgConnection};

/// Bounded startup wait for the database (FR-003, research R5).
pub const STARTUP_WAIT: Duration = Duration::from_secs(10);

/// Migrations embedded at build time. The API only compares against them;
/// it never applies them (FR-004).
pub static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

/// Why the API refuses to start. Messages never contain the connection URL,
/// credentials, host, or driver text (FR-003).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartupError {
    MissingConfig,
    InvalidConfig,
    Unreachable,
    AuthFailed,
    DatabaseMissing,
    MigrationsPending { versions: Vec<i64> },
    MigrationDirty(i64),
    MigrationModified(i64),
}

impl fmt::Display for StartupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingConfig => f.write_str(
                "DATABASE_URL is not set. Set it to a PostgreSQL URL (see .env.example).",
            ),
            Self::InvalidConfig => f.write_str(
                "DATABASE_URL is not a valid PostgreSQL connection URL. \
                 Set it to a URL like the one in .env.example.",
            ),
            Self::Unreachable => write!(
                f,
                "Could not connect to the database within {} s. \
                 Is it running? (docker compose up -d --wait db)",
                STARTUP_WAIT.as_secs()
            ),
            Self::AuthFailed => f.write_str(
                "The database rejected the configured credentials. \
                 Check the user and password in DATABASE_URL.",
            ),
            Self::DatabaseMissing => f.write_str(
                "The configured database does not exist. Check the database name in DATABASE_URL.",
            ),
            Self::MigrationsPending { versions } => {
                let list: Vec<String> = versions.iter().map(i64::to_string).collect();
                write!(
                    f,
                    "Database schema is out of date: {} migration(s) pending ({}). \
                     Run \"sqlx migrate run\", then start the API again.",
                    versions.len(),
                    list.join(", ")
                )
            }
            Self::MigrationDirty(v) => write!(
                f,
                "Migration {v} failed partway; fix it and rerun \"sqlx migrate run\"."
            ),
            Self::MigrationModified(v) => write!(
                f,
                "Migration {v} was modified after it was applied. \
                 Restore the original migration file or reset the database."
            ),
        }
    }
}

impl std::error::Error for StartupError {}

/// Connection options for the API pool: custom plans for every execution (research R3).
pub fn api_connect_options(_opts: PgConnectOptions) -> PgConnectOptions {
    todo!()
}

/// Pause between connection attempts (research R5).
const RETRY_EVERY: Duration = Duration::from_millis(500);

/// Connects, retrying every 500 ms until `deadline` has passed (research R5).
/// Credential and missing-database errors fail at once.
pub async fn connect_with_retry(
    opts: &PgConnectOptions,
    deadline: Duration,
) -> Result<PgConnection, StartupError> {
    let started = Instant::now();
    loop {
        let remaining = deadline.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            return Err(StartupError::Unreachable);
        }
        match timeout(remaining, PgConnection::connect_with(opts)).await {
            Ok(Ok(conn)) => return Ok(conn),
            Ok(Err(err)) => {
                if let Some(code) = err.as_database_error().and_then(|e| e.code()) {
                    match code.as_ref() {
                        "28P01" | "28000" => return Err(StartupError::AuthFailed),
                        "3D000" => return Err(StartupError::DatabaseMissing),
                        _ => {}
                    }
                }
            }
            Err(_elapsed) => return Err(StartupError::Unreachable),
        }
        let remaining = deadline.saturating_sub(started.elapsed());
        sleep(RETRY_EVERY.min(remaining)).await;
    }
}

/// Read-only comparison of applied and embedded migrations (research R4).
/// Never creates, locks, or writes the migrations table.
/// `Ok` carries warnings (applied versions this build does not know).
pub async fn verify_migrations(
    conn: &mut PgConnection,
    migrator: &Migrator,
) -> Result<Vec<String>, StartupError> {
    let embedded: Vec<&Migration> = migrator
        .iter()
        .filter(|m| m.migration_type.is_up_migration())
        .collect();

    let table: Option<String> = sqlx::query_scalar("SELECT to_regclass($1)::text")
        .bind(migrator.table_name.as_ref())
        .fetch_one(&mut *conn)
        .await
        .map_err(|_| StartupError::Unreachable)?;
    if table.is_none() {
        return Err(StartupError::MigrationsPending {
            versions: embedded.iter().map(|m| m.version).collect(),
        });
    }

    if let Some(version) = conn
        .dirty_version(&migrator.table_name)
        .await
        .map_err(|_| StartupError::Unreachable)?
    {
        return Err(StartupError::MigrationDirty(version));
    }

    let applied: HashMap<i64, Vec<u8>> = conn
        .list_applied_migrations(&migrator.table_name)
        .await
        .map_err(|_| StartupError::Unreachable)?
        .into_iter()
        .map(|m| (m.version, m.checksum.into_owned()))
        .collect();

    let mut pending = Vec::new();
    for m in &embedded {
        match applied.get(&m.version) {
            None => pending.push(m.version),
            Some(checksum) if checksum.as_slice() != m.checksum.as_ref() => {
                return Err(StartupError::MigrationModified(m.version));
            }
            Some(_) => {}
        }
    }
    if !pending.is_empty() {
        return Err(StartupError::MigrationsPending { versions: pending });
    }

    let mut unknown: Vec<i64> = applied
        .keys()
        .copied()
        .filter(|v| !embedded.iter().any(|m| m.version == *v))
        .collect();
    unknown.sort_unstable();
    Ok(unknown
        .into_iter()
        .map(|v| format!("Database has migration {v}, which this build does not know; continuing."))
        .collect())
}

/// A warning when `paleo_name_sort` changed version since it was created
/// (data-model §1.4).
pub async fn collation_version_warning(conn: &mut PgConnection) -> Option<String> {
    let drifted: Option<bool> = sqlx::query_scalar(
        "SELECT collversion IS DISTINCT FROM pg_collation_actual_version(oid) \
           FROM pg_collation WHERE collname = 'paleo_name_sort'",
    )
    .fetch_optional(conn)
    .await
    .ok()
    .flatten();
    drifted.filter(|d| *d).map(|_| {
        "Collation paleo_name_sort changed version; name-sorted pages may be misordered. \
         REINDEX the name-sort indexes, then ALTER COLLATION paleo_name_sort REFRESH VERSION \
         (see data-model §1.4)."
            .to_string()
    })
}
