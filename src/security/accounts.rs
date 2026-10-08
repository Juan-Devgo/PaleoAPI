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

pub async fn find_for_login<'e>(
    _e: impl PgExecutor<'e>,
    _username: &str,
) -> sqlx::Result<Option<LoginAccount>> {
    todo!()
}

pub async fn find_for_write_check<'e>(
    _e: impl PgExecutor<'e>,
    _username: &str,
) -> sqlx::Result<Option<WriteCheckAccount>> {
    todo!()
}
