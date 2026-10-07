//! Migration build, idempotence, and reversibility (AC 1, FR-005, FR-006, FR-013).

use paleo_api::db::MIGRATOR;
use sqlx::PgPool;

use crate::support::{assert_violation, exec};

/// Ordered, name-based description of every user schema object.
async fn fingerprint(pool: &PgPool) -> Vec<String> {
    sqlx::query_scalar(
        r#"
        SELECT x FROM (
          SELECT 'class ' || c.relname || ' ' || c.relkind::text AS x
            FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
           WHERE n.nspname = 'public'
          UNION ALL
          SELECT 'column ' || c.relname || '.' || a.attname || ' ' || format_type(a.atttypid, a.atttypmod)
                 || ' notnull=' || a.attnotnull || ' default=' || coalesce(pg_get_expr(d.adbin, d.adrelid), '')
                 || ' collation=' || coalesce(a.attcollation::regcollation::text, '')
            FROM pg_attribute a
            JOIN pg_class c ON c.oid = a.attrelid
            JOIN pg_namespace n ON n.oid = c.relnamespace
            LEFT JOIN pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum
           WHERE n.nspname = 'public' AND a.attnum > 0 AND NOT a.attisdropped
          UNION ALL
          SELECT 'constraint ' || conrelid::regclass::text || '.' || conname || ' ' || pg_get_constraintdef(oid)
                 || ' deferrable=' || condeferrable || ' deferred=' || condeferred
            FROM pg_constraint
           WHERE connamespace = 'public'::regnamespace
          UNION ALL
          SELECT 'index ' || pg_get_indexdef(i.indexrelid)
            FROM pg_index i JOIN pg_class c ON c.oid = i.indexrelid
           WHERE c.relnamespace = 'public'::regnamespace
          UNION ALL
          SELECT 'trigger ' || pg_get_triggerdef(t.oid)
            FROM pg_trigger t JOIN pg_class c ON c.oid = t.tgrelid
           WHERE c.relnamespace = 'public'::regnamespace AND NOT t.tgisinternal
          UNION ALL
          SELECT 'function ' || p.proname || '(' || pg_get_function_identity_arguments(p.oid) || ') '
                 || p.prokind::text || ' ' || md5(coalesce(p.prosrc, ''))
            FROM pg_proc p
           WHERE p.pronamespace = 'public'::regnamespace
          UNION ALL
          SELECT 'collation ' || collname || ' ' || collprovider::text || ' ' || collisdeterministic
                 || ' ' || coalesce(colllocale, '')
            FROM pg_collation
           WHERE collnamespace = 'public'::regnamespace
          UNION ALL
          SELECT 'extension ' || extname || ' ' || extnamespace::regnamespace::text
            FROM pg_extension
        ) f
        ORDER BY x
        "#,
    )
    .fetch_all(pool)
    .await
    .unwrap()
}

async fn applied_rows(pool: &PgPool) -> Vec<(i64, Vec<u8>)> {
    sqlx::query_as("SELECT version, checksum FROM _sqlx_migrations ORDER BY version")
        .fetch_all(pool)
        .await
        .unwrap()
}

#[sqlx::test(migrations = false)]
async fn migrations_build_full_schema_on_empty_database(pool: PgPool) {
    MIGRATOR
        .run(&pool)
        .await
        .expect("migrations must apply cleanly");
    let applied = applied_rows(&pool).await;
    let up: Vec<i64> = MIGRATOR
        .iter()
        .filter(|m| m.migration_type.is_up_migration())
        .map(|m| m.version)
        .collect();
    assert_eq!(applied.iter().map(|r| r.0).collect::<Vec<_>>(), up);
    assert_eq!(up.len(), 6, "data-model §7 defines six migrations");
}

#[sqlx::test(migrations = false)]
async fn rerunning_migrations_changes_nothing(pool: PgPool) {
    MIGRATOR.run(&pool).await.unwrap();
    let rows = applied_rows(&pool).await;
    let before = fingerprint(&pool).await;

    MIGRATOR.run(&pool).await.unwrap();

    assert_eq!(applied_rows(&pool).await, rows);
    assert_eq!(fingerprint(&pool).await, before);
}

#[sqlx::test(migrations = false)]
async fn migrations_are_reversible(pool: PgPool) {
    MIGRATOR.run(&pool).await.unwrap();
    let before = fingerprint(&pool).await;

    MIGRATOR
        .undo(&pool, 0)
        .await
        .expect("every down migration must apply");
    let empty: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM pg_class WHERE relnamespace = 'public'::regnamespace \
         AND relname <> '_sqlx_migrations' AND relname NOT LIKE '_sqlx_migrations%'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(empty, 0, "down migrations must remove every object");

    MIGRATOR.run(&pool).await.unwrap();
    assert_eq!(fingerprint(&pool).await, before);
}

#[sqlx::test(migrations = false)]
async fn foundation_creates_shared_objects(pool: PgPool) {
    MIGRATOR.run(&pool).await.unwrap();

    let trgm: i64 =
        sqlx::query_scalar("SELECT count(*) FROM pg_extension WHERE extname = 'pg_trgm'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(trgm, 1);

    let (provider, deterministic, locale): (String, bool, Option<String>) = sqlx::query_as(
        "SELECT collprovider::text, collisdeterministic, colllocale \
           FROM pg_collation WHERE collname = 'paleo_name_sort'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(provider, "i");
    assert!(deterministic);
    assert_eq!(locale.as_deref(), Some("und"));

    for f in [
        "paleo_forbid_id_change",
        "paleo_forbid_truncate",
        "paleo_require_safe_isolation",
    ] {
        let n: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM pg_proc WHERE proname = $1 AND pronamespace = 'public'::regnamespace",
        )
        .bind(f)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(n, 1, "function {f} must exist");
    }
}

#[sqlx::test(migrations = false)]
async fn isolation_guard_rejects_repeatable_read(pool: PgPool) {
    MIGRATOR.run(&pool).await.unwrap();

    let mut tx = pool.begin().await.unwrap();
    exec(&mut *tx, "SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
        .await
        .unwrap();
    assert_violation(
        exec(&mut *tx, "SELECT paleo_require_safe_isolation()").await,
        "0A000",
        None,
    );
    tx.rollback().await.unwrap();

    for level in ["READ COMMITTED", "SERIALIZABLE"] {
        let mut tx = pool.begin().await.unwrap();
        exec(&mut *tx, format!("SET TRANSACTION ISOLATION LEVEL {level}"))
            .await
            .unwrap();
        exec(&mut *tx, "SELECT paleo_require_safe_isolation()")
            .await
            .unwrap_or_else(|e| panic!("guard must allow {level}: {e}"));
        tx.rollback().await.unwrap();
    }
}
