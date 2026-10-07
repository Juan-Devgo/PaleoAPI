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

// ---------------------------------------------------------------- reads

/// `n` species `sp-01 …` named `Species 01 …` on one genus and period.
async fn seed_species(pool: &sqlx::PgPool, n: usize) {
    let (genus, period) = seed_base(pool, "g").await;
    for i in 1..=n {
        let id = format!("sp-{i:02}");
        let name = format!("Species {i:02}");
        species_row(pool, &id, &genus, &period, &name, &id, None).await;
    }
}

#[sqlx::test]
async fn list_defaults_and_limits(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app(pool_opts, opts).await;
    seed_species(&app.pool, 25).await;

    let resp = get(&app, "/api/v1/species").await;
    assert_eq!(ids(&resp).len(), 20);
    assert_eq!(
        pagination(&resp),
        serde_json::json!({ "page": 1, "limit": 20, "total_items": 25, "total_pages": 2 })
    );

    let resp = get(&app, "/api/v1/species?page=2&limit=20").await;
    assert_eq!(ids(&resp), ["sp-21", "sp-22", "sp-23", "sp-24", "sp-25"]);

    let resp = get(&app, "/api/v1/species?limit=100").await;
    assert_eq!(ids(&resp).len(), 25);

    for q in [
        "limit=101",
        "limit=0",
        "page=0",
        "page=1000001",
        "page=abc",
        "page=",
        "page=1&page=2",
    ] {
        let resp = get(&app, &format!("/api/v1/species?{q}")).await;
        assert_error(&resp, 400, "INVALID_QUERY_PARAMETER");
        assert_eq!(resp.header("cache-control"), Some("no-store"), "{q}");
    }
    // unknown parameters are ignored
    let resp = get(&app, "/api/v1/species?colour=red&colour=blue").await;
    assert_eq!(ids(&resp).len(), 20);
}

#[sqlx::test]
async fn page_past_the_end_and_empty_lists(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app(pool_opts, opts).await;
    let resp = get(&app, "/api/v1/species").await;
    assert!(ids(&resp).is_empty());
    assert_eq!(
        pagination(&resp),
        serde_json::json!({ "page": 1, "limit": 20, "total_items": 0, "total_pages": 0 })
    );

    seed_species(&app.pool, 7).await;
    let resp = get(&app, "/api/v1/species?page=9&limit=5").await;
    assert!(ids(&resp).is_empty());
    assert_eq!(
        pagination(&resp),
        serde_json::json!({ "page": 9, "limit": 5, "total_items": 7, "total_pages": 2 })
    );
    let resp = get(&app, "/api/v1/species?page=1000000").await;
    assert!(ids(&resp).is_empty());
}

#[sqlx::test]
async fn unknown_sort_lists_the_allowed_fields(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app(pool_opts, opts).await;
    let resp = get(&app, "/api/v1/species?sort=size").await;
    assert_error(&resp, 400, "INVALID_QUERY_PARAMETER");
    let msg = resp.json()["error"]["message"]
        .as_str()
        .unwrap()
        .to_string();
    for field in [
        "name",
        "-name",
        "scientific_name",
        "-scientific_name",
        "discovery_year",
        "-discovery_year",
    ] {
        assert!(msg.contains(field), "{msg}");
    }
}

#[sqlx::test]
async fn sorts_are_stable_and_ignore_case(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app(pool_opts, opts).await;
    let (genus, period) = seed_base(&app.pool, "g").await;
    for (id, name, sci) in [
        ("cc", "banana", "Zeta"),
        ("aa", "Banana", "alpha"),
        ("bb", "apple", "Beta"),
        ("dd", "Cherry", "gamma"),
        ("ee", "BANANA", "delta"),
    ] {
        species_row(&app.pool, id, &genus, &period, name, sci, None).await;
    }
    let order = |q: &'static str| {
        let app = &app;
        async move { ids(&get(app, &format!("/api/v1/species{q}")).await) }
    };
    assert_eq!(order("").await, ["bb", "aa", "cc", "ee", "dd"]);
    assert_eq!(order("?sort=name").await, ["bb", "aa", "cc", "ee", "dd"]);
    assert_eq!(order("?sort=-name").await, ["dd", "aa", "cc", "ee", "bb"]);
    assert_eq!(
        order("?sort=scientific_name").await,
        ["aa", "bb", "ee", "dd", "cc"]
    );
    assert_eq!(
        order("?sort=-scientific_name").await,
        ["cc", "dd", "ee", "bb", "aa"]
    );
    // same request, same order; pages never repeat or skip an item
    let mut paged = Vec::new();
    for page in 1..=5 {
        paged.extend(order_page(&app, page).await);
    }
    assert_eq!(paged, ["bb", "aa", "cc", "ee", "dd"]);
}

async fn order_page(app: &TestApp, page: u32) -> Vec<String> {
    ids(&get(app, &format!("/api/v1/species?limit=1&page={page}")).await)
}

#[sqlx::test]
async fn etag_and_cache_headers(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app(pool_opts, opts).await;
    seed_species(&app.pool, 2).await;
    for uri in ["/api/v1/species", "/api/v1/species/sp-01"] {
        let resp = get(&app, uri).await;
        assert_eq!(resp.status.as_u16(), 200, "{uri}: {}", resp.text());
        assert_eq!(resp.header("cache-control"), Some("public, max-age=300"));
        assert_cors(&resp);
        let tag = etag(&resp);
        assert!(tag.starts_with('"') && tag.ends_with('"'), "{tag}");

        let again = get(&app, uri).await;
        assert_eq!(etag(&again), tag, "same data, same ETag");

        for inm in [
            tag.clone(),
            format!("W/{tag}"),
            format!("\"x\", {tag}"),
            "*".into(),
        ] {
            let resp = get_with(&app, uri, &[("If-None-Match", &inm)]).await;
            assert_eq!(resp.status.as_u16(), 304, "{uri} {inm}");
            assert!(resp.body.is_empty());
            assert_eq!(etag(&resp), tag);
            assert_eq!(resp.header("cache-control"), Some("public, max-age=300"));
            assert_cors(&resp);
        }
        let resp = get_with(&app, uri, &[("If-None-Match", "\"other\"")]).await;
        assert_eq!(resp.status.as_u16(), 200);
    }
    // different pages, different tags
    let p1 = etag(&get(&app, "/api/v1/species?limit=1").await);
    let p2 = etag(&get(&app, "/api/v1/species?limit=1&page=2").await);
    assert_ne!(p1, p2);
}

#[sqlx::test]
async fn cors_on_errors_and_preflight_on_known_routes(
    pool_opts: PgPoolOptions,
    opts: PgConnectOptions,
) {
    let app = app(pool_opts, opts).await;
    let resp = get(&app, "/api/v1/species/nope").await;
    assert_error(&resp, 404, "SPECIES_NOT_FOUND");
    assert_cors(&resp);
    assert_eq!(resp.header("cache-control"), Some("no-store"));

    for uri in ["/api/v1/species", "/api/v1/species/nope"] {
        let resp = app
            .call(
                TestRequest::default()
                    .method(actix_web::http::Method::OPTIONS)
                    .uri(uri)
                    .insert_header(("Origin", "http://example.org"))
                    .insert_header(("Access-Control-Request-Method", "GET"))
                    .insert_header(("Access-Control-Request-Headers", "if-none-match")),
            )
            .await;
        assert_eq!(resp.status.as_u16(), 204, "{uri}");
        assert_cors(&resp);
        assert_eq!(resp.header("access-control-allow-methods"), Some("GET"));
    }
}

#[sqlx::test]
async fn unsupported_method_is_405_with_allow(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app(pool_opts, opts).await;
    for (method, uri) in [
        (actix_web::http::Method::PUT, "/api/v1/species"),
        (actix_web::http::Method::PUT, "/api/v1/species/x1"),
    ] {
        let resp = app
            .call(TestRequest::default().method(method).uri(uri))
            .await;
        assert_error(&resp, 405, "METHOD_NOT_ALLOWED");
        let allow = resp.header("allow").unwrap_or_default().to_string();
        assert!(allow.contains("GET"), "Allow: {allow:?}");
        assert!(
            resp.json()["error"]["message"]
                .as_str()
                .unwrap()
                .contains("GET"),
            "{}",
            resp.text()
        );
        assert_cors(&resp);
    }
}
