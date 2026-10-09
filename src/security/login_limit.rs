//! Login limits: per-client and per-username sliding logs (FR-022, data-model §3.3).

use std::collections::{HashMap, VecDeque};
use std::hash::{BuildHasher, RandomState};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use super::client_ip::ClientKey;
use crate::api::error::{ApiError, Overload};

/// Why the limiter could not decide (research R14).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginLimitError {
    /// A table is full of live keys: a new key gets `503`.
    Full,
    /// A lock was poisoned: `500`.
    Poisoned,
}

/// A rejected login (`429`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rejected {
    /// `Retry-After`: whole seconds, at least 1.
    pub retry_after: u64,
    /// First rejection of this reason for this client within the window (FR-039).
    pub log: bool,
}

/// Outcome of the per-client check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admission {
    Allowed,
    Rejected(Rejected),
}

/// Outcome of the per-username check.
#[derive(Debug)]
pub enum Reserve<'a> {
    Reserved(Reservation<'a>),
    Rejected(Rejected),
}

/// The login tables of one process.
#[derive(Debug)]
pub struct LoginLimiter {
    clients: Mutex<HashMap<ClientKey, ClientEntry>>,
    usernames: Mutex<HashMap<u64, UserEntry>>,
    hasher: RandomState,
    per_client: usize,
    per_username: usize,
    window: Duration,
    capacity: usize,
}

#[derive(Debug, Default)]
struct ClientEntry {
    log: VecDeque<Instant>,
    logged: [Option<Instant>; 2],
}

#[derive(Debug, Default)]
struct UserEntry {
    log: VecDeque<Instant>,
    pending: usize,
}

/// A reserved per-username attempt. Dropping it releases the reservation (success, `503`,
/// a cancelled request); [`Reservation::fail`] turns it into a recorded failure.
#[derive(Debug)]
pub struct Reservation<'a> {
    limiter: &'a LoginLimiter,
    id: u64,
    done: bool,
}

/// Whole seconds, rounded up, at least 1.
fn retry_secs(until: Duration) -> u64 {
    u64::try_from(until.as_nanos().div_ceil(1_000_000_000))
        .unwrap_or(u64::MAX)
        .max(1)
}

/// Drops the instants that left the window.
fn prune(log: &mut VecDeque<Instant>, now: Instant, window: Duration) {
    while log
        .front()
        .is_some_and(|&at| now.saturating_duration_since(at) >= window)
    {
        log.pop_front();
    }
}

/// Seconds until the oldest instant leaves the window (`1` for an empty log).
fn retry_after(log: &VecDeque<Instant>, now: Instant, window: Duration) -> u64 {
    log.front().map_or(1, |&oldest| {
        retry_secs((oldest + window).saturating_duration_since(now))
    })
}

/// `true` (and the slot is set to `now`) when the slot is empty or outside the window:
/// one `rate_limited` line per client and reason per window (FR-039).
fn dedupe(slot: &mut Option<Instant>, now: Instant, window: Duration) -> bool {
    let first = slot.is_none_or(|at| now.saturating_duration_since(at) >= window);
    if first {
        *slot = Some(now);
    }
    first
}

impl ClientEntry {
    /// No live instant and no live logged slot: equal to an absent entry.
    fn is_idle(&mut self, now: Instant, window: Duration) -> bool {
        prune(&mut self.log, now, window);
        self.log.is_empty()
            && self
                .logged
                .iter()
                .all(|slot| slot.is_none_or(|at| now.saturating_duration_since(at) >= window))
    }
}

impl UserEntry {
    fn is_idle(&mut self, now: Instant, window: Duration) -> bool {
        prune(&mut self.log, now, window);
        self.log.is_empty() && self.pending == 0
    }
}

fn lock<T>(m: &Mutex<T>) -> Result<MutexGuard<'_, T>, LoginLimitError> {
    m.lock().map_err(|_| LoginLimitError::Poisoned)
}

impl LoginLimiter {
    /// Limits of `per_client` requests and `per_username` failures per `window`; each
    /// table holds up to `capacity` keys.
    pub fn new(per_client: u32, per_username: u32, window: Duration, capacity: usize) -> Self {
        Self {
            clients: Mutex::new(HashMap::new()),
            usernames: Mutex::new(HashMap::new()),
            hasher: RandomState::new(),
            per_client: per_client.max(1) as usize,
            per_username: per_username.max(1) as usize,
            window,
            capacity: capacity.max(1),
        }
    }

    pub fn per_client(&self) -> u32 {
        u32::try_from(self.per_client).unwrap_or(u32::MAX)
    }

    pub fn per_username(&self) -> u32 {
        u32::try_from(self.per_username).unwrap_or(u32::MAX)
    }

    pub fn window(&self) -> Duration {
        self.window
    }

    /// Plan §Login step 1: records the request unless the client's log is full.
    /// A rejected request is not recorded.
    pub fn admit_client(&self, key: ClientKey, now: Instant) -> Result<Admission, LoginLimitError> {
        let mut clients = lock(&self.clients)?;
        if !clients.contains_key(&key) && clients.len() >= self.capacity {
            clients.retain(|_, entry| !entry.is_idle(now, self.window));
            if clients.len() >= self.capacity {
                return Err(LoginLimitError::Full);
            }
        }
        let entry = clients.entry(key).or_default();
        prune(&mut entry.log, now, self.window);
        if entry.log.len() >= self.per_client {
            return Ok(Admission::Rejected(Rejected {
                retry_after: retry_after(&entry.log, now, self.window),
                log: dedupe(&mut entry.logged[0], now, self.window),
            }));
        }
        entry.log.push_back(now);
        Ok(Admission::Allowed)
    }

    /// Plan §Login step 4: reserves an attempt for `username`, or rejects when the
    /// failures in the window plus the unfinished attempts reach the limit.
    pub fn reserve(
        &self,
        key: ClientKey,
        username: &str,
        now: Instant,
    ) -> Result<Reserve<'_>, LoginLimitError> {
        let id = self.hasher.hash_one(username);
        let retry = {
            let mut usernames = lock(&self.usernames)?;
            if !usernames.contains_key(&id) && usernames.len() >= self.capacity {
                usernames.retain(|_, entry| !entry.is_idle(now, self.window));
                if usernames.len() >= self.capacity {
                    return Err(LoginLimitError::Full);
                }
            }
            let entry = usernames.entry(id).or_default();
            prune(&mut entry.log, now, self.window);
            if entry.log.len() + entry.pending >= self.per_username {
                Some(retry_after(&entry.log, now, self.window))
            } else {
                entry.pending += 1;
                None
            }
        };
        let Some(retry_after) = retry else {
            return Ok(Reserve::Reserved(Reservation {
                limiter: self,
                id,
                done: false,
            }));
        };
        // The client entry exists unless the window expired it; then there is nothing
        // to dedupe against and the rejection is logged.
        let log = lock(&self.clients)?
            .get_mut(&key)
            .is_none_or(|entry| dedupe(&mut entry.logged[1], now, self.window));
        Ok(Reserve::Rejected(Rejected { retry_after, log }))
    }

    /// Drops one pending attempt; with `failed_at`, records a failure in its place.
    /// Returns whether the log is then full. Ignores a poisoned lock: the next
    /// check reports it.
    fn finish(&self, id: u64, failed_at: Option<Instant>) -> bool {
        let mut usernames = self
            .usernames
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let Some(entry) = usernames.get_mut(&id) else {
            return false;
        };
        entry.pending = entry.pending.saturating_sub(1);
        let Some(now) = failed_at else {
            return false;
        };
        prune(&mut entry.log, now, self.window);
        entry.log.push_back(now);
        entry.log.len() >= self.per_username
    }
}

impl Reservation<'_> {
    /// Records the failure and drops the reservation. `true`: this failure filled the
    /// username's log (`login_locked`).
    pub fn fail(mut self, now: Instant) -> bool {
        self.done = true;
        self.limiter.finish(self.id, Some(now))
    }
}

impl Drop for Reservation<'_> {
    fn drop(&mut self) {
        if !self.done {
            self.limiter.finish(self.id, None);
        }
    }
}

/// Fail closed (research R14): a full table is load (`503`), a poisoned lock a defect (`500`).
impl From<LoginLimitError> for ApiError {
    fn from(err: LoginLimitError) -> Self {
        match err {
            LoginLimitError::Full => ApiError::busy(Overload::Limiter),
            LoginLimitError::Poisoned => ApiError::internal(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: ClientKey = ClientKey::V4([203, 0, 113, 7]);
    const B: ClientKey = ClientKey::V4([198, 51, 100, 1]);
    const WINDOW: Duration = Duration::from_secs(900);

    fn secs(s: u64) -> Duration {
        Duration::from_secs(s)
    }

    fn limiter(per_client: u32, per_username: u32) -> LoginLimiter {
        LoginLimiter::new(per_client, per_username, WINDOW, 1000)
    }

    fn rejected(retry_after: u64, log: bool) -> Admission {
        Admission::Rejected(Rejected { retry_after, log })
    }

    #[track_caller]
    fn reserved<'a>(r: Result<Reserve<'a>, LoginLimitError>) -> Reservation<'a> {
        match r.unwrap() {
            Reserve::Reserved(reservation) => reservation,
            Reserve::Rejected(rej) => panic!("rejected: {rej:?}"),
        }
    }

    #[track_caller]
    fn username_rejection(r: Result<Reserve<'_>, LoginLimitError>) -> Rejected {
        match r.unwrap() {
            Reserve::Rejected(rej) => rej,
            Reserve::Reserved(_) => panic!("reserved"),
        }
    }

    #[test]
    fn the_client_log_holds_ten_requests_per_window() {
        let l = limiter(10, 20);
        let t0 = Instant::now();
        for i in 0..10 {
            assert_eq!(
                l.admit_client(A, t0 + secs(i)).unwrap(),
                Admission::Allowed,
                "request {i}"
            );
        }
        // Oldest at t0: free again at t0 + 900 s.
        assert_eq!(
            l.admit_client(A, t0 + secs(10)).unwrap(),
            rejected(890, true)
        );
        // Retry-After shrinks as time passes and is never below 1.
        assert_eq!(
            l.admit_client(A, t0 + secs(899)).unwrap(),
            rejected(1, false)
        );
        // Rejected requests are not recorded: the second oldest frees the next slot.
        assert_eq!(
            l.admit_client(A, t0 + secs(900)).unwrap(),
            Admission::Allowed
        );
        assert_eq!(
            l.admit_client(A, t0 + secs(900)).unwrap(),
            rejected(1, false)
        );
        assert_eq!(
            l.admit_client(A, t0 + secs(901)).unwrap(),
            Admission::Allowed
        );
    }

    #[test]
    fn retry_after_rounds_up_to_whole_seconds() {
        let l = limiter(1, 20);
        let t0 = Instant::now();
        assert_eq!(l.admit_client(A, t0).unwrap(), Admission::Allowed);
        let r = l.admit_client(A, t0 + Duration::from_millis(500)).unwrap();
        assert_eq!(r, rejected(900, true), "899.5 s rounds up");
    }

    #[test]
    fn clients_are_counted_apart() {
        let l = limiter(1, 20);
        let t0 = Instant::now();
        assert_eq!(l.admit_client(A, t0).unwrap(), Admission::Allowed);
        assert!(matches!(
            l.admit_client(A, t0).unwrap(),
            Admission::Rejected(_)
        ));
        assert_eq!(l.admit_client(B, t0).unwrap(), Admission::Allowed);
    }

    #[test]
    fn the_username_log_counts_failures_not_attempts() {
        let l = limiter(100, 3);
        let t0 = Instant::now();
        for i in 0..2 {
            // A released reservation (success or 503) leaves no trace.
            drop(reserved(l.reserve(A, "alice", t0 + secs(i))));
        }
        for i in 0..2 {
            assert!(!reserved(l.reserve(A, "alice", t0 + secs(i))).fail(t0 + secs(i)));
        }
        // Third failure fills the log: locked.
        assert!(reserved(l.reserve(A, "alice", t0 + secs(2))).fail(t0 + secs(2)));
        let rej = username_rejection(l.reserve(A, "alice", t0 + secs(3)));
        assert_eq!(rej.retry_after, 897, "oldest failure at t0 + 900 s");
        // The correct password is refused too: the check precedes verification.
        assert!(matches!(
            l.reserve(B, "alice", t0 + secs(4)).unwrap(),
            Reserve::Rejected(_)
        ));
        // Other usernames are unaffected, from the same client.
        drop(reserved(l.reserve(A, "bob", t0 + secs(3))));
        // One second before the oldest failure expires, then once it has.
        assert_eq!(
            username_rejection(l.reserve(A, "alice", t0 + secs(899))).retry_after,
            1
        );
        drop(reserved(l.reserve(A, "alice", t0 + secs(900))));
    }

    #[test]
    fn reservations_count_against_the_limit_until_released() {
        let l = limiter(100, 2);
        let t0 = Instant::now();
        let first = reserved(l.reserve(A, "alice", t0));
        let second = reserved(l.reserve(B, "alice", t0));
        // Parallel guesses cannot overshoot: two pending fill a limit of two.
        let rej = username_rejection(l.reserve(A, "alice", t0));
        assert_eq!(rej.retry_after, 1, "nothing recorded yet: retry shortly");
        drop(first);
        drop(reserved(l.reserve(A, "alice", t0)));
        assert!(!second.fail(t0));
        // One failure and one new pending: full again.
        let third = reserved(l.reserve(A, "alice", t0));
        assert!(matches!(
            l.reserve(A, "alice", t0).unwrap(),
            Reserve::Rejected(_)
        ));
        assert!(third.fail(t0), "second failure fills the log");
    }

    #[test]
    fn usernames_are_keyed_by_hash_so_any_text_costs_the_same() {
        let l = limiter(100, 1);
        let t0 = Instant::now();
        let long = "x".repeat(100_000);
        for name in ["", "Not A Slug\n", "nul\u{0}name", long.as_str()] {
            assert!(reserved(l.reserve(A, name, t0)).fail(t0), "{name:?}");
            assert!(
                matches!(l.reserve(A, name, t0).unwrap(), Reserve::Rejected(_)),
                "{name:?} counted"
            );
        }
        assert_eq!(l.usernames.lock().unwrap().len(), 4);
    }

    #[test]
    fn rate_limited_is_logged_once_per_client_per_reason_per_window() {
        let l = limiter(1, 1);
        let t0 = Instant::now();
        assert_eq!(l.admit_client(A, t0).unwrap(), Admission::Allowed);
        assert_eq!(
            l.admit_client(A, t0 + secs(1)).unwrap(),
            rejected(899, true)
        );
        assert_eq!(
            l.admit_client(A, t0 + secs(2)).unwrap(),
            rejected(898, false)
        );

        // The username reason has its own slot on the same client entry.
        assert!(reserved(l.reserve(A, "alice", t0 + secs(3))).fail(t0 + secs(3)));
        assert!(username_rejection(l.reserve(A, "alice", t0 + secs(4))).log);
        assert!(!username_rejection(l.reserve(A, "alice", t0 + secs(5))).log);

        // Another client logs its own first rejection.
        assert_eq!(l.admit_client(B, t0).unwrap(), Admission::Allowed);
        assert_eq!(l.admit_client(B, t0).unwrap(), rejected(900, true));

        // Past the window the next rejection logs again.
        assert_eq!(
            l.admit_client(A, t0 + secs(900)).unwrap(),
            Admission::Allowed
        );
        assert_eq!(
            l.admit_client(A, t0 + secs(901)).unwrap(),
            rejected(899, true),
            "slot set at t0 + 1 s is older than the window"
        );
    }

    #[test]
    fn expired_entries_are_removed_and_the_tables_are_bounded() {
        let l = LoginLimiter::new(1, 1, WINDOW, 2);
        let t0 = Instant::now();
        let c = |n: u8| ClientKey::V4([10, 0, 0, n]);
        assert_eq!(l.admit_client(c(1), t0).unwrap(), Admission::Allowed);
        assert_eq!(l.admit_client(c(2), t0).unwrap(), Admission::Allowed);
        assert_eq!(l.admit_client(c(3), t0), Err(LoginLimitError::Full));
        // Known clients still get an answer when the table is full.
        assert!(matches!(
            l.admit_client(c(1), t0 + secs(1)).unwrap(),
            Admission::Rejected(_)
        ));
        // Entries with an empty log and no live logged slot are dropped to make room.
        assert_eq!(
            l.admit_client(c(3), t0 + secs(901)).unwrap(),
            Admission::Allowed
        );
        assert!(l.clients.lock().unwrap().len() <= 2);

        drop(reserved(l.reserve(c(3), "a", t0)));
        assert!(reserved(l.reserve(c(3), "b", t0)).fail(t0));
        assert!(reserved(l.reserve(c(3), "c", t0 + secs(1))).fail(t0 + secs(1)));
        assert!(matches!(
            l.reserve(c(3), "d", t0 + secs(2)),
            Err(LoginLimitError::Full)
        ));
        drop(reserved(l.reserve(c(3), "d", t0 + secs(902))));
        assert!(l.usernames.lock().unwrap().len() <= 2);
    }

    #[test]
    fn a_poisoned_lock_is_an_internal_error() {
        let l = limiter(1, 1);
        std::thread::scope(|s| {
            let _ = s
                .spawn(|| {
                    let _guard = l.clients.lock().unwrap();
                    panic!("poison");
                })
                .join();
        });
        assert_eq!(
            l.admit_client(A, Instant::now()),
            Err(LoginLimitError::Poisoned)
        );
        assert_eq!(
            ApiError::from(LoginLimitError::Poisoned).status().as_u16(),
            500
        );
        assert_eq!(ApiError::from(LoginLimitError::Full).status().as_u16(), 503);
    }
}
