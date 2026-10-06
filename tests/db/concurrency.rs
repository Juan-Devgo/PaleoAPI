//! Rules hold under concurrent writes (AC 5, AC 11, AC 17).
//!
//! Each case proves the second transaction actually waited on the first
//! (`pg_blocking_pids`), so a sequential run could not pass by accident.

use std::time::Duration;

use sqlx::pool::PoolConnection;
use sqlx::postgres::PgQueryResult;
use sqlx::{PgPool, Postgres};

use crate::support::{
    accepts, assert_violation, backend_pid, continent, count, country_with_continent, era, exec,
    genus_chain, period, species_with_period, wait_until_blocked, within,
};

type Conn = PoolConnection<Postgres>;

async fn begin(pool: &PgPool, isolation: &str) -> (Conn, i32) {
    let mut c = pool.acquire().await.unwrap();
    exec(&mut *c, format!("BEGIN ISOLATION LEVEL {isolation}"))
        .await
        .unwrap();
    exec(&mut *c, "SET LOCAL lock_timeout = '8s'")
        .await
        .unwrap();
    let pid = backend_pid(&mut c).await;
    (c, pid)
}

/// tx1 runs `tx1_stmts`; tx2 runs `tx2_pre`, then `tx2_blocking` must block
/// on tx1 until tx1 ends with `tx1_end`. Returns tx2's connection (still in
/// its transaction) and the blocking statement's result.
async fn race(
    pool: &PgPool,
    isolation: &str,
    tx1_stmts: &[&str],
    tx2_pre: &[&str],
    tx2_blocking: &str,
    tx1_end: &str,
) -> (Conn, sqlx::Result<PgQueryResult>) {
    let (mut c1, p1) = begin(pool, isolation).await;
    let (mut c2, p2) = begin(pool, isolation).await;
    let mut observer = pool.acquire().await.unwrap();

    for s in tx1_stmts {
        within(exec(&mut *c1, *s))
            .await
            .unwrap_or_else(|e| panic!("tx1: {s}: {e}"));
    }
    for s in tx2_pre {
        within(exec(&mut *c2, *s))
            .await
            .unwrap_or_else(|e| panic!("tx2: {s}: {e}"));
    }
    let blocking = tx2_blocking.to_string();
    let handle = tokio::spawn(async move {
        let r = exec(&mut *c2, blocking).await;
        (c2, r)
    });

    assert!(
        wait_until_blocked(&mut observer, p2, p1, Duration::from_secs(5)).await,
        "tx2 ({tx2_blocking}) never waited on tx1"
    );
    assert!(!handle.is_finished(), "tx2 finished before tx1 ended");

    within(exec(&mut *c1, tx1_end)).await.unwrap();
    within(handle).await.unwrap()
}

async fn end(mut c: Conn, sql: &str) -> sqlx::Result<PgQueryResult> {
    within(exec(&mut *c, sql)).await
}

// (a) overlapping eras
#[sqlx::test]
async fn concurrent_overlapping_eras_cannot_both_commit(pool: PgPool) {
    let (c2, r) = race(
        &pool,
        "READ COMMITTED",
        &["INSERT INTO eras VALUES ('mesozoic', 'Mesozoic', 251.902, 66.0)"],
        &[],
        "INSERT INTO eras VALUES ('overlap', 'Overlap', 70.0, 0)",
        "COMMIT",
    )
    .await;
    assert_violation(r, "23P01", Some("eras_range_ex"));
    end(c2, "ROLLBACK").await.unwrap();
    assert_eq!(count(&pool, "SELECT count(*) FROM eras").await, 1);
}

#[sqlx::test]
async fn overlapping_era_succeeds_when_first_rolls_back(pool: PgPool) {
    let (c2, r) = race(
        &pool,
        "READ COMMITTED",
        &["INSERT INTO eras VALUES ('mesozoic', 'Mesozoic', 251.902, 66.0)"],
        &[],
        "INSERT INTO eras VALUES ('overlap', 'Overlap', 70.0, 0)",
        "ROLLBACK",
    )
    .await;
    r.unwrap();
    end(c2, "COMMIT").await.unwrap();
    assert_eq!(
        count(&pool, "SELECT count(*) FROM eras WHERE id = 'overlap'").await,
        1
    );
}

// (b) overlapping periods of one era
#[sqlx::test]
async fn concurrent_overlapping_periods_cannot_both_commit(pool: PgPool) {
    era(&pool, "mesozoic", "251.902", "66.0").await.unwrap();
    let (c2, r) = race(
        &pool,
        "READ COMMITTED",
        &["INSERT INTO periods VALUES ('jurassic', 'mesozoic', 'Jurassic', 201.4, 145)"],
        &[],
        "INSERT INTO periods VALUES ('overlap', 'mesozoic', 'Overlap', 150, 140)",
        "COMMIT",
    )
    .await;
    assert_violation(r, "23P01", Some("periods_range_ex"));
    end(c2, "ROLLBACK").await.unwrap();
    assert_eq!(count(&pool, "SELECT count(*) FROM periods").await, 1);
}

#[sqlx::test]
async fn overlapping_period_succeeds_when_first_rolls_back(pool: PgPool) {
    era(&pool, "mesozoic", "251.902", "66.0").await.unwrap();
    let (c2, r) = race(
        &pool,
        "READ COMMITTED",
        &["INSERT INTO periods VALUES ('jurassic', 'mesozoic', 'Jurassic', 201.4, 145)"],
        &[],
        "INSERT INTO periods VALUES ('overlap', 'mesozoic', 'Overlap', 150, 140)",
        "ROLLBACK",
    )
    .await;
    r.unwrap();
    end(c2, "COMMIT").await.unwrap();
}

// (c) era shrinks while a period is inserted into the removed part
#[sqlx::test]
async fn period_insert_waits_for_era_shrink(pool: PgPool) {
    era(&pool, "mesozoic", "251.902", "66.0").await.unwrap();
    let (c2, r) = race(
        &pool,
        "READ COMMITTED",
        &["UPDATE eras SET end_mya = 100 WHERE id = 'mesozoic'"],
        &[],
        "INSERT INTO periods VALUES ('cretaceous', 'mesozoic', 'Cretaceous', 145, 66.0)",
        "COMMIT",
    )
    .await;
    assert_violation(r, "23514", Some("periods_within_era_ck"));
    end(c2, "ROLLBACK").await.unwrap();
    assert_eq!(count(&pool, "SELECT count(*) FROM periods").await, 0);
}

// (d) period inserted while the era shrinks around it
#[sqlx::test]
async fn era_shrink_waits_for_period_insert(pool: PgPool) {
    era(&pool, "mesozoic", "251.902", "66.0").await.unwrap();
    let (c2, r) = race(
        &pool,
        "READ COMMITTED",
        &["INSERT INTO periods VALUES ('cretaceous', 'mesozoic', 'Cretaceous', 145, 66.0)"],
        &[],
        "UPDATE eras SET end_mya = 100 WHERE id = 'mesozoic'",
        "COMMIT",
    )
    .await;
    assert_violation(r, "23514", Some("eras_contains_periods_ck"));
    end(c2, "ROLLBACK").await.unwrap();
    let end_mya: String = sqlx::query_scalar("SELECT end_mya::text FROM eras")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(end_mya, "66.0");
}

// (e) two transactions each remove one of a country's last two continents
#[sqlx::test]
async fn concurrent_removal_of_last_two_continents(pool: PgPool) {
    continent(&pool, "europe", "modern").await.unwrap();
    continent(&pool, "asia", "modern").await.unwrap();
    country_with_continent(&pool, "turkey", "europe")
        .await
        .unwrap();
    accepts(
        &pool,
        "INSERT INTO country_continents (country_id, continent_id) VALUES ('turkey', 'asia')",
    )
    .await;

    let (c2, r) = race(
        &pool,
        "READ COMMITTED",
        &[
            "DELETE FROM country_continents WHERE continent_id = 'europe'",
            "SET CONSTRAINTS country_continents_min_trg IMMEDIATE",
        ],
        &["DELETE FROM country_continents WHERE continent_id = 'asia'"],
        "COMMIT",
        "COMMIT",
    )
    .await;
    assert_violation(r, "23514", Some("countries_min_continents_ck"));
    drop(c2);
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM country_continents WHERE country_id = 'turkey'"
        )
        .await,
        1
    );
}

// (f) the same for a species' last two periods
#[sqlx::test]
async fn concurrent_removal_of_last_two_periods(pool: PgPool) {
    era(&pool, "mesozoic", "251.902", "66.0").await.unwrap();
    period(&pool, "jurassic", "mesozoic", "201.4", "145")
        .await
        .unwrap();
    period(&pool, "cretaceous", "mesozoic", "145", "66.0")
        .await
        .unwrap();
    let genus = genus_chain(&pool, "cc").await;
    species_with_period(&pool, "t-rex", &genus, "jurassic")
        .await
        .unwrap();
    accepts(
        &pool,
        "INSERT INTO species_periods VALUES ('t-rex', 'cretaceous')",
    )
    .await;

    let (c2, r) = race(
        &pool,
        "READ COMMITTED",
        &[
            "DELETE FROM species_periods WHERE period_id = 'jurassic'",
            "SET CONSTRAINTS species_periods_min_trg IMMEDIATE",
        ],
        &["DELETE FROM species_periods WHERE period_id = 'cretaceous'"],
        "COMMIT",
        "COMMIT",
    )
    .await;
    assert_violation(r, "23514", Some("species_min_periods_ck"));
    drop(c2);
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM species_periods WHERE species_id = 't-rex'"
        )
        .await,
        1
    );
}

// (g) REPEATABLE READ writes are refused by the guard
#[sqlx::test]
async fn repeatable_read_writes_are_refused(pool: PgPool) {
    era(&pool, "mesozoic", "251.902", "66.0").await.unwrap();
    let (mut c, _) = begin(&pool, "REPEATABLE READ").await;
    assert_violation(
        exec(
            &mut *c,
            "INSERT INTO periods VALUES ('cretaceous', 'mesozoic', 'Cretaceous', 145, 66.0)",
        )
        .await,
        "0A000",
        None,
    );
    exec(&mut *c, "ROLLBACK").await.unwrap();
}

// (h) case (c) under SERIALIZABLE
#[sqlx::test]
async fn serializable_period_insert_fails_to_serialize(pool: PgPool) {
    era(&pool, "mesozoic", "251.902", "66.0").await.unwrap();
    let (c2, r) = race(
        &pool,
        "SERIALIZABLE",
        &["UPDATE eras SET end_mya = 100 WHERE id = 'mesozoic'"],
        &[],
        "INSERT INTO periods VALUES ('cretaceous', 'mesozoic', 'Cretaceous', 145, 66.0)",
        "COMMIT",
    )
    .await;
    assert_violation(r, "40001", None);
    end(c2, "ROLLBACK").await.unwrap();
    assert_eq!(count(&pool, "SELECT count(*) FROM periods").await, 0);
}
