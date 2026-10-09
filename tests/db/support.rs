//! Shared helpers for the database integration tests (plan §Test strategy).

#![allow(dead_code)]

use std::time::Duration;

use sqlx::postgres::PgQueryResult;
use sqlx::{AssertSqlSafe, PgConnection, PgExecutor, PgPool};

/// The 12 resource tables, parents before children.
pub const RESOURCE_TABLES: [&str; 12] = [
    "eras",
    "periods",
    "domains",
    "kingdoms",
    "phyla",
    "classes",
    "orders",
    "families",
    "genera",
    "continents",
    "countries",
    "species",
];

/// The 4 link tables.
pub const LINK_TABLES: [&str; 4] = [
    "country_continents",
    "species_periods",
    "species_continents",
    "species_countries",
];

/// The account table (003 data-model §1).
pub const ACCOUNT_TABLES: [&str; 1] = ["accounts"];

/// Every table of the schema (resource, link, and account).
pub fn all_tables() -> Vec<&'static str> {
    RESOURCE_TABLES
        .iter()
        .chain(LINK_TABLES.iter())
        .chain(ACCOUNT_TABLES.iter())
        .copied()
        .collect()
}

/// Taxonomy ranks top-down: (table, parent column, parent table).
pub const RANKS: [(&str, Option<(&str, &str)>); 7] = [
    ("domains", None),
    ("kingdoms", Some(("domain_id", "domains"))),
    ("phyla", Some(("kingdom_id", "kingdoms"))),
    ("classes", Some(("phylum_id", "phyla"))),
    ("orders", Some(("class_id", "classes"))),
    ("families", Some(("order_id", "orders"))),
    ("genera", Some(("family_id", "families"))),
];

/// Checks that `result` failed with `sqlstate` and, when given, `constraint`.
pub fn check_violation<T: std::fmt::Debug>(
    result: Result<T, sqlx::Error>,
    sqlstate: &str,
    constraint: Option<&str>,
) -> Result<(), String> {
    let err = match result {
        Ok(v) => {
            return Err(format!(
                "expected SQLSTATE {sqlstate} ({constraint:?}), but the write succeeded: {v:?}"
            ));
        }
        Err(e) => e,
    };
    let Some(db) = err.as_database_error() else {
        return Err(format!("expected a database error {sqlstate}, got: {err}"));
    };
    if db.code().as_deref() != Some(sqlstate) {
        return Err(format!(
            "expected SQLSTATE {sqlstate} ({constraint:?}), got {:?}: {db}",
            db.code()
        ));
    }
    if let Some(expected) = constraint
        && db.constraint() != Some(expected)
    {
        return Err(format!(
            "expected constraint {expected}, got {:?}: {db}",
            db.constraint()
        ));
    }
    Ok(())
}

/// Asserts that `result` failed with `sqlstate` and, when given, `constraint`.
#[track_caller]
pub fn assert_violation<T: std::fmt::Debug>(
    result: Result<T, sqlx::Error>,
    sqlstate: &str,
    constraint: Option<&str>,
) {
    if let Err(msg) = check_violation(result, sqlstate, constraint) {
        panic!("{msg}");
    }
}

/// Like [`assert_violation`] with a constraint name.
#[track_caller]
pub fn assert_ck<T: std::fmt::Debug>(
    result: Result<T, sqlx::Error>,
    sqlstate: &str,
    constraint: &str,
) {
    assert_violation(result, sqlstate, Some(constraint));
}

/// Every row of `table` as JSON text, in a stable order.
pub async fn snapshot(pool: &PgPool, table: &str) -> Vec<String> {
    sqlx::query_scalar(AssertSqlSafe(format!(
        "SELECT row_to_json(t)::text FROM {table} t ORDER BY 1"
    )))
    .fetch_all(pool)
    .await
    .unwrap()
}

/// Snapshot of every table, for "stored data unchanged" checks.
pub async fn snapshot_all(pool: &PgPool) -> Vec<(String, Vec<String>)> {
    let mut out = Vec::new();
    for t in all_tables() {
        out.push((t.to_string(), snapshot(pool, t).await));
    }
    out
}

/// Runs `sql` and returns the result (any executor).
pub async fn exec<'e>(
    e: impl PgExecutor<'e>,
    sql: impl Into<String>,
) -> sqlx::Result<PgQueryResult> {
    sqlx::query(AssertSqlSafe(sql.into())).execute(e).await
}

/// Runs a multi-statement script without parameters.
pub async fn exec_script<'e>(e: impl PgExecutor<'e>, sql: impl Into<String>) -> sqlx::Result<()> {
    sqlx::raw_sql(AssertSqlSafe(sql.into()))
        .execute(e)
        .await
        .map(|_| ())
}

/// Count rows matching `sql` (a full `SELECT count(*) ...`).
pub async fn count<'e>(e: impl PgExecutor<'e>, sql: impl Into<String>) -> i64 {
    sqlx::query_scalar(AssertSqlSafe(sql.into()))
        .fetch_one(e)
        .await
        .unwrap()
}

/// Runs one invalid statement and asserts it fails with `sqlstate`/`constraint`
/// and leaves every table unchanged (AC 3).
#[track_caller]
pub fn rejects<'a>(
    pool: &'a PgPool,
    sql: impl Into<String>,
    sqlstate: &'a str,
    constraint: Option<&'a str>,
) -> impl std::future::Future<Output = ()> + 'a {
    let sql = sql.into();
    let caller = std::panic::Location::caller();
    async move {
        let before = snapshot_all(pool).await;
        let result = exec(pool, sql.clone()).await;
        if let Err(msg) = check_violation(result, sqlstate, constraint) {
            panic!("{caller}: {msg}\n  statement: {sql}");
        }
        assert_eq!(
            snapshot_all(pool).await,
            before,
            "{caller}: rejected write changed data: {sql}"
        );
    }
}

/// Runs the statements in one transaction and asserts that `COMMIT` fails with
/// `sqlstate`/`constraint` (deferred rules) and that no data changed.
pub async fn commit_rejects(pool: &PgPool, stmts: &[&str], sqlstate: &str, constraint: &str) {
    let before = snapshot_all(pool).await;
    let mut tx = pool.begin().await.unwrap();
    for s in stmts {
        exec(&mut *tx, *s)
            .await
            .unwrap_or_else(|e| panic!("statement before COMMIT failed: {s}: {e}"));
    }
    assert_violation(tx.commit().await, sqlstate, Some(constraint));
    assert_eq!(
        snapshot_all(pool).await,
        before,
        "rejected transaction changed data"
    );
}

/// Runs the statements in one transaction and asserts that it commits.
pub async fn commit_accepts(pool: &PgPool, stmts: &[&str]) {
    let mut tx = pool.begin().await.unwrap();
    for s in stmts {
        exec(&mut *tx, *s)
            .await
            .unwrap_or_else(|e| panic!("statement failed: {s}: {e}"));
    }
    tx.commit()
        .await
        .unwrap_or_else(|e| panic!("COMMIT failed for {stmts:?}: {e}"));
}

/// Runs one statement that must succeed.
#[track_caller]
pub fn accepts<'a>(
    pool: &'a PgPool,
    sql: impl Into<String>,
) -> impl std::future::Future<Output = PgQueryResult> + 'a {
    let sql = sql.into();
    let caller = std::panic::Location::caller();
    async move {
        exec(pool, sql.clone())
            .await
            .unwrap_or_else(|e| panic!("{caller}: valid write failed: {sql}: {e}"))
    }
}

/// Current UTC year on the database clock.
pub async fn utc_year(pool: &PgPool) -> i32 {
    sqlx::query_scalar("SELECT extract(year FROM now() AT TIME ZONE 'UTC')::int")
        .fetch_one(pool)
        .await
        .unwrap()
}

// ---------------------------------------------------------------- builders

pub async fn era<'e>(
    e: impl PgExecutor<'e>,
    id: &str,
    start: &str,
    end: &str,
) -> sqlx::Result<PgQueryResult> {
    sqlx::query(
        "INSERT INTO eras (id, name, start_mya, end_mya) VALUES ($1, $1, $2::numeric, $3::numeric)",
    )
    .bind(id)
    .bind(start)
    .bind(end)
    .execute(e)
    .await
}

pub async fn period<'e>(
    e: impl PgExecutor<'e>,
    id: &str,
    era_id: &str,
    start: &str,
    end: &str,
) -> sqlx::Result<PgQueryResult> {
    sqlx::query(
        "INSERT INTO periods (id, era_id, name, start_mya, end_mya) \
         VALUES ($1, $2, $1, $3::numeric, $4::numeric)",
    )
    .bind(id)
    .bind(era_id)
    .bind(start)
    .bind(end)
    .execute(e)
    .await
}

/// Inserts a taxonomy row; `parent` is the parent id for every rank but `domains`.
pub async fn rank<'e>(
    e: impl PgExecutor<'e>,
    table: &str,
    id: &str,
    parent: Option<&str>,
) -> sqlx::Result<PgQueryResult> {
    let parent_col = RANKS
        .iter()
        .find(|(t, _)| *t == table)
        .unwrap_or_else(|| panic!("unknown rank {table}"))
        .1;
    match (parent_col, parent) {
        (None, _) => {
            sqlx::query(AssertSqlSafe(format!(
                "INSERT INTO {table} (id, name) VALUES ($1, $1)"
            )))
            .bind(id)
            .execute(e)
            .await
        }
        (Some((col, _)), p) => {
            sqlx::query(AssertSqlSafe(format!(
                "INSERT INTO {table} (id, name, {col}) VALUES ($1, $1, $2)"
            )))
            .bind(id)
            .bind(p)
            .execute(e)
            .await
        }
    }
}

pub async fn domain<'e>(e: impl PgExecutor<'e>, id: &str) -> sqlx::Result<PgQueryResult> {
    rank(e, "domains", id, None).await
}
pub async fn kingdom<'e>(
    e: impl PgExecutor<'e>,
    id: &str,
    domain_id: &str,
) -> sqlx::Result<PgQueryResult> {
    rank(e, "kingdoms", id, Some(domain_id)).await
}
pub async fn phylum<'e>(
    e: impl PgExecutor<'e>,
    id: &str,
    kingdom_id: &str,
) -> sqlx::Result<PgQueryResult> {
    rank(e, "phyla", id, Some(kingdom_id)).await
}
pub async fn class<'e>(
    e: impl PgExecutor<'e>,
    id: &str,
    phylum_id: &str,
) -> sqlx::Result<PgQueryResult> {
    rank(e, "classes", id, Some(phylum_id)).await
}
pub async fn order<'e>(
    e: impl PgExecutor<'e>,
    id: &str,
    class_id: &str,
) -> sqlx::Result<PgQueryResult> {
    rank(e, "orders", id, Some(class_id)).await
}
pub async fn family<'e>(
    e: impl PgExecutor<'e>,
    id: &str,
    order_id: &str,
) -> sqlx::Result<PgQueryResult> {
    rank(e, "families", id, Some(order_id)).await
}
pub async fn genus<'e>(
    e: impl PgExecutor<'e>,
    id: &str,
    family_id: &str,
) -> sqlx::Result<PgQueryResult> {
    rank(e, "genera", id, Some(family_id)).await
}

/// Creates a full domain → genus chain with ids `<prefix>-<rank table>`
/// and returns the genus id.
pub async fn genus_chain(pool: &PgPool, prefix: &str) -> String {
    let mut parent: Option<String> = None;
    for (table, _) in RANKS {
        let id = format!("{prefix}-{table}");
        rank(pool, table, &id, parent.as_deref()).await.unwrap();
        parent = Some(id);
    }
    parent.unwrap()
}

pub async fn continent<'e>(
    e: impl PgExecutor<'e>,
    id: &str,
    kind: &str,
) -> sqlx::Result<PgQueryResult> {
    sqlx::query("INSERT INTO continents (id, name, type) VALUES ($1, $1, $2)")
        .bind(id)
        .bind(kind)
        .execute(e)
        .await
}

/// Inserts a country and its first continent link in one statement.
pub async fn country_with_continent<'e>(
    e: impl PgExecutor<'e>,
    id: &str,
    continent_id: &str,
) -> sqlx::Result<PgQueryResult> {
    sqlx::query(
        "WITH c AS (INSERT INTO countries (id, name) VALUES ($1, $1) RETURNING id) \
         INSERT INTO country_continents (country_id, continent_id) SELECT id, $2 FROM c",
    )
    .bind(id)
    .bind(continent_id)
    .execute(e)
    .await
}

/// Inserts a species (valid defaults) and its first period link in one statement.
pub async fn species_with_period<'e>(
    e: impl PgExecutor<'e>,
    id: &str,
    genus_id: &str,
    period_id: &str,
) -> sqlx::Result<PgQueryResult> {
    sqlx::query(
        "WITH s AS ( \
           INSERT INTO species (id, genus_id, name, scientific_name, diet, description) \
           VALUES ($1, $2, $1, $1, 'carnivore', 'A test species.') RETURNING id) \
         INSERT INTO species_periods (species_id, period_id) SELECT id, $3 FROM s",
    )
    .bind(id)
    .bind(genus_id)
    .bind(period_id)
    .execute(e)
    .await
}

/// Ids created by [`full_chain`].
pub struct Chain {
    pub era: String,
    pub period: String,
    pub genus: String,
    pub continent: String,
    pub prehistoric: String,
    pub country: String,
    pub species: String,
}

/// One row in every table: era, period, taxonomy chain, a modern and a
/// prehistoric continent, a country, and a species linked to all of them.
pub async fn full_chain(pool: &PgPool, prefix: &str) -> Chain {
    let c = Chain {
        era: format!("{prefix}-era"),
        period: format!("{prefix}-period"),
        genus: format!("{prefix}-genera"),
        continent: format!("{prefix}-modern"),
        prehistoric: format!("{prefix}-pangaea"),
        country: format!("{prefix}-country"),
        species: format!("{prefix}-species"),
    };
    era(pool, &c.era, "251.902", "66").await.unwrap();
    period(pool, &c.period, &c.era, "201.4", "145")
        .await
        .unwrap();
    genus_chain(pool, prefix).await;
    continent(pool, &c.continent, "modern").await.unwrap();
    continent(pool, &c.prehistoric, "prehistoric")
        .await
        .unwrap();
    country_with_continent(pool, &c.country, &c.continent)
        .await
        .unwrap();
    species_with_period(pool, &c.species, &c.genus, &c.period)
        .await
        .unwrap();
    exec(
        pool,
        format!(
            "INSERT INTO species_continents VALUES ('{0}', '{1}'), ('{0}', '{2}')",
            c.species, c.continent, c.prehistoric
        ),
    )
    .await
    .unwrap();
    exec(
        pool,
        format!(
            "INSERT INTO species_countries VALUES ('{}', '{}')",
            c.species, c.country
        ),
    )
    .await
    .unwrap();
    c
}

// ---------------------------------------------------------------- planner

/// `EXPLAIN (COSTS OFF)` of `sql`, optionally with `enable_seqscan = off`.
pub async fn explain(pool: &PgPool, sql: &str, seqscan_off: bool) -> String {
    let mut tx = pool.begin().await.unwrap();
    if seqscan_off {
        exec(&mut *tx, "SET LOCAL enable_seqscan = off")
            .await
            .unwrap();
    }
    let lines: Vec<String> =
        sqlx::query_scalar(AssertSqlSafe(format!("EXPLAIN (COSTS OFF) {sql}")))
            .fetch_all(&mut *tx)
            .await
            .unwrap_or_else(|e| panic!("EXPLAIN failed for {sql}: {e}"));
    tx.rollback().await.unwrap();
    lines.join("\n")
}

/// `EXPLAIN (ANALYZE, COSTS OFF, TIMING OFF, SUMMARY OFF)` of `sql`.
pub async fn explain_analyze(pool: &PgPool, sql: &str, seqscan_off: bool) -> String {
    let mut tx = pool.begin().await.unwrap();
    if seqscan_off {
        exec(&mut *tx, "SET LOCAL enable_seqscan = off")
            .await
            .unwrap();
    }
    let lines: Vec<String> = sqlx::query_scalar(AssertSqlSafe(format!(
        "EXPLAIN (ANALYZE, COSTS OFF, TIMING OFF, SUMMARY OFF) {sql}"
    )))
    .fetch_all(&mut *tx)
    .await
    .unwrap_or_else(|e| panic!("EXPLAIN ANALYZE failed for {sql}: {e}"));
    tx.rollback().await.unwrap();
    lines.join("\n")
}

/// Rows returned by `Bitmap Index Scan on <index>` in an analyzed plan.
pub fn index_rows(plan: &str, index: &str) -> f64 {
    let marker = format!("Bitmap Index Scan on {index} ");
    let line = plan
        .lines()
        .find(|l| l.contains(&marker))
        .unwrap_or_else(|| panic!("no Bitmap Index Scan on {index} in plan:\n{plan}"));
    let rows = line
        .split("actual rows=")
        .nth(1)
        .and_then(|r| r.split_whitespace().next())
        .unwrap_or_else(|| panic!("no actual rows in line: {line}"));
    rows.trim_end_matches(')').parse().unwrap()
}

/// `Rows Removed by Index Recheck` values in an analyzed plan.
pub fn recheck_removed(plan: &str) -> Vec<f64> {
    plan.lines()
        .filter_map(|l| l.split("Rows Removed by Index Recheck: ").nth(1))
        .map(|n| n.trim().parse().unwrap())
        .collect()
}

/// Index names used by scans in a plan.
fn plan_indexes(plan: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in plan.lines() {
        for marker in [" using ", "Bitmap Index Scan on "] {
            if let Some(rest) = line.split(marker).nth(1)
                && let Some(name) = rest.split_whitespace().next()
            {
                out.push(name.to_string());
            }
        }
    }
    out
}

/// Asserts that the plan uses at least one of `allowed`.
#[track_caller]
pub fn assert_uses_any_index(shape: &str, plan: &str, allowed: &[&str]) {
    let used = plan_indexes(plan);
    assert!(
        used.iter().any(|u| allowed.contains(&u.as_str())),
        "{shape}: plan uses none of {allowed:?} (uses {used:?}):\n{plan}"
    );
}

/// Asserts that the plan has no `Seq Scan` on any of `tables`.
#[track_caller]
pub fn assert_no_seq_scan_on(shape: &str, plan: &str, tables: &[&str]) {
    for line in plan.lines() {
        if let Some(rest) = line.split("Seq Scan on ").nth(1) {
            let table = rest.split_whitespace().next().unwrap_or("");
            assert!(
                !tables.contains(&table),
                "{shape}: Seq Scan on {table}:\n{plan}"
            );
        }
    }
}

// ---------------------------------------------------------------- concurrency

pub async fn backend_pid(conn: &mut PgConnection) -> i32 {
    sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(conn)
        .await
        .unwrap()
}

/// Polls every 50 ms until `blocked` waits on a lock held by `blocker`.
pub async fn wait_until_blocked(
    observer: &mut PgConnection,
    blocked: i32,
    blocker: i32,
    timeout: Duration,
) -> bool {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let is_blocked: bool = sqlx::query_scalar("SELECT $2 = ANY(pg_blocking_pids($1))")
            .bind(blocked)
            .bind(blocker)
            .fetch_one(&mut *observer)
            .await
            .unwrap();
        if is_blocked {
            return true;
        }
        if tokio::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Awaits `fut` with the 10 s per-step limit of the concurrency tests.
pub async fn within<F: std::future::Future>(fut: F) -> F::Output {
    tokio::time::timeout(Duration::from_secs(10), fut)
        .await
        .expect("step did not finish within 10 s")
}

#[sqlx::test]
async fn smoke_select_one(pool: PgPool) {
    let one: i32 = sqlx::query_scalar("SELECT 1")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(one, 1);
}
