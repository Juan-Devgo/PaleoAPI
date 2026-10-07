//! `GET /species` filters, search, and year sort (spec §4.8, §5.6; AC 6.5.3, 6.5.7–6.5.9).

use sqlx::PgPool;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

use crate::support::*;

/// Two eras, three periods, two taxonomy chains (chain `a` has two genera), a modern
/// and a prehistoric continent, two countries, and five species.
async fn seed(pool: &PgPool) {
    genus_chain(pool, "a").await;
    genus_chain(pool, "b").await;
    for sql in [
        "INSERT INTO genera VALUES ('a-genera2', 'A second genus', 'a-families')",
        "INSERT INTO eras VALUES ('e1', 'Mesozoic', 251.902, 66), ('e2', 'Cenozoic', 66, 0)",
        "INSERT INTO periods VALUES ('p1', 'e1', 'Jurassic', 201.4, 145), \
         ('p2', 'e1', 'Cretaceous', 145, 66), ('p3', 'e2', 'Paleogene', 66, 23.03)",
        "INSERT INTO continents VALUES ('c-mod', 'Modern land', 'modern'), \
         ('c-pre', 'Old land', 'prehistoric')",
        "WITH c AS (INSERT INTO countries VALUES ('k-usa', 'United States'), ('k-can', 'Canada') \
         RETURNING id) INSERT INTO country_continents (country_id, continent_id) SELECT id, 'c-mod' FROM c",
    ] {
        exec(pool, sql)
            .await
            .unwrap_or_else(|e| panic!("{sql}: {e}"));
    }
    let mut tx = pool.begin().await.unwrap();
    for sql in [
        "INSERT INTO species (id, genus_id, name, scientific_name, diet, description, discovery_year) VALUES \
         ('s1', 'a-genera', 'Tyrannosaurus', 'Tyrannosaurus rex', 'carnivore', 'x', 1905), \
         ('s2', 'a-genera2', 'Triceratops', 'Triceratops horridus', 'herbivore', 'x', 1889), \
         ('s3', 'b-genera', 'Odd %%_ name', 'Weirdus maximus', 'omnivore', 'x', NULL), \
         ('s4', 'b-genera', 'Rexy', 'Pseudo percent', 'carnivore', 'x', NULL), \
         ('s5', 'b-genera', 'Percent 100% a_b', 'Percentus', 'piscivore', 'x', 2000)",
        "INSERT INTO species_periods VALUES ('s1', 'p1'), ('s2', 'p2'), ('s3', 'p3'), \
         ('s4', 'p1'), ('s4', 'p3'), ('s5', 'p3')",
        "INSERT INTO species_continents VALUES ('s1', 'c-pre'), ('s2', 'c-mod'), \
         ('s3', 'c-mod'), ('s3', 'c-pre')",
        "INSERT INTO species_countries VALUES ('s1', 'k-usa'), ('s2', 'k-can'), ('s4', 'k-usa')",
    ] {
        exec(&mut *tx, sql)
            .await
            .unwrap_or_else(|e| panic!("{sql}: {e}"));
    }
    tx.commit().await.unwrap();
}

async fn list(app: &TestApp, query: &str) -> Vec<String> {
    ids(&get(app, &format!("/api/v1/species?{query}")).await)
}

#[sqlx::test]
async fn each_filter_alone(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app(pool_opts, opts).await;
    seed(&app.pool).await;
    // default order is by name: Odd(s3), Percent(s5), Rexy(s4), Triceratops(s2), Tyrannosaurus(s1)
    assert_eq!(list(&app, "").await, ["s3", "s5", "s4", "s2", "s1"]);
    for (query, want) in [
        ("diet=carnivore", vec!["s4", "s1"]),
        ("diet=herbivore", vec!["s2"]),
        ("diet=insectivore", vec![]),
        ("era=e1", vec!["s4", "s2", "s1"]),
        ("era=e2", vec!["s3", "s5", "s4"]),
        ("period=p1", vec!["s4", "s1"]),
        ("period=p2", vec!["s2"]),
        ("continent=c-pre", vec!["s3", "s1"]),
        ("continent=c-mod", vec!["s3", "s2"]),
        ("country=k-usa", vec!["s4", "s1"]),
        ("country=k-can", vec!["s2"]),
        ("q=REX", vec!["s4", "s1"]),
        ("q=%25%25_", vec!["s3"]),
        ("q=100%25%20a_b", vec!["s5"]),
        ("q=%20%20percent%20", vec!["s5", "s4"]),
    ] {
        assert_eq!(list(&app, query).await, want, "{query}");
    }
}

#[sqlx::test]
async fn each_rank_filter(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app(pool_opts, opts).await;
    seed(&app.pool).await;
    for (param, table) in [
        ("domain", "domains"),
        ("kingdom", "kingdoms"),
        ("phylum", "phyla"),
        ("class", "classes"),
        ("order", "orders"),
        ("family", "families"),
        ("genus", "genera"),
    ] {
        let a = list(&app, &format!("{param}=a-{table}")).await;
        let b = list(&app, &format!("{param}=b-{table}")).await;
        if param == "genus" {
            assert_eq!(a, ["s1"], "{param}");
        } else {
            assert_eq!(a, ["s2", "s1"], "{param}");
        }
        assert_eq!(b, ["s3", "s5", "s4"], "{param}");
    }
    assert_eq!(list(&app, "genus=a-genera2").await, ["s2"]);
}

#[sqlx::test]
async fn filters_combine_with_and(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app(pool_opts, opts).await;
    seed(&app.pool).await;
    for (query, want) in [
        ("diet=carnivore&era=e2", vec!["s4"]),
        ("continent=c-mod&genus=b-genera", vec!["s3"]),
        ("country=k-usa&diet=herbivore", vec![]),
        ("era=e1&period=p2&kingdom=a-kingdoms", vec!["s2"]),
        ("q=rex&diet=carnivore&country=k-usa", vec!["s4", "s1"]),
        ("q=rex&continent=c-pre", vec!["s1"]),
        (
            "domain=b-domains&sort=-discovery_year",
            vec!["s5", "s3", "s4"],
        ),
    ] {
        assert_eq!(list(&app, query).await, want, "{query}");
    }
    let resp = get(&app, "/api/v1/species?diet=carnivore&limit=1&page=2").await;
    assert_eq!(ids(&resp), ["s1"]);
    assert_eq!(
        pagination(&resp),
        serde_json::json!({ "page": 2, "limit": 1, "total_items": 2, "total_pages": 2 })
    );
}

#[sqlx::test]
async fn discovery_year_sorts_nulls_last_both_ways(
    pool_opts: PgPoolOptions,
    opts: PgConnectOptions,
) {
    let app = app(pool_opts, opts).await;
    seed(&app.pool).await;
    assert_eq!(
        list(&app, "sort=discovery_year").await,
        ["s2", "s1", "s5", "s3", "s4"]
    );
    assert_eq!(
        list(&app, "sort=-discovery_year").await,
        ["s5", "s1", "s2", "s3", "s4"]
    );
}

#[sqlx::test]
async fn unknown_well_formed_ids_give_empty_lists(
    pool_opts: PgPoolOptions,
    opts: PgConnectOptions,
) {
    let app = app(pool_opts, opts).await;
    seed(&app.pool).await;
    for param in [
        "era",
        "period",
        "domain",
        "kingdom",
        "phylum",
        "class",
        "order",
        "family",
        "genus",
        "continent",
        "country",
    ] {
        let resp = get(&app, &format!("/api/v1/species?{param}=unknown-id")).await;
        assert!(ids(&resp).is_empty(), "{param}");
        assert_eq!(pagination(&resp)["total_items"], 0, "{param}");
    }
}

#[sqlx::test]
async fn invalid_filter_values_are_400(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app(pool_opts, opts).await;
    for query in [
        "diet=banana",
        "diet=",
        "diet=Carnivore",
        "era=",
        "era=Not_A_Slug",
        "genus=-x",
        "country=a--b",
        "q=re",
        "q=%20re%20",
        "q=",
        "diet=carnivore&diet=herbivore",
    ] {
        let resp = get(&app, &format!("/api/v1/species?{query}")).await;
        assert_error(&resp, 400, "INVALID_QUERY_PARAMETER");
    }
}
