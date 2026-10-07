//! Species (spec §5.6, AC 6.5): card reads and writes.

use serde_json::{Value, json};
use sqlx::PgPool;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

use crate::support::*;

// ---------------------------------------------------------------- reads

/// `full_chain("t")` plus extra links that exercise every card order (spec §5.6).
async fn seed_card(pool: &PgPool) -> Chain {
    let c = full_chain(pool, "t").await;
    for sql in [
        // periods: oldest first
        "INSERT INTO periods VALUES ('t-older', 't-era', 'Older', 251.902, 201.4)",
        "INSERT INTO periods VALUES ('t-young', 't-era', 'Young', 145, 66)",
        "INSERT INTO species_periods VALUES ('t-species', 't-young'), ('t-species', 't-older')",
        // continents: prehistoric first, then by name
        "INSERT INTO continents VALUES ('aa-gondwana', 'Gondwana', 'prehistoric')",
        "INSERT INTO continents VALUES ('zz-africa', 'Africa', 'modern')",
        "INSERT INTO species_continents VALUES ('t-species', 'zz-africa'), ('t-species', 'aa-gondwana')",
        // countries: by name, ignoring case
        "WITH c AS (INSERT INTO countries VALUES ('b-country', 'Zambia') RETURNING id) \
         INSERT INTO country_continents (country_id, continent_id) SELECT id, 'zz-africa' FROM c",
        "WITH c AS (INSERT INTO countries VALUES ('c-country', 'albania') RETURNING id) \
         INSERT INTO country_continents (country_id, continent_id) SELECT id, 'zz-africa' FROM c",
        "INSERT INTO species_countries VALUES ('t-species', 'b-country'), ('t-species', 'c-country')",
        // partial size, year, image
        "UPDATE species SET length_min_m = 11.000, length_max_m = 12.300, discovery_year = 1905, \
         image_url = 'https://example.org/t.png' WHERE id = 't-species'",
    ] {
        exec(pool, sql)
            .await
            .unwrap_or_else(|e| panic!("{sql}: {e}"));
    }
    c
}

/// A JSON number from its exact text (`json!` formats small floats with an exponent).
fn num(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

fn summary(id: &str) -> Value {
    json!({ "id": id, "name": id })
}

fn expected_card() -> Value {
    json!({
        "id": "t-species",
        "name": "t-species",
        "scientific_name": "t-species",
        "diet": "carnivore",
        "description": "A test species.",
        "discovery_year": 1905,
        "image_url": "https://example.org/t.png",
        "size": { "length_m": { "min": 11, "max": 12.3 }, "height_m": null, "weight_kg": null },
        "taxonomy": {
            "domain": summary("t-domains"),
            "kingdom": summary("t-kingdoms"),
            "phylum": summary("t-phyla"),
            "class": summary("t-classes"),
            "order": summary("t-orders"),
            "family": summary("t-families"),
            "genus": summary("t-genera"),
        },
        "periods": [
            { "id": "t-older", "name": "Older", "era": summary("t-era") },
            { "id": "t-period", "name": "t-period", "era": summary("t-era") },
            { "id": "t-young", "name": "Young", "era": summary("t-era") },
        ],
        "continents": [
            { "id": "aa-gondwana", "name": "Gondwana", "type": "prehistoric" },
            { "id": "t-pangaea", "name": "t-pangaea", "type": "prehistoric" },
            { "id": "zz-africa", "name": "Africa", "type": "modern" },
            { "id": "t-modern", "name": "t-modern", "type": "modern" },
        ],
        "countries": [
            { "id": "c-country", "name": "albania" },
            { "id": "t-country", "name": "t-country" },
            { "id": "b-country", "name": "Zambia" },
        ],
    })
}

#[sqlx::test]
async fn detail_is_the_full_card(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app(pool_opts, opts).await;
    seed_card(&app.pool).await;
    let resp = get(&app, "/api/v1/species/t-species").await;
    assert_eq!(resp.status.as_u16(), 200, "{}", resp.text());
    assert_eq!(resp.json(), json!({ "data": expected_card() }));
}

#[sqlx::test]
async fn list_items_are_full_cards(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app(pool_opts, opts).await;
    seed_card(&app.pool).await;
    let resp = get(&app, "/api/v1/species").await;
    assert_eq!(resp.status.as_u16(), 200, "{}", resp.text());
    let body = resp.json();
    assert_eq!(body["data"], json!([expected_card()]));
    assert_eq!(
        body["pagination"],
        json!({ "page": 1, "limit": 20, "total_items": 1, "total_pages": 1 })
    );
}

#[sqlx::test]
async fn bare_species_has_null_size_and_empty_lists(
    pool_opts: PgPoolOptions,
    opts: PgConnectOptions,
) {
    let app = app(pool_opts, opts).await;
    let (genus, period) = seed_base(&app.pool, "b").await;
    species_row(&app.pool, "bare", &genus, &period, "Bare", "Bare one", None).await;
    let card = get(&app, "/api/v1/species/bare").await.json()["data"].clone();
    assert_eq!(card["size"], Value::Null);
    assert_eq!(card["discovery_year"], Value::Null);
    assert_eq!(card["image_url"], Value::Null);
    assert_eq!(card["continents"], json!([]));
    assert_eq!(card["countries"], json!([]));
    assert_eq!(card["periods"].as_array().unwrap().len(), 1);
}

#[sqlx::test]
async fn full_size_is_three_ranges(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app(pool_opts, opts).await;
    let (genus, period) = seed_base(&app.pool, "s").await;
    species_row(
        &app.pool,
        "sized",
        &genus,
        &period,
        "Sized",
        "Sized one",
        None,
    )
    .await;
    exec(
        &app.pool,
        "UPDATE species SET length_min_m = 1, length_max_m = 2.5, height_min_m = 0.000001, \
         height_max_m = 0.5, weight_min_kg = 5000, weight_max_kg = 8000 WHERE id = 'sized'",
    )
    .await
    .unwrap();
    let card = get(&app, "/api/v1/species/sized").await.json()["data"].clone();
    assert_eq!(
        card["size"],
        json!({
            "length_m": { "min": 1, "max": 2.5 },
            "height_m": { "min": num("0.000001"), "max": 0.5 },
            "weight_kg": { "min": 5000, "max": 8000 },
        })
    );
}

#[sqlx::test]
async fn unknown_and_malformed_ids_are_species_not_found(
    pool_opts: PgPoolOptions,
    opts: PgConnectOptions,
) {
    let app = app(pool_opts, opts).await;
    let resp = get(&app, "/api/v1/species/trex01").await;
    assert_error(&resp, 404, "SPECIES_NOT_FOUND");
    assert!(resp.text().contains("'trex01'"), "{}", resp.text());
    for bad in ["NOT_A_SLUG", "a", "-ab", "a--b"] {
        let resp = get(&app, &format!("/api/v1/species/{bad}")).await;
        assert_error(&resp, 404, "SPECIES_NOT_FOUND");
    }
}
