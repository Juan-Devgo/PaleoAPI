//! Eras and periods (spec §5.1, §5.2; AC 6.2).

use serde_json::{Value, json};
use sqlx::PgPool;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

use crate::support::*;

/// A JSON number from its exact text.
fn num(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

// ---------------------------------------------------------------- reads

async fn seed_timeline(pool: &PgPool) {
    for sql in [
        "INSERT INTO eras VALUES ('paleozoic', 'Paleozoic', 538.8, 251.902), \
         ('mesozoic', 'Mesozoic', 251.902, 66), ('cenozoic', 'Cenozoic', 66, 0)",
        "INSERT INTO periods VALUES ('triassic', 'mesozoic', 'Triassic', 251.902, 201.4), \
         ('jurassic', 'mesozoic', 'Jurassic', 201.4, 145), \
         ('cretaceous', 'mesozoic', 'Cretaceous', 145, 66.0), \
         ('paleogene', 'cenozoic', 'Paleogene', 66, 23.03)",
    ] {
        exec(pool, sql)
            .await
            .unwrap_or_else(|e| panic!("{sql}: {e}"));
    }
}

async fn list(app: &TestApp, uri: &str) -> Vec<String> {
    ids(&get(app, uri).await)
}

#[sqlx::test]
async fn era_list_and_sorts(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app(pool_opts, opts).await;
    seed_timeline(&app.pool).await;
    for (query, want) in [
        ("", ["paleozoic", "mesozoic", "cenozoic"]),
        ("?sort=-start_mya", ["paleozoic", "mesozoic", "cenozoic"]),
        ("?sort=start_mya", ["cenozoic", "mesozoic", "paleozoic"]),
        ("?sort=name", ["cenozoic", "mesozoic", "paleozoic"]),
        ("?sort=-name", ["paleozoic", "mesozoic", "cenozoic"]),
    ] {
        assert_eq!(
            list(&app, &format!("/api/v1/eras{query}")).await,
            want,
            "{query}"
        );
    }
    let resp = get(&app, "/api/v1/eras?sort=end_mya").await;
    assert_error(&resp, 400, "INVALID_QUERY_PARAMETER");
    let resp = get(&app, "/api/v1/eras?limit=2").await;
    assert_eq!(
        pagination(&resp),
        json!({ "page": 1, "limit": 2, "total_items": 3, "total_pages": 2 })
    );
}

#[sqlx::test]
async fn era_detail_has_exact_decimals(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app(pool_opts, opts).await;
    seed_timeline(&app.pool).await;
    let resp = get(&app, "/api/v1/eras/mesozoic").await;
    assert_eq!(resp.status.as_u16(), 200, "{}", resp.text());
    assert_eq!(
        resp.json(),
        json!({ "data": { "id": "mesozoic", "name": "Mesozoic", "start_mya": num("251.902"), "end_mya": 66 } })
    );
    assert!(resp.text().contains("\"end_mya\":66}"), "{}", resp.text());
    let listed = get(&app, "/api/v1/eras").await.json()["data"][1].clone();
    assert_eq!(listed, resp.json()["data"]);
}

#[sqlx::test]
async fn period_lists_and_sorts(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app(pool_opts, opts).await;
    seed_timeline(&app.pool).await;
    for (uri, want) in [
        (
            "/api/v1/periods",
            vec!["triassic", "jurassic", "cretaceous", "paleogene"],
        ),
        (
            "/api/v1/periods?sort=start_mya",
            vec!["paleogene", "cretaceous", "jurassic", "triassic"],
        ),
        (
            "/api/v1/periods?sort=name",
            vec!["cretaceous", "jurassic", "paleogene", "triassic"],
        ),
        (
            "/api/v1/periods?sort=-name",
            vec!["triassic", "paleogene", "jurassic", "cretaceous"],
        ),
        ("/api/v1/periods?era=cenozoic", vec!["paleogene"]),
        (
            "/api/v1/periods?era=mesozoic&sort=start_mya",
            vec!["cretaceous", "jurassic", "triassic"],
        ),
        ("/api/v1/periods?era=unknown-era", vec![]),
        ("/api/v1/periods?era=paleozoic", vec![]),
        (
            "/api/v1/eras/mesozoic/periods",
            vec!["triassic", "jurassic", "cretaceous"],
        ),
        (
            "/api/v1/eras/mesozoic/periods?sort=name",
            vec!["cretaceous", "jurassic", "triassic"],
        ),
        (
            "/api/v1/eras/mesozoic/periods?sort=-name",
            vec!["triassic", "jurassic", "cretaceous"],
        ),
        (
            "/api/v1/eras/mesozoic/periods?sort=start_mya&limit=1&page=2",
            vec!["jurassic"],
        ),
        ("/api/v1/eras/paleozoic/periods", vec![]),
    ] {
        assert_eq!(list(&app, uri).await, want, "{uri}");
    }
    for uri in [
        "/api/v1/periods?era=Bad_Id",
        "/api/v1/periods?era=",
        "/api/v1/eras/mesozoic/periods?sort=size",
    ] {
        assert_error(&get(&app, uri).await, 400, "INVALID_QUERY_PARAMETER");
    }
}

#[sqlx::test]
async fn period_detail_and_items_embed_the_era(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app(pool_opts, opts).await;
    seed_timeline(&app.pool).await;
    let want = json!({
        "id": "cretaceous", "name": "Cretaceous", "start_mya": 145, "end_mya": 66,
        "era": { "id": "mesozoic", "name": "Mesozoic" }
    });
    let resp = get(&app, "/api/v1/periods/cretaceous").await;
    assert_eq!(resp.status.as_u16(), 200, "{}", resp.text());
    assert_eq!(resp.json(), json!({ "data": want }));
    assert_eq!(resp.header("cache-control"), Some("public, max-age=300"));
    let items = get(&app, "/api/v1/eras/mesozoic/periods").await.json()["data"].clone();
    assert_eq!(items[2], want);
    let items = get(&app, "/api/v1/periods?sort=name").await.json()["data"].clone();
    assert_eq!(items[0], want);
}

#[sqlx::test]
async fn unknown_and_malformed_ids_are_404(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app(pool_opts, opts).await;
    seed_timeline(&app.pool).await;
    for (uri, code) in [
        ("/api/v1/eras/unknown-era", "ERA_NOT_FOUND"),
        ("/api/v1/eras/NOT_A_SLUG", "ERA_NOT_FOUND"),
        ("/api/v1/eras/unknown-era/periods", "ERA_NOT_FOUND"),
        ("/api/v1/eras/NOT_A_SLUG/periods", "ERA_NOT_FOUND"),
        ("/api/v1/periods/unknown-period", "PERIOD_NOT_FOUND"),
        ("/api/v1/periods/x", "PERIOD_NOT_FOUND"),
    ] {
        let resp = get(&app, uri).await;
        assert_error(&resp, 404, code);
        assert_eq!(resp.header("cache-control"), Some("no-store"));
    }
    let resp = get(&app, "/api/v1/eras/unknown-era").await;
    assert!(resp.text().contains("'unknown-era'"), "{}", resp.text());
}
