//! Every statement the API runs for reads and write lookups is index-served on the
//! 001 fixture (Constitution III, 001 data-model §6.1, data-model §3).

use paleo_api::api::db_error::{
    CONTINENT_COUNTRIES_SQL, CONTINENT_SPECIES_SQL, COUNTRY_DEPENDENTS_SQL, ERA_DEPENDENTS_SQL,
    PERIOD_DEPENDENTS_SQL,
};
use paleo_api::api::geography::{
    self as geo, CONTINENT_BY_ID_SQL, COUNTRY_BY_ID_SQL, COUNTRY_CONTINENTS_SQL, ContinentQuery,
    CountryQuery,
};
use paleo_api::api::geologic_time::{
    self as time, ERA_BY_ID_SQL, ERA_OVERLAP_SQL, ERA_PERIODS_OUTSIDE_SQL, EraQuery,
    PERIOD_BY_ID_SQL, PERIOD_OVERLAP_SQL, PeriodQuery, TimeSort,
};
use paleo_api::api::params::{Page, Sort};
use paleo_api::api::species::SPECIES_BY_ID_SQL;
use paleo_api::api::species::card::{
    SPECIES_CONTINENTS_SQL, SPECIES_COUNTRIES_SQL, SPECIES_PERIODS_SQL, SPECIES_TAXONOMY_SQL,
};
use paleo_api::api::species::list::{
    self as species_list, SpeciesFilter, SpeciesQuery, SpeciesSort,
};
use paleo_api::api::taxonomy::{self, NameSort, RANKS, RankQuery};
use sqlx::PgPool;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

use crate::support::db_support::{
    all_tables, assert_no_seq_scan_on, assert_uses_any_index, exec_script, index_rows,
    recheck_removed,
};
use crate::support::*;

const DATASET: &str = include_str!("../db/fixtures/index_dataset.sql");

/// Unfiltered counts: any index of the table may serve them.
const ANY_INDEX: &[&str] = &["*"];

/// 10% of the fixture's 10,000 species (001 data-model §6.1, Q-S11).
const SELECTIVE: f64 = 1_000.0;

/// A deep page, so `OFFSET` is part of the planned statement.
const PAGE: Page = Page { page: 3, limit: 20 };

async fn load(pool: &PgPool) {
    exec_script(pool, DATASET).await.expect("fixture must load");
    exec(pool, "ANALYZE").await.unwrap();
}

/// Collects failures so one run reports every non-index-served shape.
#[derive(Default)]
struct Checks {
    failures: Vec<String>,
}

impl Checks {
    fn check(&mut self, id: &str, plan: &str, allowed: &[&str]) {
        let tables = all_tables();
        let result = std::panic::catch_unwind(|| {
            if allowed == ANY_INDEX {
                assert!(plan.contains("Index"), "{id}: no index used:\n{plan}");
            } else {
                assert_uses_any_index(id, plan, allowed);
            }
            assert_no_seq_scan_on(id, plan, &tables);
        });
        if result.is_err() {
            self.failures
                .push(format!("{id}\n  allowed: {allowed:?}\n{plan}"));
        }
    }

    fn finish(self) {
        assert!(
            self.failures.is_empty(),
            "{} shape(s) failed:\n\n{}",
            self.failures.len(),
            self.failures.join("\n\n")
        );
    }
}

fn sorts<S: Copy>(keys: &[S]) -> Vec<Sort<S>> {
    keys.iter()
        .flat_map(|&key| [Sort { key, desc: false }, Sort { key, desc: true }])
        .collect()
}

#[sqlx::test]
async fn timeline_geography_and_taxonomy_lists(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app(pool_opts, opts).await;
    load(&app.pool).await;
    let pool = &app.pool;
    let mut c = Checks::default();

    for sort in sorts(&[TimeSort::StartMya, TimeSort::Name]) {
        let id = format!("eras {sort:?}");
        let q = EraQuery {
            page: PAGE,
            sort,
            filter: (),
        };
        let allowed = match sort.key {
            TimeSort::StartMya => ["eras_start_mya_idx"],
            TimeSort::Name => ["eras_name_sort_idx"],
        };
        let plan = explain_with_binds(pool, false, |qb| time::push_era_page_sql(qb, &q)).await;
        c.check(&id, &plan, &allowed);
        let plan = explain_with_binds(pool, false, |qb| time::push_era_count_sql(qb, &q)).await;
        c.check(&format!("{id} count"), &plan, ANY_INDEX);

        for filter in [None, Some("era-03".to_string())] {
            let id = format!("periods {sort:?} {filter:?}");
            let q = PeriodQuery {
                page: PAGE,
                sort,
                filter: filter.clone(),
            };
            let (page_allowed, count_allowed): (&[&str], &[&str]) = match (&filter, sort.key) {
                (Some(_), _) => (&["periods_era_idx"], &["periods_era_idx"]),
                (None, TimeSort::StartMya) => (&["periods_start_mya_idx"], ANY_INDEX),
                (None, TimeSort::Name) => (&["periods_name_sort_idx"], ANY_INDEX),
            };
            let plan =
                explain_with_binds(pool, false, |qb| time::push_period_page_sql(qb, &q)).await;
            c.check(&id, &plan, page_allowed);
            let plan =
                explain_with_binds(pool, false, |qb| time::push_period_count_sql(qb, &q)).await;
            c.check(&format!("{id} count"), &plan, count_allowed);
        }
    }

    for sort in sorts(&[NameSort]) {
        for rank in RANKS.iter() {
            let mut filters = vec![None];
            if let Some(parent) = rank.parent_rank() {
                filters.push(Some(format!("{}-1", parent.singular)));
            }
            for filter in filters {
                let id = format!("{} {:?} {filter:?}", rank.plural, sort.desc);
                let q = RankQuery {
                    page: PAGE,
                    sort,
                    filter: filter.clone(),
                };
                let by_parent = rank
                    .parent_col
                    .map(|col| format!("{}_{}_idx", rank.plural, col.trim_end_matches("_id")));
                let name_idx = format!("{}_name_sort_idx", rank.plural);
                let (page_allowed, count_allowed): (Vec<&str>, Vec<&str>) =
                    match (&filter, &by_parent) {
                        (Some(_), Some(idx)) => (vec![idx], vec![idx]),
                        _ => (vec![&name_idx], ANY_INDEX.to_vec()),
                    };
                let plan =
                    explain_with_binds(pool, false, |qb| taxonomy::push_page_sql(qb, rank, &q))
                        .await;
                c.check(&id, &plan, &page_allowed);
                let plan =
                    explain_with_binds(pool, false, |qb| taxonomy::push_count_sql(qb, rank, &q))
                        .await;
                c.check(&format!("{id} count"), &plan, &count_allowed);
            }
        }

        for kind in [None, Some("modern")] {
            let id = format!("continents {:?} {kind:?}", sort.desc);
            let q = ContinentQuery {
                page: PAGE,
                sort: Sort {
                    key: geo::NameSort,
                    desc: sort.desc,
                },
                filter: kind,
            };
            let (page_allowed, count_allowed): (&[&str], &[&str]) = match kind {
                Some(_) => (&["continents_type_idx"], &["continents_type_idx"]),
                None => (&["continents_name_sort_idx"], ANY_INDEX),
            };
            let plan =
                explain_with_binds(pool, false, |qb| geo::push_continent_page_sql(qb, &q)).await;
            c.check(&id, &plan, page_allowed);
            let plan =
                explain_with_binds(pool, false, |qb| geo::push_continent_count_sql(qb, &q)).await;
            c.check(&format!("{id} count"), &plan, count_allowed);
        }

        for continent in [None, Some("continent-3".to_string())] {
            let id = format!("countries {:?} {continent:?}", sort.desc);
            let q = CountryQuery {
                page: PAGE,
                sort: Sort {
                    key: geo::NameSort,
                    desc: sort.desc,
                },
                filter: continent.clone(),
            };
            let (page_allowed, count_allowed): (&[&str], &[&str]) = match continent {
                Some(_) => (
                    &["country_continents_continent_idx"],
                    &["country_continents_continent_idx"],
                ),
                None => (&["countries_name_sort_idx"], ANY_INDEX),
            };
            let plan =
                explain_with_binds(pool, false, |qb| geo::push_country_page_sql(qb, &q)).await;
            c.check(&id, &plan, page_allowed);
            let plan =
                explain_with_binds(pool, false, |qb| geo::push_country_count_sql(qb, &q)).await;
            c.check(&format!("{id} count"), &plan, count_allowed);
        }
    }
    c.finish();
}

/// One species filter at a time with the indexes that may serve it.
fn species_filters() -> Vec<(&'static str, SpeciesFilter, Vec<&'static str>)> {
    let f = SpeciesFilter::default;
    let chain = [
        "genera_family_idx",
        "families_order_idx",
        "orders_class_idx",
        "classes_phylum_idx",
        "phyla_kingdom_idx",
        "kingdoms_domain_idx",
    ];
    let ranked = |depth: usize| {
        let mut v = vec!["species_genus_idx"];
        v.extend(&chain[..depth]);
        v
    };
    vec![
        ("none", f(), vec![]),
        (
            "diet",
            SpeciesFilter {
                diet: Some("piscivore"),
                ..f()
            },
            vec!["species_diet_idx"],
        ),
        (
            "genus",
            SpeciesFilter {
                genus: Some("genus-42".into()),
                ..f()
            },
            ranked(0),
        ),
        (
            "family",
            SpeciesFilter {
                family: Some("family-2".into()),
                ..f()
            },
            ranked(1),
        ),
        (
            "order",
            SpeciesFilter {
                order: Some("order-2".into()),
                ..f()
            },
            ranked(2),
        ),
        (
            "class",
            SpeciesFilter {
                class: Some("class-2".into()),
                ..f()
            },
            ranked(3),
        ),
        (
            "phylum",
            SpeciesFilter {
                phylum: Some("phylum-2".into()),
                ..f()
            },
            ranked(4),
        ),
        (
            "kingdom",
            SpeciesFilter {
                kingdom: Some("kingdom-2".into()),
                ..f()
            },
            ranked(5),
        ),
        (
            "domain",
            SpeciesFilter {
                domain: Some("domain-2".into()),
                ..f()
            },
            ranked(6),
        ),
        (
            "period",
            SpeciesFilter {
                period: Some("period-07".into()),
                ..f()
            },
            vec!["species_periods_period_idx"],
        ),
        (
            "era",
            SpeciesFilter {
                era: Some("era-03".into()),
                ..f()
            },
            vec!["periods_era_idx", "species_periods_period_idx"],
        ),
        (
            "continent",
            SpeciesFilter {
                continent: Some("continent-5".into()),
                ..f()
            },
            vec!["species_continents_continent_idx"],
        ),
        (
            "country",
            SpeciesFilter {
                country: Some("country-17".into()),
                ..f()
            },
            vec!["species_countries_country_idx"],
        ),
        (
            "q rax",
            SpeciesFilter {
                q: Some("%rax%".into()),
                ..f()
            },
            vec!["species_name_trgm_idx", "species_scientific_name_trgm_idx"],
        ),
        (
            "q saurus",
            SpeciesFilter {
                q: Some("%saurus%".into()),
                ..f()
            },
            vec!["species_name_trgm_idx", "species_scientific_name_trgm_idx"],
        ),
    ]
}

fn sort_index(sort: Sort<SpeciesSort>) -> &'static str {
    match (sort.key, sort.desc) {
        (SpeciesSort::Name, _) => "species_name_sort_idx",
        (SpeciesSort::ScientificName, _) => "species_scientific_name_sort_idx",
        (SpeciesSort::DiscoveryYear, false) => "species_discovery_year_asc_idx",
        (SpeciesSort::DiscoveryYear, true) => "species_discovery_year_desc_idx",
    }
}

#[sqlx::test]
async fn species_filters_and_sorts(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app(pool_opts, opts).await;
    load(&app.pool).await;
    let pool = &app.pool;
    let mut c = Checks::default();
    let all_sorts = sorts(&[
        SpeciesSort::Name,
        SpeciesSort::ScientificName,
        SpeciesSort::DiscoveryYear,
    ]);
    for (name, filter, filter_indexes) in species_filters() {
        for &sort in &all_sorts {
            let id = format!("species {name} {sort:?}");
            let q = SpeciesQuery {
                page: PAGE,
                sort,
                filter: filter.clone(),
            };
            // A filtered page may also walk its sort index and probe the filter (001-DM §6.1).
            let mut allowed = filter_indexes.clone();
            allowed.push(sort_index(sort));
            let plan =
                explain_with_binds(pool, false, |qb| species_list::push_page_sql(qb, &q)).await;
            c.check(&id, &plan, &allowed);
        }
        let q = SpeciesQuery {
            page: PAGE,
            sort: species_list::DEFAULT_SORT,
            filter: filter.clone(),
        };
        let plan = explain_with_binds(pool, false, |qb| species_list::push_count_sql(qb, &q)).await;
        let count_allowed: Vec<&str> = if filter_indexes.is_empty() {
            ANY_INDEX.to_vec()
        } else {
            filter_indexes.clone()
        };
        c.check(&format!("species {name} count"), &plan, &count_allowed);
    }
    c.finish();
}

#[sqlx::test]
async fn species_search_count_is_selective(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app(pool_opts, opts).await;
    load(&app.pool).await;
    for term in ["rax", "saurus"] {
        let q = SpeciesQuery {
            page: PAGE,
            sort: species_list::DEFAULT_SORT,
            filter: SpeciesFilter {
                q: Some(format!("%{term}%")),
                ..SpeciesFilter::default()
            },
        };
        let plan =
            explain_with_binds(&app.pool, true, |qb| species_list::push_count_sql(qb, &q)).await;
        for idx in ["species_name_trgm_idx", "species_scientific_name_trgm_idx"] {
            let rows = index_rows(&plan, idx);
            assert!(
                rows < SELECTIVE,
                "{term}: {idx} returned {rows} rows:\n{plan}"
            );
        }
        for removed in recheck_removed(&plan) {
            assert!(
                removed < SELECTIVE,
                "{term}: recheck removed {removed} rows:\n{plan}"
            );
        }
    }
}

#[sqlx::test]
async fn detail_lookups_and_embeddings(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app(pool_opts, opts).await;
    load(&app.pool).await;
    let pool = &app.pool;
    let mut c = Checks::default();
    let species = texts(&[
        "species-1",
        "species-2",
        "species-3",
        "species-500",
        "species-9999",
    ]);
    let shapes: Vec<(String, &str, Vec<Bind>, Vec<&str>)> = vec![
        (
            "Q-E3".into(),
            ERA_BY_ID_SQL,
            vec![text("era-03")],
            vec!["eras_pk"],
        ),
        (
            "Q-P4".into(),
            PERIOD_BY_ID_SQL,
            vec![text("period-07")],
            vec!["periods_pk"],
        ),
        (
            "continent by id".into(),
            CONTINENT_BY_ID_SQL,
            vec![text("continent-3")],
            // the composite FK target (id, type) serves an id lookup as well
            vec!["continents_pk", "continents_id_type_uq"],
        ),
        (
            "Q-K2".into(),
            COUNTRY_BY_ID_SQL,
            vec![text("country-17")],
            vec!["countries_pk"],
        ),
        (
            "Q-K2 links".into(),
            COUNTRY_CONTINENTS_SQL,
            vec![texts(&["country-17", "country-18", "country-150"])],
            vec!["country_continents_pk"],
        ),
        (
            "Q-S13".into(),
            SPECIES_BY_ID_SQL,
            vec![text("species-4242")],
            vec!["species_pk"],
        ),
        (
            "Q-S12 lineage".into(),
            SPECIES_TAXONOMY_SQL,
            vec![species.clone()],
            vec!["species_pk"],
        ),
        (
            "Q-S12 periods".into(),
            SPECIES_PERIODS_SQL,
            vec![species.clone()],
            vec!["species_periods_pk"],
        ),
        (
            "Q-S12 continents".into(),
            SPECIES_CONTINENTS_SQL,
            vec![species.clone()],
            vec!["species_continents_pk"],
        ),
        (
            "Q-S12 countries".into(),
            SPECIES_COUNTRIES_SQL,
            vec![species],
            vec!["species_countries_pk"],
        ),
    ];
    for (id, sql, binds, allowed) in shapes {
        let plan = explain_sql(pool, sql, binds).await;
        c.check(&id, &plan, &allowed);
    }
    for rank in RANKS.iter() {
        let plan = explain_sql(
            pool,
            rank.by_id_sql,
            vec![text(&format!("{}-1", rank.singular))],
        )
        .await;
        c.check(
            &format!("Q-T3 {}", rank.plural),
            &plan,
            &[&format!("{}_pk", rank.plural)],
        );
    }
    c.finish();
}

#[sqlx::test]
async fn write_lookups(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app(pool_opts, opts).await;
    load(&app.pool).await;
    let pool = &app.pool;
    let mut c = Checks::default();
    let shapes: Vec<(String, &str, Vec<Bind>, Vec<&str>)> = vec![
        (
            "Q-W5".into(),
            ERA_OVERLAP_SQL,
            vec![num("10"), num("20"), text("era-03")],
            vec!["eras_range_ex"],
        ),
        (
            "Q-W6".into(),
            PERIOD_OVERLAP_SQL,
            vec![num("10"), num("20"), text("period-07")],
            vec!["periods_range_ex"],
        ),
        (
            "Q-W7".into(),
            ERA_PERIODS_OUTSIDE_SQL,
            vec![text("era-03"), num("800"), num("700")],
            vec!["periods_era_idx"],
        ),
        (
            "Q-W9".into(),
            CONTINENT_COUNTRIES_SQL,
            vec![text("continent-3")],
            vec!["country_continents_continent_idx"],
        ),
        (
            "Q-W10 era".into(),
            ERA_DEPENDENTS_SQL,
            vec![text("era-03")],
            vec!["periods_era_idx"],
        ),
        (
            "Q-W10 period".into(),
            PERIOD_DEPENDENTS_SQL,
            vec![text("period-07")],
            vec!["species_periods_period_idx"],
        ),
        (
            "Q-W10 continent species".into(),
            CONTINENT_SPECIES_SQL,
            vec![text("continent-5")],
            vec!["species_continents_continent_idx"],
        ),
        (
            "Q-W10 country".into(),
            COUNTRY_DEPENDENTS_SQL,
            vec![text("country-17")],
            vec!["species_countries_country_idx"],
        ),
    ];
    for (id, sql, binds, allowed) in shapes {
        let plan = explain_sql(pool, sql, binds).await;
        c.check(&id, &plan, &allowed);
    }
    for rank in RANKS.iter() {
        let allowed = match rank.child_rank() {
            Some(child) => format!(
                "{}_{}_idx",
                child.plural,
                child.parent_col.unwrap().trim_end_matches("_id")
            ),
            None => "species_genus_idx".to_string(),
        };
        let plan = explain_sql(
            pool,
            rank.dependents_sql,
            vec![text(&format!("{}-1", rank.singular))],
        )
        .await;
        c.check(&format!("Q-W10 {}", rank.plural), &plan, &[&allowed]);
    }
    c.finish();
}
