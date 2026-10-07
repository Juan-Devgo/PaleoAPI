//! Runs the API binary and checks that it refuses to start safely
//! (AC 2, AC 15, FR-002–FR-004).

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use sqlx::{AssertSqlSafe, PgPool};

const LEAKS: [&str; 2] = ["leaky_user", "leaky-secret-123"];
const KILL_AFTER: Duration = Duration::from_secs(15);

struct Outcome {
    status: Option<ExitStatus>,
    stderr: String,
    elapsed: Duration,
}

fn spawn_api(database_url: Option<&str>) -> Child {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_paleo_api"));
    cmd.env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    if let Some(url) = database_url {
        cmd.env("DATABASE_URL", url);
    }
    cmd.spawn().expect("failed to start the API binary")
}

fn read_stderr(child: &mut Child) -> String {
    let mut stderr = String::new();
    if let Some(mut pipe) = child.stderr.take() {
        pipe.read_to_string(&mut stderr).unwrap();
    }
    stderr
}

/// Runs the API until it exits, killing it after `KILL_AFTER`.
fn run_api(database_url: Option<&str>) -> Outcome {
    let started = Instant::now();
    let mut child = spawn_api(database_url);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break Some(status);
        }
        if started.elapsed() > KILL_AFTER {
            child.kill().unwrap();
            child.wait().unwrap();
            break None;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let elapsed = started.elapsed();
    Outcome {
        status,
        stderr: read_stderr(&mut child),
        elapsed,
    }
}

/// Asserts a non-zero exit with exactly one `paleo_api: …` line containing `gist`.
#[track_caller]
fn assert_refused(out: &Outcome, gist: &str) {
    let status = out
        .status
        .unwrap_or_else(|| panic!("API did not exit; stderr: {}", out.stderr));
    assert!(
        !status.success(),
        "API must exit non-zero; stderr: {}",
        out.stderr
    );
    let lines: Vec<&str> = out.stderr.lines().collect();
    assert_eq!(
        lines.len(),
        1,
        "exactly one stderr line expected: {:?}",
        out.stderr
    );
    assert!(lines[0].starts_with("paleo_api: "), "{}", lines[0]);
    assert!(
        lines[0].contains(gist),
        "expected {gist:?} in {:?}",
        lines[0]
    );
    for leak in LEAKS {
        assert!(
            !out.stderr.contains(leak),
            "stderr leaks {leak}: {}",
            out.stderr
        );
    }
}

/// The harness URL with its database replaced by the per-test database.
fn url_for(pool: &PgPool) -> String {
    let harness = std::env::var("DATABASE_URL").expect("run with the documented test command");
    let db = pool.connect_options().get_database().unwrap().to_string();
    let (base, query) = match harness.split_once('?') {
        Some((b, q)) => (b.to_string(), format!("?{q}")),
        None => (harness.clone(), String::new()),
    };
    let prefix = &base[..base.rfind('/').unwrap()];
    format!("{prefix}/{db}{query}")
}

/// Replaces the password in `scheme://user:password@host/...`.
fn with_password(url: &str, password: &str) -> String {
    let (scheme, rest) = url.split_once("://").unwrap();
    let (userinfo, host) = rest.split_once('@').unwrap();
    let user = userinfo.split(':').next().unwrap();
    format!("{scheme}://{user}:{password}@{host}")
}

async fn migration_rows(pool: &PgPool) -> Vec<String> {
    sqlx::query_scalar("SELECT row_to_json(m)::text FROM _sqlx_migrations m ORDER BY version")
        .fetch_all(pool)
        .await
        .unwrap()
}

/// Sends one raw HTTP/1.1 request to the running API and returns the whole response.
fn raw_http(request: &str) -> String {
    let mut stream = TcpStream::connect("127.0.0.1:8000").expect("connect to the API");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream.write_all(request.as_bytes()).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
}

async fn exec(pool: &PgPool, sql: &str) {
    sqlx::query(AssertSqlSafe(sql.to_string()))
        .execute(pool)
        .await
        .unwrap();
}

// (a)
#[test]
fn refuses_without_database_url() {
    assert_refused(&run_api(None), "DATABASE_URL is not set");
}

// (b)
#[test]
fn refuses_an_invalid_url() {
    assert_refused(
        &run_api(Some("not-a-url")),
        "not a valid PostgreSQL connection URL",
    );
}

// (c)
#[test]
fn refuses_an_unreachable_database_within_the_wait() {
    let scheme = "postgres";
    let url = format!(
        "{}://{}:{}@{}/{}",
        scheme, "leaky_user", "leaky-secret-123", "127.0.0.1:9", "paleo"
    );
    let out = run_api(Some(&url));
    assert_refused(&out, "Could not connect to the database within 10 s");
    assert!(
        out.elapsed < Duration::from_secs(13),
        "took {:?}",
        out.elapsed
    );
}

// (d)
#[sqlx::test]
async fn refuses_wrong_credentials_immediately(pool: PgPool) {
    let url = with_password(&url_for(&pool), "leaky-secret-123");
    let out = run_api(Some(&url));
    assert_refused(&out, "rejected the configured credentials");
    assert!(
        out.elapsed < Duration::from_secs(5),
        "took {:?}",
        out.elapsed
    );
}

// (e)
#[sqlx::test(migrations = false)]
async fn refuses_an_unmigrated_database(pool: PgPool) {
    let out = run_api(Some(&url_for(&pool)));
    assert_refused(&out, "migration(s) pending");
    assert_refused(&out, "sqlx migrate run");
    let table: Option<String> = sqlx::query_scalar("SELECT to_regclass('_sqlx_migrations')::text")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(table, None, "the API must never apply migrations");
}

// (f)
#[sqlx::test]
async fn refuses_with_one_pending_migration(pool: PgPool) {
    exec(
        &pool,
        "DELETE FROM _sqlx_migrations WHERE version = (SELECT max(version) FROM _sqlx_migrations)",
    )
    .await;
    let before = migration_rows(&pool).await;
    assert_refused(&run_api(Some(&url_for(&pool))), "1 migration(s) pending");
    assert_eq!(migration_rows(&pool).await, before);
}

// (g)
#[sqlx::test]
async fn refuses_a_dirty_migration(pool: PgPool) {
    exec(
        &pool,
        "UPDATE _sqlx_migrations SET success = false WHERE version = (SELECT max(version) FROM _sqlx_migrations)",
    )
    .await;
    let before = migration_rows(&pool).await;
    assert_refused(&run_api(Some(&url_for(&pool))), "failed partway");
    assert_eq!(migration_rows(&pool).await, before);
}

// (h) The only test that binds port 8000.
#[sqlx::test]
async fn warns_about_collation_drift_and_starts(pool: PgPool) {
    exec(
        &pool,
        "UPDATE pg_collation SET collversion = '0' WHERE collname = 'paleo_name_sort'",
    )
    .await;
    drop(TcpListener::bind("127.0.0.1:8000").expect("port 8000 busy: stop the process using it"));

    let mut child = spawn_api(Some(&url_for(&pool)));
    let started = Instant::now();
    let listening = loop {
        if TcpStream::connect("127.0.0.1:8000").is_ok() {
            break true;
        }
        if started.elapsed() > Duration::from_secs(10) || child.try_wait().unwrap().is_some() {
            break false;
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    // (h) serve: the binary serves `api::app`, not the old stub routes (AC 6.1.15).
    let served = listening
        .then(|| raw_http("GET / HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"));
    let still_running = child.try_wait().unwrap().is_none();
    child.kill().ok();
    child.wait().unwrap();
    let stderr = read_stderr(&mut child);

    assert!(
        listening,
        "API never listened on 127.0.0.1:8000; stderr: {stderr}"
    );
    assert!(still_running, "API exited; stderr: {stderr}");
    assert!(
        stderr.contains("paleo_api: warning:") && stderr.contains("paleo_name_sort"),
        "missing collation warning: {stderr}"
    );
    let served = served.unwrap();
    assert!(
        served.starts_with("HTTP/1.1 404"),
        "GET / must be 404: {served}"
    );
    assert!(
        served.contains("\"ROUTE_NOT_FOUND\""),
        "GET / must answer the ROUTE_NOT_FOUND envelope: {served}"
    );
}
