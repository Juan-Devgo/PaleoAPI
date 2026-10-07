//! Taxonomy hierarchy: required parents and delete protection (AC 3, AC 10).

use sqlx::PgPool;

use crate::support::{RANKS, accepts, rank, rejects};

/// Two parallel chains `a-<table>` and `b-<table>` from domains down.
async fn two_chains(pool: &PgPool) {
    for prefix in ["a", "b"] {
        let mut parent: Option<String> = None;
        for (table, _) in RANKS {
            let id = format!("{prefix}-{table}");
            rank(pool, table, &id, parent.as_deref()).await.unwrap();
            parent = Some(id);
        }
    }
}

#[sqlx::test]
async fn every_rank_below_domain_needs_an_existing_parent(pool: PgPool) {
    two_chains(&pool).await;
    for (table, parent) in RANKS {
        let Some((col, parent_table)) = parent else {
            continue;
        };
        let fk = format!("{table}_{}_fk", col.trim_end_matches("_id"));
        rejects(
            &pool,
            format!("INSERT INTO {table} (id, name, {col}) VALUES ('new', 'New', NULL)"),
            "23502",
            None,
        )
        .await;
        rejects(
            &pool,
            format!("INSERT INTO {table} (id, name, {col}) VALUES ('new', 'New', 'missing')"),
            "23503",
            Some(&fk),
        )
        .await;
        accepts(
            &pool,
            format!(
                "INSERT INTO {table} (id, name, {col}) VALUES ('new', 'New', 'a-{parent_table}')"
            ),
        )
        .await;
    }
}

#[sqlx::test]
async fn reparenting_requires_an_existing_parent(pool: PgPool) {
    two_chains(&pool).await;
    for (table, parent) in RANKS {
        let Some((col, parent_table)) = parent else {
            continue;
        };
        let fk = format!("{table}_{}_fk", col.trim_end_matches("_id"));
        rejects(
            &pool,
            format!("UPDATE {table} SET {col} = 'missing' WHERE id = 'a-{table}'"),
            "23503",
            Some(&fk),
        )
        .await;
        rejects(
            &pool,
            format!("UPDATE {table} SET {col} = NULL WHERE id = 'a-{table}'"),
            "23502",
            None,
        )
        .await;
        accepts(
            &pool,
            format!("UPDATE {table} SET {col} = 'b-{parent_table}' WHERE id = 'a-{table}'"),
        )
        .await;
    }
}

#[sqlx::test]
async fn deleting_a_rank_with_children_is_rejected(pool: PgPool) {
    two_chains(&pool).await;
    for (table, parent) in RANKS.iter().rev() {
        let Some((col, parent_table)) = parent else {
            continue;
        };
        let fk = format!("{table}_{}_fk", col.trim_end_matches("_id"));
        rejects(
            &pool,
            format!("DELETE FROM {parent_table} WHERE id = 'a-{parent_table}'"),
            "23001",
            Some(&fk),
        )
        .await;
        // Deleting the leaf works, which frees the parent for the next round.
        accepts(&pool, format!("DELETE FROM {table} WHERE id = 'a-{table}'")).await;
    }
    accepts(&pool, "DELETE FROM domains WHERE id = 'a-domains'").await;
}

#[sqlx::test]
async fn domains_have_no_parent_column(pool: PgPool) {
    let cols: Vec<String> = sqlx::query_scalar(
        "SELECT attname::text FROM pg_attribute WHERE attrelid = 'domains'::regclass
           AND attnum > 0 AND NOT attisdropped ORDER BY attnum",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(cols, ["id", "name"]);
}
