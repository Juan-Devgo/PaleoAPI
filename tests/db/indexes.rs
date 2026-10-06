//! Every public query shape and integrity lookup can be served by an index,
//! verified with the planner on the 10,000-species dataset
//! (AC 14, FR-011, FR-012, data-model §6, research R15).

use std::time::{Duration, Instant};

use sqlx::{AssertSqlSafe, PgPool};

use crate::support::{
    RANKS, all_tables, assert_no_seq_scan_on, assert_uses_any_index, exec, exec_script, explain,
    explain_analyze, index_rows, recheck_removed,
};

const DATASET: &str = include_str!("fixtures/index_dataset.sql");
/// Unfiltered counts: any index of the table may serve them; the check is
/// that the plan uses an index and has no Seq Scan.
const ANY_INDEX: &[&str] = &["*"];

/// 10% of the fixture's 10,000 species (data-model §6.1, Q-S11).
const SELECTIVE: f64 = 1_000.0;

async fn load(pool: &PgPool) {
    exec_script(pool, DATASET).await.expect("fixture must load");
    exec(pool, "ANALYZE").await.unwrap();
}

fn ns(col: &str) -> String {
    format!("lower({col}) COLLATE paleo_name_sort")
}

/// A query shape and the indexes allowed to serve it.
struct Shape {
    id: String,
    sql: String,
    allowed: Vec<String>,
}

fn shape(id: impl Into<String>, sql: impl Into<String>, allowed: &[&str]) -> Shape {
    Shape {
        id: id.into(),
        sql: sql.into(),
        allowed: allowed.iter().map(|s| s.to_string()).collect(),
    }
}

/// Adds a page shape for both sort directions.
fn sorted(out: &mut Vec<Shape>, id: &str, from_where: &str, order: &str, allowed: &[&str]) {
    for dir in ["", " DESC"] {
        let order_by = order.replace("{dir}", dir);
        out.push(shape(
            format!("{id}{dir}"),
            format!("SELECT * FROM {from_where} ORDER BY {order_by} LIMIT 20 OFFSET 40"),
            allowed,
        ));
    }
}

/// Data-model §6.1 shapes, each with its `count(*)` variant.
fn public_shapes() -> Vec<Shape> {
    let mut s = Vec::new();
    let name = ns("name");

    // Eras
    sorted(
        &mut s,
        "Q-E1",
        "eras",
        "start_mya{dir}, id",
        &["eras_start_mya_idx"],
    );
    sorted(
        &mut s,
        "Q-E2",
        "eras",
        &format!("{name}{{dir}}, id"),
        &["eras_name_sort_idx"],
    );
    s.push(shape("Q-E1 count", "SELECT count(*) FROM eras", ANY_INDEX));
    s.push(shape(
        "Q-E3",
        "SELECT * FROM eras WHERE id = 'era-03'",
        &["eras_pk"],
    ));

    // Periods
    sorted(
        &mut s,
        "Q-P1",
        "periods WHERE era_id = 'era-03'",
        "start_mya{dir}, id",
        &["periods_era_idx"],
    );
    s.push(shape(
        "Q-P1 name",
        format!("SELECT * FROM periods WHERE era_id = 'era-03' ORDER BY {name}, id"),
        &["periods_era_idx"],
    ));
    s.push(shape(
        "Q-P1 count",
        "SELECT count(*) FROM periods WHERE era_id = 'era-03'",
        &["periods_era_idx"],
    ));
    sorted(
        &mut s,
        "Q-P2",
        "periods",
        "start_mya{dir}, id",
        &["periods_start_mya_idx"],
    );
    sorted(
        &mut s,
        "Q-P3",
        "periods",
        &format!("{name}{{dir}}, id"),
        &["periods_name_sort_idx"],
    );
    s.push(shape(
        "Q-P2 count",
        "SELECT count(*) FROM periods",
        ANY_INDEX,
    ));
    s.push(shape(
        "Q-P4",
        "SELECT p.*, e.name AS era_name FROM periods p JOIN eras e ON e.id = p.era_id WHERE p.id = 'period-07'",
        &["periods_pk", "eras_pk"],
    ));

    // Taxonomy
    for (rank, parent) in RANKS {
        sorted(
            &mut s,
            &format!("Q-T1 {rank}"),
            rank,
            &format!("{name}{{dir}}, id"),
            &[&format!("{rank}_name_sort_idx")],
        );
        s.push(shape(
            format!("Q-T1 {rank} count"),
            format!("SELECT count(*) FROM {rank}"),
            ANY_INDEX,
        ));
        let Some((col, parent_table)) = parent else {
            continue;
        };
        let idx = format!("{rank}_{}_idx", col.trim_end_matches("_id"));
        let parent_id = format!("{}-1", parent_table_prefix(parent_table));
        s.push(shape(
            format!("Q-T2 {rank}"),
            format!(
                "SELECT * FROM {rank} WHERE {col} = '{parent_id}' ORDER BY {name}, id LIMIT 20"
            ),
            &[&idx],
        ));
        s.push(shape(
            format!("Q-T2 {rank} count"),
            format!("SELECT count(*) FROM {rank} WHERE {col} = '{parent_id}'"),
            &[&idx],
        ));
        s.push(shape(
            format!("Q-T3 {rank}"),
            format!(
                "SELECT c.*, p.name AS parent_name FROM {rank} c JOIN {parent_table} p ON p.id = c.{col} \
                 WHERE c.id = '{}-1'",
                parent_table_prefix(rank)
            ),
            &[&format!("{rank}_pk"), &format!("{parent_table}_pk")],
        ));
    }

    // Continents and countries
    s.push(shape(
        "Q-C1",
        format!("SELECT * FROM continents WHERE type = 'modern' ORDER BY {name}, id"),
        &["continents_type_idx"],
    ));
    s.push(shape(
        "Q-C1 count",
        "SELECT count(*) FROM continents WHERE type = 'modern'",
        &["continents_type_idx"],
    ));
    sorted(
        &mut s,
        "Q-C2",
        "continents",
        &format!("{name}{{dir}}, id"),
        &["continents_name_sort_idx"],
    );
    s.push(shape(
        "Q-C3",
        format!(
            "SELECT c.* FROM countries c JOIN country_continents cc ON cc.country_id = c.id \
             WHERE cc.continent_id = 'continent-3' ORDER BY {}, c.id LIMIT 20",
            ns("c.name")
        ),
        &["country_continents_continent_idx"],
    ));
    s.push(shape(
        "Q-C3 count",
        "SELECT count(*) FROM countries c JOIN country_continents cc ON cc.country_id = c.id \
         WHERE cc.continent_id = 'continent-3'",
        &["country_continents_continent_idx"],
    ));
    sorted(
        &mut s,
        "Q-K1",
        "countries",
        &format!("{name}{{dir}}, id"),
        &["countries_name_sort_idx"],
    );
    s.push(shape(
        "Q-K1 count",
        "SELECT count(*) FROM countries",
        ANY_INDEX,
    ));
    s.push(shape(
        "Q-K2",
        "SELECT * FROM countries WHERE id = 'country-17'",
        &["countries_pk"],
    ));
    s.push(shape(
        "Q-K2 continents",
        "SELECT cc.country_id, co.* FROM country_continents cc JOIN continents co ON co.id = cc.continent_id \
         WHERE cc.country_id = ANY(ARRAY['country-17', 'country-18'])",
        &["country_continents_pk"],
    ));

    // Species
    let species_count = |id: &str, filter: &str, allowed: &[&str]| {
        shape(
            format!("{id} count"),
            format!("SELECT count(*) FROM species s WHERE {filter}"),
            allowed,
        )
    };
    let page = |id: &str, filter: &str, allowed: &[&str]| {
        shape(
            id,
            format!(
                "SELECT * FROM species s WHERE {filter} ORDER BY {}, s.id LIMIT 20 OFFSET 40",
                ns("s.name")
            ),
            allowed,
        )
    };
    // EXISTS / IN filters: the planner may also walk the page's sort index and
    // probe primary keys (index-served, as data-model §6.1 allows for Q-S11);
    // the count variant proves the filter index itself is usable.
    let filtered_page = |id: &str, filter: &str, allowed: &[&str]| {
        let mut allowed = allowed.to_vec();
        allowed.push("species_name_sort_idx");
        page(id, filter, &allowed)
    };
    sorted(
        &mut s,
        "Q-S1",
        "species",
        &format!("{name}{{dir}}, id"),
        &["species_name_sort_idx"],
    );
    s.push(shape(
        "Q-S1 count",
        "SELECT count(*) FROM species",
        ANY_INDEX,
    ));
    sorted(
        &mut s,
        "Q-S2",
        "species",
        &format!("{}{{dir}}, id", ns("scientific_name")),
        &["species_scientific_name_sort_idx"],
    );
    s.push(shape(
        "Q-S3",
        "SELECT * FROM species ORDER BY discovery_year ASC NULLS LAST, id LIMIT 20 OFFSET 40",
        &["species_discovery_year_asc_idx"],
    ));
    s.push(shape(
        "Q-S3 DESC",
        "SELECT * FROM species ORDER BY discovery_year DESC NULLS LAST, id LIMIT 20 OFFSET 40",
        &["species_discovery_year_desc_idx"],
    ));
    s.push(page("Q-S4", "s.diet = 'piscivore'", &["species_diet_idx"]));
    s.push(species_count(
        "Q-S4",
        "s.diet = 'piscivore'",
        &["species_diet_idx"],
    ));
    s.push(page(
        "Q-S5",
        "s.genus_id = 'genus-42'",
        &["species_genus_idx"],
    ));
    s.push(species_count(
        "Q-S5",
        "s.genus_id = 'genus-42'",
        &["species_genus_idx"],
    ));

    let chain = [
        ("genera", "family_id", "families"),
        ("families", "order_id", "orders"),
        ("orders", "class_id", "classes"),
        ("classes", "phylum_id", "phyla"),
        ("phyla", "kingdom_id", "kingdoms"),
        ("kingdoms", "domain_id", "domains"),
    ];
    for depth in 1..=chain.len() {
        let (_, col, top) = chain[depth - 1];
        let mut sub = String::from("SELECT g.id FROM genera g");
        for (i, (_, c, parent)) in chain.iter().enumerate().take(depth - 1) {
            let alias_child = if i == 0 {
                "g".to_string()
            } else {
                format!("t{i}")
            };
            sub.push_str(&format!(
                " JOIN {parent} t{} ON t{}.id = {alias_child}.{c}",
                i + 1,
                i + 1
            ));
        }
        let last = if depth == 1 {
            "g".to_string()
        } else {
            format!("t{}", depth - 1)
        };
        sub.push_str(&format!(
            " WHERE {last}.{col} = '{}-2'",
            parent_table_prefix(top)
        ));
        let mut allowed: Vec<String> = vec!["species_genus_idx".into()];
        for (child, c, _) in chain.iter().take(depth) {
            allowed.push(format!("{child}_{}_idx", c.trim_end_matches("_id")));
        }
        let allowed: Vec<&str> = allowed.iter().map(String::as_str).collect();
        let filter = format!("s.genus_id IN ({sub})");
        s.push(filtered_page(&format!("Q-S6 {top}"), &filter, &allowed));
        s.push(species_count(&format!("Q-S6 {top}"), &filter, &allowed));
    }

    let exists = |table: &str, col: &str, value: &str| {
        format!(
            "EXISTS (SELECT 1 FROM {table} l WHERE l.species_id = s.id AND l.{col} = '{value}')"
        )
    };
    let f = exists("species_periods", "period_id", "period-07");
    s.push(filtered_page("Q-S7", &f, &["species_periods_period_idx"]));
    s.push(species_count("Q-S7", &f, &["species_periods_period_idx"]));
    let f = "EXISTS (SELECT 1 FROM species_periods sp JOIN periods p ON p.id = sp.period_id \
             AND p.era_id = 'era-03' WHERE sp.species_id = s.id)";
    s.push(filtered_page(
        "Q-S8",
        f,
        &["periods_era_idx", "species_periods_period_idx"],
    ));
    s.push(species_count(
        "Q-S8",
        f,
        &["periods_era_idx", "species_periods_period_idx"],
    ));
    let f = exists("species_continents", "continent_id", "continent-5");
    s.push(filtered_page(
        "Q-S9",
        &f,
        &["species_continents_continent_idx"],
    ));
    s.push(species_count(
        "Q-S9",
        &f,
        &["species_continents_continent_idx"],
    ));
    let f = exists("species_countries", "country_id", "country-17");
    s.push(filtered_page(
        "Q-S10",
        &f,
        &["species_countries_country_idx"],
    ));
    s.push(species_count(
        "Q-S10",
        &f,
        &["species_countries_country_idx"],
    ));

    for term in ["rax", "saurus"] {
        let f = q_filter(term);
        s.push(page(
            &format!("Q-S11 {term}"),
            &f,
            &[
                "species_name_trgm_idx",
                "species_scientific_name_trgm_idx",
                "species_name_sort_idx",
            ],
        ));
        s.push(species_count(
            &format!("Q-S11 {term}"),
            &f,
            &["species_name_trgm_idx", "species_scientific_name_trgm_idx"],
        ));
    }

    let ids = "ARRAY['species-1', 'species-2', 'species-3', 'species-500', 'species-9999']";
    s.push(shape(
        "Q-S12 periods",
        format!(
            "SELECT sp.species_id, p.*, e.name AS era_name FROM species_periods sp \
             JOIN periods p ON p.id = sp.period_id JOIN eras e ON e.id = p.era_id \
             WHERE sp.species_id = ANY({ids})"
        ),
        &["species_periods_pk"],
    ));
    s.push(shape(
        "Q-S12 continents",
        format!(
            "SELECT sc.species_id, c.* FROM species_continents sc JOIN continents c ON c.id = sc.continent_id \
             WHERE sc.species_id = ANY({ids})"
        ),
        &["species_continents_pk"],
    ));
    s.push(shape(
        "Q-S12 countries",
        format!(
            "SELECT sk.species_id, c.* FROM species_countries sk JOIN countries c ON c.id = sk.country_id \
             WHERE sk.species_id = ANY({ids})"
        ),
        &["species_countries_pk"],
    ));
    s.push(shape(
        "Q-S12 lineage",
        "SELECT g.id, f.id, o.id, c.id, p.id, k.id, d.id FROM genera g \
         JOIN families f ON f.id = g.family_id JOIN orders o ON o.id = f.order_id \
         JOIN classes c ON c.id = o.class_id JOIN phyla p ON p.id = c.phylum_id \
         JOIN kingdoms k ON k.id = p.kingdom_id JOIN domains d ON d.id = k.domain_id \
         WHERE g.id = ANY(ARRAY['genus-1', 'genus-2', 'genus-4999'])",
        &["genera_pk"],
    ));
    s.push(shape(
        "Q-S13",
        "SELECT * FROM species WHERE id = 'species-4242'",
        &["species_pk"],
    ));
    s
}

fn parent_table_prefix(table: &str) -> &'static str {
    match table {
        "domains" => "domain",
        "kingdoms" => "kingdom",
        "phyla" => "phylum",
        "classes" => "class",
        "orders" => "order",
        "families" => "family",
        "genera" => "genus",
        t => panic!("not a rank: {t}"),
    }
}

/// Spec 002 `q` filter: escaped term inside `%…%`, matched with ILIKE.
fn q_filter(term: &str) -> String {
    let escaped = term
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    format!(
        "(s.name ILIKE '%{escaped}%' ESCAPE '\\' OR s.scientific_name ILIKE '%{escaped}%' ESCAPE '\\')"
    )
}

#[sqlx::test]
async fn every_public_shape_can_use_an_index(pool: PgPool) {
    load(&pool).await;
    let tables = all_tables();
    let mut failures = Vec::new();
    for sh in public_shapes() {
        let plan = explain(&pool, &sh.sql, true).await;
        let allowed: Vec<&str> = sh.allowed.iter().map(String::as_str).collect();
        let result = std::panic::catch_unwind(|| {
            if allowed == ANY_INDEX {
                assert!(plan.contains("Index"), "{}: no index used:\n{plan}", sh.id);
            } else {
                assert_uses_any_index(&sh.id, &plan, &allowed);
            }
            assert_no_seq_scan_on(&sh.id, &plan, &tables);
        });
        if result.is_err() {
            failures.push(format!(
                "{}\n  allowed: {allowed:?}\n  sql: {}\n{plan}",
                sh.id, sh.sql
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} shape(s) failed:\n\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}

#[sqlx::test]
async fn selective_species_shapes_avoid_seq_scan_by_default(pool: PgPool) {
    load(&pool).await;
    let shapes = public_shapes();
    // Q-S7 and Q-S10 are not listed: with 10,000 species (200 heap pages) the
    // planner correctly prefers Seq Scan + hash join for their EXISTS filters
    // (3.3% and 1% of rows, ≈ 3 ms). Both are still verified as index-servable
    // in `every_public_shape_can_use_an_index`.
    for id in ["Q-S1", "Q-S13", "Q-S5"] {
        let sh = shapes.iter().find(|s| s.id == id).unwrap();
        let plan = explain(&pool, &sh.sql, false).await;
        assert_no_seq_scan_on(id, &plan, &["species"]);
    }
}

#[sqlx::test]
async fn every_integrity_lookup_uses_its_index(pool: PgPool) {
    load(&pool).await;
    let mut lookups: Vec<Shape> = vec![
        shape(
            "era → periods",
            "SELECT 1 FROM periods WHERE era_id = 'era-03'",
            &["periods_era_idx"],
        ),
        shape(
            "genus → species",
            "SELECT 1 FROM species WHERE genus_id = 'genus-42'",
            &["species_genus_idx"],
        ),
        shape(
            "period → species",
            "SELECT 1 FROM species_periods WHERE period_id = 'period-07'",
            &["species_periods_period_idx"],
        ),
        shape(
            "continent → species",
            "SELECT 1 FROM species_continents WHERE continent_id = 'continent-5'",
            &["species_continents_continent_idx"],
        ),
        shape(
            "country → species",
            "SELECT 1 FROM species_countries WHERE country_id = 'country-17'",
            &["species_countries_country_idx"],
        ),
        shape(
            "continent → countries",
            "SELECT 1 FROM country_continents WHERE continent_id = 'continent-3'",
            &["country_continents_continent_idx"],
        ),
        shape(
            "cascade species periods",
            "SELECT 1 FROM species_periods WHERE species_id = 'species-7'",
            &["species_periods_pk"],
        ),
        shape(
            "cascade species continents",
            "SELECT 1 FROM species_continents WHERE species_id = 'species-7'",
            &["species_continents_pk"],
        ),
        shape(
            "cascade species countries",
            "SELECT 1 FROM species_countries WHERE species_id = 'species-7'",
            &["species_countries_pk"],
        ),
        shape(
            "cascade country continents",
            "SELECT 1 FROM country_continents WHERE country_id = 'country-17'",
            &["country_continents_pk"],
        ),
        shape(
            "composite FK target",
            "SELECT 1 FROM continents WHERE id = 'continent-3' AND type = 'modern'",
            &["continents_id_type_uq", "continents_pk"],
        ),
        shape(
            "era overlap",
            "SELECT 1 FROM eras WHERE numrange(end_mya, start_mya, '[)') && numrange(10, 20, '[)')",
            &["eras_range_ex"],
        ),
        shape(
            "period overlap",
            "SELECT 1 FROM periods WHERE numrange(end_mya, start_mya, '[)') && numrange(10, 20, '[)')",
            &["periods_range_ex"],
        ),
        shape(
            "containment",
            "SELECT 1 FROM periods WHERE era_id = 'era-03' AND (start_mya > 800 OR end_mya < 700)",
            &["periods_era_idx"],
        ),
        shape(
            "scientific name unique",
            "SELECT 1 FROM species WHERE lower(scientific_name) = 'x'",
            &["species_scientific_name_uq"],
        ),
    ];
    for (rank, parent) in RANKS {
        lookups.push(shape(
            format!("{rank} name unique"),
            format!("SELECT 1 FROM {rank} WHERE lower(name) = 'x'"),
            &[&format!("{rank}_name_uq")],
        ));
        if let Some((col, parent_table)) = parent {
            lookups.push(shape(
                format!("{parent_table} → {rank}"),
                format!(
                    "SELECT 1 FROM {rank} WHERE {col} = '{}-1'",
                    parent_table_prefix(parent_table)
                ),
                &[&format!("{rank}_{}_idx", col.trim_end_matches("_id"))],
            ));
        }
    }
    for t in ["eras", "periods", "continents", "countries"] {
        lookups.push(shape(
            format!("{t} name unique"),
            format!("SELECT 1 FROM {t} WHERE lower(name) = 'x'"),
            &[&format!("{t}_name_uq")],
        ));
    }
    let tables = all_tables();
    for sh in lookups {
        let plan = explain(&pool, &sh.sql, true).await;
        let allowed: Vec<&str> = sh.allowed.iter().map(String::as_str).collect();
        assert_uses_any_index(&sh.id, &plan, &allowed);
        assert_no_seq_scan_on(&sh.id, &plan, &tables);
    }
}

#[sqlx::test]
async fn substring_search_is_index_selective(pool: PgPool) {
    load(&pool).await;
    for term in ["rax", "saurus"] {
        let sql = format!("SELECT count(*) FROM species s WHERE {}", q_filter(term));
        let plan = explain_analyze(&pool, &sql, true).await;
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
        let n: i64 = sqlx::query_scalar(AssertSqlSafe(sql.clone()))
            .fetch_one(&pool)
            .await
            .unwrap();
        assert!(
            (100..=400).contains(&n),
            "{term} should match 1–4% of species, matched {n}"
        );
    }
}

#[sqlx::test]
async fn punctuation_only_search_meets_latency_target(pool: PgPool) {
    load(&pool).await;
    let f = q_filter("%%_");
    let page = format!(
        "SELECT * FROM species s WHERE {f} ORDER BY {}, s.id LIMIT 20",
        ns("s.name")
    );
    let count = format!("SELECT count(*) FROM species s WHERE {f}");
    for sql in [page, count] {
        exec(&pool, sql.clone()).await.unwrap(); // warm-up
        let mut times: Vec<Duration> = Vec::with_capacity(20);
        for _ in 0..20 {
            let t = Instant::now();
            exec(&pool, sql.clone()).await.unwrap();
            times.push(t.elapsed());
        }
        times.sort();
        let p95 = times[18];
        assert!(p95 < Duration::from_millis(50), "p95 {p95:?} for {sql}");
    }
}
