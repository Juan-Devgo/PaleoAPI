//! Linguistic, case-insensitive name sorts (AC 18, FR-016).

use sqlx::{AssertSqlSafe, PgPool};

use crate::support::{assert_uses_any_index, era, exec, explain, genus_chain, period};

const SORT: &str = "lower(name) COLLATE paleo_name_sort";

async fn names(pool: &PgPool, table: &str, dir: &str) -> Vec<String> {
    sqlx::query_scalar(AssertSqlSafe(format!(
        "SELECT name FROM {table} ORDER BY {SORT} {dir}, id"
    )))
    .fetch_all(pool)
    .await
    .unwrap()
}

#[sqlx::test]
async fn species_sort_linguistically_ignoring_case(pool: PgPool) {
    era(&pool, "mesozoic", "251.902", "66.0").await.unwrap();
    period(&pool, "cretaceous", "mesozoic", "145", "66.0")
        .await
        .unwrap();
    let genus = genus_chain(&pool, "co").await;
    // Ids decide the Nandu/nandu tie: "a-nandu" before "b-nandu".
    for (id, name) in [
        ("zebra", "Zebra"),
        ("oviraptor", "Oviraptor"),
        ("nandu-accent", "Ñandú"),
        ("b-nandu", "nandu"),
        ("a-nandu", "Nandu"),
    ] {
        exec(
            &pool,
            format!(
                "WITH s AS (INSERT INTO species (id, genus_id, name, scientific_name, diet, description) \
                 VALUES ('{id}', '{genus}', '{name}', '{id}', 'herbivore', 'Text.') RETURNING id) \
                 INSERT INTO species_periods SELECT id, 'cretaceous' FROM s"
            ),
        )
        .await
        .unwrap();
    }
    assert_eq!(
        names(&pool, "species", "").await,
        ["Nandu", "nandu", "Ñandú", "Oviraptor", "Zebra"]
    );
    assert_eq!(
        names(&pool, "species", "DESC").await,
        ["Zebra", "Oviraptor", "Ñandú", "Nandu", "nandu"]
    );
}

#[sqlx::test]
async fn eras_sort_linguistically(pool: PgPool) {
    for (i, name) in ["Zebra", "Oviraptor", "Ñandú", "Nandu"].iter().enumerate() {
        exec(
            &pool,
            format!(
                "INSERT INTO eras VALUES ('era-{i}', '{name}', {}, {})",
                i + 1,
                i
            ),
        )
        .await
        .unwrap();
    }
    assert_eq!(
        names(&pool, "eras", "").await,
        ["Nandu", "Ñandú", "Oviraptor", "Zebra"]
    );
    assert_eq!(
        names(&pool, "eras", "DESC").await,
        ["Zebra", "Oviraptor", "Ñandú", "Nandu"]
    );
}

#[sqlx::test]
async fn name_sort_is_served_by_its_index(pool: PgPool) {
    let plan = explain(
        &pool,
        &format!("SELECT * FROM species ORDER BY {SORT}, id LIMIT 20"),
        true,
    )
    .await;
    assert_uses_any_index("species name sort", &plan, &["species_name_sort_idx"]);
}

#[sqlx::test]
async fn collation_version_is_recorded(pool: PgPool) {
    let (recorded, actual): (Option<String>, Option<String>) = sqlx::query_as(
        "SELECT collversion, pg_collation_actual_version(oid) FROM pg_collation \
          WHERE collname = 'paleo_name_sort'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(recorded.is_some(), "collversion must be recorded");
    assert_eq!(recorded, actual);
}
