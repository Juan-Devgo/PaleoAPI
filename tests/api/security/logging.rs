//! Security event log (003 AC 21; FR-039, FR-040; contracts/security-events.md).

use std::time::Duration;

use actix_web::http::Method;
use actix_web::test::TestRequest;
use jsonwebtoken::get_current_timestamp;
use paleo_api::security::token::{Claims, issue};
use serde_json::{Value, json};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

use crate::support::*;

const WRONG_PASSWORD: &str = "wrong-password-never-logged";

fn settings(vars: &[(&str, &str)]) -> SecuritySettings {
    SecuritySettings {
        config: security_env(vars),
        manual_clock: true,
        ..SecuritySettings::default()
    }
}

fn of_kind(events: &[Value], kind: &str) -> Vec<Value> {
    events
        .iter()
        .filter(|e| e["event"] == kind)
        .cloned()
        .collect()
}

fn era_body(id: &str) -> Value {
    json!({ "id": id, "name": id, "start_mya": 250, "end_mya": 66 })
}

async fn write_with(app: &TestApp, token: Option<&str>) -> Resp {
    send_as(
        app,
        Method::POST,
        "/api/v1/eras",
        era_body("mesozoic"),
        token,
    )
    .await
}

async fn get_from(app: &TestApp, n: u32) -> Resp {
    app.call(TestRequest::get().uri("/api/v1/eras").peer_addr(peer(n)))
        .await
}

/// One scenario produces one event of each kind a request can raise, with the fields of
/// the contract, and no secret in any line.
#[sqlx::test]
async fn a_scenario_logs_one_event_of_each_kind(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app_with(
        pool_opts,
        opts,
        settings(&[
            ("RATE_LIMIT_PER_MINUTE", "60"),
            ("RATE_LIMIT_BURST", "40"),
            ("LOGIN_LIMIT_PER_USERNAME", "2"),
            ("LOGIN_LIMIT_WINDOW_SECONDS", "60"),
            ("MAX_CONCURRENT_PASSWORD_HASHES", "1"),
        ]),
    )
    .await;
    let admin_token = app.admin_token();

    // login_succeeded, login_failed, login_locked, rate_limited (login_username)
    let ok = login(&app, ADMIN, TEST_PASSWORD, peer(1)).await;
    assert_eq!(ok.status.as_u16(), 200);
    let token_in_response = ok.json()["data"]["access_token"]
        .as_str()
        .unwrap()
        .to_string();
    for _ in 0..2 {
        let bad = login(&app, ADMIN, WRONG_PASSWORD, peer(1)).await;
        assert_error(&bad, 401, "INVALID_CREDENTIALS");
    }
    assert_error(
        &login(&app, ADMIN, WRONG_PASSWORD, peer(1)).await,
        429,
        "RATE_LIMITED",
    );

    // overloaded (hashing)
    let held = app.security.hashes().try_acquire().expect("a free permit");
    let busy = login(&app, USER, TEST_PASSWORD, peer(2)).await;
    assert_error(&busy, 503, "SERVICE_UNAVAILABLE");
    drop(held);

    // auth_rejected (missing, expired, invalid), forbidden, admin_write
    assert_eq!(write_with(&app, None).await.status.as_u16(), 401);
    let expired = issue(
        app.security.keys(),
        &Claims::new(ADMIN, 1, get_current_timestamp() - 7200),
    )
    .unwrap();
    assert_eq!(write_with(&app, Some(&expired)).await.status.as_u16(), 401);
    assert_eq!(write_with(&app, Some("garbage")).await.status.as_u16(), 401);
    assert_eq!(
        write_with(&app, Some(&app.user_token()))
            .await
            .status
            .as_u16(),
        403
    );
    let created = write_with(&app, Some(&admin_token)).await;
    assert_eq!(created.status.as_u16(), 201, "{}", created.text());

    // rate_limited (general)
    let mut limited = false;
    for _ in 0..60 {
        if get_from(&app, 9).await.status.as_u16() == 429 {
            limited = true;
            break;
        }
    }
    assert!(limited, "the general limit was never reached");

    let events = events(&app);
    let one = |kind: &str| {
        let found = of_kind(&events, kind);
        assert!(!found.is_empty(), "no {kind} event in {events:#?}");
        found
    };

    let [succeeded] = one("login_succeeded")
        .try_into()
        .expect("one login_succeeded");
    assert_eq!(succeeded["account"], ADMIN);
    assert_eq!(succeeded["status"], 200);
    assert_eq!(succeeded["method"], "POST");
    assert_eq!(succeeded["route"], "/api/v1/auth/login");
    assert_eq!(succeeded["client"], "10.0.0.1");
    assert!(succeeded["ts"].as_str().unwrap().ends_with('Z'));

    let failed = one("login_failed");
    assert_eq!(failed.len(), 2);
    assert!(
        failed
            .iter()
            .all(|e| e["status"] == 401 && e["account"] == ADMIN)
    );

    let [locked] = one("login_locked").try_into().expect("one login_locked");
    assert_eq!(locked["account"], ADMIN);

    let rejected = one("auth_rejected");
    let reasons: Vec<&str> = rejected
        .iter()
        .map(|e| e["reason"].as_str().unwrap())
        .collect();
    assert_eq!(reasons, ["missing", "expired", "invalid"]);
    assert!(rejected.iter().all(|e| e["status"] == 401
        && e["method"] == "POST"
        && e["route"] == "/api/v1/eras"
        && e.get("account").is_none()));

    let [forbidden] = one("forbidden").try_into().expect("one forbidden");
    assert_eq!(forbidden["status"], 403);
    assert_eq!(forbidden["account"], USER);

    let [write] = one("admin_write").try_into().expect("one admin_write");
    assert_eq!(write["account"], ADMIN);
    assert_eq!(write["method"], "POST");
    assert_eq!(write["path"], "/api/v1/eras");
    assert_eq!(write["resource"], "mesozoic");
    assert_eq!(write["status"], 201);

    let limited = one("rate_limited");
    let by_reason = |reason: &str| {
        limited
            .iter()
            .filter(|e| e["reason"] == reason)
            .cloned()
            .collect::<Vec<_>>()
    };
    assert_eq!(by_reason("login_username").len(), 1);
    let [general] = by_reason("general").try_into().expect("one general");
    assert_eq!(general["status"], 429);
    assert_eq!(general["client"], "10.0.0.9");
    assert_eq!(general["route"], "/api/v1/eras");

    let [overloaded] = one("overloaded").try_into().expect("one overloaded");
    assert_eq!(overloaded["reason"], "hashing");
    assert_eq!(overloaded["status"], 503);
    assert!(overloaded["count"].as_u64().unwrap() >= 1);

    // FR-040: nothing secret in any line.
    let text = events
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    for secret in [
        TEST_PASSWORD,
        WRONG_PASSWORD,
        TEST_SECRET,
        SEED_HASH,
        &admin_token,
        &token_in_response,
        &expired,
        "Bearer",
        "Authorization",
        "authorization",
    ] {
        assert!(!text.contains(secret), "a line contains {secret:?}");
    }
}

/// A username that is not an account never reaches the log; a line break in it cannot
/// start a new line.
#[sqlx::test]
async fn a_username_with_a_line_break_adds_exactly_one_line(
    pool_opts: PgPoolOptions,
    opts: PgConnectOptions,
) {
    let app = app(pool_opts, opts).await;
    let before = events(&app).len();
    let hostile = "admin\n{\"event\":\"login_succeeded\"}";
    let resp = login(&app, hostile, WRONG_PASSWORD, peer(1)).await;
    assert_error(&resp, 401, "INVALID_CREDENTIALS");
    let all = events(&app);
    assert_eq!(all.len(), before + 1, "{all:#?}");
    let line = &all[before];
    assert_eq!(line["event"], "login_failed");
    assert!(line.get("account").is_none());
    assert!(!line.to_string().contains("admin\\n"));

    // An unknown (valid slug) username is not named either.
    login(&app, "nobody", WRONG_PASSWORD, peer(1)).await;
    let all = events(&app);
    assert!(all.last().unwrap().get("account").is_none());
}

/// 503 for the same reason is logged once per second with a count; a second later the
/// next one is logged again (FR-039).
#[sqlx::test]
async fn overloaded_is_logged_once_per_reason_per_second(
    pool_opts: PgPoolOptions,
    opts: PgConnectOptions,
) {
    let app = app_with(
        pool_opts,
        opts,
        settings(&[("MAX_CONCURRENT_REQUESTS", "1")]),
    )
    .await;
    let permit = app
        .security
        .requests()
        .try_acquire()
        .expect("a free permit");
    for _ in 0..2 {
        let resp = get_from(&app, 1).await;
        assert_error(&resp, 503, "SERVICE_UNAVAILABLE");
    }
    let lines = of_kind(&events(&app), "overloaded");
    assert_eq!(lines.len(), 1, "{lines:#?}");
    assert_eq!(lines[0]["reason"], "capacity");
    assert_eq!(lines[0]["status"], 503);
    assert_eq!(lines[0]["client"], "10.0.0.1");
    assert_eq!(lines[0]["method"], "GET");
    assert_eq!(lines[0]["count"], 1);

    app.clock().advance(Duration::from_secs(1));
    assert_error(&get_from(&app, 1).await, 503, "SERVICE_UNAVAILABLE");
    let lines = of_kind(&events(&app), "overloaded");
    assert_eq!(lines.len(), 2, "{lines:#?}");
    assert_eq!(lines[1]["reason"], "capacity");
    assert!(lines[1]["count"].as_u64().unwrap() >= 1);
    drop(permit);
}

/// A database that cannot be reached is the `database` reason.
#[actix_web::test]
async fn an_unreachable_database_is_overloaded_database() {
    let app = app_with_pool(
        closed_pool(),
        SecuritySettings {
            seed: false,
            manual_clock: true,
            ..SecuritySettings::default()
        },
    )
    .await;
    assert_error(&get_from(&app, 1).await, 503, "SERVICE_UNAVAILABLE");
    let lines = of_kind(&events(&app), "overloaded");
    assert_eq!(lines.len(), 1, "{lines:#?}");
    assert_eq!(lines[0]["reason"], "database");
}

/// Repeated `429`s of one reason from one client are one line per window; after the
/// window the next one is logged again (FR-039).
#[sqlx::test]
async fn general_429s_are_logged_once_per_window(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    // 60 per minute, burst 2: window w = 2 s.
    let app = app_with(
        pool_opts,
        opts,
        settings(&[("RATE_LIMIT_PER_MINUTE", "60"), ("RATE_LIMIT_BURST", "2")]),
    )
    .await;
    for _ in 0..2 {
        assert_eq!(get_from(&app, 1).await.status.as_u16(), 200);
    }
    for _ in 0..5 {
        assert_eq!(get_from(&app, 1).await.status.as_u16(), 429);
    }
    let lines = of_kind(&events(&app), "rate_limited");
    assert_eq!(lines.len(), 1, "{lines:#?}");
    assert_eq!(lines[0]["reason"], "general");

    // Another client has its own line.
    for _ in 0..3 {
        get_from(&app, 2).await;
    }
    assert_eq!(of_kind(&events(&app), "rate_limited").len(), 2);

    app.clock().advance(Duration::from_secs(3));
    for _ in 0..2 {
        assert_eq!(get_from(&app, 1).await.status.as_u16(), 200);
    }
    for _ in 0..3 {
        assert_eq!(get_from(&app, 1).await.status.as_u16(), 429);
    }
    let lines = of_kind(&events(&app), "rate_limited");
    assert_eq!(lines.len(), 3, "{lines:#?}");
    assert!(lines.iter().all(|l| l["reason"] == "general"));
}

async fn assert_login_429s_dedupe(
    app: &TestApp,
    reason: &str,
    attempt: impl AsyncFn() -> Resp,
    window: u64,
) {
    for _ in 0..4 {
        assert_error(&attempt().await, 429, "RATE_LIMITED");
    }
    let lines: Vec<Value> = of_kind(&events(app), "rate_limited")
        .into_iter()
        .filter(|e| e["reason"] == reason)
        .collect();
    assert_eq!(lines.len(), 1, "{reason}: {lines:#?}");
    assert_eq!(lines[0]["status"], 429);
    assert_eq!(lines[0]["route"], "/api/v1/auth/login");

    app.clock().advance(Duration::from_secs(window - 1));
    let _ = attempt().await;
    let again = of_kind(&events(app), "rate_limited")
        .into_iter()
        .filter(|e| e["reason"] == reason)
        .count();
    assert_eq!(again, 1, "{reason}: still inside the window");
}

/// Per-client login log: one `login_client` line per window.
#[sqlx::test]
async fn login_client_429s_are_logged_once_per_window(
    pool_opts: PgPoolOptions,
    opts: PgConnectOptions,
) {
    let app = app_with(
        pool_opts,
        opts,
        settings(&[
            ("LOGIN_LIMIT_PER_CLIENT", "2"),
            ("LOGIN_LIMIT_WINDOW_SECONDS", "60"),
        ]),
    )
    .await;
    for _ in 0..2 {
        login(&app, ADMIN, TEST_PASSWORD, peer(1)).await;
    }
    assert_login_429s_dedupe(
        &app,
        "login_client",
        async || login(&app, ADMIN, TEST_PASSWORD, peer(1)).await,
        60,
    )
    .await;

    // Past the window of the first rejection the log is free again; fill and reject again.
    app.clock().advance(Duration::from_secs(2));
    for _ in 0..2 {
        login(&app, ADMIN, TEST_PASSWORD, peer(1)).await;
    }
    assert_error(
        &login(&app, ADMIN, TEST_PASSWORD, peer(1)).await,
        429,
        "RATE_LIMITED",
    );
    let count = of_kind(&events(&app), "rate_limited")
        .into_iter()
        .filter(|e| e["reason"] == "login_client")
        .count();
    assert_eq!(count, 2);
}

/// Per-username login log: one `login_username` line per client and window.
#[sqlx::test]
async fn login_username_429s_are_logged_once_per_window(
    pool_opts: PgPoolOptions,
    opts: PgConnectOptions,
) {
    let app = app_with(
        pool_opts,
        opts,
        settings(&[
            ("LOGIN_LIMIT_PER_USERNAME", "2"),
            ("LOGIN_LIMIT_WINDOW_SECONDS", "60"),
        ]),
    )
    .await;
    for _ in 0..2 {
        assert_error(
            &login(&app, ADMIN, WRONG_PASSWORD, peer(1)).await,
            401,
            "INVALID_CREDENTIALS",
        );
    }
    assert_login_429s_dedupe(
        &app,
        "login_username",
        async || login(&app, ADMIN, WRONG_PASSWORD, peer(1)).await,
        60,
    )
    .await;

    // Past the window the failures expired too: fail twice again, then the next 429 is logged.
    app.clock().advance(Duration::from_secs(2));
    for _ in 0..2 {
        login(&app, ADMIN, WRONG_PASSWORD, peer(1)).await;
    }
    assert_error(
        &login(&app, ADMIN, WRONG_PASSWORD, peer(1)).await,
        429,
        "RATE_LIMITED",
    );
    let count = of_kind(&events(&app), "rate_limited")
        .into_iter()
        .filter(|e| e["reason"] == "login_username")
        .count();
    assert_eq!(count, 2);
}
