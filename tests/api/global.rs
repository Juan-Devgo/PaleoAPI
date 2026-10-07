//! Global rules (spec §4, AC 6.1): router, reads, writes, leaks.

use actix_web::test::TestRequest;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

use crate::support::*;

// ---------------------------------------------------------------- router

#[track_caller]
fn assert_cors(resp: &Resp) {
    assert_eq!(resp.header("access-control-allow-origin"), Some("*"));
    let expose = resp
        .header("access-control-expose-headers")
        .unwrap_or_default();
    assert!(
        expose.to_ascii_lowercase().contains("etag"),
        "Access-Control-Expose-Headers: {expose:?}"
    );
}

#[sqlx::test]
async fn unknown_route_is_route_not_found(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app(pool_opts, opts).await;
    for uri in ["/", "/api/v1/specie", "/api/v2/species", "/api/v1"] {
        let resp = get(&app, uri).await;
        assert_error(&resp, 404, "ROUTE_NOT_FOUND");
        assert_cors(&resp);
        assert_eq!(resp.header("cache-control"), Some("no-store"), "{uri}");
        assert!(
            resp.json()["error"]["message"]
                .as_str()
                .unwrap()
                .contains(uri),
            "{uri}"
        );
    }
}

#[sqlx::test]
async fn options_on_an_unknown_route_is_204(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app(pool_opts, opts).await;
    let resp = app
        .call(
            TestRequest::default()
                .method(actix_web::http::Method::OPTIONS)
                .uri("/nowhere")
                .insert_header(("Origin", "http://example.org"))
                .insert_header(("Access-Control-Request-Method", "GET")),
        )
        .await;
    assert_eq!(resp.status.as_u16(), 204, "{}", resp.text());
    assert_cors(&resp);
    assert_eq!(resp.header("access-control-allow-methods"), Some("GET"));
    assert_eq!(
        resp.header("access-control-allow-headers"),
        Some("If-None-Match")
    );
    assert!(resp.body.is_empty());
}

#[sqlx::test]
async fn api_pool_forces_custom_plans(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app(pool_opts, opts).await;
    let mode: String = sqlx::query_scalar("SHOW plan_cache_mode")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(mode, "force_custom_plan");
}
