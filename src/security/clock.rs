//! Monotonic time source of the limiter tables (data-model §3.5).

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

pub trait Clock: Send + Sync {
    fn now(&self) -> Instant;
}

/// `Instant::now`.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

/// A clock that only moves when told to (tests): a base instant plus an atomic offset.
#[derive(Debug)]
pub struct ManualClock {
    base: Instant,
    offset_nanos: AtomicU64,
}

impl ManualClock {
    pub fn new() -> Self {
        Self {
            base: Instant::now(),
            offset_nanos: AtomicU64::new(0),
        }
    }

    pub fn advance(&self, by: Duration) {
        let nanos = u64::try_from(by.as_nanos()).unwrap_or(u64::MAX);
        self.offset_nanos.fetch_add(nanos, Ordering::SeqCst);
    }
}

impl Default for ManualClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for ManualClock {
    fn now(&self) -> Instant {
        self.base + Duration::from_nanos(self.offset_nanos.load(Ordering::SeqCst))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_clock_moves_only_when_advanced() {
        let clock = ManualClock::new();
        let start = clock.now();
        std::thread::sleep(Duration::from_millis(5));
        assert_eq!(clock.now(), start);
        clock.advance(Duration::from_millis(1500));
        clock.advance(Duration::from_secs(2));
        assert_eq!(clock.now() - start, Duration::from_millis(3500));
    }

    #[test]
    fn system_clock_is_monotonic() {
        let a = SystemClock.now();
        let b = SystemClock.now();
        assert!(b >= a);
    }
}
