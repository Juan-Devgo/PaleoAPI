//! General per-client limit: sharded GCRA table (FR-021, FR-023, FR-024, data-model §3.2).

use std::collections::HashMap;
use std::hash::{BuildHasher, RandomState};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use super::client_ip::ClientKey;
use crate::api::error::{ApiError, Overload};

/// Number of independently locked shards.
pub const SHARDS: usize = 16;

/// Per-client state: the theoretical arrival time and the last logged rejection.
#[derive(Debug, Clone, Copy)]
struct Entry {
    tat: Instant,
    last_logged: Option<Instant>,
}

/// The result of one request against the general limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Counted. `remaining` = `RateLimit r`, `reset` = `RateLimit t`.
    Allow { remaining: u64, reset: u64 },
    /// Over the limit (`429`); the state is unchanged. `log`: the first rejection of this
    /// client within `w` seconds (FR-039).
    Reject {
        retry_after: u64,
        reset: u64,
        log: bool,
    },
}

/// Why the limiter could not decide (research R14).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitError {
    /// The shard is full of live entries: a new client gets `503`.
    Full,
    /// A shard lock was poisoned: `500`.
    Poisoned,
}

/// The sharded GCRA table of one process.
#[derive(Debug)]
pub struct RateLimiter {
    shards: [Mutex<HashMap<ClientKey, Entry>>; SHARDS],
    hasher: RandomState,
    per_shard: usize,
    per_minute: u32,
    burst: u32,
    /// `T`: the emission interval, 60 s / per-minute.
    period: Duration,
    /// `τ`: `T × burst`.
    tau: Duration,
    /// `w` of `RateLimit-Policy`: ⌈burst × 60 / per-minute⌉ seconds.
    window: u64,
}

/// Whole seconds, rounded up.
fn ceil_secs(d: Duration) -> u64 {
    u64::try_from(d.as_nanos().div_ceil(1_000_000_000)).unwrap_or(u64::MAX)
}

impl RateLimiter {
    /// A table for `per_minute` requests with bursts of `burst`, holding up to
    /// `capacity` clients.
    pub fn new(per_minute: u32, burst: u32, capacity: usize) -> Self {
        let per_minute = per_minute.max(1);
        let burst = burst.max(1);
        let period = Duration::from_nanos(60_000_000_000 / u64::from(per_minute));
        Self {
            shards: std::array::from_fn(|_| Mutex::new(HashMap::new())),
            hasher: RandomState::new(),
            per_shard: capacity.div_ceil(SHARDS).max(1),
            per_minute,
            burst,
            period,
            tau: period * burst,
            window: (u64::from(burst) * 60).div_ceil(u64::from(per_minute)),
        }
    }

    pub fn per_minute(&self) -> u32 {
        self.per_minute
    }

    pub fn burst(&self) -> u32 {
        self.burst
    }

    /// Counts a request of `key` at `now` (data-model §3.2).
    pub fn check(&self, key: ClientKey, now: Instant) -> Result<Decision, LimitError> {
        let mut shard = self.shards[self.shard_index(&key)]
            .lock()
            .map_err(|_| LimitError::Poisoned)?;
        if !shard.contains_key(&key) && shard.len() >= self.per_shard {
            shard.retain(|_, entry| !self.is_stale(entry, now));
            if shard.len() >= self.per_shard {
                return Err(LimitError::Full);
            }
        }
        let entry = shard.entry(key).or_insert(Entry {
            tat: now,
            last_logged: None,
        });
        let base = entry.tat.max(now);
        let ahead = base - now + self.period;
        if ahead > self.tau {
            let log = entry
                .last_logged
                .is_none_or(|at| now.saturating_duration_since(at) >= self.window());
            if log {
                entry.last_logged = Some(now);
            }
            return Ok(Decision::Reject {
                retry_after: ceil_secs(ahead - self.tau).max(1),
                reset: ceil_secs(entry.tat.saturating_duration_since(now)),
                log,
            });
        }
        entry.tat = base + self.period;
        let used = entry.tat - now;
        let remaining = (self.tau - used).as_nanos() / self.period.as_nanos();
        Ok(Decision::Allow {
            remaining: u64::try_from(remaining).unwrap_or(u64::MAX),
            reset: ceil_secs(used),
        })
    }

    /// `RateLimit-Policy`: `"general";q=<burst>;w=<seconds>` (research R4).
    pub fn policy(&self) -> String {
        format!(r#""general";q={};w={}"#, self.burst, self.window)
    }

    fn window(&self) -> Duration {
        Duration::from_secs(self.window)
    }

    /// An entry equal to a fresh one whose last logged rejection is outside the window.
    fn is_stale(&self, entry: &Entry, now: Instant) -> bool {
        entry.tat <= now
            && entry
                .last_logged
                .is_none_or(|at| now.saturating_duration_since(at) >= self.window())
    }

    fn shard_index(&self, key: &ClientKey) -> usize {
        // Truncation is fine: only the remainder matters.
        (self.hasher.hash_one(key) as usize) % SHARDS
    }
}

impl Decision {
    /// `RateLimit`: `"general";r=<remaining>;t=<reset>` (research R4); `r=0` on reject.
    pub fn header(&self) -> String {
        let (remaining, reset) = match *self {
            Self::Allow { remaining, reset } => (remaining, reset),
            Self::Reject { reset, .. } => (0, reset),
        };
        format!(r#""general";r={remaining};t={reset}"#)
    }
}

/// Fail closed (research R14): a full table is load (`503`), a poisoned lock a defect (`500`).
impl From<LimitError> for ApiError {
    fn from(err: LimitError) -> Self {
        match err {
            LimitError::Full => ApiError::busy(Overload::Limiter),
            LimitError::Poisoned => ApiError::internal(),
        }
    }
}

#[cfg(test)]
mod tests {
    use actix_web::http::StatusCode;

    use super::*;

    const A: ClientKey = ClientKey::V4([203, 0, 113, 7]);
    const B: ClientKey = ClientKey::V4([198, 51, 100, 1]);

    fn secs(s: u64) -> Duration {
        Duration::from_secs(s)
    }

    fn allow(remaining: u64, reset: u64) -> Decision {
        Decision::Allow { remaining, reset }
    }

    fn reject(retry_after: u64, reset: u64, log: bool) -> Decision {
        Decision::Reject {
            retry_after,
            reset,
            log,
        }
    }

    #[test]
    fn a_fresh_client_starts_with_a_full_bucket() {
        let limiter = RateLimiter::new(60, 5, 1000);
        let now = Instant::now();
        assert_eq!(limiter.check(A, now), Ok(allow(4, 1)));
    }

    #[test]
    fn the_burst_passes_then_requests_are_rejected() {
        let limiter = RateLimiter::new(60, 5, 1000);
        let now = Instant::now();
        for i in 0..5 {
            assert_eq!(
                limiter.check(A, now),
                Ok(allow(4 - i, i + 1)),
                "request {i}"
            );
        }
        assert_eq!(limiter.check(A, now), Ok(reject(1, 5, true)));
    }

    #[test]
    fn rejected_requests_do_not_extend_the_wait() {
        // T = 10 s, τ = 30 s.
        let limiter = RateLimiter::new(6, 3, 1000);
        let start = Instant::now();
        for _ in 0..3 {
            assert!(matches!(
                limiter.check(A, start),
                Ok(Decision::Allow { .. })
            ));
        }
        assert_eq!(limiter.check(A, start), Ok(reject(10, 30, true)));
        for _ in 0..50 {
            assert_eq!(limiter.check(A, start), Ok(reject(10, 30, false)));
        }
        let later = start + Duration::from_millis(9_500);
        assert_eq!(
            limiter.check(A, later),
            Ok(reject(1, 21, false)),
            "ceil(0.5 s) = 1"
        );
        assert_eq!(limiter.check(A, start + secs(10)), Ok(allow(0, 30)));
    }

    #[test]
    fn the_bucket_refills_at_the_rate() {
        let limiter = RateLimiter::new(60, 5, 1000);
        let start = Instant::now();
        for _ in 0..5 {
            limiter.check(A, start).unwrap();
        }
        assert_eq!(limiter.check(A, start + secs(2)), Ok(allow(1, 4)));
        assert_eq!(limiter.check(A, start + secs(60)), Ok(allow(4, 1)));
    }

    #[test]
    fn requests_exactly_at_the_rate_are_never_rejected() {
        // The defaults: 600 per minute (T = 100 ms), burst 120.
        let limiter = RateLimiter::new(600, 120, 1000);
        let start = Instant::now();
        for i in 0..10_000u32 {
            let now = start + Duration::from_millis(100) * i;
            assert_eq!(limiter.check(A, now), Ok(allow(119, 1)), "request {i}");
        }
    }

    #[test]
    fn clients_are_independent() {
        let limiter = RateLimiter::new(1, 1, 1000);
        let now = Instant::now();
        assert_eq!(limiter.check(A, now), Ok(allow(0, 60)));
        assert_eq!(limiter.check(A, now), Ok(reject(60, 60, true)));
        assert_eq!(limiter.check(B, now), Ok(allow(0, 60)));
    }

    #[test]
    fn a_rejection_is_logged_at_most_once_per_window() {
        // T = τ = w = 60 s.
        let limiter = RateLimiter::new(1, 1, 1000);
        let start = Instant::now();
        limiter.check(A, start).unwrap();
        assert_eq!(limiter.check(A, start), Ok(reject(60, 60, true)));
        assert_eq!(
            limiter.check(A, start + secs(30)),
            Ok(reject(30, 30, false))
        );
        assert_eq!(limiter.check(A, start + secs(60)), Ok(allow(0, 60)));
        assert_eq!(limiter.check(A, start + secs(60)), Ok(reject(60, 60, true)));
    }

    #[test]
    fn headers_follow_the_draft() {
        let defaults = RateLimiter::new(600, 120, 1000);
        assert_eq!(defaults.policy(), r#""general";q=120;w=12"#);
        assert_eq!(
            RateLimiter::new(120, 7, 1000).policy(),
            r#""general";q=7;w=4"#
        );
        assert_eq!(allow(117, 1).header(), r#""general";r=117;t=1"#);
        assert_eq!(reject(3, 12, true).header(), r#""general";r=0;t=12"#);
    }

    #[test]
    fn a_full_table_refuses_new_clients_until_entries_expire() {
        // One entry per shard; T = τ = 60 s.
        let limiter = RateLimiter::new(1, 1, SHARDS);
        let start = Instant::now();
        let keys: Vec<ClientKey> = (0..=255u8).map(|i| ClientKey::V4([10, 0, 0, i])).collect();
        let mut admitted = Vec::new();
        let mut refused = 0;
        for key in &keys {
            match limiter.check(*key, start) {
                Ok(Decision::Allow { .. }) => admitted.push(*key),
                Err(LimitError::Full) => refused += 1,
                other => panic!("{key}: {other:?}"),
            }
        }
        assert!(admitted.len() <= SHARDS, "{} admitted", admitted.len());
        assert_eq!(admitted.len() + refused, keys.len());
        // Tracked clients keep their state; they are never refused as new.
        for key in &admitted {
            assert!(matches!(
                limiter.check(*key, start),
                Ok(Decision::Reject { .. })
            ));
        }
        // Once every entry equals a fresh one, it is dropped to make room.
        let later = start + secs(60);
        let newcomer = keys
            .iter()
            .find(|k| !admitted.contains(k))
            .expect("some key was refused");
        assert_eq!(limiter.check(*newcomer, later), Ok(allow(0, 60)));
    }

    #[test]
    fn limit_errors_fail_closed() {
        let internal = ApiError::from(LimitError::Poisoned);
        assert_eq!(internal.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(internal.code(), "INTERNAL_ERROR");
        let full = ApiError::from(LimitError::Full);
        assert_eq!(full.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(full.code(), "SERVICE_UNAVAILABLE");
        assert_eq!(full.retry_after(), Some(1));
    }

    /// AC 20: a poisoned shard denies its clients with `500`; other shards still decide.
    #[test]
    fn a_poisoned_shard_is_a_500() {
        let limiter = RateLimiter::new(60, 5, 1000);
        let now = Instant::now();
        let shard = limiter.shard_index(&A);
        std::thread::scope(|s| {
            let poisoned = s
                .spawn(|| {
                    let _guard = limiter.shards[shard].lock().unwrap();
                    panic!("poison the shard");
                })
                .join();
            assert!(poisoned.is_err());
        });
        let err = limiter.check(A, now).unwrap_err();
        assert_eq!(err, LimitError::Poisoned);
        assert_eq!(
            ApiError::from(err).status(),
            StatusCode::INTERNAL_SERVER_ERROR
        );
        let other = (0..=255u8)
            .map(|i| ClientKey::V4([192, 0, 2, i]))
            .find(|k| limiter.shard_index(k) != shard)
            .expect("a key in another shard");
        assert_eq!(limiter.check(other, now), Ok(allow(4, 1)));
    }
}
