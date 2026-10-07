//! Continents, countries, and their links (AC 3, AC 9, AC 10, AC 17).

use sqlx::PgPool;

use crate::support::{
    accepts, commit_accepts, commit_rejects, continent, count, country_with_continent, rejects,
};

async fn world(pool: &PgPool) {
    continent(pool, "europe", "modern").await.unwrap();
    continent(pool, "asia", "modern").await.unwrap();
    continent(pool, "pangaea", "prehistoric").await.unwrap();
}

#[sqlx::test]
async fn continent_type_is_lowercase_enum(pool: PgPool) {
    for t in ["Modern", "ancient", "MODERN", ""] {
        rejects(
            &pool,
            format!("INSERT INTO continents VALUES ('cc', 'C', '{t}')"),
            "23514",
            Some("continents_type_ck"),
        )
        .await;
    }
    accepts(
        &pool,
        "INSERT INTO continents VALUES ('cc', 'C', 'prehistoric')",
    )
    .await;
}

#[sqlx::test]
async fn countries_link_only_to_modern_continents(pool: PgPool) {
    world(&pool).await;
    country_with_continent(&pool, "spain", "europe")
        .await
        .unwrap();
    rejects(
        &pool,
        "INSERT INTO country_continents (country_id, continent_id) VALUES ('spain', 'pangaea')",
        "23503",
        Some("country_continents_continent_fk"),
    )
    .await;
    rejects(
        &pool,
        "INSERT INTO country_continents (country_id, continent_id, continent_type) \
         VALUES ('spain', 'pangaea', 'prehistoric')",
        "23514",
        Some("country_continents_continent_type_ck"),
    )
    .await;
    accepts(
        &pool,
        "INSERT INTO country_continents (country_id, continent_id) VALUES ('spain', 'asia')",
    )
    .await;
}

#[sqlx::test]
async fn linked_continent_cannot_become_prehistoric(pool: PgPool) {
    world(&pool).await;
    continent(&pool, "unlinked", "modern").await.unwrap();
    country_with_continent(&pool, "spain", "europe")
        .await
        .unwrap();
    rejects(
        &pool,
        "UPDATE continents SET type = 'prehistoric' WHERE id = 'europe'",
        "23001",
        Some("country_continents_continent_fk"),
    )
    .await;
    accepts(
        &pool,
        "UPDATE continents SET type = 'prehistoric' WHERE id = 'unlinked'",
    )
    .await;
    accepts(
        &pool,
        "UPDATE continents SET name = 'Europa' WHERE id = 'europe'",
    )
    .await;
}

#[sqlx::test]
async fn deleting_a_linked_continent_is_rejected(pool: PgPool) {
    world(&pool).await;
    country_with_continent(&pool, "spain", "europe")
        .await
        .unwrap();
    rejects(
        &pool,
        "DELETE FROM continents WHERE id = 'europe'",
        "23001",
        Some("country_continents_continent_fk"),
    )
    .await;
    accepts(&pool, "DELETE FROM continents WHERE id = 'asia'").await;
}

#[sqlx::test]
async fn deleting_a_country_removes_its_links(pool: PgPool) {
    world(&pool).await;
    country_with_continent(&pool, "turkey", "europe")
        .await
        .unwrap();
    accepts(
        &pool,
        "INSERT INTO country_continents (country_id, continent_id) VALUES ('turkey', 'asia')",
    )
    .await;
    accepts(&pool, "DELETE FROM countries WHERE id = 'turkey'").await;
    assert_eq!(
        count(&pool, "SELECT count(*) FROM country_continents").await,
        0
    );
}

#[sqlx::test]
async fn duplicate_link_is_rejected(pool: PgPool) {
    world(&pool).await;
    country_with_continent(&pool, "spain", "europe")
        .await
        .unwrap();
    rejects(
        &pool,
        "INSERT INTO country_continents (country_id, continent_id) VALUES ('spain', 'europe')",
        "23505",
        Some("country_continents_pk"),
    )
    .await;
}

#[sqlx::test]
async fn link_needs_existing_country_and_continent(pool: PgPool) {
    world(&pool).await;
    country_with_continent(&pool, "spain", "europe")
        .await
        .unwrap();
    rejects(
        &pool,
        "INSERT INTO country_continents (country_id, continent_id) VALUES ('missing', 'europe')",
        "23503",
        Some("country_continents_country_fk"),
    )
    .await;
    rejects(
        &pool,
        "INSERT INTO country_continents (country_id, continent_id) VALUES ('spain', 'missing')",
        "23503",
        Some("country_continents_continent_fk"),
    )
    .await;
}

#[sqlx::test]
async fn country_needs_at_least_one_continent(pool: PgPool) {
    world(&pool).await;
    commit_rejects(
        &pool,
        &["INSERT INTO countries VALUES ('nowhere', 'Nowhere')"],
        "23514",
        "countries_min_continents_ck",
    )
    .await;
    commit_accepts(
        &pool,
        &[
            "INSERT INTO countries VALUES ('spain', 'Spain')",
            "INSERT INTO country_continents (country_id, continent_id) VALUES ('spain', 'europe')",
        ],
    )
    .await;
}

#[sqlx::test]
async fn removing_the_last_continent_is_rejected(pool: PgPool) {
    world(&pool).await;
    country_with_continent(&pool, "turkey", "europe")
        .await
        .unwrap();
    accepts(
        &pool,
        "INSERT INTO country_continents (country_id, continent_id) VALUES ('turkey', 'asia')",
    )
    .await;

    accepts(
        &pool,
        "DELETE FROM country_continents WHERE continent_id = 'asia'",
    )
    .await;
    commit_rejects(
        &pool,
        &["DELETE FROM country_continents WHERE country_id = 'turkey'"],
        "23514",
        "countries_min_continents_ck",
    )
    .await;
    // Moving the last link to another continent keeps the minimum.
    accepts(
        &pool,
        "UPDATE country_continents SET continent_id = 'asia' WHERE country_id = 'turkey'",
    )
    .await;
    // Replacing the last link in one transaction is fine.
    commit_accepts(
        &pool,
        &[
            "DELETE FROM country_continents WHERE country_id = 'turkey'",
            "INSERT INTO country_continents (country_id, continent_id) VALUES ('turkey', 'europe')",
        ],
    )
    .await;
}
