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

// ---------------------------------------------------------------- writes

use actix_web::http::Method;

/// The read representation of `uri`, for "rejected writes change nothing".
async fn snapshot(app: &TestApp, uri: &str) -> Value {
    get(app, uri).await.json()
}

#[sqlx::test]
async fn era_ranges_bounds_and_overlaps(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app(pool_opts, opts).await;
    seed_timeline(&app.pool).await;
    let before = snapshot(&app, "/api/v1/eras?limit=100").await;
    let era = |id: &str, start: Value, end: Value| json!({ "id": id, "name": id, "start_mya": start, "end_mya": end });
    for (body, fields) in [
        (era("x-era", json!(10), json!(20)), vec!["end_mya"]),
        (era("x-era", json!(20), json!(20)), vec!["end_mya"]),
        (era("x-era", json!(4601), json!(4000)), vec!["start_mya"]),
        (
            era("x-era", json!(4600.001), json!(4000)),
            vec!["start_mya"],
        ),
        (era("x-era", json!(10), json!(-1)), vec!["end_mya"]),
        (era("x-era", json!(10.0001), json!(5)), vec!["start_mya"]),
        (era("x-era", json!("10"), json!(5)), vec!["start_mya"]),
        (era("x-era", json!(100), json!(50)), vec!["start_mya"]),
        (era("x-era", json!(600), json!(500)), vec!["start_mya"]),
    ] {
        let resp = send(&app, Method::POST, "/api/v1/eras", body.clone()).await;
        assert_details(&resp, &fields);
    }
    let resp = send(
        &app,
        Method::POST,
        "/api/v1/eras",
        era("x-era", json!(100), json!(50)),
    )
    .await;
    let msg = resp.text();
    assert!(
        msg.contains("cenozoic") && msg.contains("mesozoic"),
        "{msg}"
    );
    assert_eq!(snapshot(&app, "/api/v1/eras?limit=100").await, before);

    // touching boundaries and gaps are allowed; 3 places and trailing zeros are fine
    for body in [
        era("hadean", json!(4600), json!(4599.999)),
        era("archean", num("4000.000"), json!(2500)),
    ] {
        let resp = send(&app, Method::POST, "/api/v1/eras", body.clone()).await;
        assert_eq!(resp.status.as_u16(), 201, "{body}: {}", resp.text());
    }
    let archean = get(&app, "/api/v1/eras/archean").await.json()["data"].clone();
    assert!(
        get(&app, "/api/v1/eras/archean")
            .await
            .text()
            .contains("\"start_mya\":4000,")
    );
    assert_eq!(archean["end_mya"], 2500);
}

#[sqlx::test]
async fn era_patch_validates_the_merged_range(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app(pool_opts, opts).await;
    seed_timeline(&app.pool).await;
    exec(
        &app.pool,
        "INSERT INTO eras VALUES ('hadean', 'Hadean', 4600, 4000)",
    )
    .await
    .unwrap();
    let before = snapshot(&app, "/api/v1/eras?limit=100").await;
    for (uri, body, fields) in [
        // range: end_mya, or start_mya when only start_mya was sent
        (
            "/api/v1/eras/mesozoic",
            json!({ "end_mya": 300 }),
            vec!["end_mya"],
        ),
        (
            "/api/v1/eras/hadean",
            json!({ "start_mya": 3000 }),
            vec!["start_mya"],
        ),
        (
            "/api/v1/eras/hadean",
            json!({ "start_mya": 3000, "end_mya": 3500 }),
            vec!["end_mya"],
        ),
        // overlap: start_mya, or end_mya when only end_mya was sent
        (
            "/api/v1/eras/paleozoic",
            json!({ "end_mya": 200 }),
            vec!["end_mya"],
        ),
        (
            "/api/v1/eras/hadean",
            json!({ "end_mya": 500 }),
            vec!["end_mya"],
        ),
        (
            "/api/v1/eras/hadean",
            json!({ "start_mya": 4600, "end_mya": 500 }),
            vec!["start_mya"],
        ),
        // the era must still contain its periods, one detail per violated side
        (
            "/api/v1/eras/mesozoic",
            json!({ "start_mya": 240 }),
            vec!["start_mya"],
        ),
        (
            "/api/v1/eras/mesozoic",
            json!({ "end_mya": 100 }),
            vec!["end_mya"],
        ),
        (
            "/api/v1/eras/mesozoic",
            json!({ "start_mya": 240, "end_mya": 100 }),
            vec!["start_mya", "end_mya"],
        ),
    ] {
        let resp = send(&app, Method::PATCH, uri, body.clone()).await;
        assert_details(&resp, &fields);
    }
    let resp = send(
        &app,
        Method::PATCH,
        "/api/v1/eras/mesozoic",
        json!({ "start_mya": 240 }),
    )
    .await;
    assert!(resp.text().contains("triassic"), "{}", resp.text());
    assert_eq!(snapshot(&app, "/api/v1/eras?limit=100").await, before);

    let resp = send(
        &app,
        Method::PATCH,
        "/api/v1/eras/hadean",
        json!({ "end_mya": 3900 }),
    )
    .await;
    assert_eq!(resp.status.as_u16(), 200, "{}", resp.text());
    assert_eq!(
        resp.json(),
        json!({ "data": { "id": "hadean", "name": "Hadean", "start_mya": 4600, "end_mya": 3900 } })
    );
    assert_eq!(resp.json(), get(&app, "/api/v1/eras/hadean").await.json());
}

#[sqlx::test]
async fn nested_period_create(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app(pool_opts, opts).await;
    seed_timeline(&app.pool).await;
    exec(
        &app.pool,
        "INSERT INTO eras VALUES ('deep', 'Deep', 3000, 2000)",
    )
    .await
    .unwrap();
    let p = |id: &str, start: f64, end: f64| json!({ "id": id, "name": id, "start_mya": start, "end_mya": end });
    for (body, fields) in [
        (p("d1", 3100.0, 2900.0), vec!["start_mya"]),
        (p("d1", 2100.0, 1900.0), vec!["end_mya"]),
        (p("d1", 3100.0, 1900.0), vec!["start_mya", "end_mya"]),
        (p("d1", 2100.0, 2500.0), vec!["end_mya"]),
        (p("d1", 3000.0, 2000.0001), vec!["end_mya"]),
    ] {
        let resp = send(
            &app,
            Method::POST,
            "/api/v1/eras/deep/periods",
            body.clone(),
        )
        .await;
        assert_details(&resp, &fields);
    }
    let mut with_era = p("d1", 2900.0, 2100.0);
    with_era["era_id"] = json!("deep");
    let resp = send(&app, Method::POST, "/api/v1/eras/deep/periods", with_era).await;
    assert_details(&resp, &["era_id"]);
    let resp = send(
        &app,
        Method::POST,
        "/api/v1/eras/mesozoic/periods",
        p("late", 70.0, 66.0),
    )
    .await;
    assert_details(&resp, &["start_mya"]);
    assert!(resp.text().contains("cretaceous"), "{}", resp.text());

    let resp = send(
        &app,
        Method::POST,
        "/api/v1/eras/deep/periods",
        p("d1", 3000.0, 2000.0),
    )
    .await;
    assert_eq!(resp.status.as_u16(), 201, "{}", resp.text());
    assert_eq!(resp.header("location"), Some("/api/v1/periods/d1"));
    assert_eq!(
        resp.json()["data"]["era"],
        json!({ "id": "deep", "name": "Deep" })
    );
    assert_eq!(resp.json(), get(&app, "/api/v1/periods/d1").await.json());

    let resp = send(
        &app,
        Method::POST,
        "/api/v1/eras/cenozoic/periods",
        p("neogene", 23.03, 2.58),
    )
    .await;
    assert_eq!(resp.status.as_u16(), 201, "{}", resp.text());
    assert_eq!(resp.header("location"), Some("/api/v1/periods/neogene"));
    let resp = send(
        &app,
        Method::POST,
        "/api/v1/eras/cenozoic/periods",
        p("neogene", 2.58, 0.0),
    )
    .await;
    assert_error(&resp, 409, "PERIOD_ALREADY_EXISTS");
}

#[sqlx::test]
async fn period_patch_moves_and_merges(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app(pool_opts, opts).await;
    seed_timeline(&app.pool).await;
    for sql in [
        "INSERT INTO eras VALUES ('deep', 'Deep', 3000, 2000), ('other', 'Other', 1000, 500)",
        "INSERT INTO periods VALUES ('d1', 'deep', 'D1', 2900, 2100)",
    ] {
        exec(&app.pool, sql).await.unwrap();
    }
    let before = snapshot(&app, "/api/v1/periods?limit=100").await;
    for (uri, body, fields) in [
        (
            "/api/v1/periods/jurassic",
            json!({ "end_mya": 300 }),
            vec!["end_mya"],
        ),
        (
            "/api/v1/periods/jurassic",
            json!({ "start_mya": 140 }),
            vec!["start_mya"],
        ),
        (
            "/api/v1/periods/jurassic",
            json!({ "start_mya": 210 }),
            vec!["start_mya"],
        ),
        (
            "/api/v1/periods/jurassic",
            json!({ "end_mya": 140 }),
            vec!["end_mya"],
        ),
        (
            "/api/v1/periods/d1",
            json!({ "era_id": "other" }),
            vec!["era_id"],
        ),
        (
            "/api/v1/periods/d1",
            json!({ "era_id": "nope-era" }),
            vec!["era_id"],
        ),
        (
            "/api/v1/periods/d1",
            json!({ "era_id": "Bad" }),
            vec!["era_id"],
        ),
        (
            "/api/v1/periods/d1",
            json!({ "era_id": null }),
            vec!["era_id"],
        ),
        (
            "/api/v1/periods/d1",
            json!({ "start_mya": 3100 }),
            vec!["start_mya"],
        ),
        (
            "/api/v1/periods/d1",
            json!({ "era_id": "other", "start_mya": 900 }),
            vec!["start_mya"],
        ),
        (
            "/api/v1/periods/d1",
            json!({ "era_id": "other", "start_mya": 1100, "end_mya": 400 }),
            vec!["start_mya", "end_mya"],
        ),
    ] {
        let resp = send(&app, Method::PATCH, uri, body.clone()).await;
        assert_details(&resp, &fields);
    }
    assert_eq!(snapshot(&app, "/api/v1/periods?limit=100").await, before);

    let resp = send(
        &app,
        Method::PATCH,
        "/api/v1/periods/d1",
        json!({ "era_id": "other", "start_mya": 900, "end_mya": 600 }),
    )
    .await;
    assert_eq!(resp.status.as_u16(), 200, "{}", resp.text());
    assert_eq!(
        resp.json()["data"],
        json!({ "id": "d1", "name": "D1", "start_mya": 900, "end_mya": 600, "era": { "id": "other", "name": "Other" } })
    );
    assert_eq!(ids(&get(&app, "/api/v1/eras/other/periods").await), ["d1"]);
    assert!(ids(&get(&app, "/api/v1/eras/deep/periods").await).is_empty());
}

#[sqlx::test]
async fn delete_protection_for_eras_and_periods(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app(pool_opts, opts).await;
    seed_timeline(&app.pool).await;
    let genus = genus_chain(&app.pool, "g").await;
    species_row(
        &app.pool,
        "rex",
        &genus,
        "cretaceous",
        "Rex",
        "Rex rex",
        None,
    )
    .await;

    let resp = delete(&app, "/api/v1/eras/mesozoic").await;
    assert_error(&resp, 409, "ERA_HAS_DEPENDENTS");
    assert!(resp.text().contains("3 periods"), "{}", resp.text());
    let resp = delete(&app, "/api/v1/periods/cretaceous").await;
    assert_error(&resp, 409, "PERIOD_HAS_DEPENDENTS");
    assert!(
        resp.text().contains("1 species") && resp.text().contains("rex"),
        "{}",
        resp.text()
    );

    assert_eq!(
        delete(&app, "/api/v1/periods/paleogene")
            .await
            .status
            .as_u16(),
        204
    );
    assert_eq!(
        delete(&app, "/api/v1/eras/cenozoic").await.status.as_u16(),
        204
    );
    assert_error(
        &get(&app, "/api/v1/periods/paleogene").await,
        404,
        "PERIOD_NOT_FOUND",
    );
    assert_error(
        &delete(&app, "/api/v1/periods/paleogene").await,
        404,
        "PERIOD_NOT_FOUND",
    );
}
