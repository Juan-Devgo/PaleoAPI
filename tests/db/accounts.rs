//! Table `accounts`: constraints, credential-version trigger, TRUNCATE guard, and the
//! login and write-check lookups (003 data-model §1; AC 24, FR-005, FR-006).

use paleo_api::security::accounts::{
    LoginAccount, WriteCheckAccount, find_for_login, find_for_write_check,
};
use sqlx::PgPool;

use crate::support::{accepts, rejects};

/// A well-formed Argon2id PHC string (shape only; not a hash of any password).
const HASH: &str = "$argon2id$v=19$m=19456,t=2,p=1$c2FsdHNhbHRzYWx0$aGFzaGhhc2hoYXNoaGFzaA";
const OTHER_HASH: &str = "$argon2id$v=19$m=19456,t=2,p=1$b3RoZXJzYWx0$b3RoZXJoYXNoaGFzaA";

fn insert(username: &str, hash: &str, role: Option<&str>) -> String {
    let role = role.map_or("NULL".to_string(), |r| format!("'{r}'"));
    format!(
        "INSERT INTO accounts (username, password_hash, role) VALUES ('{username}', '{hash}', {role})"
    )
}

async fn credentials(pool: &PgPool, username: &str) -> (i32, String) {
    sqlx::query_as(
        "SELECT credentials_version, credentials_changed_at::text FROM accounts WHERE username = $1",
    )
    .bind(username)
    .fetch_one(pool)
    .await
    .unwrap()
}

/// Creates `alice` with an old `credentials_changed_at`, so a trigger bump is visible.
async fn alice(pool: &PgPool) {
    accepts(pool, insert("alice", HASH, Some("admin"))).await;
    accepts(
        pool,
        "UPDATE accounts SET credentials_changed_at = '2000-01-01T00:00:00Z' WHERE username = 'alice'",
    )
    .await;
}

#[sqlx::test]
async fn a_valid_account_gets_its_defaults(pool: PgPool) {
    accepts(&pool, insert("alice", HASH, None)).await;
    let (role, status, version): (Option<String>, String, i32) = sqlx::query_as(
        "SELECT role, status, credentials_version FROM accounts WHERE username = 'alice'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(role, None);
    assert_eq!(status, "active");
    assert_eq!(version, 1);
    accepts(&pool, insert("bob-2", HASH, Some("admin"))).await;
}

#[sqlx::test]
async fn duplicate_username_is_rejected(pool: PgPool) {
    accepts(&pool, insert("alice", HASH, None)).await;
    rejects(
        &pool,
        insert("alice", OTHER_HASH, Some("admin")),
        "23505",
        Some("accounts_pk"),
    )
    .await;
}

#[sqlx::test]
async fn username_must_be_a_slug(pool: PgPool) {
    let too_long = "a".repeat(65);
    for bad in [
        "Alice", "a", "-alice", "alice-", "al--ice", "al ice", "alice_1", &too_long,
    ] {
        rejects(
            &pool,
            insert(bad, HASH, None),
            "23514",
            Some("accounts_username_ck"),
        )
        .await;
    }
    accepts(&pool, insert(&"a".repeat(64), HASH, None)).await;
}

#[sqlx::test]
async fn role_and_status_are_limited_to_their_values(pool: PgPool) {
    for role in ["Admin", "superuser", ""] {
        rejects(
            &pool,
            insert("alice", HASH, Some(role)),
            "23514",
            Some("accounts_role_ck"),
        )
        .await;
    }
    for status in ["locked", "Active", ""] {
        rejects(
            &pool,
            format!(
                "INSERT INTO accounts (username, password_hash, status) \
                 VALUES ('alice', '{HASH}', '{status}')"
            ),
            "23514",
            Some("accounts_status_ck"),
        )
        .await;
    }
    accepts(
        &pool,
        format!(
            "INSERT INTO accounts (username, password_hash, status) \
             VALUES ('alice', '{HASH}', 'disabled')"
        ),
    )
    .await;
}

#[sqlx::test]
async fn password_hash_must_be_an_argon2id_phc_string(pool: PgPool) {
    for bad in [
        "",
        "hunter2hunter2",
        "$argon2i$v=19$m=19456,t=2,p=1$c2FsdA$aGFzaA",
        "$argon2id$v=16$m=19456,t=2,p=1$c2FsdA$aGFzaA",
        "$argon2id$v=19$m=19456,t=2$c2FsdA$aGFzaA",
        "$argon2id$v=19$m=19456,t=2,p=1$c2FsdA",
        "$argon2id$v=19$m=19456,t=2,p=1$c2FsdA$aGFzaA$",
        "$argon2id$v=19$m=19456,t=2,p=1$c2F sdA$aGFzaA",
        "$2b$12$abcdefghijklmnopqrstuuJ9eTnX5BpXbVZ1u7n9KLlqKa1A2B3C4",
    ] {
        rejects(
            &pool,
            insert("alice", bad, None),
            "23514",
            Some("accounts_password_hash_ck"),
        )
        .await;
    }
}

#[sqlx::test]
async fn credentials_version_is_at_least_one(pool: PgPool) {
    rejects(
        &pool,
        format!(
            "INSERT INTO accounts (username, password_hash, credentials_version) \
             VALUES ('alice', '{HASH}', 0)"
        ),
        "23514",
        Some("accounts_credentials_version_ck"),
    )
    .await;
}

#[sqlx::test]
async fn password_change_bumps_the_credentials_version(pool: PgPool) {
    alice(&pool).await;
    let (v1, at1) = credentials(&pool, "alice").await;
    accepts(
        &pool,
        format!("UPDATE accounts SET password_hash = '{OTHER_HASH}' WHERE username = 'alice'"),
    )
    .await;
    let (v2, at2) = credentials(&pool, "alice").await;
    assert_eq!(v2, v1 + 1);
    assert_ne!(at2, at1, "credentials_changed_at must move");
}

#[sqlx::test]
async fn status_change_bumps_the_credentials_version(pool: PgPool) {
    alice(&pool).await;
    let (v1, at1) = credentials(&pool, "alice").await;
    accepts(
        &pool,
        "UPDATE accounts SET status = 'disabled' WHERE username = 'alice'",
    )
    .await;
    let (v2, at2) = credentials(&pool, "alice").await;
    assert_eq!(v2, v1 + 1);
    assert_ne!(at2, at1);

    // Re-enabling bumps again: tokens from before the disable stay invalid (research R7).
    accepts(
        &pool,
        "UPDATE accounts SET status = 'active' WHERE username = 'alice'",
    )
    .await;
    assert_eq!(credentials(&pool, "alice").await.0, v1 + 2);
}

#[sqlx::test]
async fn role_change_and_no_op_updates_keep_the_version(pool: PgPool) {
    alice(&pool).await;
    let before = credentials(&pool, "alice").await;
    for sql in [
        "UPDATE accounts SET role = NULL WHERE username = 'alice'".to_string(),
        "UPDATE accounts SET role = 'admin' WHERE username = 'alice'".to_string(),
        "UPDATE accounts SET status = 'active' WHERE username = 'alice'".to_string(),
        format!("UPDATE accounts SET password_hash = '{HASH}' WHERE username = 'alice'"),
    ] {
        accepts(&pool, sql.clone()).await;
        assert_eq!(credentials(&pool, "alice").await, before, "{sql}");
    }
}

#[sqlx::test]
async fn truncate_is_rejected(pool: PgPool) {
    accepts(&pool, insert("alice", HASH, None)).await;
    rejects(
        &pool,
        "TRUNCATE accounts",
        "23514",
        Some("accounts_no_truncate_ck"),
    )
    .await;
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM accounts")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 1);
}

// ---------------------------------------------------------------- lookups (data-model §1.1)

#[sqlx::test]
async fn login_lookup_returns_the_stored_credentials(pool: PgPool) {
    assert_eq!(find_for_login(&pool, "alice").await.unwrap(), None);
    alice(&pool).await;
    accepts(
        &pool,
        format!(
            "UPDATE accounts SET password_hash = '{OTHER_HASH}', status = 'disabled' \
             WHERE username = 'alice'"
        ),
    )
    .await;
    assert_eq!(
        find_for_login(&pool, "alice").await.unwrap(),
        Some(LoginAccount {
            password_hash: OTHER_HASH.into(),
            status: "disabled".into(),
            credentials_version: 2,
        })
    );
    assert_eq!(find_for_login(&pool, "Alice").await.unwrap(), None);
    assert_eq!(
        find_for_login(&pool, "alice' OR '1'='1").await.unwrap(),
        None
    );
}

#[sqlx::test]
async fn write_check_lookup_returns_role_status_and_version(pool: PgPool) {
    assert_eq!(find_for_write_check(&pool, "alice").await.unwrap(), None);
    alice(&pool).await;
    accepts(&pool, insert("bob", HASH, None)).await;
    assert_eq!(
        find_for_write_check(&pool, "alice").await.unwrap(),
        Some(WriteCheckAccount {
            role: Some("admin".into()),
            status: "active".into(),
            credentials_version: 1,
        })
    );
    assert_eq!(
        find_for_write_check(&pool, "bob").await.unwrap(),
        Some(WriteCheckAccount {
            role: None,
            status: "active".into(),
            credentials_version: 1,
        })
    );
    assert_eq!(find_for_write_check(&pool, "carol").await.unwrap(), None);
}
