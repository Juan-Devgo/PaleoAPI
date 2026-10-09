//! Security headers and preflight (003 AC 18, 19; FR-033–FR-035, research R15).

use actix_web::http::Method;
use actix_web::test::TestRequest;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

use crate::support::*;

const HSTS: &str = "max-age=63072000; includeSubDomains";

#[track_caller]
fn assert_security_headers(resp: &Resp, what: &str) {
    let h = |name| resp.header(name);
    assert_eq!(h("x-content-type-options"), Some("nosniff"), "{what}");
    assert_eq!(
        h("content-security-policy"),
        Some("default-src 'none'; frame-ancestors 'none'"),
        "{what}"
    );
    assert_eq!(h("referrer-policy"), Some("no-referrer"), "{what}");
    assert_eq!(h("strict-transport-security"), Some(HSTS), "{what}");
    assert_eq!(h("access-control-allow-origin"), Some("*"), "{what}");
    assert!(h("server").is_none(), "{what}: Server header present");
    assert!(
        h("access-control-allow-credentials").is_none(),
        "{what}: Allow-Credentials present"
    );
}

/// AC 18: the FR-033 set and no `Server` on `200`, `304`, `4xx`, `429`, and `5xx`.
#[sqlx::test]
async fn every_response_carries_the_security_headers(
    pool_opts: PgPoolOptions,
    opts: PgConnectOptions,
) {
    let app = app(pool_opts, opts).await;
    let ok = get(&app, "/api/v1/eras").await;
    assert_eq!(ok.status.as_u16(), 200);
    assert_security_headers(&ok, "200");

    let not_modified = get_with(&app, "/api/v1/eras", &[("If-None-Match", &etag(&ok))]).await;
    assert_eq!(not_modified.status.as_u16(), 304);
    assert_security_headers(&not_modified, "304");

    let missing = get(&app, "/api/v1/eras/nope").await;
    assert_error(&missing, 404, "ERA_NOT_FOUND");
    assert_security_headers(&missing, "404");

    let route = get(&app, "/nowhere").await;
    assert_error(&route, 404, "ROUTE_NOT_FOUND");
    assert_security_headers(&route, "404 route");

    let unauthorized = send_as(
        &app,
        Method::POST,
        "/api/v1/eras",
        serde_json::json!({}),
        None,
    )
    .await;
    assert_error(&unauthorized, 401, "UNAUTHORIZED");
    assert_security_headers(&unauthorized, "401");

    let forbidden = send_as(
        &app,
        Method::POST,
        "/api/v1/eras",
        serde_json::json!({}),
        Some(&app.user_token()),
    )
    .await;
    assert_error(&forbidden, 403, "FORBIDDEN");
    assert_security_headers(&forbidden, "403");

    let not_allowed = send(&app, Method::PUT, "/api/v1/eras", serde_json::json!({})).await;
    assert_error(&not_allowed, 405, "METHOD_NOT_ALLOWED");
    assert_security_headers(&not_allowed, "405");

    let preflight = app
        .call(
            TestRequest::default()
                .method(Method::OPTIONS)
                .uri("/api/v1/eras"),
        )
        .await;
    assert_eq!(preflight.status.as_u16(), 204);
    assert_security_headers(&preflight, "204");
}

#[sqlx::test]
async fn a_429_carries_the_security_headers(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let settings = SecuritySettings {
        config: security_env(&[("RATE_LIMIT_PER_MINUTE", "60"), ("RATE_LIMIT_BURST", "1")]),
        manual_clock: true,
        ..SecuritySettings::default()
    };
    let app = app_with(pool_opts, opts, settings).await;
    assert_eq!(get(&app, "/api/v1/eras").await.status.as_u16(), 200);
    let limited = get(&app, "/api/v1/eras").await;
    assert_error(&limited, 429, "RATE_LIMITED");
    assert_security_headers(&limited, "429");
}

#[actix_web::test]
async fn a_5xx_carries_the_security_headers() {
    let settings = SecuritySettings {
        seed: false,
        ..SecuritySettings::default()
    };
    let app = app_with_pool(closed_pool(), settings).await;
    let resp = get(&app, "/api/v1/eras").await;
    assert_error(&resp, 503, "SERVICE_UNAVAILABLE");
    assert_security_headers(&resp, "503");
}

/// FR-035: login and writes are `no-store`, whatever the outcome.
#[sqlx::test]
async fn non_safe_responses_are_no_store(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app(pool_opts, opts).await;
    let bad = login(&app, ADMIN, "wrong-password-0000", peer(1)).await;
    assert_eq!(bad.header("cache-control"), Some("no-store"));
    let ok = login(&app, ADMIN, TEST_PASSWORD, peer(1)).await;
    assert_eq!(ok.status.as_u16(), 200);
    assert_eq!(ok.header("cache-control"), Some("no-store"));
    assert_security_headers(&ok, "login");
}

/// AC 19: a preflight for `POST` with `Authorization` is answered with the R15 values,
/// on every path, and never allows credentials.
#[sqlx::test]
async fn preflight_allows_writes_without_credentials(
    pool_opts: PgPoolOptions,
    opts: PgConnectOptions,
) {
    let app = app(pool_opts, opts).await;
    for uri in ["/api/v1/eras", "/api/v1/auth/login", "/nowhere"] {
        let resp = app
            .call(
                TestRequest::default()
                    .method(Method::OPTIONS)
                    .uri(uri)
                    .insert_header(("Origin", "https://example.org"))
                    .insert_header(("Access-Control-Request-Method", "POST"))
                    .insert_header(("Access-Control-Request-Headers", "authorization")),
            )
            .await;
        assert_eq!(resp.status.as_u16(), 204, "{uri}");
        assert_eq!(
            resp.header("access-control-allow-methods"),
            Some("GET, POST, PATCH, DELETE"),
            "{uri}"
        );
        assert_eq!(
            resp.header("access-control-allow-headers"),
            Some("Authorization, Content-Type, If-None-Match"),
            "{uri}"
        );
        assert_eq!(
            resp.header("access-control-max-age"),
            Some("86400"),
            "{uri}"
        );
        assert_security_headers(&resp, uri);
    }
}
