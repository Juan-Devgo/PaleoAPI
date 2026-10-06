//! Startup checks called directly (FR-003, FR-004, FR-016).

use std::time::{Duration, Instant};

use paleo_api::db::{
    MIGRATOR, StartupError, collation_version_warning, connect_with_retry, verify_migrations,
};
use sqlx::PgPool;
use sqlx::postgres::PgConnectOptions;

use crate::support::exec;

fn up_versions() -> Vec<i64> {
    MIGRATOR
        .iter()
        .filter(|m| m.migration_type.is_up_migration())
        .map(|m| m.version)
        .collect()
}

async fn migration_rows(pool: &PgPool) -> Vec<String> {
    sqlx::query_scalar("SELECT row_to_json(m)::text FROM _sqlx_migrations m ORDER BY version")
        .fetch_all(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn unreachable_database_fails_within_deadline() {
    let opts = PgConnectOptions::new()
        .host("127.0.0.1")
        .port(9)
        .username("nobody")
        .database("nothing");
    let started = Instant::now();
    let err = connect_with_retry(&opts, Duration::from_secs(1))
        .await
        .unwrap_err();
    assert_eq!(err, StartupError::Unreachable);
    assert!(
        started.elapsed() < Duration::from_millis(1500),
        "took {:?}",
        started.elapsed()
    );
}

#[sqlx::test]
async fn connects_to_a_running_database(pool: PgPool) {
    let opts = pool.connect_options();
    let mut conn = connect_with_retry(&opts, Duration::from_secs(5))
        .await
        .unwrap();
    exec(&mut conn, "SELECT 1").await.unwrap();
}

#[sqlx::test(migrations = false)]
async fn unmigrated_database_reports_every_migration_pending(pool: PgPool) {
    let mut conn = pool.acquire().await.unwrap();
    let err = verify_migrations(&mut conn, &MIGRATOR).await.unwrap_err();
    assert_eq!(
        err,
        StartupError::MigrationsPending {
            versions: up_versions()
        }
    );
    let table: Option<String> = sqlx::query_scalar("SELECT to_regclass('_sqlx_migrations')::text")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        table, None,
        "the check must not create the migrations table"
    );
}

#[sqlx::test]
async fn fully_migrated_database_passes(pool: PgPool) {
    let before = migration_rows(&pool).await;
    let mut conn = pool.acquire().await.unwrap();
    assert_eq!(verify_migrations(&mut conn, &MIGRATOR).await, Ok(vec![]));
    assert_eq!(migration_rows(&pool).await, before);
}

#[sqlx::test]
async fn missing_last_migration_is_pending(pool: PgPool) {
    let last = *up_versions().last().unwrap();
    exec(
        &pool,
        format!("DELETE FROM _sqlx_migrations WHERE version = {last}"),
    )
    .await
    .unwrap();
    let before = migration_rows(&pool).await;
    let mut conn = pool.acquire().await.unwrap();
    assert_eq!(
        verify_migrations(&mut conn, &MIGRATOR).await,
        Err(StartupError::MigrationsPending {
            versions: vec![last]
        })
    );
    assert_eq!(migration_rows(&pool).await, before);
}

#[sqlx::test]
async fn failed_migration_is_dirty(pool: PgPool) {
    let v = up_versions()[1];
    exec(
        &pool,
        format!("UPDATE _sqlx_migrations SET success = false WHERE version = {v}"),
    )
    .await
    .unwrap();
    let before = migration_rows(&pool).await;
    let mut conn = pool.acquire().await.unwrap();
    assert_eq!(
        verify_migrations(&mut conn, &MIGRATOR).await,
        Err(StartupError::MigrationDirty(v))
    );
    assert_eq!(migration_rows(&pool).await, before);
}

#[sqlx::test]
async fn edited_migration_is_modified(pool: PgPool) {
    let v = up_versions()[0];
    exec(
        &pool,
        format!("UPDATE _sqlx_migrations SET checksum = '\\x00'::bytea WHERE version = {v}"),
    )
    .await
    .unwrap();
    let before = migration_rows(&pool).await;
    let mut conn = pool.acquire().await.unwrap();
    assert_eq!(
        verify_migrations(&mut conn, &MIGRATOR).await,
        Err(StartupError::MigrationModified(v))
    );
    assert_eq!(migration_rows(&pool).await, before);
}

#[sqlx::test]
async fn unknown_newer_migration_is_only_a_warning(pool: PgPool) {
    exec(
        &pool,
        "INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time) \
         VALUES (99991231235959, 'from the future', true, '\\x00'::bytea, 0)",
    )
    .await
    .unwrap();
    let before = migration_rows(&pool).await;
    let mut conn = pool.acquire().await.unwrap();
    let warnings = verify_migrations(&mut conn, &MIGRATOR).await.unwrap();
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("99991231235959"), "{}", warnings[0]);
    assert_eq!(migration_rows(&pool).await, before);
}

#[sqlx::test]
async fn collation_drift_is_reported(pool: PgPool) {
    let mut conn = pool.acquire().await.unwrap();
    assert_eq!(collation_version_warning(&mut conn).await, None);
    exec(
        &mut *conn,
        "UPDATE pg_collation SET collversion = '0' WHERE collname = 'paleo_name_sort'",
    )
    .await
    .unwrap();
    let warning = collation_version_warning(&mut conn)
        .await
        .expect("drift must warn");
    assert!(warning.contains("paleo_name_sort"), "{warning}");
}
