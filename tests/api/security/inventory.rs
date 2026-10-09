//! Route inventory: every write route is protected (003 AC 10; FR-015, research R16).

use actix_web::http::Method;
use actix_web::test::TestRequest;
use paleo_api::api::ROUTES;
use paleo_api::security::middleware::{LOGIN_PATH, needs_admin};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

use crate::support::*;

fn is_safe(method: &Method) -> bool {
    method == Method::GET || method == Method::HEAD || method == Method::OPTIONS
}

/// A concrete path for a `ROUTES` pattern.
fn concrete(pattern: &str) -> String {
    let filled: Vec<String> = pattern
        .split('/')
        .map(|s| {
            if s.starts_with('{') {
                "some-id".to_string()
            } else {
                s.to_string()
            }
        })
        .collect();
    format!("/api/v1{}", filled.join("/"))
}

/// Every non-safe `(pattern, method)` of `ROUTES` except login answers `401` without a token.
#[sqlx::test]
async fn every_write_route_needs_a_token(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app(pool_opts, opts).await;
    let mut checked = 0;
    for (pattern, methods) in ROUTES {
        let path = concrete(pattern);
        for method in methods.iter().filter(|m| !is_safe(m)) {
            if method == Method::POST && path == LOGIN_PATH {
                continue;
            }
            let resp = app
                .call(
                    TestRequest::default()
                        .method(method.clone())
                        .uri(&path)
                        .insert_header(("Content-Type", "application/json"))
                        .set_payload("{}"),
                )
                .await;
            assert_error(&resp, 401, "UNAUTHORIZED");
            checked += 1;
        }
    }
    assert!(checked > 30, "too few write routes checked: {checked}");
}

/// The decision function denies a matched pattern that `ROUTES` does not list.
#[test]
fn a_registered_pattern_absent_from_routes_is_denied() {
    assert!(ROUTES.iter().all(|(p, _)| *p != "/debug/reset"));
    for method in [Method::POST, Method::PATCH, Method::DELETE] {
        assert!(needs_admin(
            &method,
            "/api/v1/debug/reset",
            Some("/api/v1/debug/reset")
        ));
    }
    for (pattern, methods) in ROUTES {
        let path = concrete(pattern);
        for method in methods.iter().filter(|m| !is_safe(m)) {
            let login = method == Method::POST && path == LOGIN_PATH;
            assert_eq!(needs_admin(method, &path, None), !login, "{method} {path}");
        }
    }
}
