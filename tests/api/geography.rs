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
