//! `POST /api/v1/auth/login` (003 AC 1, AC 7; FR-007–FR-009).

use actix_web::http::Method;
use actix_web::test::TestRequest;
use paleo_api::security::token::{self, LIFETIME_SECS};
use serde_json::{Value, json};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

use crate::support::*;

const LOGIN: &str = "/api/v1/auth/login";
const CHALLENGE: &str = r#"Bearer realm="paleo-api""#;

/// A login with a raw JSON body from `peer(1)`.
async fn login_json(app: &TestApp, body: &Value) -> Resp {
    app.call(
        TestRequest::post()
            .uri(LOGIN)
            .peer_addr(peer(1))
            .insert_header(("Content-Type", "application/json"))
            .set_payload(serde_json::to_vec(body).unwrap()),
    )
    .await
}

#[track_caller]
fn assert_invalid_credentials(resp: &Resp) {
    assert_error(resp, 401, "INVALID_CREDENTIALS");
    assert_eq!(
        resp.json()["error"]["message"],
        "Wrong username or password."
    );
    assert_eq!(resp.header("www-authenticate"), Some(CHALLENGE));
    assert_eq!(resp.header("cache-control"), Some("no-store"));
}

#[sqlx::test]
async fn no_login_succeeds_on_a_fresh_database(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app_with(
        pool_opts,
        opts,
        SecuritySettings {
            seed: false,
            ..SecuritySettings::default()
        },
    )
    .await;
    let accounts: i64 = sqlx::query_scalar("SELECT count(*) FROM accounts")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(accounts, 0, "no account exists by default (FR-003)");
    for (username, password) in [
        (ADMIN, TEST_PASSWORD),
        ("admin", "admin"),
        ("root", "root"),
        ("postgres", "postgres"),
    ] {
        assert_invalid_credentials(&login(&app, username, password, peer(1)).await);
    }
}

#[sqlx::test]
async fn seeded_admin_gets_a_token(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app(pool_opts, opts).await;
    let resp = login(&app, ADMIN, TEST_PASSWORD, peer(1)).await;
    assert_eq!(resp.status.as_u16(), 200, "{}", resp.text());
    assert_eq!(resp.header("cache-control"), Some("no-store"));
    assert_eq!(resp.header("content-type"), Some("application/json"));
    let body = resp.json();
    let data = body["data"].as_object().expect("data object");
    assert_eq!(data.len(), 3, "{body}");
    assert_eq!(data["token_type"], "Bearer");
    assert_eq!(data["expires_in"], 3600);
    let access = data["access_token"].as_str().expect("access_token string");
    let claims = token::verify(app.security.keys(), access).expect("a valid token");
    assert_eq!(claims.sub, ADMIN);
    assert_eq!(claims.ver, 1);
    assert_eq!(claims.exp - claims.iat, LIFETIME_SECS);
    assert!(!resp.text().contains(TEST_PASSWORD));
    assert!(!resp.text().contains(SEED_HASH));

    // The token authorizes a write (AC 1 → AC 2).
    let created = send_as(
        &app,
        Method::POST,
        "/api/v1/eras",
        json!({ "id": "mesozoic", "name": "Mesozoic", "start_mya": 251.902, "end_mya": 66 }),
        Some(access),
    )
    .await;
    assert_eq!(created.status.as_u16(), 201, "{}", created.text());
}

#[sqlx::test]
async fn roleless_account_logs_in(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app(pool_opts, opts).await;
    let resp = login(&app, USER, TEST_PASSWORD, peer(1)).await;
    assert_eq!(resp.status.as_u16(), 200, "{}", resp.text());
    let access = resp.json()["data"]["access_token"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(
        token::verify(app.security.keys(), &access).unwrap().sub,
        USER
    );
}

#[sqlx::test]
async fn wrong_password_is_invalid_credentials(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app(pool_opts, opts).await;
    let wrong = "paleo-test-password-2025";
    let resp = login(&app, ADMIN, wrong, peer(1)).await;
    assert_invalid_credentials(&resp);
    assert!(!resp.text().contains(wrong));
    // 128 characters is a well-formed password: checked, not 422.
    assert_invalid_credentials(&login(&app, ADMIN, &"p".repeat(128), peer(1)).await);
    // Unknown username, and a username that is not a slug: unknown, never 422.
    assert_invalid_credentials(&login(&app, "nobody", TEST_PASSWORD, peer(1)).await);
    assert_invalid_credentials(&login(&app, "Not A Slug\n", TEST_PASSWORD, peer(1)).await);
    assert_invalid_credentials(&login(&app, "nul\u{0}name", TEST_PASSWORD, peer(1)).await);
    // Disabled account with the right password.
    exec(
        &app.pool,
        "UPDATE accounts SET status = 'disabled' WHERE username = 'user'",
    )
    .await
    .unwrap();
    assert_invalid_credentials(&login(&app, USER, TEST_PASSWORD, peer(1)).await);
}

#[sqlx::test]
async fn login_body_follows_the_body_rules(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app(pool_opts, opts).await;
    let raw = |ct: Option<&str>, body: Vec<u8>| {
        let mut req = TestRequest::post()
            .uri(LOGIN)
            .peer_addr(peer(1))
            .set_payload(body);
        if let Some(ct) = ct {
            req = req.insert_header(("Content-Type", ct.to_string()));
        }
        req
    };
    let resp = app.call(raw(Some("text/plain"), b"{}".to_vec())).await;
    assert_error(&resp, 415, "UNSUPPORTED_MEDIA_TYPE");
    let resp = app
        .call(raw(Some("application/json"), b"{\"username\":".to_vec()))
        .await;
    assert_error(&resp, 400, "INVALID_JSON");
    let resp = app
        .call(raw(Some("application/json"), vec![b' '; 70_000]))
        .await;
    assert_error(&resp, 413, "PAYLOAD_TOO_LARGE");
    for resp in [
        app.call(raw(Some("text/plain"), b"{}".to_vec())).await,
        app.call(raw(Some("application/json"), b"{".to_vec())).await,
    ] {
        assert_eq!(resp.header("cache-control"), Some("no-store"));
    }
}

/// Well-formed JSON that is not a valid login body → `422` with every problem (AC 7).
fn invalid_bodies() -> Vec<(Value, Vec<&'static str>)> {
    let long = "p".repeat(129);
    vec![
        (json!({}), vec!["username", "password"]),
        (json!({ "username": ADMIN }), vec!["password"]),
        (json!({ "password": TEST_PASSWORD }), vec!["username"]),
        (
            json!({ "username": "", "password": TEST_PASSWORD }),
            vec!["username"],
        ),
        (
            json!({ "username": ADMIN, "password": "" }),
            vec!["password"],
        ),
        (
            json!({ "username": null, "password": null }),
            vec!["username", "password"],
        ),
        (
            json!({ "username": 5, "password": ["x"] }),
            vec!["username", "password"],
        ),
        (
            json!({ "username": ADMIN, "password": long }),
            vec!["password"],
        ),
        (
            json!({ "username": ADMIN, "password": TEST_PASSWORD, "remember": true }),
            vec!["remember"],
        ),
        (json!([ADMIN, TEST_PASSWORD]), vec![""]),
        (json!(null), vec![""]),
        (json!(TEST_PASSWORD), vec![""]),
    ]
}

#[sqlx::test]
async fn invalid_bodies_are_422(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app(pool_opts, opts).await;
    for (body, fields) in invalid_bodies() {
        let resp = login_json(&app, &body).await;
        assert_details(&resp, &fields);
        assert_eq!(resp.header("cache-control"), Some("no-store"), "{body}");
        let text = resp.text();
        assert!(!text.contains(TEST_PASSWORD), "echoes the password: {text}");
        assert!(!text.contains(&"p".repeat(129)), "echoes the password");
    }
}

/// `422` is decided before any database access or password hashing (AC 7, FR-009):
/// the pool cannot connect and every hashing permit is taken.
#[actix_web::test]
async fn invalid_bodies_are_422_without_hashing() {
    let app = app_with_pool(
        closed_pool(),
        SecuritySettings {
            config: security_env(&[("MAX_CONCURRENT_PASSWORD_HASHES", "1")]),
            seed: false,
            ..SecuritySettings::default()
        },
    )
    .await;
    let held = app.security.hashes().try_acquire().expect("free permit");
    for (body, fields) in invalid_bodies() {
        assert_details(&login_json(&app, &body).await, &fields);
    }
    assert_eq!(app.security.hashes().in_use(), 1);
    drop(held);
}

/// Every header as sorted `(name, value)` pairs, without `Date`.
fn header_list(resp: &Resp) -> Vec<(String, Vec<u8>)> {
    let mut list: Vec<(String, Vec<u8>)> = resp
        .headers
        .iter()
        .filter(|(name, _)| name.as_str() != "date")
        .map(|(name, value)| (name.to_string(), value.as_bytes().to_vec()))
        .collect();
    list.sort();
    list
}

/// The three failures an attacker could tell apart if they differed (FR-008, AC 6).
const UNKNOWN: (&str, &str) = ("nobody", TEST_PASSWORD);
const WRONG: (&str, &str) = (USER, "paleo-test-password-2025");
const DISABLED: (&str, &str) = ("sleeper", TEST_PASSWORD);

async fn with_disabled_account(pool_opts: PgPoolOptions, opts: PgConnectOptions) -> TestApp {
    let app = app_with(
        pool_opts,
        opts,
        SecuritySettings {
            manual_clock: true,
            ..SecuritySettings::default()
        },
    )
    .await;
    seed_account(&app.pool, "sleeper", None).await;
    exec(
        &app.pool,
        "UPDATE accounts SET status = 'disabled' WHERE username = 'sleeper'",
    )
    .await
    .unwrap();
    app
}

/// AC 6: unknown username, wrong password, and disabled account answer with the same
/// status, headers (except `Date`), and body. Each case comes from a fresh peer under a
/// manual clock, so the `RateLimit` values match too.
#[sqlx::test]
async fn the_three_failures_are_byte_identical(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = with_disabled_account(pool_opts, opts).await;
    let mut seen = Vec::new();
    for (n, (username, password)) in [UNKNOWN, WRONG, DISABLED].into_iter().enumerate() {
        let resp = login(&app, username, password, peer(10 + n as u32)).await;
        assert_invalid_credentials(&resp);
        seen.push((resp.status, header_list(&resp), resp.body.to_vec()));
    }
    assert_eq!(seen[0], seen[1], "unknown vs wrong password");
    assert_eq!(seen[0], seen[2], "unknown vs disabled");
    // A username that is not a slug looks the same as an unknown one.
    let resp = login(&app, "Not A Slug", TEST_PASSWORD, peer(20)).await;
    assert_eq!(
        (resp.status, header_list(&resp), resp.body.to_vec()),
        seen[0]
    );
}

fn median(mut samples: Vec<std::time::Duration>) -> std::time::Duration {
    samples.sort();
    samples[samples.len() / 2]
}

/// AC 6: each failure costs one Argon2 verification, so the median times of the three
/// cases are within 25% of each other. Interleaved to even out machine noise.
#[sqlx::test]
async fn the_three_failures_cost_the_same(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = with_disabled_account(pool_opts, opts).await;
    const ROUNDS: u32 = 50;
    let mut samples: [Vec<std::time::Duration>; 3] = Default::default();
    // One unmeasured round warms the caches.
    for round in 0..=ROUNDS {
        for (case, (username, password)) in [UNKNOWN, WRONG, DISABLED].into_iter().enumerate() {
            let from = peer(1_000 + round * 3 + case as u32);
            let started = std::time::Instant::now();
            let resp = login(&app, username, password, from).await;
            let took = started.elapsed();
            assert_eq!(resp.status.as_u16(), 401, "{username}");
            if round > 0 {
                samples[case].push(took);
            }
        }
    }
    let [unknown, wrong, disabled] = samples.map(median);
    for (name, other) in [("wrong password", wrong), ("disabled", disabled)] {
        let (lo, hi) = (unknown.min(other), unknown.max(other));
        assert!(
            hi.as_secs_f64() <= lo.as_secs_f64() * 1.25,
            "unknown {unknown:?} vs {name} {other:?}"
        );
    }
}
