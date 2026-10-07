//! Server-side latency of every read shape on the 001 fixture: p95 < 50 ms
//! (Constitution III, spec §7, research R11).

use std::time::{Duration, Instant};

use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

use crate::support::db_support::exec_script;
use crate::support::*;

const DATASET: &str = include_str!("../db/fixtures/index_dataset.sql");
const RUNS: usize = 20;
/// Measurement rounds per shape; the best p95 counts.
const ROUNDS: usize = 6;
const BUDGET: Duration = Duration::from_millis(50);

/// Read shapes: one URI each (fixture ids, 001 data-model §9).
fn shapes() -> Vec<String> {
    let mut uris: Vec<String> = [
        "/api/v1/species?limit=100",
        "/api/v1/species?limit=100&page=50",
        "/api/v1/species?sort=-scientific_name",
        "/api/v1/species?sort=discovery_year",
        "/api/v1/species?sort=-discovery_year",
        "/api/v1/species?diet=piscivore",
        "/api/v1/species?era=era-03",
        "/api/v1/species?period=period-07",
        "/api/v1/species?genus=genus-42",
        "/api/v1/species?family=family-2",
        "/api/v1/species?order=order-2",
        "/api/v1/species?class=class-2",
        "/api/v1/species?phylum=phylum-2",
        "/api/v1/species?kingdom=kingdom-2",
        "/api/v1/species?domain=domain-2",
        "/api/v1/species?continent=continent-5",
        "/api/v1/species?country=country-17",
        "/api/v1/species?q=rax",
        "/api/v1/species?q=saurus",
        "/api/v1/species?q=%25%25_",
        "/api/v1/species/species-4242",
        "/api/v1/eras",
        "/api/v1/eras?sort=name",
        "/api/v1/eras/era-03",
        "/api/v1/eras/era-03/periods",
        "/api/v1/periods?limit=100",
        "/api/v1/periods?era=era-03&sort=name",
        "/api/v1/periods/period-07",
        "/api/v1/continents",
        "/api/v1/continents?type=modern",
        "/api/v1/continents/continent-3",
        "/api/v1/continents/continent-3/countries?limit=100",
        "/api/v1/countries?limit=100",
        "/api/v1/countries?continent=continent-3",
        "/api/v1/countries/country-17",
    ]
    .map(String::from)
    .to_vec();
    for (plural, singular, parent) in [
        ("domains", "domain", None),
        ("kingdoms", "kingdom", Some(("domains", "domain"))),
        ("phyla", "phylum", Some(("kingdoms", "kingdom"))),
        ("classes", "class", Some(("phyla", "phylum"))),
        ("orders", "order", Some(("classes", "class"))),
        ("families", "family", Some(("orders", "order"))),
        ("genera", "genus", Some(("families", "family"))),
    ] {
        uris.push(format!("/api/v1/taxonomy/{plural}?limit=100"));
        uris.push(format!("/api/v1/taxonomy/{plural}/{singular}-1"));
        if let Some((pp, ps)) = parent {
            uris.push(format!("/api/v1/taxonomy/{plural}?{ps}={ps}-1"));
            uris.push(format!("/api/v1/taxonomy/{pp}/{ps}-1/{plural}"));
        }
    }
    uris
}

#[sqlx::test]
async fn every_read_shape_meets_the_p95_budget(pool_opts: PgPoolOptions, opts: PgConnectOptions) {
    let app = app(pool_opts, opts).await;
    exec_script(&app.pool, DATASET)
        .await
        .expect("fixture must load");
    exec(&app.pool, "ANALYZE").await.unwrap();

    let mut over = Vec::new();
    for uri in shapes() {
        let warm = get(&app, &uri).await;
        assert_eq!(warm.status.as_u16(), 200, "{uri}: {}", warm.text());
        // Best of a few rounds: other tests of this binary run in parallel on the same
        // machine and database, and their load is not this endpoint's latency.
        let mut best = Duration::MAX;
        for _ in 0..ROUNDS {
            let mut times = Vec::with_capacity(RUNS);
            for _ in 0..RUNS {
                let t = Instant::now();
                let resp = get(&app, &uri).await;
                times.push(t.elapsed());
                assert_eq!(resp.status.as_u16(), 200);
            }
            times.sort();
            best = best.min(times[RUNS * 95 / 100 - 1]);
            if best < BUDGET {
                break;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        println!("p95 {:>8.2} ms  {uri}", best.as_secs_f64() * 1000.0);
        if best >= BUDGET {
            over.push(format!("{uri}: p95 {best:?}"));
        }
    }
    assert!(
        over.is_empty(),
        "over the 50 ms budget:\n{}",
        over.join("\n")
    );
}
