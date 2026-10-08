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

/// Reads the Spec 003 settings through `get` (plan §Configuration). Unset or empty
/// limit variables take their defaults. Errors never contain the secret.
pub fn security_config_from_env(
    _get: impl Fn(&str) -> Option<String>,
) -> Result<SecurityConfig, StartupError> {
    todo!()
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
    use std::collections::HashMap;
    use std::net::IpAddr;

    use super::*;
    use crate::db::SettingProblem;

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

    // ------------------------------------------------------------ security settings

    const SECRET: &str = "0123456789abcdef0123456789abcdef";

    /// (variable, default, min, max) of every numeric setting (plan §Configuration).
    const LIMITS: [(&str, u64, u64, u64); 7] = [
        ("RATE_LIMIT_PER_MINUTE", 600, 1, 1_000_000),
        ("RATE_LIMIT_BURST", 120, 1, 1_000_000),
        ("LOGIN_LIMIT_PER_CLIENT", 10, 1, 10_000),
        ("LOGIN_LIMIT_PER_USERNAME", 20, 1, 10_000),
        ("LOGIN_LIMIT_WINDOW_SECONDS", 900, 1, 86_400),
        ("MAX_CONCURRENT_REQUESTS", 256, 1, 65_535),
        ("MAX_CONCURRENT_PASSWORD_HASHES", 2, 1, 64),
    ];

    fn security_env(vars: &[(&str, &str)]) -> Result<SecurityConfig, StartupError> {
        let mut map: HashMap<String, String> = HashMap::new();
        map.insert("JWT_SECRET".into(), SECRET.into());
        for (k, v) in vars {
            map.insert((*k).into(), (*v).into());
        }
        security_config_from_env(move |key| map.get(key).cloned())
    }

    fn setting(config: &SecurityConfig, var: &str) -> u64 {
        match var {
            "RATE_LIMIT_PER_MINUTE" => config.rate_limit_per_minute.into(),
            "RATE_LIMIT_BURST" => config.rate_limit_burst.into(),
            "LOGIN_LIMIT_PER_CLIENT" => config.login_limit_per_client.into(),
            "LOGIN_LIMIT_PER_USERNAME" => config.login_limit_per_username.into(),
            "LOGIN_LIMIT_WINDOW_SECONDS" => config.login_limit_window.as_secs(),
            "MAX_CONCURRENT_REQUESTS" => config.max_concurrent_requests as u64,
            "MAX_CONCURRENT_PASSWORD_HASHES" => config.max_concurrent_password_hashes as u64,
            _ => unreachable!("{var}"),
        }
    }

    #[test]
    fn defaults_apply_when_only_the_secret_is_set() {
        let config = security_env(&[]).unwrap();
        assert_eq!(config.jwt_secret.expose(), SECRET.as_bytes());
        for (var, default, _, _) in LIMITS {
            assert_eq!(setting(&config, var), default, "{var}");
        }
        assert!(config.trusted_proxies.is_empty());
    }

    #[test]
    fn empty_values_take_the_default() {
        for (var, default, _, _) in LIMITS {
            for empty in ["", "  "] {
                let config = security_env(&[(var, empty)]).unwrap();
                assert_eq!(setting(&config, var), default, "{var}={empty:?}");
            }
        }
        for empty in ["", "  "] {
            let config = security_env(&[("TRUSTED_PROXIES", empty)]).unwrap();
            assert!(config.trusted_proxies.is_empty());
        }
    }

    #[test]
    fn range_edges_are_accepted() {
        for (var, _, min, max) in LIMITS {
            for v in [min, max] {
                let config = security_env(&[(var, &v.to_string())]).unwrap();
                assert_eq!(setting(&config, var), v, "{var}={v}");
            }
            let padded = security_env(&[(var, &format!(" {max} "))]).unwrap();
            assert_eq!(setting(&padded, var), max, "{var} with spaces");
        }
    }

    #[test]
    fn values_outside_the_range_or_not_integers_are_refused() {
        for (var, _, min, max) in LIMITS {
            let below = (min - 1).to_string();
            let above = (max + 1).to_string();
            for bad in [
                below.as_str(),
                above.as_str(),
                "-1",
                "1.5",
                "1e3",
                "ten",
                "99999999999999999999999",
            ] {
                let err = security_env(&[(var, bad)]).unwrap_err();
                assert_eq!(
                    err,
                    StartupError::InvalidSetting {
                        var,
                        problem: SettingProblem::Range { min, max }
                    },
                    "{var}={bad}"
                );
            }
        }
    }

    #[test]
    fn invalid_setting_messages_name_the_variable_and_range() {
        let err = security_env(&[("RATE_LIMIT_BURST", "0")]).unwrap_err();
        assert_eq!(
            err.to_string(),
            "RATE_LIMIT_BURST must be an integer from 1 to 1000000."
        );
        let err = security_env(&[("TRUSTED_PROXIES", "10.0.0.1, proxy.local")]).unwrap_err();
        assert_eq!(
            err.to_string(),
            "TRUSTED_PROXIES entry 'proxy.local' is not an IP address or CIDR range."
        );
    }

    #[test]
    fn trusted_proxies_accept_addresses_and_cidrs() {
        let config = security_env(&[(
            "TRUSTED_PROXIES",
            "10.0.0.1, 192.168.0.0/16,::1,2001:db8::/32",
        )])
        .unwrap();
        let proxies = &config.trusted_proxies;
        for ip in ["10.0.0.1", "192.168.44.5", "::1", "2001:db8:ffff::1"] {
            assert!(proxies.contains(ip.parse::<IpAddr>().unwrap()), "{ip}");
        }
        for ip in ["10.0.0.2", "192.169.0.1", "::2", "2001:db9::1"] {
            assert!(!proxies.contains(ip.parse::<IpAddr>().unwrap()), "{ip}");
        }
    }

    #[test]
    fn trusted_proxy_parse_errors_name_the_entry() {
        for (value, entry) in [
            ("proxy.local", "proxy.local"),
            ("10.0.0.1,nope", "nope"),
            ("10.0.0.0/33", "10.0.0.0/33"),
            ("::/129", "::/129"),
            ("10.0.0.0/", "10.0.0.0/"),
            ("10.0.0.0/x", "10.0.0.0/x"),
            ("10.0.0.1:8080", "10.0.0.1:8080"),
            ("10.0.0.1,,10.0.0.2", ""),
            ("10.0.0.1,", ""),
        ] {
            let err = security_env(&[("TRUSTED_PROXIES", value)]).unwrap_err();
            assert_eq!(
                err,
                StartupError::InvalidSetting {
                    var: "TRUSTED_PROXIES",
                    problem: SettingProblem::ProxyEntry(entry.into())
                },
                "{value}"
            );
        }
    }

    fn secret_env(secret: Option<&str>) -> Result<SecurityConfig, StartupError> {
        let secret = secret.map(String::from);
        security_config_from_env(move |key| (key == "JWT_SECRET").then(|| secret.clone()).flatten())
    }

    #[test]
    fn secret_must_be_set() {
        for secret in [None, Some("")] {
            assert_eq!(
                secret_env(secret).unwrap_err(),
                StartupError::MissingSecret,
                "{secret:?}"
            );
        }
    }

    #[test]
    fn secret_must_have_32_bytes() {
        let short = "s".repeat(31);
        assert_eq!(
            secret_env(Some(&short)).unwrap_err(),
            StartupError::ShortSecret
        );
        let exact = "s".repeat(32);
        assert_eq!(
            secret_env(Some(&exact)).unwrap().jwt_secret.expose(),
            exact.as_bytes()
        );
        // Bytes, not characters: 11 three-byte characters are 33 bytes.
        assert!(secret_env(Some(&"€".repeat(11))).is_ok());
        assert_eq!(
            secret_env(Some(&"€".repeat(10))).unwrap_err(),
            StartupError::ShortSecret
        );
    }

    #[test]
    fn placeholder_secret_is_refused_in_any_case() {
        for secret in [
            "change-me-generate-at-least-32-random-bytes",
            "CHANGE-ME-generate-at-least-32-random-bytes",
            "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxChange-Me",
        ] {
            assert_eq!(
                secret_env(Some(secret)).unwrap_err(),
                StartupError::PlaceholderSecret,
                "{secret}"
            );
        }
    }

    #[test]
    fn secret_messages_follow_the_contract_and_never_echo_the_secret() {
        assert_eq!(
            StartupError::MissingSecret.to_string(),
            "JWT_SECRET is not set. Set it to a random value of at least 32 bytes \
             (openssl rand -base64 48)."
        );
        assert_eq!(
            StartupError::ShortSecret.to_string(),
            "JWT_SECRET is shorter than 32 bytes. Use a random value (openssl rand -base64 48)."
        );
        assert_eq!(
            StartupError::PlaceholderSecret.to_string(),
            "JWT_SECRET is still the placeholder from .env.example. Replace it with a random \
             value (openssl rand -base64 48)."
        );
        let secret = "change-me-generate-at-least-32-random-bytes";
        let msg = secret_env(Some(secret)).unwrap_err().to_string();
        assert!(!msg.contains(secret), "{msg}");
        assert!(!format!("{:?}", security_env(&[]).unwrap()).contains(SECRET));
    }

    #[test]
    fn the_secret_is_checked_before_the_limits() {
        let err =
            security_config_from_env(|key| (key == "RATE_LIMIT_BURST").then(|| "0".to_string()))
                .unwrap_err();
        assert_eq!(err, StartupError::MissingSecret);
    }
}
