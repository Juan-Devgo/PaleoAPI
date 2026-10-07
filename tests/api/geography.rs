//! Continents and countries (spec §5.4, §5.5; AC 6.4).

use serde_json::{Value, json};
use sqlx::PgPool;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

use crate::support::*;

// ---------------------------------------------------------------- reads

async fn seed_world(pool: &PgPool) {
    for sql in [
        "INSERT INTO continents VALUES ('africa', 'Africa', 'modern'), ('asia', 'Asia', 'modern'), \
         ('europe', 'Europe', 'modern'), ('pangaea', 'Pangaea', 'prehistoric'), \
         ('laurasia', 'Laurasia', 'prehistoric')",
    ] {
        exec(pool, sql).await.unwrap();
    }
    country_with_continent(pool, "egypt", "asia").await.unwrap();
    country_with_continent(pool, "france", "europe")
        .await
        .unwrap();
    country_with_continent(pool, "japan", "asia").await.unwrap();
    country_with_continent(pool, "kenya", "africa")
        .await
        .unwrap();
    exec(
        pool,
        "INSERT INTO country_continents (country_id, continent_id) VALUES ('egypt', 'africa')",
    )
    .await
    .unwrap();
    exec(
        pool,
        "UPDATE countries SET name = initcap(id) WHERE id IN ('egypt', 'france', 'japan', 'kenya')",
    )
    .await
    .unwrap();
}

fn continent(id: &str, name: &str, kind: &str) -> Value {
    json!({ "id": id, "name": name, "type": kind })
}

fn africa() -> Value {
    continent("africa", "Africa", "modern")
}
fn asia() -> Value {
    continent("asia", "Asia", "modern")
}

#[sqlx::test]
async fn continent_lists_filters_and_detail(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app(pool_opts, opts).await;
    seed_world(&app.pool).await;
    for (query, want) in [
        ("", vec!["africa", "asia", "europe", "laurasia", "pangaea"]),
        (
            "?sort=-name",
            vec!["pangaea", "laurasia", "europe", "asia", "africa"],
        ),
        ("?type=prehistoric", vec!["laurasia", "pangaea"]),
        ("?type=modern", vec!["africa", "asia", "europe"]),
        ("?type=modern&sort=-name&limit=2", vec!["europe", "asia"]),
    ] {
        assert_eq!(
            ids(&get(&app, &format!("/api/v1/continents{query}")).await),
            want,
            "{query}"
        );
    }
    for query in ["type=banana", "type=", "type=Modern", "sort=type"] {
        assert_error(
            &get(&app, &format!("/api/v1/continents?{query}")).await,
            400,
            "INVALID_QUERY_PARAMETER",
        );
    }
    let resp = get(&app, "/api/v1/continents/pangaea").await;
    assert_eq!(
        resp.json(),
        json!({ "data": continent("pangaea", "Pangaea", "prehistoric") })
    );
    let first = get(&app, "/api/v1/continents").await.json()["data"][0].clone();
    assert_eq!(first, africa());
}

#[sqlx::test]
async fn countries_of_a_continent(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app(pool_opts, opts).await;
    seed_world(&app.pool).await;
    for (uri, want) in [
        (
            "/api/v1/continents/africa/countries",
            vec!["egypt", "kenya"],
        ),
        ("/api/v1/continents/asia/countries", vec!["egypt", "japan"]),
        (
            "/api/v1/continents/asia/countries?sort=-name",
            vec!["japan", "egypt"],
        ),
        ("/api/v1/continents/pangaea/countries", vec![]),
        ("/api/v1/countries?continent=asia", vec!["egypt", "japan"]),
        ("/api/v1/countries?continent=unknown-land", vec![]),
        (
            "/api/v1/countries",
            vec!["egypt", "france", "japan", "kenya"],
        ),
        ("/api/v1/countries?sort=-name&limit=1", vec!["kenya"]),
    ] {
        assert_eq!(ids(&get(&app, uri).await), want, "{uri}");
    }
    let resp = get(&app, "/api/v1/continents/asia/countries?limit=1").await;
    assert_eq!(
        pagination(&resp),
        json!({ "page": 1, "limit": 1, "total_items": 2, "total_pages": 2 })
    );
    for uri in [
        "/api/v1/continents/unknown-land/countries",
        "/api/v1/continents/NOT_A_SLUG/countries",
        "/api/v1/continents/unknown-land",
        "/api/v1/continents/x",
    ] {
        assert_error(&get(&app, uri).await, 404, "CONTINENT_NOT_FOUND");
    }
    for uri in ["/api/v1/countries/unknown-country", "/api/v1/countries/Bad"] {
        assert_error(&get(&app, uri).await, 404, "COUNTRY_NOT_FOUND");
    }
    assert_error(
        &get(&app, "/api/v1/countries?continent=Bad_Id").await,
        400,
        "INVALID_QUERY_PARAMETER",
    );
}

#[sqlx::test]
async fn country_continents_are_sorted_by_name(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app(pool_opts, opts).await;
    seed_world(&app.pool).await;
    let want = json!({ "id": "egypt", "name": "Egypt", "continents": [africa(), asia()] });
    let resp = get(&app, "/api/v1/countries/egypt").await;
    assert_eq!(resp.status.as_u16(), 200, "{}", resp.text());
    assert_eq!(resp.json(), json!({ "data": want }));
    assert_eq!(resp.header("cache-control"), Some("public, max-age=300"));
    for uri in [
        "/api/v1/countries",
        "/api/v1/countries?continent=africa",
        "/api/v1/continents/asia/countries",
    ] {
        let data = get(&app, uri).await.json()["data"].clone();
        assert_eq!(data[0], want, "{uri}");
    }
    let japan = get(&app, "/api/v1/countries/japan").await.json()["data"].clone();
    assert_eq!(japan["continents"], json!([asia()]));
}

// ---------------------------------------------------------------- writes

use actix_web::http::Method;

#[sqlx::test]
async fn continent_create_patch_delete(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app(pool_opts, opts).await;
    seed_world(&app.pool).await;
    for (body, fields) in [
        (
            json!({ "id": "x1", "name": "X", "type": "ancient" }),
            vec!["type"],
        ),
        (json!({ "id": "x1", "name": "X" }), vec!["type"]),
        (
            json!({ "id": "x1", "name": "X", "type": null }),
            vec!["type"],
        ),
        (
            json!({ "id": "x1", "name": "X", "type": "Modern" }),
            vec!["type"],
        ),
    ] {
        assert_details(
            &send(&app, Method::POST, "/api/v1/continents", body).await,
            &fields,
        );
    }
    let resp = send(
        &app,
        Method::POST,
        "/api/v1/continents",
        json!({ "id": "gondwana", "name": "Gondwana", "type": "prehistoric" }),
    )
    .await;
    assert_eq!(resp.status.as_u16(), 201, "{}", resp.text());
    assert_eq!(resp.header("location"), Some("/api/v1/continents/gondwana"));
    assert_eq!(
        resp.json(),
        get(&app, "/api/v1/continents/gondwana").await.json()
    );
    assert_error(
        &send(
            &app,
            Method::POST,
            "/api/v1/continents",
            json!({ "id": "x2", "name": "africa", "type": "modern" }),
        )
        .await,
        409,
        "CONTINENT_ALREADY_EXISTS",
    );

    // modern → prehistoric is blocked while countries are linked (AC 6.4.3)
    let resp = send(
        &app,
        Method::PATCH,
        "/api/v1/continents/asia",
        json!({ "type": "prehistoric" }),
    )
    .await;
    assert_details(&resp, &["type"]);
    assert!(
        resp.text().contains("egypt") && resp.text().contains("japan"),
        "{}",
        resp.text()
    );
    assert_eq!(
        get(&app, "/api/v1/continents/asia").await.json()["data"]["type"],
        "modern"
    );
    let resp = send(
        &app,
        Method::PATCH,
        "/api/v1/continents/gondwana",
        json!({ "type": "modern", "name": "Gondwanaland" }),
    )
    .await;
    assert_eq!(resp.status.as_u16(), 200, "{}", resp.text());
    assert_eq!(
        resp.json(),
        json!({ "data": { "id": "gondwana", "name": "Gondwanaland", "type": "modern" } })
    );
    let resp = send(
        &app,
        Method::PATCH,
        "/api/v1/continents/gondwana",
        json!({ "type": "prehistoric" }),
    )
    .await;
    assert_eq!(resp.status.as_u16(), 200, "{}", resp.text());

    // delete protection: countries and species (AC 6.4.4)
    let resp = delete(&app, "/api/v1/continents/asia").await;
    assert_error(&resp, 409, "CONTINENT_HAS_DEPENDENTS");
    assert!(resp.text().contains("2 countries"), "{}", resp.text());
    let (genus, period) = seed_base(&app.pool, "g").await;
    species_row(&app.pool, "rex", &genus, &period, "Rex", "Rex rex", None).await;
    exec(
        &app.pool,
        "INSERT INTO species_continents VALUES ('rex', 'pangaea')",
    )
    .await
    .unwrap();
    let resp = delete(&app, "/api/v1/continents/pangaea").await;
    assert_error(&resp, 409, "CONTINENT_HAS_DEPENDENTS");
    assert!(
        resp.text().contains("1 species") && resp.text().contains("rex"),
        "{}",
        resp.text()
    );
    assert_eq!(
        delete(&app, "/api/v1/continents/laurasia")
            .await
            .status
            .as_u16(),
        204
    );
}

#[sqlx::test]
async fn country_continent_rules(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app(pool_opts, opts).await;
    seed_world(&app.pool).await;
    let many: Vec<String> = (0..11).map(|i| format!("m{i:02}")).collect();
    let mut values = Vec::new();
    for id in &many {
        values.push(format!("('{id}', 'Modern {id}', 'modern')"));
    }
    exec(
        &app.pool,
        format!("INSERT INTO continents VALUES {}", values.join(", ")),
    )
    .await
    .unwrap();
    let country = |ids: Value| json!({ "id": "chad", "name": "Chad", "continent_ids": ids });
    for (body, fields) in [
        (country(json!([])), vec!["continent_ids"]),
        (country(json!(["pangaea"])), vec!["continent_ids"]),
        (
            country(json!(["africa", "laurasia"])),
            vec!["continent_ids"],
        ),
        (country(json!(["nope-land"])), vec!["continent_ids"]),
        (country(json!(["africa", "africa"])), vec!["continent_ids"]),
        (country(json!(many)), vec!["continent_ids"]),
        (country(json!(null)), vec!["continent_ids"]),
        (
            json!({ "id": "chad", "name": "Chad" }),
            vec!["continent_ids"],
        ),
        (
            json!({ "id": "chad", "name": "Chad", "continents": [] , "continent_ids": ["africa"] }),
            vec!["continents"],
        ),
    ] {
        assert_details(
            &send(&app, Method::POST, "/api/v1/countries", body).await,
            &fields,
        );
    }
    let resp = send(
        &app,
        Method::POST,
        "/api/v1/countries",
        country(json!(["pangaea"])),
    )
    .await;
    assert!(resp.text().contains("prehistoric"), "{}", resp.text());

    let resp = send(
        &app,
        Method::POST,
        "/api/v1/countries",
        country(json!(many[..10])),
    )
    .await;
    assert_eq!(resp.status.as_u16(), 201, "{}", resp.text());
    assert_eq!(resp.header("location"), Some("/api/v1/countries/chad"));
    assert_eq!(
        resp.json()["data"]["continents"].as_array().unwrap().len(),
        10
    );
    assert_eq!(
        resp.json(),
        get(&app, "/api/v1/countries/chad").await.json()
    );

    // PATCH replaces the whole list
    let resp = send(
        &app,
        Method::PATCH,
        "/api/v1/countries/chad",
        json!({ "continent_ids": ["asia", "africa"] }),
    )
    .await;
    assert_eq!(resp.status.as_u16(), 200, "{}", resp.text());
    assert_eq!(
        resp.json()["data"]["continents"],
        json!([
            { "id": "africa", "name": "Africa", "type": "modern" },
            { "id": "asia", "name": "Asia", "type": "modern" },
        ])
    );
    assert!(ids(&get(&app, "/api/v1/continents/m00/countries").await).is_empty());
    for body in [
        json!({ "continent_ids": [] }),
        json!({ "continent_ids": null }),
        json!({ "continent_ids": ["pangaea"] }),
    ] {
        assert_details(
            &send(&app, Method::PATCH, "/api/v1/countries/chad", body).await,
            &["continent_ids"],
        );
    }
    let resp = send(
        &app,
        Method::PATCH,
        "/api/v1/countries/chad",
        json!({ "name": "Tchad" }),
    )
    .await;
    assert_eq!(
        resp.json()["data"]["continents"].as_array().unwrap().len(),
        2
    );
    assert_error(
        &send(
            &app,
            Method::POST,
            "/api/v1/countries",
            json!({ "id": "chad2", "name": "TCHAD", "continent_ids": ["africa"] }),
        )
        .await,
        409,
        "COUNTRY_ALREADY_EXISTS",
    );
}

#[sqlx::test]
async fn country_delete(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app(pool_opts, opts).await;
    seed_world(&app.pool).await;
    let (genus, period) = seed_base(&app.pool, "g").await;
    species_row(&app.pool, "rex", &genus, &period, "Rex", "Rex rex", None).await;
    exec(
        &app.pool,
        "INSERT INTO species_countries VALUES ('rex', 'japan')",
    )
    .await
    .unwrap();
    let resp = delete(&app, "/api/v1/countries/japan").await;
    assert_error(&resp, 409, "COUNTRY_HAS_DEPENDENTS");
    assert!(
        resp.text().contains("1 species") && resp.text().contains("rex"),
        "{}",
        resp.text()
    );

    assert_eq!(
        delete(&app, "/api/v1/countries/egypt")
            .await
            .status
            .as_u16(),
        204
    );
    assert_eq!(
        ids(&get(&app, "/api/v1/continents/asia/countries").await),
        ["japan"]
    );
    assert_eq!(
        ids(&get(&app, "/api/v1/continents/africa/countries").await),
        ["kenya"]
    );
    assert_error(
        &get(&app, "/api/v1/countries/egypt").await,
        404,
        "COUNTRY_NOT_FOUND",
    );
}
