//! Every database rule has a row in the error map (data-model §4).

use paleo_api::api::db_error::is_mapped;
use sqlx::PgPool;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

use crate::support::app;
use crate::support::db_support::{RESOURCE_TABLES, all_tables};

/// Constraint names and unique index names of schema `public`.
async fn schema_names(pool: &PgPool) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT conname::text FROM pg_constraint c JOIN pg_namespace n ON n.oid = c.connamespace \
          WHERE n.nspname = 'public' \
         UNION \
         SELECT i.relname::text FROM pg_index x JOIN pg_class i ON i.oid = x.indexrelid \
           JOIN pg_namespace n ON n.oid = i.relnamespace \
          WHERE n.nspname = 'public' AND x.indisunique \
         ORDER BY 1",
    )
    .fetch_all(pool)
    .await
    .unwrap()
}

/// Names raised by triggers with `CONSTRAINT = …` (001 data-model §1.1).
fn trigger_names() -> Vec<String> {
    let mut names: Vec<String> = RESOURCE_TABLES
        .iter()
        .map(|t| format!("{t}_id_immutable_ck"))
        .collect();
    names.extend(all_tables().iter().map(|t| format!("{t}_no_truncate_ck")));
    names.extend(
        [
            "periods_within_era_ck",
            "eras_contains_periods_ck",
            "species_discovery_year_not_future_ck",
            "countries_min_continents_ck",
            "species_min_periods_ck",
        ]
        .map(String::from),
    );
    names
}

#[sqlx::test]
async fn every_constraint_is_mapped(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app(pool_opts, opts).await;
    let names = schema_names(&app.pool).await;
    assert!(names.len() > 80, "suspiciously few constraints: {names:?}");
    let triggers = trigger_names();
    let unmapped: Vec<&String> = names
        .iter()
        .chain(triggers.iter())
        .filter(|n| !is_mapped(n))
        .collect();
    assert!(unmapped.is_empty(), "unmapped constraints: {unmapped:?}");
}

#[test]
fn unknown_names_are_not_mapped() {
    for name in [
        "",
        "eras",
        "eras_pk_x",
        "foo_pk",
        "species_size_ck",
        "eras_name_idx",
    ] {
        assert!(!is_mapped(name), "{name}");
    }
}
