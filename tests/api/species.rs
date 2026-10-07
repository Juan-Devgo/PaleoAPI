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

// ---------------------------------------------------------------- writes

use actix_web::http::Method;

/// Genus `g-genera`, periods `g-period` and `g-late`, continents `c-mod` (modern) and
/// `c-pre` (prehistoric), countries `k-usa` and `k-can`.
async fn seed_refs(pool: &PgPool) {
    seed_base(pool, "g").await;
    for sql in [
        "INSERT INTO periods VALUES ('g-late', 'g-era', 'Late', 145, 66)",
        "INSERT INTO continents VALUES ('c-mod', 'North America', 'modern'), ('c-pre', 'Laramidia', 'prehistoric')",
        "WITH c AS (INSERT INTO countries VALUES ('k-usa', 'United States'), ('k-can', 'Canada') RETURNING id) \
         INSERT INTO country_continents (country_id, continent_id) SELECT id, 'c-mod' FROM c",
    ] {
        exec(pool, sql)
            .await
            .unwrap_or_else(|e| panic!("{sql}: {e}"));
    }
}

fn trex() -> Value {
    json!({
        "id": "t-rex",
        "name": "Tyrannosaurus",
        "scientific_name": "Tyrannosaurus rex",
        "diet": "carnivore",
        "description": "Large theropod.",
        "genus_id": "g-genera",
        "period_ids": ["g-late"],
        "continent_ids": ["c-pre", "c-mod"],
        "country_ids": ["k-usa", "k-can"],
        "size": { "length_m": { "min": 11.0, "max": 12.3 }, "weight_kg": { "min": 5000, "max": 8000 } },
        "discovery_year": 1905,
        "image_url": "HTTPS://example.org/trex.png",
    })
}

fn with(mut body: Value, key: &str, value: Value) -> Value {
    body[key] = value;
    body
}

fn without(mut body: Value, key: &str) -> Value {
    body.as_object_mut().unwrap().remove(key);
    body
}

#[sqlx::test]
async fn create_full_card(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app(pool_opts, opts).await;
    seed_refs(&app.pool).await;
    let resp = send(&app, Method::POST, "/api/v1/species", trex()).await;
    assert_eq!(resp.status.as_u16(), 201, "{}", resp.text());
    assert_eq!(resp.header("location"), Some("/api/v1/species/t-rex"));
    let card = resp.json()["data"].clone();
    assert_eq!(
        card["size"],
        json!({ "length_m": { "min": 11, "max": 12.3 }, "height_m": null, "weight_kg": { "min": 5000, "max": 8000 } })
    );
    assert_eq!(card["image_url"], "https://example.org/trex.png");
    assert_eq!(card["discovery_year"], 1905);
    assert_eq!(card["taxonomy"]["genus"]["id"], "g-genera");
    assert_eq!(card["periods"][0]["era"]["id"], "g-era");
    assert_eq!(
        card["continents"],
        json!([
            { "id": "c-pre", "name": "Laramidia", "type": "prehistoric" },
            { "id": "c-mod", "name": "North America", "type": "modern" },
        ])
    );
    assert_eq!(
        card["countries"],
        json!([{ "id": "k-can", "name": "Canada" }, { "id": "k-usa", "name": "United States" }])
    );
    assert_eq!(resp.json(), get(&app, "/api/v1/species/t-rex").await.json());

    // optional fields default to empty / null
    let minimal = json!({
        "id": "minimal", "name": "Tyrannosaurus", "scientific_name": "Tyrannosaurus minimus",
        "diet": "omnivore", "description": "x", "genus_id": "g-genera", "period_ids": ["g-period"],
        "continent_ids": null, "size": null, "image_url": null, "discovery_year": null,
    });
    let resp = send(&app, Method::POST, "/api/v1/species", minimal).await;
    assert_eq!(resp.status.as_u16(), 201, "{}", resp.text());
    let card = resp.json()["data"].clone();
    assert_eq!(card["continents"], json!([]));
    assert_eq!(card["countries"], json!([]));
    assert_eq!(card["size"], Value::Null);

    // same common name is fine, same scientific name (any casing) is not
    let resp = send(
        &app,
        Method::POST,
        "/api/v1/species",
        with(
            with(trex(), "id", json!("t-rex-2")),
            "scientific_name",
            json!("TYRANNOSAURUS REX"),
        ),
    )
    .await;
    assert_error(&resp, 409, "SPECIES_ALREADY_EXISTS");
    let resp = send(&app, Method::POST, "/api/v1/species", trex()).await;
    assert_error(&resp, 409, "SPECIES_ALREADY_EXISTS");
}

#[sqlx::test]
async fn create_validation(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app(pool_opts, opts).await;
    seed_refs(&app.pool).await;
    let year = utc_year(&app.pool).await;
    let many = |prefix: &str, n: usize| {
        json!(
            (0..n)
                .map(|i| format!("{prefix}{i:02}"))
                .collect::<Vec<_>>()
        )
    };
    for (body, fields) in [
        (without(trex(), "genus_id"), vec!["genus_id"]),
        (without(trex(), "period_ids"), vec!["period_ids"]),
        (with(trex(), "period_ids", json!([])), vec!["period_ids"]),
        (with(trex(), "period_ids", json!(null)), vec!["period_ids"]),
        (
            with(trex(), "genus_id", json!("nope-genus")),
            vec!["genus_id"],
        ),
        (
            with(trex(), "period_ids", json!(["g-late", "nope-period"])),
            vec!["period_ids"],
        ),
        (
            with(trex(), "continent_ids", json!(["nope-land"])),
            vec!["continent_ids"],
        ),
        (
            with(trex(), "country_ids", json!(["nope-country"])),
            vec!["country_ids"],
        ),
        (
            with(trex(), "country_ids", json!(["k-usa", "k-usa"])),
            vec!["country_ids"],
        ),
        (with(trex(), "diet", json!("banana")), vec!["diet"]),
        (
            with(trex(), "description", json!("x".repeat(1001))),
            vec!["description"],
        ),
        (
            with(trex(), "scientific_name", json!("x".repeat(129))),
            vec!["scientific_name"],
        ),
        (with(trex(), "name", json!("   ")), vec!["name"]),
        (
            with(trex(), "discovery_year", json!(year + 1)),
            vec!["discovery_year"],
        ),
        (
            with(trex(), "discovery_year", json!(1599)),
            vec!["discovery_year"],
        ),
        (
            with(trex(), "image_url", json!("ftp://example.org/x.png")),
            vec!["image_url"],
        ),
        (with(trex(), "size", json!({})), vec!["size"]),
        (
            with(
                trex(),
                "size",
                json!({ "length_m": { "min": 12, "max": 11 } }),
            ),
            vec!["size.length_m"],
        ),
        (
            with(
                trex(),
                "size",
                json!({ "length_m": { "min": 0, "max": 11 } }),
            ),
            vec!["size.length_m.min"],
        ),
        (
            with(
                trex(),
                "size",
                json!({ "weight_kg": { "min": 1, "max": 1.0000001 } }),
            ),
            vec!["size.weight_kg.max"],
        ),
        (
            with(
                trex(),
                "size",
                json!({ "wingspan_m": { "min": 1, "max": 2 } }),
            ),
            vec!["size.wingspan_m"],
        ),
        (
            with(trex(), "period_ids", many("p", 21)),
            vec!["period_ids"],
        ),
        (
            with(trex(), "continent_ids", many("c", 21)),
            vec!["continent_ids"],
        ),
        (
            with(trex(), "country_ids", many("k", 51)),
            vec!["country_ids"],
        ),
        (with(trex(), "taxonomy", json!({})), vec!["taxonomy"]),
        (
            json!({ "id": "x1", "genus_id": "nope", "period_ids": ["nope"], "diet": "x" }),
            vec![
                "name",
                "scientific_name",
                "description",
                "genus_id",
                "period_ids",
                "diet",
            ],
        ),
    ] {
        let resp = send(&app, Method::POST, "/api/v1/species", body.clone()).await;
        assert_details(&resp, &fields);
    }
    let resp = send(
        &app,
        Method::POST,
        "/api/v1/species",
        with(trex(), "discovery_year", json!(year)),
    )
    .await;
    assert_eq!(resp.status.as_u16(), 201, "{}", resp.text());
    assert_eq!(ids(&get(&app, "/api/v1/species").await), ["t-rex"]);
}

#[sqlx::test]
async fn patch_replaces_lists_and_size(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app(pool_opts, opts).await;
    seed_refs(&app.pool).await;
    assert_eq!(
        send(&app, Method::POST, "/api/v1/species", trex())
            .await
            .status
            .as_u16(),
        201
    );
    let uri = "/api/v1/species/t-rex";
    let patch = |body: Value| send(&app, Method::PATCH, uri, body);

    let resp = patch(json!({ "period_ids": ["g-period"] })).await;
    assert_eq!(resp.status.as_u16(), 200, "{}", resp.text());
    let periods: Vec<_> = resp.json()["data"]["periods"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["id"].clone())
        .collect();
    assert_eq!(periods, [json!("g-period")]);

    let resp =
        patch(json!({ "size": { "length_m": { "min": 10, "max": 13 }, "height_m": null } })).await;
    assert_eq!(
        resp.json()["data"]["size"],
        json!({ "length_m": { "min": 10, "max": 13 }, "height_m": null, "weight_kg": null })
    );
    let resp = patch(json!({ "size": null })).await;
    assert_eq!(resp.json()["data"]["size"], Value::Null);

    let resp = patch(json!({ "continent_ids": null, "country_ids": null, "discovery_year": null, "image_url": null })).await;
    assert_eq!(resp.status.as_u16(), 200, "{}", resp.text());
    let card = resp.json()["data"].clone();
    assert_eq!(card["continents"], json!([]));
    assert_eq!(card["countries"], json!([]));
    assert_eq!(card["discovery_year"], Value::Null);
    assert_eq!(card["image_url"], Value::Null);
    assert_eq!(card["name"], "Tyrannosaurus");

    let resp = patch(json!({ "country_ids": ["k-usa"], "name": " Rex " })).await;
    assert_eq!(
        resp.json()["data"]["countries"],
        json!([{ "id": "k-usa", "name": "United States" }])
    );
    assert_eq!(resp.json()["data"]["name"], "Rex");
    assert_eq!(resp.json(), get(&app, uri).await.json());

    let before = get(&app, uri).await.json();
    for (body, fields) in [
        (json!({ "period_ids": [] }), vec!["period_ids"]),
        (json!({ "period_ids": null }), vec!["period_ids"]),
        (json!({ "id": "other" }), vec!["id"]),
        (json!({}), vec![""]),
        (json!({ "size": {} }), vec!["size"]),
        (json!({ "genus_id": "nope" }), vec!["genus_id"]),
        (json!({ "scientific_name": null }), vec!["scientific_name"]),
        (
            json!({ "diet": "banana", "country_ids": ["nope"] }),
            vec!["diet", "country_ids"],
        ),
    ] {
        assert_details(&patch(body).await, &fields);
    }
    assert_eq!(get(&app, uri).await.json(), before);
    assert_error(
        &send(
            &app,
            Method::PATCH,
            "/api/v1/species/nope",
            json!({ "name": "X" }),
        )
        .await,
        404,
        "SPECIES_NOT_FOUND",
    );
}

#[sqlx::test]
async fn delete_is_always_allowed(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app(pool_opts, opts).await;
    seed_refs(&app.pool).await;
    assert_eq!(
        send(&app, Method::POST, "/api/v1/species", trex())
            .await
            .status
            .as_u16(),
        201
    );
    let resp = delete(&app, "/api/v1/species/t-rex").await;
    assert_eq!(resp.status.as_u16(), 204, "{}", resp.text());
    assert_error(
        &get(&app, "/api/v1/species/t-rex").await,
        404,
        "SPECIES_NOT_FOUND",
    );
    assert_error(
        &delete(&app, "/api/v1/species/t-rex").await,
        404,
        "SPECIES_NOT_FOUND",
    );
    // links are gone, so the references can be deleted now
    assert_eq!(
        delete(&app, "/api/v1/countries/k-usa")
            .await
            .status
            .as_u16(),
        204
    );
    assert_eq!(
        delete(&app, "/api/v1/periods/g-late").await.status.as_u16(),
        204
    );
}
