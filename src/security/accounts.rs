//! Account statements shared by the API and the operator tool (data-model §1.1).

use sqlx::PgExecutor;

pub const ACTIVE: &str = "active";
pub const ADMIN: &str = "admin";

/// Columns read by login.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginAccount {
    pub password_hash: String,
    pub status: String,
    pub credentials_version: i32,
}

/// Columns read by the write check (FR-015).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteCheckAccount {
    pub role: Option<String>,
    pub status: String,
    pub credentials_version: i32,
}

/// The login row of `username`, if it exists (plan §Login step 6).
pub async fn find_for_login<'e>(
    e: impl PgExecutor<'e>,
    username: &str,
) -> sqlx::Result<Option<LoginAccount>> {
    sqlx::query_as!(
        LoginAccount,
        "SELECT password_hash, status, credentials_version FROM accounts WHERE username = $1",
        username
    )
    .fetch_optional(e)
    .await
}

/// The live role, status, and credential version of `username` (FR-015, research R7).
pub async fn find_for_write_check<'e>(
    e: impl PgExecutor<'e>,
    username: &str,
) -> sqlx::Result<Option<WriteCheckAccount>> {
    sqlx::query_as!(
        WriteCheckAccount,
        "SELECT role, status, credentials_version FROM accounts WHERE username = $1",
        username
    )
    .fetch_optional(e)
    .await
}

/// One line of the operator tool's `list` (timestamps as UTC ISO-8601 text).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountSummary {
    pub username: String,
    pub role: Option<String>,
    pub status: String,
    pub credentials_changed_at: String,
    pub created_at: String,
}

/// Inserts an active account (operator tool). A taken username fails with `23505 accounts_pk`.
pub async fn insert<'e>(
    e: impl PgExecutor<'e>,
    username: &str,
    password_hash: &str,
    role: Option<&str>,
) -> sqlx::Result<()> {
    sqlx::query!(
        "INSERT INTO accounts (username, password_hash, role) VALUES ($1, $2, $3)",
        username,
        password_hash,
        role
    )
    .execute(e)
    .await
    .map(|_| ())
}

/// Replaces the hash; the trigger bumps the credential version. `false`: no such account.
pub async fn set_password<'e>(
    e: impl PgExecutor<'e>,
    username: &str,
    password_hash: &str,
) -> sqlx::Result<bool> {
    sqlx::query!(
        "UPDATE accounts SET password_hash = $2 WHERE username = $1",
        username,
        password_hash
    )
    .execute(e)
    .await
    .map(|done| done.rows_affected() == 1)
}

/// Sets or clears the role. `false`: no such account.
pub async fn set_role<'e>(
    e: impl PgExecutor<'e>,
    username: &str,
    role: Option<&str>,
) -> sqlx::Result<bool> {
    sqlx::query!(
        "UPDATE accounts SET role = $2 WHERE username = $1",
        username,
        role
    )
    .execute(e)
    .await
    .map(|done| done.rows_affected() == 1)
}

/// Sets `active` or `disabled`; the trigger bumps the credential version on a change.
/// `false`: no such account.
pub async fn set_status<'e>(
    e: impl PgExecutor<'e>,
    username: &str,
    status: &str,
) -> sqlx::Result<bool> {
    sqlx::query!(
        "UPDATE accounts SET status = $2 WHERE username = $1",
        username,
        status
    )
    .execute(e)
    .await
    .map(|done| done.rows_affected() == 1)
}

/// Every account ordered by username, without hashes.
pub async fn list<'e>(e: impl PgExecutor<'e>) -> sqlx::Result<Vec<AccountSummary>> {
    sqlx::query_as!(
        AccountSummary,
        r#"SELECT username, role, status,
                  to_char(credentials_changed_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS"Z"') AS "credentials_changed_at!",
                  to_char(created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS"Z"') AS "created_at!"
           FROM accounts ORDER BY username"#
    )
    .fetch_all(e)
    .await
}
