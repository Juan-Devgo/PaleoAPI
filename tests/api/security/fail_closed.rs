//! Fail-closed paths (003 AC 20, 30; FR-030, FR-031, research R13, R14).

use std::time::{Duration, Instant};

use actix_web::http::Method;
use actix_web::test::TestRequest;
use serde_json::json;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

use crate::support::*;

fn unseeded() -> SecuritySettings {
    SecuritySettings {
        seed: false,
        ..SecuritySettings::default()
    }
}

#[track_caller]
fn assert_unavailable(resp: &Resp, retry_after: &str) {
    assert_error(resp, 503, "SERVICE_UNAVAILABLE");
    assert_eq!(resp.header("retry-after"), Some(retry_after));
}

/// The account lookup cannot reach the database: `503`, and the handler never runs
/// (a body the handler would answer `400` or `415` to is not looked at).
#[actix_web::test]
async fn an_unreachable_account_lookup_is_503_and_runs_no_handler() {
    let app = app_with_pool(closed_pool(), unseeded()).await;
    let token = app.admin_token();
    for body in [b"{ not json".to_vec(), Vec::new()] {
        let resp = app
            .call(
                TestRequest::post()
                    .uri("/api/v1/eras")
                    .insert_header(("Authorization", format!("Bearer {token}")))
                    .set_payload(body),
            )
            .await;
        assert_unavailable(&resp, "5");
    }
    let resp = app
        .call(
            TestRequest::delete()
                .uri("/api/v1/eras/mesozoic")
                .insert_header(("Authorization", format!("Bearer {token}"))),
        )
        .await;
    assert_unavailable(&resp, "5");
}

/// A read on an unreachable pool is `503`, not `500`.
#[actix_web::test]
async fn a_read_on_an_unreachable_pool_is_503() {
    let app = app_with_pool(closed_pool(), unseeded()).await;
    assert_unavailable(&get(&app, "/api/v1/eras").await, "5");
    assert_unavailable(&get(&app, "/api/v1/species/trex").await, "5");
}

/// A database that stops answering the account lookup is `503` within about 2 s.
#[sqlx::test]
async fn a_slow_account_lookup_is_503_within_the_database_timeout(
    pool_opts: PgPoolOptions,
    opts: PgConnectOptions,
) {
    let app = app(pool_opts, opts).await;
    let mut lock = app.pool.begin().await.unwrap();
    sqlx::query("LOCK TABLE accounts IN ACCESS EXCLUSIVE MODE")
        .execute(&mut *lock)
        .await
        .unwrap();
    let started = Instant::now();
    let resp = send(
        &app,
        Method::POST,
        "/api/v1/eras",
        json!({ "id": "e1", "name": "E1", "start_mya": 250, "end_mya": 66 }),
    )
    .await;
    let elapsed = started.elapsed();
    lock.rollback().await.unwrap();
    assert_unavailable(&resp, "5");
    assert!(
        elapsed >= Duration::from_millis(1_800) && elapsed < Duration::from_secs(5),
        "answered after {elapsed:?}"
    );
    // The handler did not run, and the lock is gone.
    assert_eq!(get(&app, "/api/v1/eras/e1").await.status.as_u16(), 404);
}

/// With every request permit taken the request is `503` with `Retry-After: 1`, before
/// the handler or the account lookup; the permit comes back when the response is done.
#[sqlx::test]
async fn a_limiter_at_capacity_is_503_and_runs_no_handler(
    pool_opts: PgPoolOptions,
    opts: PgConnectOptions,
) {
    let settings = SecuritySettings {
        config: security_env(&[("MAX_CONCURRENT_REQUESTS", "1")]),
        ..SecuritySettings::default()
    };
    let app = app_with(pool_opts, opts, settings).await;
    let permit = app
        .security
        .requests()
        .try_acquire()
        .expect("a free permit");
    let created = send(
        &app,
        Method::POST,
        "/api/v1/eras",
        json!({ "id": "e1", "name": "E1", "start_mya": 250, "end_mya": 66 }),
    )
    .await;
    assert_unavailable(&created, "1");
    assert_unavailable(&get(&app, "/api/v1/eras").await, "1");
    assert_eq!(app.security.requests().in_use(), 1);
    drop(permit);

    assert_eq!(get(&app, "/api/v1/eras/e1").await.status.as_u16(), 404);
    assert_eq!(app.security.requests().in_use(), 0);
    let created = send(
        &app,
        Method::POST,
        "/api/v1/eras",
        json!({ "id": "e1", "name": "E1", "start_mya": 250, "end_mya": 66 }),
    )
    .await;
    assert_eq!(created.status.as_u16(), 201, "{}", created.text());
    assert_eq!(app.security.requests().in_use(), 0);
}
