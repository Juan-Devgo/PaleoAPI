//! Eras and periods: ranges, precision, overlaps, containment, deletes
//! (AC 3–6, AC 10, FR-009).

use sqlx::PgPool;

use crate::support::{accepts, era, period, rejects};

const CK: &str = "23514";

#[sqlx::test]
async fn adjacent_eras_store_exact_decimals(pool: PgPool) {
    era(&pool, "mesozoic", "251.902", "66.0").await.unwrap();
    era(&pool, "cenozoic", "66.0", "0").await.unwrap();
    let stored: Vec<(String, String)> = sqlx::query_as(
        "SELECT start_mya::text AS s, end_mya::text AS e FROM eras ORDER BY start_mya DESC",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        stored,
        vec![
            ("251.902".into(), "66.0".into()),
            ("66.0".into(), "0".into())
        ]
    );
}

#[sqlx::test]
async fn overlapping_eras_are_rejected(pool: PgPool) {
    era(&pool, "mesozoic", "251.902", "66.0").await.unwrap();
    rejects(
        &pool,
        "INSERT INTO eras VALUES ('overlap', 'Overlap', 70.0, 0)",
        "23P01",
        Some("eras_range_ex"),
    )
    .await;
    accepts(
        &pool,
        "INSERT INTO eras VALUES ('cenozoic', 'Cenozoic', 66.0, 0)",
    )
    .await;
    // A gap is allowed too.
    accepts(&pool, "INSERT INTO eras VALUES ('old', 'Old', 600, 541)").await;
}

async fn range_and_precision_rules(
    pool: &PgPool,
    table: &str,
    insert: impl Fn(&str, &str, &str) -> String,
) {
    for (start, end) in [("10", "10"), ("5", "10")] {
        rejects(
            pool,
            insert("bad", start, end),
            CK,
            Some(&format!("{table}_range_ck")),
        )
        .await;
    }
    for start in ["4600.001", "66.0001", "'NaN'", "'Infinity'"] {
        rejects(
            pool,
            insert("bad", start, "0"),
            CK,
            Some(&format!("{table}_start_mya_ck")),
        )
        .await;
    }
    for end in ["-0.001", "66.0001", "'NaN'", "'Infinity'", "'-Infinity'"] {
        rejects(
            pool,
            insert("bad", "100", end),
            CK,
            Some(&format!("{table}_end_mya_ck")),
        )
        .await;
    }
    accepts(pool, insert("good", "4600", "4599.999")).await;
}

#[sqlx::test]
async fn era_range_and_precision_rules(pool: PgPool) {
    range_and_precision_rules(&pool, "eras", |id, s, e| {
        format!("INSERT INTO eras VALUES ('{id}', '{id}', {s}, {e})")
    })
    .await;
}

async fn container(pool: &PgPool) {
    era(pool, "all", "4600", "0").await.unwrap();
}

#[sqlx::test]
async fn period_range_and_precision_rules(pool: PgPool) {
    container(&pool).await;
    range_and_precision_rules(&pool, "periods", |id, s, e| {
        format!("INSERT INTO periods VALUES ('{id}', 'all', '{id}', {s}, {e})")
    })
    .await;
    let stored: String = sqlx::query_scalar("SELECT start_mya::text FROM periods")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(stored, "4600");
}

#[sqlx::test]
async fn period_values_are_stored_exactly(pool: PgPool) {
    era(&pool, "mesozoic", "251.902", "66.0").await.unwrap();
    period(&pool, "triassic", "mesozoic", "251.902", "201.4")
        .await
        .unwrap();
    let stored: (String, String) =
        sqlx::query_as("SELECT start_mya::text, end_mya::text FROM periods")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(stored, ("251.902".into(), "201.4".into()));
}

#[sqlx::test]
async fn sibling_periods_cannot_overlap(pool: PgPool) {
    era(&pool, "mesozoic", "251.902", "66.0").await.unwrap();
    period(&pool, "triassic", "mesozoic", "251.902", "201.4")
        .await
        .unwrap();
    rejects(
        &pool,
        "INSERT INTO periods VALUES ('overlap', 'mesozoic', 'Overlap', 210, 190)",
        "23P01",
        Some("periods_range_ex"),
    )
    .await;
    accepts(
        &pool,
        "INSERT INTO periods VALUES ('jurassic', 'mesozoic', 'Jurassic', 201.4, 145)",
    )
    .await;
}

#[sqlx::test]
async fn period_needs_an_existing_era(pool: PgPool) {
    container(&pool).await;
    rejects(
        &pool,
        "INSERT INTO periods VALUES ('pp', NULL, 'P', 10, 0)",
        "23502",
        None,
    )
    .await;
    rejects(
        &pool,
        "INSERT INTO periods VALUES ('pp', 'missing', 'P', 10, 0)",
        "23503",
        Some("periods_era_fk"),
    )
    .await;
    accepts(
        &pool,
        "INSERT INTO periods VALUES ('pp', 'all', 'P', 10, 0)",
    )
    .await;
}

#[sqlx::test]
async fn period_must_lie_within_its_era(pool: PgPool) {
    era(&pool, "mesozoic", "251.902", "66.0").await.unwrap();
    for (s, e) in [("260", "240"), ("70", "60"), ("300", "0")] {
        rejects(
            &pool,
            format!("INSERT INTO periods VALUES ('out', 'mesozoic', 'Out', {s}, {e})"),
            "23514",
            Some("periods_within_era_ck"),
        )
        .await;
    }
    // Boundaries may coincide.
    accepts(
        &pool,
        "INSERT INTO periods VALUES ('all-of-it', 'mesozoic', 'All', 251.902, 66.0)",
    )
    .await;
}

#[sqlx::test]
async fn period_update_must_stay_within_era(pool: PgPool) {
    era(&pool, "mesozoic", "251.902", "66.0").await.unwrap();
    period(&pool, "triassic", "mesozoic", "251.902", "201.4")
        .await
        .unwrap();
    rejects(
        &pool,
        "UPDATE periods SET start_mya = 260 WHERE id = 'triassic'",
        "23514",
        Some("periods_within_era_ck"),
    )
    .await;
    accepts(
        &pool,
        "UPDATE periods SET end_mya = 201.5 WHERE id = 'triassic'",
    )
    .await;
}

#[sqlx::test]
async fn shrinking_an_era_around_its_periods_is_rejected(pool: PgPool) {
    era(&pool, "mesozoic", "251.902", "66.0").await.unwrap();
    period(&pool, "cretaceous", "mesozoic", "145", "66.0")
        .await
        .unwrap();
    rejects(
        &pool,
        "UPDATE eras SET end_mya = 100 WHERE id = 'mesozoic'",
        "23514",
        Some("eras_contains_periods_ck"),
    )
    .await;
    rejects(
        &pool,
        "UPDATE eras SET start_mya = 140 WHERE id = 'mesozoic'",
        "23514",
        Some("eras_contains_periods_ck"),
    )
    .await;
    accepts(
        &pool,
        "UPDATE eras SET start_mya = 150 WHERE id = 'mesozoic'",
    )
    .await;
    accepts(
        &pool,
        "UPDATE eras SET start_mya = 300 WHERE id = 'mesozoic'",
    )
    .await;
}

#[sqlx::test]
async fn moving_a_period_requires_a_containing_era(pool: PgPool) {
    era(&pool, "mesozoic", "251.902", "66.0").await.unwrap();
    era(&pool, "cenozoic", "66.0", "0").await.unwrap();
    period(&pool, "cretaceous", "mesozoic", "145", "66.0")
        .await
        .unwrap();
    rejects(
        &pool,
        "UPDATE periods SET era_id = 'cenozoic' WHERE id = 'cretaceous'",
        "23514",
        Some("periods_within_era_ck"),
    )
    .await;
    accepts(
        &pool,
        "UPDATE periods SET era_id = 'cenozoic', start_mya = 66.0, end_mya = 23.03 WHERE id = 'cretaceous'",
    )
    .await;
}

#[sqlx::test]
async fn deleting_an_era_with_periods_is_rejected(pool: PgPool) {
    era(&pool, "mesozoic", "251.902", "66.0").await.unwrap();
    era(&pool, "cenozoic", "66.0", "0").await.unwrap();
    period(&pool, "cretaceous", "mesozoic", "145", "66.0")
        .await
        .unwrap();
    rejects(
        &pool,
        "DELETE FROM eras WHERE id = 'mesozoic'",
        "23001",
        Some("periods_era_fk"),
    )
    .await;
    accepts(&pool, "DELETE FROM eras WHERE id = 'cenozoic'").await;
    accepts(&pool, "DELETE FROM periods WHERE id = 'cretaceous'").await;
    accepts(&pool, "DELETE FROM eras WHERE id = 'mesozoic'").await;
}
