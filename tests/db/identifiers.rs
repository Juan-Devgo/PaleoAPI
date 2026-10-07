//! Identifiers, names, text rules, and the TRUNCATE guard over every table
//! (AC 7, AC 8, spec §Integrity).

use std::sync::atomic::{AtomicU32, Ordering};

use sqlx::PgPool;

use crate::support::{
    LINK_TABLES, RESOURCE_TABLES, accepts, assert_violation, continent, era, exec, full_chain,
    genus_chain, period, rejects, snapshot_all,
};

/// Parents every resource table's valid insert can reference.
async fn parents(pool: &PgPool) {
    era(pool, "host-era", "4600", "4000").await.unwrap();
    period(pool, "host-period", "host-era", "4600", "4599")
        .await
        .unwrap();
    genus_chain(pool, "host").await;
    continent(pool, "host-continent", "modern").await.unwrap();
}

/// Distinct, non-overlapping one-thousandth ranges for era and period inserts:
/// eras within 0–3999, periods within 4000–4598 (inside `host-era`).
static RANGE: AtomicU32 = AtomicU32::new(0);

fn next_range(base: u32) -> (String, String) {
    let n = RANGE.fetch_add(1, Ordering::Relaxed) % 598_000;
    let end = u64::from(base) * 1000 + u64::from(n);
    let fmt = |v: u64| format!("{}.{:03}", v / 1000, v % 1000);
    (fmt(end + 1), fmt(end))
}

/// A single statement that inserts one valid row (plus required links) into `table`.
fn insert(table: &str, id: &str, name: &str) -> String {
    let id = id.replace('\'', "''");
    let name = name.replace('\'', "''");
    let parent_of = |t: &str| match t {
        "kingdoms" => Some(("domain_id", "host-domains")),
        "phyla" => Some(("kingdom_id", "host-kingdoms")),
        "classes" => Some(("phylum_id", "host-phyla")),
        "orders" => Some(("class_id", "host-classes")),
        "families" => Some(("order_id", "host-orders")),
        "genera" => Some(("family_id", "host-families")),
        _ => None,
    };
    match table {
        "eras" => {
            let (s, e) = next_range(0);
            format!("INSERT INTO eras VALUES ('{id}', '{name}', {s}, {e})")
        }
        "periods" => {
            let (s, e) = next_range(4000);
            format!("INSERT INTO periods VALUES ('{id}', 'host-era', '{name}', {s}, {e})")
        }
        "domains" => format!("INSERT INTO domains VALUES ('{id}', '{name}')"),
        "continents" => format!("INSERT INTO continents VALUES ('{id}', '{name}', 'prehistoric')"),
        "countries" => format!(
            "WITH c AS (INSERT INTO countries VALUES ('{id}', '{name}') RETURNING id) \
             INSERT INTO country_continents (country_id, continent_id) SELECT id, 'host-continent' FROM c"
        ),
        "species" => format!(
            "WITH s AS (INSERT INTO species (id, genus_id, name, scientific_name, diet, description) \
             VALUES ('{id}', 'host-genera', '{name}', 'sci {id}', 'herbivore', 'Text.') RETURNING id) \
             INSERT INTO species_periods SELECT id, 'host-period' FROM s"
        ),
        t => {
            let (col, parent) = parent_of(t).unwrap_or_else(|| panic!("unknown table {t}"));
            format!("INSERT INTO {t} (id, name, {col}) VALUES ('{id}', '{name}', '{parent}')")
        }
    }
}

#[sqlx::test]
async fn ids_must_be_slugs(pool: PgPool) {
    parents(&pool).await;
    let too_long = format!("a{}", "-b".repeat(32));
    assert_eq!(too_long.len(), 65);
    let max = "a".repeat(64);
    for t in RESOURCE_TABLES {
        let ck = format!("{t}_id_ck");
        for bad in [
            "A",
            "a",
            "-ab",
            "ab-",
            "a--b",
            "Ab",
            "a b",
            "a_b",
            "ñu",
            too_long.as_str(),
        ] {
            rejects(&pool, insert(t, bad, "Valid name"), "23514", Some(&ck)).await;
        }
        accepts(&pool, insert(t, &max, &format!("{t} max"))).await;
        accepts(&pool, insert(t, "a1-b2", &format!("{t} short"))).await;
    }
}

#[sqlx::test]
async fn ids_are_unique_and_immutable(pool: PgPool) {
    parents(&pool).await;
    for t in RESOURCE_TABLES {
        accepts(&pool, insert(t, "same-id", &format!("{t} one"))).await;
        rejects(
            &pool,
            insert(t, "same-id", &format!("{t} two")),
            "23505",
            Some(&format!("{t}_pk")),
        )
        .await;
        rejects(
            &pool,
            format!("UPDATE {t} SET id = 'other-id' WHERE id = 'same-id'"),
            "23514",
            Some(&format!("{t}_id_immutable_ck")),
        )
        .await;
        accepts(
            &pool,
            format!("UPDATE {t} SET id = 'same-id' WHERE id = 'same-id'"),
        )
        .await;
    }
}

#[sqlx::test]
async fn names_are_trimmed_bounded_text(pool: PgPool) {
    parents(&pool).await;
    for t in RESOURCE_TABLES {
        let ck = format!("{t}_name_ck");
        for bad in [
            String::new(),
            " x".into(),
            "x ".into(),
            "x\u{a0}".into(),
            "\tx".into(),
            "x\n".into(),
            "a".repeat(65),
        ] {
            rejects(&pool, insert(t, "bad-name", &bad), "23514", Some(&ck)).await;
        }
        accepts(&pool, insert(t, "max-name", &"ñ".repeat(64))).await;
        accepts(&pool, insert(t, "inner-space", "Two  words")).await;
    }
}

#[sqlx::test]
async fn names_are_unique_ignoring_case_except_species(pool: PgPool) {
    parents(&pool).await;
    for t in RESOURCE_TABLES {
        accepts(&pool, insert(t, "first", "Mesozoic")).await;
        if t == "species" {
            accepts(&pool, insert(t, "second", "MESOZOIC")).await;
            accepts(&pool, insert(t, "third", "Mesozoic")).await;
        } else {
            rejects(
                &pool,
                insert(t, "second", "MESOZOIC"),
                "23505",
                Some(&format!("{t}_name_uq")),
            )
            .await;
            rejects(
                &pool,
                insert(t, "second", "Mesozoic"),
                "23505",
                Some(&format!("{t}_name_uq")),
            )
            .await;
        }
        // Renaming a row to a different casing of its own name is allowed (AC 8).
        accepts(
            &pool,
            format!("UPDATE {t} SET name = 'MESOZOIC' WHERE id = 'first'"),
        )
        .await;
    }
}

#[sqlx::test]
async fn truncate_is_always_rejected(pool: PgPool) {
    full_chain(&pool, "tr").await;
    let before = snapshot_all(&pool).await;
    for t in RESOURCE_TABLES {
        assert_violation(exec(&pool, format!("TRUNCATE {t}")).await, "0A000", None);
        assert_violation(
            exec(&pool, format!("TRUNCATE {t} CASCADE")).await,
            "23514",
            Some(&format!("{t}_no_truncate_ck")),
        );
    }
    for t in LINK_TABLES {
        assert_violation(
            exec(&pool, format!("TRUNCATE {t}")).await,
            "23514",
            Some(&format!("{t}_no_truncate_ck")),
        );
    }
    assert_eq!(snapshot_all(&pool).await, before);
}
