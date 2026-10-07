//! Database errors → responses (data-model §4, §5).

use super::error::ApiError;

/// Whether `constraint` (a constraint, unique index, or trigger-raised name) has a row in the error map.
pub fn is_mapped(_constraint: &str) -> bool {
    todo!()
}

/// `500 INTERNAL_ERROR` after one stderr line with SQLSTATE and constraint only
/// (plan §Write path). Never logs messages, details, or values.
pub fn internal(e: &sqlx::Error) -> ApiError {
    match e.as_database_error() {
        Some(db) => eprintln!(
            "paleo_api: internal error: {} {}",
            db.code().as_deref().unwrap_or("-"),
            db.constraint().unwrap_or("-")
        ),
        None => eprintln!("paleo_api: internal error: {}", kind(e)),
    }
    ApiError::internal()
}

/// A short, value-free name for a non-database error.
fn kind(e: &sqlx::Error) -> &'static str {
    match e {
        sqlx::Error::PoolTimedOut => "pool-timed-out",
        sqlx::Error::PoolClosed => "pool-closed",
        sqlx::Error::Io(_) => "io",
        sqlx::Error::Tls(_) => "tls",
        sqlx::Error::Protocol(_) => "protocol",
        sqlx::Error::RowNotFound => "row-not-found",
        sqlx::Error::ColumnDecode { .. } | sqlx::Error::Decode(_) => "decode",
        _ => "driver",
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(e: sqlx::Error) -> Self {
        internal(&e)
    }
}
