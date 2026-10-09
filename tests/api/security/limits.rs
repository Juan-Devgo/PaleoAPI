//! Request limits over real TCP (003 AC 16, 17, 30; FR-028–FR-032, research R11–R14).

use std::io::{Read, Write};
use std::time::{Duration, Instant};

use serde_json::json;
use sqlx::PgPool;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

use crate::support::*;

/// A server whose database has the seeded accounts and one era, `e1`.
async fn live(
    pool_opts: PgPoolOptions,
    opts: PgConnectOptions,
    limits: &[(&str, &str)],
) -> (Live, PgPool) {
    let pool = pool_opts.connect_with(opts.clone()).await.unwrap();
    seed_account(&pool, ADMIN, Some("admin")).await;
    era(&pool, "e1", "251.902", "66").await.unwrap();
    (serve(opts, security_env(limits)), pool)
}

fn patch_era(live: &Live) -> Vec<u8> {
    let body = json!({ "name": "Renamed" }).to_string();
    format!(
        "PATCH /api/v1/eras/e1 HTTP/1.1\r\nHost: t\r\nConnection: close\r\n\
         Authorization: Bearer {}\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\n\r\n{body}",
        live.token(ADMIN),
        body.len()
    )
    .into_bytes()
}

/// Holds a row lock on era `e1` until `release`.
async fn lock_era(pool: &PgPool) -> sqlx::Transaction<'static, sqlx::Postgres> {
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("SELECT 1 FROM eras WHERE id = 'e1' FOR UPDATE")
        .execute(&mut *tx)
        .await
        .unwrap();
    tx
}

#[sqlx::test]
async fn a_request_target_over_2048_bytes_is_414(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let (live, _pool) = live(pool_opts, opts, &[]).await;
    let prefix = "/api/v1/eras?x=";
    let at_limit = format!("{prefix}{}", "a".repeat(2048 - prefix.len()));
    assert_eq!(at_limit.len(), 2048);
    assert_eq!(live.get(&at_limit).status, 200);

    let over = format!("{at_limit}a");
    let resp = live.get(&over);
    resp.assert_error(414, "URI_TOO_LONG");
    assert_eq!(resp.header("access-control-allow-origin"), Some("*"));
}

#[sqlx::test]
async fn headers_over_16_kib_are_431(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let (live, _pool) = live(pool_opts, opts, &[]).await;
    let request = |pad: usize| {
        format!(
            "GET /api/v1/eras HTTP/1.1\r\nHost: t\r\nConnection: close\r\nX-Pad: {}\r\n\r\n",
            "a".repeat(pad)
        )
    };
    assert_eq!(live.send(request(8_000).as_bytes()).status, 200);
    live.send(request(17_000).as_bytes())
        .assert_error(431, "REQUEST_HEADERS_TOO_LARGE");
    // The URI is checked first.
    let both = format!(
        "GET /api/v1/eras?x={} HTTP/1.1\r\nHost: t\r\nConnection: close\r\nX-Pad: {}\r\n\r\n",
        "a".repeat(2_100),
        "a".repeat(17_000)
    );
    live.send(both.as_bytes()).assert_error(414, "URI_TOO_LONG");
}

/// Headers sent one byte per second are cut by the server within 10 s (FR-029).
#[sqlx::test]
async fn slow_headers_are_cut_within_10_seconds(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let (live, _pool) = live(pool_opts, opts, &[]).await;
    let mut stream = live.connect();
    stream
        .set_read_timeout(Some(Duration::from_millis(1_000)))
        .unwrap();
    let started = Instant::now();
    let head = b"GET /api/v1/eras HTTP/1.1\r\nHost: t\r\nX-Slow: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let mut sent = 0;
    let mut closed = false;
    while started.elapsed() < Duration::from_secs(20) && !closed {
        if sent < head.len() && stream.write_all(&head[sent..=sent]).is_ok() {
            sent += 1;
        }
        let mut buf = [0u8; 512];
        match stream.read(&mut buf) {
            Ok(0) => closed = true,
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(_) => closed = true,
        }
    }
    let elapsed = started.elapsed();
    assert!(closed, "the connection was still open after {elapsed:?}");
    assert!(
        elapsed < Duration::from_secs(13),
        "closed only after {elapsed:?}"
    );
    assert!(
        elapsed >= Duration::from_secs(8),
        "closed after {elapsed:?}"
    );
}

/// Complete headers, then a body that stalls: `408` with the envelope in 30 s (D2).
#[sqlx::test]
async fn a_stalled_body_is_408_within_30_seconds(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let (live, _pool) = live(pool_opts, opts, &[]).await;
    let mut stream = live.connect();
    let head = format!(
        "POST /api/v1/eras HTTP/1.1\r\nHost: t\r\nAuthorization: Bearer {}\r\n\
         Content-Type: application/json\r\nContent-Length: 100\r\n\r\n{{\"id\":",
        live.token(ADMIN)
    );
    stream.write_all(head.as_bytes()).unwrap();
    let started = Instant::now();
    let resp = read_raw(&mut stream).expect("a response");
    let elapsed = started.elapsed();
    resp.assert_error(408, "REQUEST_TIMEOUT");
    assert!(
        elapsed >= Duration::from_secs(29) && elapsed < Duration::from_secs(33),
        "answered after {elapsed:?}"
    );
}

/// After a complete response an idle kept-alive connection is closed after 15 s (FR-029).
#[sqlx::test]
async fn an_idle_kept_alive_connection_is_closed_after_15_seconds(
    pool_opts: PgPoolOptions,
    opts: PgConnectOptions,
) {
    let (live, _pool) = live(pool_opts, opts, &[]).await;
    let mut stream = live.connect();
    stream
        .write_all(b"GET /api/v1/eras HTTP/1.1\r\nHost: t\r\n\r\n")
        .unwrap();
    let resp = read_raw(&mut stream).expect("a response");
    assert_eq!(resp.status, 200);
    let started = Instant::now();
    let mut buf = [0u8; 16];
    let closed = matches!(stream.read(&mut buf), Ok(0));
    let elapsed = started.elapsed();
    assert!(closed, "no clean close after {elapsed:?}");
    assert!(
        elapsed >= Duration::from_secs(13) && elapsed < Duration::from_secs(19),
        "closed after {elapsed:?}"
    );
}

/// AC 17: at capacity a request is `503` with `Retry-After: 1`, then service is normal.
#[sqlx::test]
async fn capacity_is_503_while_a_request_is_in_flight(
    pool_opts: PgPoolOptions,
    opts: PgConnectOptions,
) {
    let (live, pool) = live(pool_opts, opts, &[("MAX_CONCURRENT_REQUESTS", "1")]).await;
    let lock = lock_era(&pool).await;
    let blocked = {
        let request = patch_era(&live);
        let addr = live.addr;
        std::thread::spawn(move || {
            let mut stream = std::net::TcpStream::connect(addr).unwrap();
            stream.write_all(&request).unwrap();
            read_raw(&mut stream).expect("a response")
        })
    };
    // Wait until the write holds the permit.
    let deadline = Instant::now() + Duration::from_secs(1);
    while live.security.requests().in_use() == 0 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    let busy = live.get("/api/v1/eras");
    busy.assert_error(503, "SERVICE_UNAVAILABLE");
    assert_eq!(busy.header("retry-after"), Some("1"));

    lock.rollback().await.unwrap();
    assert_eq!(blocked.join().unwrap().status, 200);
    assert_eq!(live.get("/api/v1/eras").status, 200);
    assert_eq!(live.security.requests().in_use(), 0);
}

/// AC 17: a statement blocked on a lock for more than 2 s is `503` without SQL.
#[sqlx::test]
async fn a_write_blocked_on_a_lock_is_503_after_2_seconds(
    pool_opts: PgPoolOptions,
    opts: PgConnectOptions,
) {
    let (live, pool) = live(pool_opts, opts, &[]).await;
    let lock = lock_era(&pool).await;
    let started = Instant::now();
    let resp = live.send(&patch_era(&live));
    let elapsed = started.elapsed();
    lock.rollback().await.unwrap();
    resp.assert_error(503, "SERVICE_UNAVAILABLE");
    assert_eq!(resp.header("retry-after"), Some("5"));
    let text = String::from_utf8_lossy(&resp.body).to_lowercase();
    for leak in ["select", "update", "eras", "statement", "postgres"] {
        assert!(!text.contains(leak), "{leak} in {text}");
    }
    assert!(
        elapsed >= Duration::from_millis(1_800) && elapsed < Duration::from_secs(5),
        "answered after {elapsed:?}"
    );
    // The lock is gone: the same write now succeeds.
    assert_eq!(live.send(&patch_era(&live)).status, 200);
}
