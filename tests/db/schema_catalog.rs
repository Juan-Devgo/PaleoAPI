//! The schema contains exactly the objects of data-model §2–§5 and §6.2
//! (FR-008–FR-010, FR-012).

use std::collections::BTreeSet;

use sqlx::PgPool;

use crate::support::{LINK_TABLES, RANKS, RESOURCE_TABLES, all_tables};

async fn names(pool: &PgPool, sql: &'static str) -> BTreeSet<String> {
    sqlx::query_scalar::<_, String>(sql)
        .fetch_all(pool)
        .await
        .unwrap()
        .into_iter()
        .collect()
}

#[track_caller]
fn assert_contains_all(kind: &str, actual: &BTreeSet<String>, expected: &[String]) {
    let missing: Vec<&String> = expected.iter().filter(|e| !actual.contains(*e)).collect();
    assert!(missing.is_empty(), "missing {kind}: {missing:?}");
}

#[sqlx::test]
async fn exactly_the_sixteen_tables_exist(pool: PgPool) {
    let tables = names(
        &pool,
        "SELECT relname::text FROM pg_class WHERE relnamespace = 'public'::regnamespace AND relkind IN ('r', 'p')",
    )
    .await;
    let mut expected: BTreeSet<String> = all_tables().into_iter().map(String::from).collect();
    expected.insert("_sqlx_migrations".into());
    assert_eq!(tables, expected);

    for forbidden in [
        "taxonomy",
        "user",
        "users",
        "role",
        "roles",
        "user_role",
        "user_roles",
    ] {
        assert!(
            !tables.contains(forbidden),
            "{forbidden} must not exist (FR-010)"
        );
    }
}

#[sqlx::test]
async fn no_varchar_and_numeric_is_unconstrained(pool: PgPool) {
    let bad = names(
        &pool,
        "SELECT c.relname || '.' || a.attname || ' ' || format_type(a.atttypid, a.atttypmod)
           FROM pg_attribute a JOIN pg_class c ON c.oid = a.attrelid
          WHERE c.relnamespace = 'public'::regnamespace AND c.relkind = 'r'
            AND c.relname <> '_sqlx_migrations' AND a.attnum > 0 AND NOT a.attisdropped
            AND (a.atttypid = 'varchar'::regtype
                 OR (a.atttypid = 'numeric'::regtype AND a.atttypmod <> -1))",
    )
    .await;
    assert!(bad.is_empty(), "columns violating type rules: {bad:?}");
}

fn expected_constraints() -> Vec<String> {
    let mut c: Vec<String> = Vec::new();
    for t in RESOURCE_TABLES {
        c.extend([
            format!("{t}_pk"),
            format!("{t}_id_ck"),
            format!("{t}_name_ck"),
        ]);
    }
    for t in ["eras", "periods"] {
        for r in ["start_mya_ck", "end_mya_ck", "range_ck", "range_ex"] {
            c.push(format!("{t}_{r}"));
        }
    }
    c.push("periods_era_fk".into());
    for (t, parent) in RANKS {
        if let Some((col, _)) = parent {
            c.push(format!("{t}_{}_fk", col.trim_end_matches("_id")));
        }
    }
    c.extend(
        [
            "continents_type_ck",
            "continents_id_type_uq",
            "country_continents_pk",
            "country_continents_country_fk",
            "country_continents_continent_fk",
            "country_continents_continent_type_ck",
            "species_scientific_name_ck",
            "species_diet_ck",
            "species_description_ck",
            "species_image_url_ck",
            "species_discovery_year_ck",
            "species_length_ck",
            "species_height_ck",
            "species_weight_ck",
            "species_genus_fk",
        ]
        .map(String::from),
    );
    for (link, target) in [
        ("species_periods", "period"),
        ("species_continents", "continent"),
        ("species_countries", "country"),
    ] {
        c.extend([
            format!("{link}_pk"),
            format!("{link}_species_fk"),
            format!("{link}_{target}_fk"),
        ]);
    }
    c
}

#[sqlx::test]
async fn every_constraint_exists(pool: PgPool) {
    let actual = names(
        &pool,
        "SELECT conname::text FROM pg_constraint WHERE connamespace = 'public'::regnamespace",
    )
    .await;
    assert_contains_all("constraints", &actual, &expected_constraints());
}

#[sqlx::test]
async fn every_integrity_index_exists(pool: PgPool) {
    let actual = names(
        &pool,
        "SELECT c.relname::text FROM pg_index i JOIN pg_class c ON c.oid = i.indexrelid
          WHERE c.relnamespace = 'public'::regnamespace",
    )
    .await;
    let mut expected: Vec<String> = RESOURCE_TABLES
        .iter()
        .filter(|t| **t != "species")
        .map(|t| format!("{t}_name_uq"))
        .collect();
    expected.extend(
        [
            "periods_era_idx",
            "country_continents_continent_idx",
            "species_scientific_name_uq",
            "species_genus_idx",
            "species_periods_period_idx",
            "species_continents_continent_idx",
            "species_countries_country_idx",
            "continents_id_type_uq",
        ]
        .map(String::from),
    );
    for (t, parent) in RANKS {
        if let Some((col, _)) = parent {
            expected.push(format!("{t}_{}_idx", col.trim_end_matches("_id")));
        }
    }
    assert_contains_all("indexes", &actual, &expected);

    let unique = names(
        &pool,
        "SELECT c.relname::text FROM pg_index i JOIN pg_class c ON c.oid = i.indexrelid
          WHERE c.relnamespace = 'public'::regnamespace AND i.indisunique",
    )
    .await;
    assert!(unique.contains("species_scientific_name_uq"));
    assert!(
        !unique.iter().any(|u| u == "species_name_uq"),
        "species name is not unique"
    );
}

#[sqlx::test]
async fn every_trigger_and_function_exists(pool: PgPool) {
    let triggers = names(
        &pool,
        "SELECT t.tgname::text FROM pg_trigger t JOIN pg_class c ON c.oid = t.tgrelid
          WHERE c.relnamespace = 'public'::regnamespace AND NOT t.tgisinternal",
    )
    .await;
    let mut expected: Vec<String> = RESOURCE_TABLES
        .iter()
        .map(|t| format!("{t}_id_immutable_trg"))
        .collect();
    expected.extend(all_tables().iter().map(|t| format!("{t}_no_truncate_trg")));
    let rules = [
        "eras_contains_periods",
        "periods_within_era",
        "countries_min_continents",
        "country_continents_min",
        "species_discovery_year",
        "species_min_periods",
        "species_periods_min",
    ];
    expected.extend(rules.iter().map(|r| format!("{r}_trg")));
    assert_contains_all("triggers", &triggers, &expected);

    let functions = names(
        &pool,
        "SELECT proname::text FROM pg_proc WHERE pronamespace = 'public'::regnamespace",
    )
    .await;
    let mut expected: Vec<String> = rules.iter().map(|r| format!("{r}_fn")).collect();
    expected.extend(
        [
            "countries_assert_min_continents",
            "species_assert_min_periods",
            "paleo_forbid_id_change",
            "paleo_forbid_truncate",
            "paleo_require_safe_isolation",
        ]
        .map(String::from),
    );
    assert_contains_all("functions", &functions, &expected);
}

#[sqlx::test]
async fn foreign_key_actions_follow_delete_rules(pool: PgPool) {
    let fks: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT conname::text, confdeltype::text, confupdtype::text FROM pg_constraint
          WHERE connamespace = 'public'::regnamespace AND contype = 'f' ORDER BY 1",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert!(!fks.is_empty());
    let owners = [
        "country_continents_country_fk",
        "species_periods_species_fk",
        "species_continents_species_fk",
        "species_countries_species_fk",
    ];
    for (name, on_delete, on_update) in &fks {
        let want_delete = if owners.contains(&name.as_str()) {
            "c"
        } else {
            "r"
        };
        assert_eq!(on_delete, want_delete, "{name}: ON DELETE");
        let want_update = if name == "country_continents_continent_fk" {
            "r"
        } else {
            "a"
        };
        assert_eq!(on_update, want_update, "{name}: ON UPDATE");
    }
}

#[sqlx::test]
async fn minimum_triggers_are_deferred_constraint_triggers(pool: PgPool) {
    for trg in [
        "countries_min_continents_trg",
        "country_continents_min_trg",
        "species_min_periods_trg",
        "species_periods_min_trg",
    ] {
        let row: Option<(bool, bool, bool)> = sqlx::query_as(
            "SELECT tgconstraint <> 0, tgdeferrable, tginitdeferred FROM pg_trigger WHERE tgname = $1",
        )
        .bind(trg)
        .fetch_optional(&pool)
        .await
        .unwrap();
        assert_eq!(
            row,
            Some((true, true, true)),
            "{trg} must be DEFERRABLE INITIALLY DEFERRED"
        );
    }
}

#[sqlx::test]
async fn every_table_has_a_truncate_guard(pool: PgPool) {
    // tgtype bits: 1 = row, 2 = before, 32 = truncate.
    let guarded = names(
        &pool,
        "SELECT c.relname::text FROM pg_trigger t JOIN pg_class c ON c.oid = t.tgrelid
          WHERE c.relnamespace = 'public'::regnamespace AND NOT t.tgisinternal
            AND t.tgtype & 32 = 32 AND t.tgtype & 2 = 2 AND t.tgtype & 1 = 0
            AND t.tgname = c.relname || '_no_truncate_trg'",
    )
    .await;
    let expected: BTreeSet<String> = RESOURCE_TABLES
        .iter()
        .chain(LINK_TABLES.iter())
        .map(|t| t.to_string())
        .collect();
    assert_eq!(guarded, expected);
}
