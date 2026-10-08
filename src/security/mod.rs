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
use std::time::Instant;

use crate::config::SecurityConfig;
use clock::Clock;
use events::{Event, EventSink};
use permits::Permits;
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
        todo!()
    }
}

/// Configuration, signing keys, permits, clock, and event sink shared by all workers.
pub struct Security {
    config: SecurityConfig,
    capacities: Capacities,
    keys: Keys,
    requests: Arc<Permits>,
    hashes: Arc<Permits>,
    clock: Arc<dyn Clock>,
    sink: Arc<dyn EventSink>,
    dummy_hash: String,
}

impl Security {
    /// Production construction: default capacities, system clock, stdout sink.
    /// Computes the dummy hash, so it takes one Argon2 hashing time.
    pub fn new(_config: SecurityConfig) -> Self {
        todo!()
    }

    /// Construction with injected capacities, clock, and sink (tests).
    pub fn with(
        _config: SecurityConfig,
        _capacities: Capacities,
        _clock: Arc<dyn Clock>,
        _sink: Arc<dyn EventSink>,
    ) -> Self {
        todo!()
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

    /// Hash verified for unknown usernames so every failed login costs the same (research R8).
    pub fn dummy_hash(&self) -> &str {
        &self.dummy_hash
    }

    /// Writes one event line to the sink.
    pub fn emit(&self, _event: &Event) {
        let _ = &self.sink;
        todo!()
    }
}
