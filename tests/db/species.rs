//! Species rows and their links (AC 3, AC 6, AC 8, AC 10, AC 16, AC 17).

use sqlx::PgPool;

use crate::support::{
    accepts, commit_accepts, commit_rejects, continent, count, country_with_continent, era,
    genus_chain, period, rejects, utc_year,
};

const CK: &str = "23514";
const GENUS: &str = "sp-genera";

/// Era, two periods, a genus chain, continents, and a country.
async fn setup(pool: &PgPool) {
    era(pool, "mesozoic", "251.902", "66.0").await.unwrap();
    period(pool, "jurassic", "mesozoic", "201.4", "145")
        .await
        .unwrap();
    period(pool, "cretaceous", "mesozoic", "145", "66.0")
        .await
        .unwrap();
    genus_chain(pool, "sp").await;
    continent(pool, "laurasia", "prehistoric").await.unwrap();
    continent(pool, "north-america", "modern").await.unwrap();
    country_with_continent(pool, "usa", "north-america")
        .await
        .unwrap();
}

/// Inserts a species with valid defaults, overridden by `cols` (SQL literals),
/// together with a `cretaceous` period link.
fn insert(id: &str, cols: &[(&str, String)]) -> String {
    let mut values: Vec<(&str, String)> = vec![
        ("id", format!("'{id}'")),
        ("genus_id", format!("'{GENUS}'")),
        ("name", "'Tyrannosaurus'".into()),
        ("scientific_name", format!("'{id}'")),
        ("diet", "'carnivore'".into()),
        ("description", "'A large theropod.'".into()),
    ];
    for (col, v) in cols {
        match values.iter_mut().find(|(c, _)| c == col) {
            Some(slot) => slot.1 = v.clone(),
            None => values.push((col, v.clone())),
        }
    }
    let names: Vec<&str> = values.iter().map(|(c, _)| *c).collect();
    let vals: Vec<&str> = values.iter().map(|(_, v)| v.as_str()).collect();
    format!(
        "WITH s AS (INSERT INTO species ({}) VALUES ({}) RETURNING id) \
         INSERT INTO species_periods (species_id, period_id) SELECT id, 'cretaceous' FROM s",
        names.join(", "),
        vals.join(", ")
    )
}

fn lit(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

#[sqlx::test]
async fn diet_is_a_lowercase_enum(pool: PgPool) {
    setup(&pool).await;
    for d in ["CARNIVORE", "Herbivore", "fungivore", ""] {
        rejects(
            &pool,
            insert("t-rex", &[("diet", lit(d))]),
            CK,
            Some("species_diet_ck"),
        )
        .await;
    }
    for d in [
        "carnivore",
        "herbivore",
        "omnivore",
        "piscivore",
        "insectivore",
    ] {
        accepts(&pool, insert(&format!("s-{d}"), &[("diet", lit(d))])).await;
    }
}

#[sqlx::test]
async fn names_and_scientific_names(pool: PgPool) {
    setup(&pool).await;
    // Same common name twice is fine.
    accepts(
        &pool,
        insert("t-rex", &[("scientific_name", lit("Tyrannosaurus rex"))]),
    )
    .await;
    accepts(
        &pool,
        insert(
            "t-bataar",
            &[("scientific_name", lit("Tarbosaurus bataar"))],
        ),
    )
    .await;
    // scientific_name is unique ignoring case.
    rejects(
        &pool,
        insert("dup", &[("scientific_name", lit("TYRANNOSAURUS REX"))]),
        "23505",
        Some("species_scientific_name_uq"),
    )
    .await;
    accepts(
        &pool,
        "UPDATE species SET scientific_name = 'TYRANNOSAURUS rex' WHERE id = 't-rex'",
    )
    .await;

    rejects(
        &pool,
        insert("long", &[("name", lit(&"a".repeat(65)))]),
        CK,
        Some("species_name_ck"),
    )
    .await;
    for bad in [
        String::new(),
        " T. rex".into(),
        "T. rex ".into(),
        "T. rex\u{a0}".into(),
        "x".repeat(129),
    ] {
        rejects(
            &pool,
            insert("bad", &[("scientific_name", lit(&bad))]),
            CK,
            Some("species_scientific_name_ck"),
        )
        .await;
    }
    accepts(
        &pool,
        insert("max", &[("scientific_name", lit(&"é".repeat(128)))]),
    )
    .await;
}

#[sqlx::test]
async fn description_rules(pool: PgPool) {
    setup(&pool).await;
    for bad in [
        String::new(),
        "x".repeat(1001),
        " Leading".into(),
        "Trailing ".into(),
        "\u{a0}Leading nbsp".into(),
        "Trailing nbsp\u{a0}".into(),
        "Trailing newline\n".into(),
    ] {
        rejects(
            &pool,
            insert("bad", &[("description", lit(&bad))]),
            CK,
            Some("species_description_ck"),
        )
        .await;
    }
    accepts(
        &pool,
        insert("max", &[("description", lit(&"ñ".repeat(1000)))]),
    )
    .await;
}

#[sqlx::test]
async fn image_url_rules(pool: PgPool) {
    setup(&pool).await;
    let too_long = format!("https://example.org/{}", "a".repeat(2049 - 20));
    assert_eq!(too_long.chars().count(), 2049);
    for bad in [
        too_long.as_str(),
        "ftp://x",
        "HTTP://x",
        "https://example.org/a.png ",
        "example.org",
    ] {
        rejects(
            &pool,
            insert("bad", &[("image_url", lit(bad))]),
            CK,
            Some("species_image_url_ck"),
        )
        .await;
    }
    accepts(
        &pool,
        insert("img", &[("image_url", lit("https://example.org/a.png"))]),
    )
    .await;
    accepts(
        &pool,
        insert("img2", &[("image_url", lit("http://example.org/b.png"))]),
    )
    .await;
    accepts(&pool, insert("no-img", &[("image_url", "NULL".into())])).await;
}

#[sqlx::test]
async fn size_measure_rules(pool: PgPool) {
    setup(&pool).await;
    for (measure, min_col, max_col) in [
        ("length", "length_min_m", "length_max_m"),
        ("height", "height_min_m", "height_max_m"),
        ("weight", "weight_min_kg", "weight_max_kg"),
    ] {
        let ck = format!("species_{measure}_ck");
        for (min, max) in [
            ("1.1234567", "2"),
            ("1", "2.1234567"),
            ("3", "2"),
            ("0", "2"),
            ("-1", "2"),
            ("-2", "-1"),
            ("1", "'NaN'"),
            ("'NaN'", "'NaN'"),
            ("1", "'Infinity'"),
            ("'-Infinity'", "1"),
            ("1", "NULL"),
            ("NULL", "1"),
        ] {
            rejects(
                &pool,
                insert(
                    "bad",
                    &[(min_col, min.to_string()), (max_col, max.to_string())],
                ),
                CK,
                Some(&ck),
            )
            .await;
        }
        accepts(
            &pool,
            insert(
                &format!("ok-{measure}"),
                &[(min_col, "0.000001".into()), (max_col, "1.5".into())],
            ),
        )
        .await;
        let stored: String = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT {min_col}::text FROM species WHERE id = 'ok-{measure}'"
        )))
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(stored, "0.000001");
    }
    accepts(&pool, insert("no-size", &[])).await;
    accepts(
        &pool,
        insert(
            "huge",
            &[
                ("weight_min_kg", "1e30".into()),
                ("weight_max_kg", "1e31".into()),
            ],
        ),
    )
    .await;
}

#[sqlx::test]
async fn discovery_year_bounds(pool: PgPool) {
    setup(&pool).await;
    let year = utc_year(&pool).await;
    rejects(
        &pool,
        insert("future", &[("discovery_year", (year + 1).to_string())]),
        CK,
        Some("species_discovery_year_not_future_ck"),
    )
    .await;
    rejects(
        &pool,
        insert("old", &[("discovery_year", "1599".into())]),
        CK,
        Some("species_discovery_year_ck"),
    )
    .await;
    accepts(&pool, insert("y1600", &[("discovery_year", "1600".into())])).await;
    accepts(
        &pool,
        insert("this-year", &[("discovery_year", year.to_string())]),
    )
    .await;
    rejects(
        &pool,
        format!(
            "UPDATE species SET discovery_year = {} WHERE id = 'y1600'",
            year + 1
        ),
        CK,
        Some("species_discovery_year_not_future_ck"),
    )
    .await;
    accepts(
        &pool,
        "UPDATE species SET discovery_year = 1905 WHERE id = 'y1600'",
    )
    .await;
}

#[sqlx::test]
async fn genus_is_required_and_must_exist(pool: PgPool) {
    setup(&pool).await;
    rejects(
        &pool,
        insert("xx", &[("genus_id", "NULL".into())]),
        "23502",
        None,
    )
    .await;
    rejects(
        &pool,
        insert("xx", &[("genus_id", lit("missing"))]),
        "23503",
        Some("species_genus_fk"),
    )
    .await;
    accepts(&pool, insert("xx", &[])).await;
}

#[sqlx::test]
async fn links_need_existing_rows_and_are_stored_once(pool: PgPool) {
    setup(&pool).await;
    accepts(&pool, insert("t-rex", &[])).await;
    for (table, col, target_fk, existing) in [
        (
            "species_periods",
            "period_id",
            "species_periods_period_fk",
            "jurassic",
        ),
        (
            "species_continents",
            "continent_id",
            "species_continents_continent_fk",
            "laurasia",
        ),
        (
            "species_countries",
            "country_id",
            "species_countries_country_fk",
            "usa",
        ),
    ] {
        rejects(
            &pool,
            format!("INSERT INTO {table} (species_id, {col}) VALUES ('t-rex', 'missing')"),
            "23503",
            Some(target_fk),
        )
        .await;
        rejects(
            &pool,
            format!("INSERT INTO {table} (species_id, {col}) VALUES ('missing', '{existing}')"),
            "23503",
            Some(&format!("{table}_species_fk")),
        )
        .await;
        accepts(
            &pool,
            format!("INSERT INTO {table} (species_id, {col}) VALUES ('t-rex', '{existing}')"),
        )
        .await;
        rejects(
            &pool,
            format!("INSERT INTO {table} (species_id, {col}) VALUES ('t-rex', '{existing}')"),
            "23505",
            Some(&format!("{table}_pk")),
        )
        .await;
    }
    // A species may link to a modern continent too.
    accepts(
        &pool,
        "INSERT INTO species_continents (species_id, continent_id) VALUES ('t-rex', 'north-america')",
    )
    .await;
}

#[sqlx::test]
async fn species_needs_at_least_one_period(pool: PgPool) {
    setup(&pool).await;
    commit_rejects(
        &pool,
        &[
            "INSERT INTO species (id, genus_id, name, scientific_name, diet, description) \
           VALUES ('lonely', 'sp-genera', 'Lonely', 'Lonely', 'carnivore', 'No period.')",
        ],
        CK,
        "species_min_periods_ck",
    )
    .await;
    commit_accepts(
        &pool,
        &[
            "INSERT INTO species (id, genus_id, name, scientific_name, diet, description) \
             VALUES ('t-rex', 'sp-genera', 'T. rex', 'Tyrannosaurus rex', 'carnivore', 'Big.')",
            "INSERT INTO species_periods VALUES ('t-rex', 'cretaceous')",
        ],
    )
    .await;
    accepts(
        &pool,
        "INSERT INTO species_periods VALUES ('t-rex', 'jurassic')",
    )
    .await;
    accepts(
        &pool,
        "DELETE FROM species_periods WHERE period_id = 'jurassic'",
    )
    .await;
    commit_rejects(
        &pool,
        &["DELETE FROM species_periods WHERE species_id = 't-rex'"],
        CK,
        "species_min_periods_ck",
    )
    .await;
    accepts(
        &pool,
        "UPDATE species_periods SET period_id = 'jurassic' WHERE species_id = 't-rex'",
    )
    .await;
}

#[sqlx::test]
async fn deleting_linked_targets_is_rejected(pool: PgPool) {
    setup(&pool).await;
    accepts(&pool, insert("t-rex", &[])).await;
    accepts(
        &pool,
        "INSERT INTO species_periods VALUES ('t-rex', 'jurassic')",
    )
    .await;
    accepts(
        &pool,
        "INSERT INTO species_continents VALUES ('t-rex', 'laurasia')",
    )
    .await;
    accepts(
        &pool,
        "INSERT INTO species_countries VALUES ('t-rex', 'usa')",
    )
    .await;

    for (sql, fk) in [
        (
            "DELETE FROM periods WHERE id = 'jurassic'",
            "species_periods_period_fk",
        ),
        (
            "DELETE FROM genera WHERE id = 'sp-genera'",
            "species_genus_fk",
        ),
        (
            "DELETE FROM continents WHERE id = 'laurasia'",
            "species_continents_continent_fk",
        ),
        (
            "DELETE FROM countries WHERE id = 'usa'",
            "species_countries_country_fk",
        ),
    ] {
        rejects(&pool, sql, "23001", Some(fk)).await;
    }
}

#[sqlx::test]
async fn deleting_a_species_removes_size_and_links(pool: PgPool) {
    setup(&pool).await;
    accepts(
        &pool,
        insert(
            "t-rex",
            &[
                ("length_min_m", "11".into()),
                ("length_max_m", "12.3".into()),
            ],
        ),
    )
    .await;
    accepts(
        &pool,
        "INSERT INTO species_periods VALUES ('t-rex', 'jurassic')",
    )
    .await;
    accepts(
        &pool,
        "INSERT INTO species_continents VALUES ('t-rex', 'laurasia')",
    )
    .await;
    accepts(
        &pool,
        "INSERT INTO species_countries VALUES ('t-rex', 'usa')",
    )
    .await;

    accepts(&pool, "DELETE FROM species WHERE id = 't-rex'").await;
    for t in [
        "species",
        "species_periods",
        "species_continents",
        "species_countries",
    ] {
        assert_eq!(
            count(&pool, format!("SELECT count(*) FROM {t}")).await,
            0,
            "{t}"
        );
    }
    // The freed targets can now be deleted.
    accepts(&pool, "DELETE FROM periods WHERE id = 'jurassic'").await;
    accepts(&pool, "DELETE FROM continents WHERE id = 'laurasia'").await;
    accepts(&pool, "DELETE FROM countries WHERE id = 'usa'").await;
}
