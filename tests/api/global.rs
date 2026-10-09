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
    let mut exposed: Vec<String> = expose
        .split(',')
        .map(|h| h.trim().to_ascii_lowercase())
        .collect();
    exposed.sort();
    // 003 research R4: scripts read the ETag, Retry-After, and rate-limit headers.
    assert_eq!(
        exposed,
        ["etag", "ratelimit", "ratelimit-policy", "retry-after"],
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
    assert_eq!(
        resp.header("access-control-allow-methods"),
        Some("GET, POST, PATCH, DELETE")
    );
    assert_eq!(
        resp.header("access-control-allow-headers"),
        Some("Authorization, Content-Type, If-None-Match")
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

#[sqlx::test]
async fn api_pool_limits_each_statement_to_2_seconds(
    pool_opts: PgPoolOptions,
    opts: PgConnectOptions,
) {
    let app = app(pool_opts, opts).await;
    let timeout: String = sqlx::query_scalar("SHOW statement_timeout")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(timeout, "2s");
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
        assert_eq!(
            resp.header("access-control-allow-methods"),
            Some("GET, POST, PATCH, DELETE")
        );
        assert_eq!(
            resp.header("access-control-allow-headers"),
            Some("Authorization, Content-Type, If-None-Match")
        );
        assert_eq!(resp.header("access-control-max-age"), Some("86400"));
        assert_eq!(resp.header("access-control-allow-credentials"), None);
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

// ---------------------------------------------------------------- writes

use actix_web::http::Method;
use serde_json::json;

fn mesozoic() -> serde_json::Value {
    json!({ "id": "mesozoic", "name": "Mesozoic", "start_mya": 251.902, "end_mya": 66 })
}

/// Seeds the Mesozoic era with three periods through SQL.
async fn seed_mesozoic(pool: &sqlx::PgPool) {
    for sql in [
        "INSERT INTO eras VALUES ('mesozoic', 'Mesozoic', 251.902, 66)",
        "INSERT INTO periods VALUES ('triassic', 'mesozoic', 'Triassic', 251.902, 201.4), \
         ('jurassic', 'mesozoic', 'Jurassic', 201.4, 145), \
         ('cretaceous', 'mesozoic', 'Cretaceous', 145, 66)",
    ] {
        exec(pool, sql).await.unwrap();
    }
}

#[sqlx::test]
async fn create_returns_201_location_and_the_get_body(
    pool_opts: PgPoolOptions,
    opts: PgConnectOptions,
) {
    let app = app(pool_opts, opts).await;
    let resp = send(&app, Method::POST, "/api/v1/eras", mesozoic()).await;
    assert_eq!(resp.status.as_u16(), 201, "{}", resp.text());
    let location = resp.header("location").unwrap().to_string();
    assert_eq!(location, "/api/v1/eras/mesozoic");
    let got = get(&app, &location).await;
    assert_eq!(got.status.as_u16(), 200);
    assert_eq!(resp.json(), got.json());
}

#[sqlx::test]
async fn delete_returns_204_then_404(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app(pool_opts, opts).await;
    exec(
        &app.pool,
        "INSERT INTO eras VALUES ('cenozoic', 'Cenozoic', 66, 0)",
    )
    .await
    .unwrap();
    let resp = delete(&app, "/api/v1/eras/cenozoic").await;
    assert_eq!(resp.status.as_u16(), 204, "{}", resp.text());
    assert!(resp.body.is_empty());
    assert_error(
        &get(&app, "/api/v1/eras/cenozoic").await,
        404,
        "ERA_NOT_FOUND",
    );
}

#[sqlx::test]
async fn writes_change_the_etag(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app(pool_opts, opts).await;
    seed_mesozoic(&app.pool).await;
    for uri in [
        "/api/v1/eras/mesozoic",
        "/api/v1/eras",
        "/api/v1/periods/jurassic",
    ] {
        let before = get(&app, uri).await;
        let old = etag(&before);
        let name = format!("Mesozoic {}", uri.len());
        let resp = send(
            &app,
            Method::PATCH,
            "/api/v1/eras/mesozoic",
            json!({ "name": name }),
        )
        .await;
        assert_eq!(resp.status.as_u16(), 200, "{}", resp.text());
        let after = get_with(
            &app,
            uri,
            &[("If-None-Match", &old), ("Cache-Control", "no-cache")],
        )
        .await;
        assert_eq!(after.status.as_u16(), 200, "{uri}: old ETag must not match");
        assert_ne!(etag(&after), old, "{uri}");
        assert!(after.text().contains(&name), "{uri}: {}", after.text());
    }
}

#[sqlx::test]
async fn options_on_write_routes_is_204(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app(pool_opts, opts).await;
    for uri in [
        "/api/v1/eras",
        "/api/v1/eras/x1/periods",
        "/api/v1/taxonomy/domains",
        "/api/v1/species/x1",
    ] {
        let resp = app
            .call(
                TestRequest::default()
                    .method(Method::OPTIONS)
                    .uri(uri)
                    .insert_header(("Access-Control-Request-Method", "POST")),
            )
            .await;
        assert_eq!(resp.status.as_u16(), 204, "{uri}");
    }
}

/// `401` messages (003 FR-018, contracts/openapi.yaml `Unauthorized`).
const MISSING_TOKEN: &str = "This operation needs an admin token. Log in at POST /api/v1/auth/login \
                             and send the token as 'Authorization: Bearer <token>'.";
const INVALID_TOKEN: &str =
    "Your token is not valid. Log in again at POST /api/v1/auth/login to get a new one.";

#[sqlx::test]
async fn writes_need_admin_credentials(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app(pool_opts, opts).await;
    seed_mesozoic(&app.pool).await;
    let cases = [
        (Method::POST, "/api/v1/eras", mesozoic()),
        (
            Method::PATCH,
            "/api/v1/eras/mesozoic",
            json!({ "name": "X" }),
        ),
        (
            Method::PATCH,
            "/api/v1/eras/unknown-era",
            json!({ "name": "X" }),
        ),
        (Method::DELETE, "/api/v1/eras/mesozoic", json!({})),
        (Method::DELETE, "/api/v1/eras/unknown-era", json!({})),
        (Method::POST, "/api/v1/species", json!({})),
    ];
    for (method, uri, body) in cases {
        for (token, message) in [
            (None, MISSING_TOKEN),
            (Some("0f3c5d9e-random-token"), INVALID_TOKEN),
        ] {
            let resp = send_as(&app, method.clone(), uri, body.clone(), token).await;
            assert_error(&resp, 401, "UNAUTHORIZED");
            assert_eq!(resp.json()["error"]["message"], message, "{method} {uri}");
        }
        let resp = send_as(
            &app,
            method.clone(),
            uri,
            body.clone(),
            Some(&app.user_token()),
        )
        .await;
        assert_error(&resp, 403, "FORBIDDEN");
    }
    assert_eq!(
        get(&app, "/api/v1/eras/mesozoic").await.json()["data"]["name"],
        "Mesozoic"
    );
}

#[sqlx::test]
async fn body_problems_are_400_413_415(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app(pool_opts, opts).await;
    let resp = send_raw(
        &app,
        Method::POST,
        "/api/v1/eras",
        Some("application/json"),
        b"{\"id\": ".to_vec(),
    )
    .await;
    assert_error(&resp, 400, "INVALID_JSON");
    assert!(resp.text().contains("line 1"), "{}", resp.text());
    let resp = send_raw(
        &app,
        Method::POST,
        "/api/v1/eras",
        Some("application/json"),
        vec![],
    )
    .await;
    assert_error(&resp, 400, "INVALID_JSON");

    for ct in [
        Some("text/plain"),
        Some("application/x-www-form-urlencoded"),
        None,
    ] {
        let resp = send_raw(&app, Method::POST, "/api/v1/eras", ct, b"{}".to_vec()).await;
        assert_error(&resp, 415, "UNSUPPORTED_MEDIA_TYPE");
    }
    let ok = send_raw(
        &app,
        Method::POST,
        "/api/v1/eras",
        Some("application/json; charset=utf-8"),
        serde_json::to_vec(&mesozoic()).unwrap(),
    )
    .await;
    assert_eq!(ok.status.as_u16(), 201, "{}", ok.text());

    let mut big = br#"{"id": "big-era", "name": ""#.to_vec();
    big.extend(std::iter::repeat_n(b'x', 70_000));
    big.extend(br#""}"#);
    let resp = send_raw(
        &app,
        Method::POST,
        "/api/v1/eras",
        Some("application/json"),
        big.clone(),
    )
    .await;
    assert_error(&resp, 413, "PAYLOAD_TOO_LARGE");
    let chunked = TestRequest::default()
        .method(Method::POST)
        .uri("/api/v1/eras")
        .insert_header(("Authorization", format!("Bearer {}", app.admin_token())))
        .insert_header(("Content-Type", "application/json"))
        .set_payload(big);
    let resp = app.call_chunked(chunked).await;
    assert_error(&resp, 413, "PAYLOAD_TOO_LARGE");

    // exactly 64 KB is accepted as a body (and then validated)
    let mut edge = br#"{"id": "edge-era", "name": ""#.to_vec();
    let filler = 65_536 - edge.len() - 2;
    edge.extend(std::iter::repeat_n(b'y', filler));
    edge.extend(br#""}"#);
    assert_eq!(edge.len(), 65_536);
    let resp = send_raw(
        &app,
        Method::POST,
        "/api/v1/eras",
        Some("application/json"),
        edge,
    )
    .await;
    assert_error(&resp, 422, "VALIDATION_FAILED");
}

#[sqlx::test]
async fn validation_reports_every_problem(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app(pool_opts, opts).await;
    seed_mesozoic(&app.pool).await;

    let resp = send(
        &app,
        Method::POST,
        "/api/v1/eras",
        json!({ "id": "Bad Id", "name": "  ", "start_mya": 10, "end_mya": 20, "colour": "red" }),
    )
    .await;
    assert_details(&resp, &["id", "name", "end_mya", "colour"]);
    let details = resp.json()["error"]["details"].clone();
    assert_eq!(details.as_array().unwrap().len(), 4, "{details}");

    let resp = send(
        &app,
        Method::POST,
        "/api/v1/eras",
        json!({ "id": "x".repeat(65), "name": "Long", "start_mya": 30, "end_mya": 20 }),
    )
    .await;
    assert_details(&resp, &["id"]);

    let resp = send(
        &app,
        Method::POST,
        "/api/v1/eras",
        json!({ "id": "e2", "name": null, "start_mya": 30, "end_mya": 20 }),
    )
    .await;
    assert_details(&resp, &["name"]);
    let resp = send(
        &app,
        Method::PATCH,
        "/api/v1/eras/mesozoic",
        json!({ "name": null }),
    )
    .await;
    assert_details(&resp, &["name"]);

    for (body, fields) in [
        (json!({}), vec![""]),
        (json!({ "id": "other" }), vec!["id"]),
        (json!({ "nmae": "typo" }), vec!["nmae"]),
        (json!({ "name": "Ok", "periods": [] }), vec!["periods"]),
        (json!([]), vec![""]),
        (json!("text"), vec![""]),
    ] {
        let resp = send(&app, Method::PATCH, "/api/v1/eras/mesozoic", body.clone()).await;
        assert_details(&resp, &fields);
    }
    let resp = send(
        &app,
        Method::PATCH,
        "/api/v1/periods/jurassic",
        json!({ "era": { "id": "mesozoic" } }),
    )
    .await;
    assert_details(&resp, &["era"]);
    // rejected writes change nothing
    assert_eq!(
        get(&app, "/api/v1/eras/mesozoic").await.json()["data"]["name"],
        "Mesozoic"
    );
}

#[sqlx::test]
async fn names_are_trimmed_and_unique_ignoring_case(
    pool_opts: PgPoolOptions,
    opts: PgConnectOptions,
) {
    let app = app(pool_opts, opts).await;
    let resp = send(
        &app,
        Method::POST,
        "/api/v1/eras",
        json!({ "id": "cenozoic", "name": "  Cenozoic \t", "start_mya": 66, "end_mya": 0 }),
    )
    .await;
    assert_eq!(resp.status.as_u16(), 201, "{}", resp.text());
    assert_eq!(resp.json()["data"]["name"], "Cenozoic");

    let resp = send(
        &app,
        Method::POST,
        "/api/v1/eras",
        json!({ "id": "cenozoic", "name": "Other", "start_mya": 500, "end_mya": 400 }),
    )
    .await;
    assert_error(&resp, 409, "ERA_ALREADY_EXISTS");
    assert!(resp.text().contains("cenozoic"), "{}", resp.text());
    let resp = send(
        &app,
        Method::POST,
        "/api/v1/eras",
        json!({ "id": "other", "name": "CENOZOIC", "start_mya": 500, "end_mya": 400 }),
    )
    .await;
    assert_error(&resp, 409, "ERA_ALREADY_EXISTS");
    assert!(resp.text().contains("ignoring case"), "{}", resp.text());

    // renaming to another casing of its own name is allowed
    let resp = send(
        &app,
        Method::PATCH,
        "/api/v1/eras/cenozoic",
        json!({ "name": "CENOZOIC" }),
    )
    .await;
    assert_eq!(resp.status.as_u16(), 200, "{}", resp.text());
    assert_eq!(resp.json()["data"]["name"], "CENOZOIC");

    // 422 beats 409: invalid body with a duplicate id
    let resp = send(
        &app,
        Method::POST,
        "/api/v1/eras",
        json!({ "id": "cenozoic", "name": "Dup", "start_mya": 1, "end_mya": 2 }),
    )
    .await;
    assert_details(&resp, &["end_mya"]);
}

#[sqlx::test]
async fn unknown_ids_on_writes_are_404(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app(pool_opts, opts).await;
    let resp = send(
        &app,
        Method::PATCH,
        "/api/v1/eras/unknown-era",
        json!({ "name": "X" }),
    )
    .await;
    assert_error(&resp, 404, "ERA_NOT_FOUND");
    assert!(resp.text().contains("'unknown-era'"));
    let resp = delete(&app, "/api/v1/eras/unknown-era").await;
    assert_error(&resp, 404, "ERA_NOT_FOUND");
    let resp = delete(&app, "/api/v1/eras/NOT_A_SLUG").await;
    assert_error(&resp, 404, "ERA_NOT_FOUND");
}

#[sqlx::test]
async fn delete_protection_names_dependents(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app(pool_opts, opts).await;
    exec(&app.pool, "INSERT INTO eras VALUES ('big', 'Big', 4000, 0)")
        .await
        .unwrap();
    let mut values = Vec::new();
    for i in 0..7 {
        let start = 4000 - i * 100;
        values.push(format!("('p{i}', 'big', 'P{i}', {start}, {})", start - 100));
    }
    exec(
        &app.pool,
        format!("INSERT INTO periods VALUES {}", values.join(", ")),
    )
    .await
    .unwrap();
    let resp = delete(&app, "/api/v1/eras/big").await;
    assert_error(&resp, 409, "ERA_HAS_DEPENDENTS");
    let msg = resp.json()["error"]["message"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(msg.contains("7 periods"), "{msg}");
    let named: Vec<_> = (0..7).filter(|i| msg.contains(&format!("p{i}"))).collect();
    assert_eq!(named, [0, 1, 2, 3, 4], "{msg}");
    assert_eq!(get(&app, "/api/v1/eras/big").await.status.as_u16(), 200);
}

#[sqlx::test]
async fn check_order(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app(pool_opts, opts).await;
    seed_mesozoic(&app.pool).await;
    let huge = vec![b'x'; 70_000];

    // 404 route / 405 method beat 401
    let resp = send_as(&app, Method::POST, "/api/v1/nowhere", json!({}), None).await;
    assert_error(&resp, 404, "ROUTE_NOT_FOUND");
    let resp = send_as(&app, Method::PUT, "/api/v1/eras", json!({}), None).await;
    assert_error(&resp, 405, "METHOD_NOT_ALLOWED");
    assert!(resp.header("allow").unwrap().contains("POST"));

    // 401 beats 413 / 415 / 400
    let unauthenticated = |body: Vec<u8>, ct: &'static str| {
        TestRequest::default()
            .method(Method::POST)
            .uri("/api/v1/eras")
            .insert_header(("Content-Type", ct))
            .set_payload(body)
    };
    for (body, ct) in [
        (huge.clone(), "application/json"),
        (b"{}".to_vec(), "text/plain"),
        (b"{".to_vec(), "application/json"),
    ] {
        assert_error(
            &app.call(unauthenticated(body, ct)).await,
            401,
            "UNAUTHORIZED",
        );
    }
    // 403 beats 400
    let resp = app
        .call(
            unauthenticated(b"{".to_vec(), "application/json")
                .insert_header(("Authorization", format!("Bearer {}", app.user_token()))),
        )
        .await;
    assert_error(&resp, 403, "FORBIDDEN");

    // 413 / 415 / 400 beat 404 path
    let uri = "/api/v1/eras/unknown-era";
    assert_error(
        &send_raw(&app, Method::PATCH, uri, Some("application/json"), huge).await,
        413,
        "PAYLOAD_TOO_LARGE",
    );
    assert_error(
        &send_raw(&app, Method::PATCH, uri, Some("text/plain"), b"{}".to_vec()).await,
        415,
        "UNSUPPORTED_MEDIA_TYPE",
    );
    assert_error(
        &send_raw(
            &app,
            Method::PATCH,
            uri,
            Some("application/json"),
            b"{".to_vec(),
        )
        .await,
        400,
        "INVALID_JSON",
    );

    // 404 path (including a nested-create parent) beats 422, even for a non-object body
    for body in [json!({ "id": "Bad" }), json!([1, 2]), json!(null)] {
        assert_error(
            &send(&app, Method::PATCH, uri, body.clone()).await,
            404,
            "ERA_NOT_FOUND",
        );
        assert_error(
            &send(&app, Method::POST, "/api/v1/eras/unknown-era/periods", body).await,
            404,
            "ERA_NOT_FOUND",
        );
    }

    // 422 beats 409 (duplicate id with an invalid field)
    let resp = send(
        &app,
        Method::POST,
        "/api/v1/eras",
        json!({ "id": "mesozoic", "name": "Mesozoic", "start_mya": 251.902 }),
    )
    .await;
    assert_details(&resp, &["end_mya"]);
    // 422 beats 409 for delete protection? No: DELETE has no body; 409 follows 404.
    assert_error(
        &delete(&app, "/api/v1/eras/mesozoic").await,
        409,
        "ERA_HAS_DEPENDENTS",
    );
}

// ---------------------------------------------------------------- leaks

/// Words that would reveal internals in an error body (AC 6.1.18).
const LEAK_WORDS: &[&str] = &[
    "SQL",
    "sql",
    "constraint",
    "sqlx",
    "panicked",
    "ApiError",
    "BigDecimal",
    "PgDatabaseError",
    "DETAIL",
    "stack",
    "src/",
];

#[track_caller]
fn assert_no_leak(resp: &Resp) {
    let text = resp.text();
    for word in LEAK_WORDS {
        assert!(
            !text.contains(word),
            "{} body leaks {word:?}: {text}",
            resp.status
        );
    }
}

#[sqlx::test]
async fn error_bodies_leak_nothing(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app(pool_opts, opts).await;
    seed_mesozoic(&app.pool).await;
    let mut responses = vec![
        get(&app, "/api/v1/nowhere").await,
        get(&app, "/api/v1/species?sort=size").await,
        get(&app, "/api/v1/species?page=x").await,
        get(&app, "/api/v1/eras/nope").await,
        send_as(&app, Method::POST, "/api/v1/eras", json!({}), None).await,
        send_as(
            &app,
            Method::POST,
            "/api/v1/eras",
            json!({}),
            Some(&app.user_token()),
        )
        .await,
        send_raw(
            &app,
            Method::POST,
            "/api/v1/eras",
            Some("application/json"),
            b"{\"a\":".to_vec(),
        )
        .await,
        send_raw(
            &app,
            Method::POST,
            "/api/v1/eras",
            Some("text/plain"),
            b"x".to_vec(),
        )
        .await,
        send_raw(
            &app,
            Method::POST,
            "/api/v1/eras",
            Some("application/json"),
            vec![b' '; 70_000],
        )
        .await,
        app.call(
            TestRequest::default()
                .method(Method::PUT)
                .uri("/api/v1/eras"),
        )
        .await,
        delete(&app, "/api/v1/eras/mesozoic").await,
        send(&app, Method::POST, "/api/v1/eras", mesozoic()).await,
        send(
            &app,
            Method::POST,
            "/api/v1/eras",
            json!({ "id": "dup", "name": "MESOZOIC", "start_mya": 4000, "end_mya": 3000 }),
        )
        .await,
        send(
            &app,
            Method::POST,
            "/api/v1/eras",
            json!({ "id": "e1", "name": "E", "start_mya": 1e15, "end_mya": "x" }),
        )
        .await,
        send(
            &app,
            Method::POST,
            "/api/v1/eras",
            json!({ "id": "e1", "name": "E", "start_mya": 200, "end_mya": 100 }),
        )
        .await,
        send(
            &app,
            Method::PATCH,
            "/api/v1/eras/mesozoic",
            json!({ "start_mya": 200 }),
        )
        .await,
        send(
            &app,
            Method::POST,
            "/api/v1/species",
            json!({ "id": "s1", "genus_id": "nope", "period_ids": ["nope"] }),
        )
        .await,
        send(
            &app,
            Method::PATCH,
            "/api/v1/periods/jurassic",
            json!({ "era_id": "nope" }),
        )
        .await,
    ];
    let deep = format!("{}1{}", "[".repeat(200), "]".repeat(200));
    responses.push(
        send_raw(
            &app,
            Method::POST,
            "/api/v1/eras",
            Some("application/json"),
            deep.into_bytes(),
        )
        .await,
    );
    for resp in &responses {
        assert!(
            resp.status.as_u16() >= 400,
            "expected an error: {}",
            resp.text()
        );
        assert!(
            resp.status.as_u16() < 500,
            "unexpected 500: {}",
            resp.text()
        );
        assert_no_leak(resp);
    }
}

/// Fills `{param}` segments of a route with a well-formed id.
fn concrete(path: &str) -> String {
    path.split('/')
        .map(|seg| if seg.starts_with('{') { "x1" } else { seg })
        .collect::<Vec<_>>()
        .join("/")
}

#[sqlx::test]
async fn every_route_dispatches_its_methods(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app(pool_opts, opts).await;
    for (path, methods) in paleo_api::api::ROUTES {
        let uri = format!("/api/v1{}", concrete(path));
        for method in *methods {
            let resp = if method == Method::GET {
                get(&app, &uri).await
            } else {
                send(&app, method.clone(), &uri, json!({})).await
            };
            let code = resp.json()["error"]["code"].clone();
            assert_ne!(code, "ROUTE_NOT_FOUND", "{method} {uri}");
            assert_ne!(resp.status.as_u16(), 405, "{method} {uri}");
            assert!(
                resp.status.as_u16() < 500,
                "{method} {uri}: {}",
                resp.text()
            );
            assert_no_leak(&resp);
        }
    }
}
