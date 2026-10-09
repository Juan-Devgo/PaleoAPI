//! Non-waiting concurrency bounds (FR-030, FR-032, data-model §3.4).

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Debug)]
pub struct Permits {
    in_use: AtomicUsize,
    max: usize,
}

/// Holds one permit until dropped.
#[derive(Debug)]
pub struct Permit {
    permits: Arc<Permits>,
}

impl Permits {
    pub fn new(max: usize) -> Arc<Self> {
        Arc::new(Self {
            in_use: AtomicUsize::new(0),
            max,
        })
    }

    pub fn max(&self) -> usize {
        self.max
    }

    pub fn in_use(&self) -> usize {
        self.in_use.load(Ordering::SeqCst)
    }

    /// A permit, or `None` when all are in use. Never waits.
    pub fn try_acquire(self: &Arc<Self>) -> Option<Permit> {
        self.in_use
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
                (n < self.max).then_some(n + 1)
            })
            .ok()
            .map(|_| Permit {
                permits: Arc::clone(self),
            })
    }
}

impl Drop for Permit {
    fn drop(&mut self) {
        self.permits.in_use.fetch_sub(1, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use std::thread;

    use super::*;

    #[test]
    fn permits_are_bounded_and_released_on_drop() {
        let permits = Permits::new(2);
        assert_eq!(permits.max(), 2);
        let a = permits.try_acquire().expect("first permit");
        let b = permits.try_acquire().expect("second permit");
        assert_eq!(permits.in_use(), 2);
        assert!(permits.try_acquire().is_none(), "no third permit");
        assert_eq!(permits.in_use(), 2, "a refused acquire takes nothing");
        drop(a);
        assert_eq!(permits.in_use(), 1);
        let c = permits.try_acquire().expect("released permit is reusable");
        drop((b, c));
        assert_eq!(permits.in_use(), 0);
    }

    #[test]
    fn a_single_permit_bound() {
        let permits = Permits::new(1);
        let held = permits.try_acquire().unwrap();
        assert!(permits.try_acquire().is_none());
        drop(held);
        assert!(permits.try_acquire().is_some());
    }

    #[test]
    fn concurrent_acquires_never_exceed_the_bound() {
        let permits = Permits::new(3);
        thread::scope(|s| {
            for _ in 0..16 {
                let permits = Arc::clone(&permits);
                s.spawn(move || {
                    let mut held = Vec::new();
                    for _ in 0..1000 {
                        if let Some(p) = permits.try_acquire() {
                            assert!(permits.in_use() <= 3);
                            held.push(p);
                        }
                        if held.len() > 1 {
                            held.clear();
                        }
                    }
                });
            }
        });
        assert_eq!(permits.in_use(), 0, "every permit was dropped");
    }

    #[test]
    fn permits_outlive_the_borrow_that_acquired_them() {
        let permits = Permits::new(1);
        let permit = {
            let shared = Arc::clone(&permits);
            shared.try_acquire().unwrap()
        };
        assert_eq!(permits.in_use(), 1);
        drop(permit);
        assert_eq!(permits.in_use(), 0);
    }
}
