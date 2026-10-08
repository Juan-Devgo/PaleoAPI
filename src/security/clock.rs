//! Monotonic time source of the limiter tables (data-model §3.5).

use std::sync::atomic::AtomicU64;
use std::time::{Duration, Instant};

pub trait Clock: Send + Sync {
    fn now(&self) -> Instant;
}

/// `Instant::now`.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        todo!()
    }
}

/// A clock that only moves when told to (tests).
#[derive(Debug)]
pub struct ManualClock {
    base: Instant,
    offset_nanos: AtomicU64,
}

impl ManualClock {
    pub fn new() -> Self {
        todo!()
    }

    pub fn advance(&self, _by: Duration) {
        todo!()
    }
}

impl Default for ManualClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for ManualClock {
    fn now(&self) -> Instant {
        let _ = (&self.base, &self.offset_nanos);
        todo!()
    }
}
