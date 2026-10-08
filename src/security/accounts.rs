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
