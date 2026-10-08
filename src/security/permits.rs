//! Non-waiting concurrency bounds (FR-030, FR-032, data-model §3.4).

use std::sync::Arc;
use std::sync::atomic::AtomicUsize;

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
    pub fn new(_max: usize) -> Arc<Self> {
        todo!()
    }

    pub fn max(&self) -> usize {
        self.max
    }

    pub fn in_use(&self) -> usize {
        todo!()
    }

    /// A permit, or `None` when all are in use. Never waits.
    pub fn try_acquire(self: &Arc<Self>) -> Option<Permit> {
        let _ = &self.in_use;
        todo!()
    }
}

impl Drop for Permit {
    fn drop(&mut self) {
        let _ = &self.permits;
        todo!()
    }
}
