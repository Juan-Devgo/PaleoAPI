//! Configuration read from the environment (FR-002; Spec 003 plan §Configuration).

use std::fmt;
use std::str::FromStr;
use std::time::Duration;

use sqlx::postgres::PgConnectOptions;

use crate::db::StartupError;
use crate::security::client_ip::TrustedProxies;

/// The only configuration variable the API reads.
pub const DATABASE_URL: &str = "DATABASE_URL";

/// The token signing secret. `Debug` never prints it.
#[derive(Clone, PartialEq, Eq)]
pub struct JwtSecret(Vec<u8>);

impl JwtSecret {
    pub fn new(secret: impl Into<Vec<u8>>) -> Self {
        Self(secret.into())
    }

    pub fn expose(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for JwtSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("JwtSecret(..)")
    }
}

/// Security settings (Spec 003 plan §Configuration).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecurityConfig {
    pub jwt_secret: JwtSecret,
    pub rate_limit_per_minute: u32,
    pub rate_limit_burst: u32,
    pub login_limit_per_client: u32,
    pub login_limit_per_username: u32,
    pub login_limit_window: Duration,
    pub trusted_proxies: TrustedProxies,
    pub max_concurrent_requests: usize,
    pub max_concurrent_password_hashes: usize,
}

/// Builds the connection options from `DATABASE_URL`, read through `get`
/// (normally `std::env::var`). Errors never echo the URL.
pub fn database_options_from_env(
    get: impl Fn(&str) -> Option<String>,
) -> Result<PgConnectOptions, StartupError> {
    let url = get(DATABASE_URL).unwrap_or_default();
    let url = url.trim();
    if url.is_empty() {
        return Err(StartupError::MissingConfig);
    }
    if !(url.starts_with("postgres://") || url.starts_with("postgresql://")) {
        return Err(StartupError::InvalidConfig);
    }
    PgConnectOptions::from_str(url).map_err(|_| StartupError::InvalidConfig)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEAKS: [&str; 4] = ["leaky_user", "leaky-secret-123", "localhost", "postgres://"];

    fn env_with(url: Option<&str>) -> impl Fn(&str) -> Option<String> {
        let url = url.map(String::from);
        move |key| (key == "DATABASE_URL").then(|| url.clone()).flatten()
    }

    #[test]
    fn missing_or_empty_url_is_missing_config() {
        for url in [None, Some(""), Some("   ")] {
            let err = database_options_from_env(env_with(url)).unwrap_err();
            assert_eq!(err, StartupError::MissingConfig, "{url:?}");
        }
    }

    #[test]
    fn non_postgres_url_is_invalid_config() {
        let scheme = "mysql";
        let mysql = format!("{scheme}://u:p@h/db");
        for url in ["not a url", mysql.as_str(), "http://localhost/paleo"] {
            let err = database_options_from_env(env_with(Some(url))).unwrap_err();
            assert_eq!(err, StartupError::InvalidConfig, "{url}");
        }
    }

    #[test]
    fn postgres_url_is_accepted() {
        for url in [
            "postgres://localhost/paleo",
            "postgresql://localhost:5433/paleo",
        ] {
            let opts = database_options_from_env(env_with(Some(url))).unwrap();
            assert_eq!(opts.get_database(), Some("paleo"));
        }
    }

    #[test]
    fn messages_name_the_fix_and_leak_nothing() {
        let all = [
            StartupError::MissingConfig,
            StartupError::InvalidConfig,
            StartupError::Unreachable,
            StartupError::AuthFailed,
            StartupError::DatabaseMissing,
            StartupError::MigrationsPending {
                versions: vec![20261006210013, 20261006210918],
            },
            StartupError::MigrationDirty(20261006210013),
            StartupError::MigrationModified(20261006210013),
        ];
        let fixes = [
            "Set ",
            "Run ",
            "rerun",
            "Check ",
            "Is it running",
            "Restore ",
        ];
        for err in all {
            let msg = err.to_string();
            assert!(!msg.is_empty());
            assert!(!msg.contains('\n'), "one line: {msg}");
            assert!(fixes.iter().any(|f| msg.contains(f)), "no fix in: {msg}");
            for leak in LEAKS {
                assert!(!msg.contains(leak), "{msg} contains {leak}");
            }
        }
        assert_eq!(
            StartupError::MigrationsPending {
                versions: vec![1, 2]
            }
            .to_string(),
            "Database schema is out of date: 2 migration(s) pending (1, 2). \
             Run \"sqlx migrate run\", then start the API again."
        );
    }
}
