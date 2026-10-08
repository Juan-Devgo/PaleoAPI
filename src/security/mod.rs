//! Spec 003 security: accounts, tokens, limits, and the security event log.
//!
//! One [`Security`] value is built at startup and shared by every worker as an `Arc`.

pub mod accounts;
pub mod client_ip;
pub mod clock;
pub mod events;
pub mod login_limit;
pub mod middleware;
pub mod password;
pub mod permits;
pub mod rate_limit;
pub mod token;

use std::sync::Arc;
use std::time::{Instant, SystemTime};

use crate::config::SecurityConfig;
use clock::{Clock, SystemClock};
use events::{Event, EventSink, StdoutSink};
use permits::Permits;
use rate_limit::RateLimiter;
use token::Keys;

/// Limiter table capacities (data-model §3.2, §3.3). Overridable in tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capacities {
    /// Clients tracked by the general limit.
    pub general_clients: usize,
    /// Keys per login-limit table.
    pub login_keys: usize,
}

impl Default for Capacities {
    fn default() -> Self {
        Self {
            general_clients: 200_000,
            login_keys: 100_000,
        }
    }
}

/// Configuration, signing keys, permits, clock, and event sink shared by all workers.
pub struct Security {
    config: SecurityConfig,
    capacities: Capacities,
    keys: Keys,
    requests: Arc<Permits>,
    hashes: Arc<Permits>,
    general: RateLimiter,
    clock: Arc<dyn Clock>,
    sink: Arc<dyn EventSink>,
    dummy_hash: String,
}

impl Security {
    /// Production construction: default capacities, system clock, stdout sink.
    /// Computes the dummy hash, so it takes one Argon2 hashing time.
    pub fn new(config: SecurityConfig) -> Self {
        Self::with(
            config,
            Capacities::default(),
            Arc::new(SystemClock),
            Arc::new(StdoutSink),
        )
    }

    /// Construction with injected capacities, clock, and sink (tests).
    pub fn with(
        config: SecurityConfig,
        capacities: Capacities,
        clock: Arc<dyn Clock>,
        sink: Arc<dyn EventSink>,
    ) -> Self {
        Self {
            keys: Keys::new(config.jwt_secret.expose()),
            requests: Permits::new(config.max_concurrent_requests),
            hashes: Permits::new(config.max_concurrent_password_hashes),
            general: RateLimiter::new(
                config.rate_limit_per_minute,
                config.rate_limit_burst,
                capacities.general_clients,
            ),
            dummy_hash: password::dummy_hash(),
            config,
            capacities,
            clock,
            sink,
        }
    }

    pub fn config(&self) -> &SecurityConfig {
        &self.config
    }

    pub fn capacities(&self) -> Capacities {
        self.capacities
    }

    pub fn keys(&self) -> &Keys {
        &self.keys
    }

    /// The current instant of the injected clock.
    pub fn now(&self) -> Instant {
        self.clock.now()
    }

    /// In-flight request permits (`MAX_CONCURRENT_REQUESTS`).
    pub fn requests(&self) -> &Arc<Permits> {
        &self.requests
    }

    /// Password verification permits (`MAX_CONCURRENT_PASSWORD_HASHES`).
    pub fn hashes(&self) -> &Arc<Permits> {
        &self.hashes
    }

    /// The general per-client limit (FR-021, data-model §3.2).
    pub fn general(&self) -> &RateLimiter {
        &self.general
    }

    /// Hash verified for unknown usernames so every failed login costs the same (research R8).
    pub fn dummy_hash(&self) -> &str {
        &self.dummy_hash
    }

    /// Writes one event line to the sink.
    pub fn emit(&self, event: &Event) {
        self.sink.write_line(&event.to_line(SystemTime::now()));
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use serde_json::Value;

    use super::clock::ManualClock;
    use super::events::{CaptureSink, EventKind};
    use super::password::verify;
    use super::token::{Claims, Rejection, issue};
    use super::*;
    use crate::config::security_config_from_env;

    fn config(vars: &[(&str, &str)]) -> SecurityConfig {
        let vars: Vec<(String, String)> = vars
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect();
        security_config_from_env(move |key| {
            if key == "JWT_SECRET" {
                return Some("construction-test-secret-0123456789".into());
            }
            vars.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
        })
        .unwrap()
    }

    #[test]
    fn default_capacities_follow_the_data_model() {
        assert_eq!(
            Capacities::default(),
            Capacities {
                general_clients: 200_000,
                login_keys: 100_000,
            }
        );
    }

    #[test]
    fn settings_and_capacities_are_carried() {
        let config = config(&[
            ("RATE_LIMIT_BURST", "7"),
            ("MAX_CONCURRENT_REQUESTS", "3"),
            ("MAX_CONCURRENT_PASSWORD_HASHES", "1"),
            ("TRUSTED_PROXIES", "10.0.0.0/8"),
        ]);
        let capacities = Capacities {
            general_clients: 16,
            login_keys: 4,
        };
        let security = Security::with(
            config.clone(),
            capacities,
            Arc::new(ManualClock::new()),
            Arc::new(CaptureSink::default()),
        );
        assert_eq!(security.config(), &config);
        assert_eq!(security.capacities(), capacities);
        assert_eq!(security.requests().max(), 3);
        assert_eq!(security.hashes().max(), 1);
        assert_eq!(security.requests().in_use(), 0);
    }

    #[test]
    fn injected_clock_is_used() {
        let clock = Arc::new(ManualClock::new());
        let security = Security::with(
            config(&[]),
            Capacities::default(),
            clock.clone(),
            Arc::new(CaptureSink::default()),
        );
        let start = security.now();
        assert_eq!(start, clock.now());
        assert_eq!(
            security.now(),
            start,
            "a manual clock does not move by itself"
        );
        clock.advance(Duration::from_secs(5));
        assert_eq!(security.now() - start, Duration::from_secs(5));
    }

    #[test]
    fn injected_sink_receives_events() {
        let sink = Arc::new(CaptureSink::default());
        let security = Security::with(
            config(&[]),
            Capacities::default(),
            Arc::new(ManualClock::new()),
            sink.clone(),
        );
        let mut event = Event::new(EventKind::Forbidden);
        event.status = Some(403);
        security.emit(&event);
        let lines = sink.lines();
        assert_eq!(lines.len(), 1);
        let line: Value = serde_json::from_str(&lines[0]).unwrap();
        assert_eq!(line["event"], "forbidden");
        assert_eq!(line["status"], 403);
        assert!(line["ts"].as_str().unwrap().ends_with('Z'));
    }

    #[test]
    fn keys_come_from_the_configured_secret() {
        let security = Security::with(
            config(&[]),
            Capacities::default(),
            Arc::new(ManualClock::new()),
            Arc::new(CaptureSink::default()),
        );
        let claims = Claims::now("alice", 1);
        let token = issue(security.keys(), &claims).unwrap();
        assert_eq!(token::verify(security.keys(), &token), Ok(claims));

        let other = Keys::new(b"some-other-secret-0123456789abcdef");
        assert_eq!(token::verify(&other, &token), Err(Rejection::Invalid));
    }

    #[actix_web::test]
    async fn dummy_hash_never_verifies() {
        let security = Security::new(config(&[]));
        let dummy = security.dummy_hash().to_string();
        assert!(
            dummy.starts_with("$argon2id$v=19$m=19456,t=2,p=1$"),
            "{dummy}"
        );
        for guess in ["", "password", "construction-test-secret-0123456789"] {
            assert_eq!(verify(guess.into(), dummy.clone()).await, Ok(false));
        }
        let other = Security::new(config(&[]));
        assert_ne!(
            other.dummy_hash(),
            dummy,
            "a new random password per process"
        );
    }
}
