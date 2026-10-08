//! General rate limit (003 AC 11, 12, 15, 26, 27, 28; FR-021, FR-023–FR-027).

use std::time::{Duration, Instant};

use actix_web::http::Method;
use actix_web::test::TestRequest;
use paleo_api::config::security_config_from_env;
use paleo_api::security::Capacities;
use serde_json::json;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

use crate::support::db_support::exec_script;
use crate::support::*;

const EXPOSED: [&str; 4] = ["etag", "retry-after", "ratelimit", "ratelimit-policy"];

/// Test settings with the general limit lowered and a manual clock.
fn limited(per_minute: &str, burst: &str) -> SecuritySettings {
    SecuritySettings {
        config: security_env(&[
            ("RATE_LIMIT_PER_MINUTE", per_minute),
            ("RATE_LIMIT_BURST", burst),
        ]),
        manual_clock: true,
        ..SecuritySettings::default()
    }
}

async fn get_from(app: &TestApp, uri: &str, n: u32) -> Resp {
    app.call(TestRequest::get().uri(uri).peer_addr(peer(n)))
        .await
}

/// Asserts `429 RATE_LIMITED` with `Retry-After: <secs>` and a message naming the limit.
#[track_caller]
fn assert_limited(resp: &Resp, per_minute: u32, burst: u32, secs: u64) {
    assert_error(resp, 429, "RATE_LIMITED");
    assert_eq!(resp.header("retry-after"), Some(secs.to_string().as_str()));
    let unit = if secs == 1 { "second" } else { "seconds" };
    assert_eq!(
        resp.json()["error"]["message"],
        format!(
            "Too many requests: the limit is {per_minute} requests per minute \
             (bursts up to {burst}). Retry in {secs} {unit}."
        )
    );
}

#[track_caller]
fn assert_rate_headers(resp: &Resp, ratelimit: &str, policy: &str) {
    assert_eq!(resp.header("ratelimit"), Some(ratelimit), "{}", resp.status);
    assert_eq!(
        resp.header("ratelimit-policy"),
        Some(policy),
        "{}",
        resp.status
    );
    let mut exposed: Vec<String> = resp
        .header("access-control-expose-headers")
        .unwrap_or_default()
        .split(',')
        .map(|h| h.trim().to_ascii_lowercase())
        .collect();
    exposed.sort();
    let mut want = EXPOSED.map(String::from).to_vec();
    want.sort();
    assert_eq!(exposed, want, "{}", resp.status);
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

/// AC 11: burst, then `429` with `Retry-After`; waiting that long is enough; other
/// clients are unaffected; rejections do not extend the wait.
#[sqlx::test]
async fn over_the_limit_is_429_until_retry_after(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    // T = 10 s, burst 3.
    let app = app_with(pool_opts, opts, limited("6", "3")).await;
    for i in 0..3 {
        let resp = get_from(&app, "/api/v1/eras", 1).await;
        assert_eq!(resp.status.as_u16(), 200, "request {i}: {}", resp.text());
    }
    for _ in 0..5 {
        assert_limited(&get_from(&app, "/api/v1/eras", 1).await, 6, 3, 10);
    }
    assert_eq!(get_from(&app, "/api/v1/eras", 2).await.status.as_u16(), 200);

    app.clock().advance(Duration::from_secs(9));
    assert_limited(&get_from(&app, "/api/v1/eras", 1).await, 6, 3, 1);
    app.clock().advance(Duration::from_secs(1));
    assert_eq!(get_from(&app, "/api/v1/eras", 1).await.status.as_u16(), 200);
}

/// AC 12 (in-process): 6,000 reads at 100 ms intervals under the defaults → no `429`.
#[sqlx::test]
async fn reads_at_the_default_rate_are_never_limited(
    pool_opts: PgPoolOptions,
    opts: PgConnectOptions,
) {
    let app = app_with(pool_opts, opts, limited("600", "120")).await;
    for i in 0..6_000 {
        let resp = get_from(&app, "/api/v1/eras", 1).await;
        assert_eq!(resp.status.as_u16(), 200, "request {i}: {}", resp.text());
        assert_eq!(
            resp.header("ratelimit"),
            Some(r#""general";r=119;t=1"#),
            "request {i}"
        );
        app.clock().advance(Duration::from_millis(100));
    }
}

/// AC 12 (manual, quickstart §7.1): ten minutes at 10 requests per second from one
/// client under the default limits, real time → no `429`, read p95 < 50 ms.
#[sqlx::test]
#[ignore = "10-minute run: quickstart §7.1, with --release"]
async fn sustained_default_rate(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    const DATASET: &str = include_str!("../../db/fixtures/index_dataset.sql");
    const URIS: &[&str] = &[
        "/api/v1/species?limit=100",
        "/api/v1/species?era=era-03",
        "/api/v1/species?q=saurus",
        "/api/v1/species/species-4242",
        "/api/v1/eras",
        "/api/v1/periods?limit=100",
        "/api/v1/taxonomy/genera?limit=100",
        "/api/v1/countries?continent=continent-3",
    ];
    let app = app_with(
        pool_opts,
        opts,
        SecuritySettings {
            config: security_env(&[
                ("RATE_LIMIT_PER_MINUTE", "600"),
                ("RATE_LIMIT_BURST", "120"),
            ]),
            ..SecuritySettings::default()
        },
    )
    .await;
    exec_script(&app.pool, DATASET)
        .await
        .expect("fixture must load");
    exec(&app.pool, "ANALYZE").await.unwrap();

    let total = 6_000u32;
    let start = Instant::now();
    let mut times = Vec::with_capacity(total as usize);
    for i in 0..total {
        actix_web::rt::time::sleep_until((start + Duration::from_millis(100) * i).into()).await;
        let uri = URIS[i as usize % URIS.len()];
        let t = Instant::now();
        let resp = get_from(&app, uri, 1).await;
        times.push(t.elapsed());
        assert_eq!(
            resp.status.as_u16(),
            200,
            "request {i} {uri}: {}",
            resp.text()
        );
    }
    times.sort();
    let p95 = times[times.len() * 95 / 100 - 1];
    println!("sustained p95 {:.2} ms", p95.as_secs_f64() * 1000.0);
    assert!(p95 < Duration::from_millis(50), "p95 {p95:?}");
}

/// AC 26: limits set through the environment are the ones enforced and advertised.
#[sqlx::test]
async fn configured_values_are_enforced(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    // T = 0.5 s, burst 7, w = ⌈7 × 60 / 120⌉ = 4.
    let app = app_with(pool_opts, opts, limited("120", "7")).await;
    for i in 0..7 {
        let resp = get_from(&app, "/api/v1/eras", 1).await;
        assert_eq!(resp.status.as_u16(), 200, "request {i}");
        assert_eq!(
            resp.header("ratelimit-policy"),
            Some(r#""general";q=7;w=4"#)
        );
    }
    assert_limited(&get_from(&app, "/api/v1/eras", 1).await, 120, 7, 1);
    app.clock().advance(Duration::from_millis(500));
    assert_eq!(get_from(&app, "/api/v1/eras", 1).await.status.as_u16(), 200);
    assert_limited(&get_from(&app, "/api/v1/eras", 1).await, 120, 7, 1);
}

/// AC 26: unset limit variables mean the documented defaults.
#[sqlx::test]
async fn unset_limits_are_the_defaults(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let config =
        security_config_from_env(|key| (key == "JWT_SECRET").then(|| TEST_SECRET.into())).unwrap();
    let app = app_with(
        pool_opts,
        opts,
        SecuritySettings {
            config,
            capacities: Capacities::default(),
            manual_clock: true,
            seed: false,
        },
    )
    .await;
    for i in 0..120u64 {
        let resp = get_from(&app, "/api/v1/nowhere", 1).await;
        assert_eq!(resp.status.as_u16(), 404, "request {i}");
        assert_eq!(
            resp.header("ratelimit-policy"),
            Some(r#""general";q=120;w=12"#)
        );
    }
    assert_limited(&get_from(&app, "/api/v1/nowhere", 1).await, 600, 120, 1);
}

/// AC 27: a `429` does no database work; the pool is closed, so any query would be a
/// `5xx` instead.
#[actix_web::test]
async fn a_rejected_request_does_no_database_work() {
    let mut settings = limited("1", "1");
    settings.seed = false;
    let app = app_with_pool(closed_pool(), settings).await;
    assert_error(
        &get_from(&app, "/api/v1/nowhere", 1).await,
        404,
        "ROUTE_NOT_FOUND",
    );

    let read = get_from(&app, "/api/v1/eras", 1).await;
    assert_limited(&read, 1, 1, 60);
    let token = app.admin_token();
    let write = app
        .call(
            TestRequest::post()
                .uri("/api/v1/eras")
                .peer_addr(peer(1))
                .insert_header(("Authorization", format!("Bearer {token}")))
                .insert_header(("Content-Type", "application/json"))
                .set_payload(json!({ "id": "x" }).to_string()),
        )
        .await;
    assert_limited(&write, 1, 1, 60);
    let login = login(&app, ADMIN, TEST_PASSWORD, peer(1)).await;
    assert_limited(&login, 1, 1, 60);

    // Control: the same read from another client reaches the closed pool.
    let reached = get_from(&app, "/api/v1/eras", 2).await;
    assert!(reached.status.is_server_error(), "{}", reached.status);
}

/// AC 28: `User-Agent` changes neither the responses nor the client's limit.
#[sqlx::test]
async fn user_agent_is_ignored(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app_with(pool_opts, opts, limited("1", "3")).await;
    let agents = [
        Some("Mozilla/5.0 (X11; Linux x86_64; rv:131.0) Gecko/20100101 Firefox/131.0"),
        Some("Mozilla/5.0 (compatible; Googlebot/2.1; +http://www.google.com/bot.html)"),
        None,
    ];
    let request = |agent: Option<&str>, n: u32| {
        let mut req = TestRequest::get().uri("/api/v1/eras").peer_addr(peer(n));
        if let Some(agent) = agent {
            req = req.insert_header(("User-Agent", agent.to_string()));
        }
        req
    };

    // Fresh clients with different agents get identical responses.
    let mut first: Option<Resp> = None;
    for (n, agent) in agents.iter().enumerate() {
        let resp = app.call(request(*agent, 10 + n as u32)).await;
        assert_eq!(resp.status.as_u16(), 200);
        match &first {
            None => first = Some(resp),
            Some(base) => {
                assert_eq!(header_list(&resp), header_list(base), "{agent:?}");
                assert_eq!(resp.body, base.body, "{agent:?}");
            }
        }
    }

    // One client switching agents shares one bucket.
    for (i, agent) in agents.iter().enumerate() {
        let resp = app.call(request(*agent, 1)).await;
        assert_eq!(resp.status.as_u16(), 200, "{agent:?}");
        assert_eq!(
            resp.header("ratelimit"),
            Some(format!(r#""general";r={};t={}"#, 2 - i, 60 * (i + 1)).as_str())
        );
    }
    let mut rejected: Option<Resp> = None;
    for agent in agents {
        let resp = app.call(request(agent, 1)).await;
        assert_limited(&resp, 1, 3, 60);
        match &rejected {
            None => rejected = Some(resp),
            Some(base) => {
                assert_eq!(header_list(&resp), header_list(base), "{agent:?}");
                assert_eq!(resp.body, base.body, "{agent:?}");
            }
        }
    }
}

/// AC 15: `RateLimit` and `RateLimit-Policy` on every response past the limit check,
/// including `304` and `429`, and exposed to scripts with `Retry-After`.
#[sqlx::test]
async fn every_response_carries_the_rate_limit_headers(
    pool_opts: PgPoolOptions,
    opts: PgConnectOptions,
) {
    // T = 1 s, τ = 5 s, w = 5.
    let app = app_with(pool_opts, opts, limited("60", "5")).await;
    let policy = r#""general";q=5;w=5"#;

    let ok = get_from(&app, "/api/v1/eras", 1).await;
    assert_eq!(ok.status.as_u16(), 200);
    assert_rate_headers(&ok, r#""general";r=4;t=1"#, policy);

    let not_modified = app
        .call(
            TestRequest::get()
                .uri("/api/v1/eras")
                .peer_addr(peer(1))
                .insert_header(("If-None-Match", etag(&ok))),
        )
        .await;
    assert_eq!(not_modified.status.as_u16(), 304);
    assert_rate_headers(&not_modified, r#""general";r=3;t=2"#, policy);

    let missing = get_from(&app, "/api/v1/nowhere", 1).await;
    assert_error(&missing, 404, "ROUTE_NOT_FOUND");
    assert_rate_headers(&missing, r#""general";r=2;t=3"#, policy);

    let unauthorized = app
        .call(
            TestRequest::default()
                .method(Method::POST)
                .uri("/api/v1/eras")
                .peer_addr(peer(1))
                .insert_header(("Content-Type", "application/json"))
                .set_payload("{}"),
        )
        .await;
    assert_error(&unauthorized, 401, "UNAUTHORIZED");
    assert_rate_headers(&unauthorized, r#""general";r=1;t=4"#, policy);

    let preflight = app
        .call(
            TestRequest::default()
                .method(Method::OPTIONS)
                .uri("/api/v1/eras")
                .peer_addr(peer(1))
                .insert_header(("Origin", "https://example.org"))
                .insert_header(("Access-Control-Request-Method", "GET")),
        )
        .await;
    assert_eq!(preflight.status.as_u16(), 204);
    assert_rate_headers(&preflight, r#""general";r=0;t=5"#, policy);

    let limited = get_from(&app, "/api/v1/eras", 1).await;
    assert_limited(&limited, 60, 5, 1);
    assert_rate_headers(&limited, r#""general";r=0;t=5"#, policy);
    assert_eq!(limited.header("access-control-allow-origin"), Some("*"));
}
