//! Tests that create the same id run in parallel without interfering
//! (AC 12, FR-014): each `#[sqlx::test]` gets its own database.

use sqlx::PgPool;

use crate::support::count;

async fn insert_shared_era(pool: &PgPool) {
    sqlx::query("INSERT INTO eras VALUES ('shared-id', 'Shared', 10.0, 0)")
        .execute(pool)
        .await
        .unwrap();
    // Give the other test time to run concurrently.
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert_eq!(count(pool, "SELECT count(*) FROM eras").await, 1);
}

#[sqlx::test]
async fn first_test_creates_shared_id(pool: PgPool) {
    insert_shared_era(&pool).await;
}

#[sqlx::test]
async fn second_test_creates_shared_id(pool: PgPool) {
    insert_shared_era(&pool).await;
}
