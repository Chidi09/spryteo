//! Cooperative cancellation and deadline enforcement (ROADMAP §3.1).
//!
//! The pipeline is CPU-bound and single-threaded per conversion, so a
//! wrapper-only timeout (spawn + join with a deadline) would leave the
//! tracing work running to completion on a detached thread. Instead every
//! long-running stage takes a [`CancelToken`] and polls it at bounded
//! intervals, so a timed-out conversion actually stops burning CPU.
//!
//! Time is supplied through the [`Clock`] trait rather than read directly
//! from `std::time::Instant`, for two reasons: `Instant::now()` is not
//! available on `wasm32-unknown-unknown`, and tests need to drive a
//! deadline deterministically instead of sleeping.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use crate::error::SpryteoError;

/// A monotonic millisecond clock.
///
/// Implementations must be monotonic (never return a smaller value than a
/// previous call) so deadline arithmetic cannot wrap.
pub trait Clock: Send + Sync {
    /// Milliseconds elapsed since some fixed, implementation-defined epoch.
    fn now_ms(&self) -> u64;
}

/// Real monotonic clock backed by `std::time::Instant`.
///
/// Not available on wasm32, where `Instant::now()` panics; the WASM surface
/// supplies its own `Clock` over `performance.now()`/`Date.now()`.
#[cfg(not(target_arch = "wasm32"))]
pub struct StdClock {
    start: std::time::Instant,
}

#[cfg(not(target_arch = "wasm32"))]
impl StdClock {
    pub fn new() -> Self {
        Self {
            start: std::time::Instant::now(),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Default for StdClock {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Clock for StdClock {
    fn now_ms(&self) -> u64 {
        self.start.elapsed().as_millis() as u64
    }
}

/// A clock driven explicitly by tests. Starts at 0.
pub struct ManualClock {
    now: AtomicU64,
}

impl ManualClock {
    pub fn new() -> Self {
        Self {
            now: AtomicU64::new(0),
        }
    }

    /// Move the clock forward by `ms`.
    pub fn advance(&self, ms: u64) {
        self.now.fetch_add(ms, Ordering::SeqCst);
    }
}

impl Default for ManualClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for ManualClock {
    fn now_ms(&self) -> u64 {
        self.now.load(Ordering::SeqCst)
    }
}

/// How often a stage should poll a token inside a tight loop.
///
/// Polling every iteration would make the atomic load a measurable share of
/// the inner loop cost on multi-million-pixel images; polling every 4096
/// iterations bounds the overshoot to well under a millisecond of work
/// while keeping the load off the hot path.
pub const POLL_INTERVAL: usize = 4096;

/// Shared cancellation state: an explicit cancel flag plus an optional
/// deadline.
///
/// Cloning a token shares the same underlying state, so a token handed to a
/// worker thread observes cancellation requested from the caller.
#[derive(Clone)]
pub struct CancelToken {
    inner: Option<Arc<Inner>>,
}

struct Inner {
    cancelled: AtomicBool,
    /// Absolute deadline in `clock` milliseconds, if a timeout was set.
    deadline_ms: Option<u64>,
    clock: Arc<dyn Clock>,
}

impl CancelToken {
    /// A token that never cancels. Zero-cost: `is_cancelled` short-circuits
    /// on the `None` discriminant without touching an atomic.
    pub fn none() -> Self {
        Self { inner: None }
    }

    /// A token that can be cancelled explicitly but has no deadline.
    pub fn new(clock: Arc<dyn Clock>) -> Self {
        Self {
            inner: Some(Arc::new(Inner {
                cancelled: AtomicBool::new(false),
                deadline_ms: None,
                clock,
            })),
        }
    }

    /// A token that trips automatically `timeout_ms` after `clock`'s current
    /// reading.
    ///
    /// A timeout of `0` means "already expired": the very first check fails.
    /// This is deliberate — it gives callers a way to assert the deadline
    /// path is wired without racing a real clock.
    pub fn with_timeout(clock: Arc<dyn Clock>, timeout_ms: u64) -> Self {
        let deadline = clock.now_ms().saturating_add(timeout_ms);
        Self {
            inner: Some(Arc::new(Inner {
                cancelled: AtomicBool::new(false),
                deadline_ms: Some(deadline),
                clock,
            })),
        }
    }

    /// Build a token from `ConvertOptions::timeout_ms` using the real clock.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn from_timeout_opt(timeout_ms: Option<u64>) -> Self {
        match timeout_ms {
            Some(ms) => Self::with_timeout(Arc::new(StdClock::new()), ms),
            None => Self::none(),
        }
    }

    /// Request cancellation. Idempotent and safe from any thread.
    pub fn cancel(&self) {
        if let Some(inner) = &self.inner {
            inner.cancelled.store(true, Ordering::Relaxed);
        }
    }

    /// Whether this token has been cancelled or its deadline has passed.
    #[inline]
    pub fn is_cancelled(&self) -> bool {
        match &self.inner {
            None => false,
            Some(inner) => {
                if inner.cancelled.load(Ordering::Relaxed) {
                    return true;
                }
                match inner.deadline_ms {
                    Some(deadline) => inner.clock.now_ms() >= deadline,
                    None => false,
                }
            }
        }
    }

    /// Whether the token tripped specifically because its deadline passed
    /// (as opposed to an explicit `cancel()`), which selects between
    /// [`SpryteoError::Timeout`] and [`SpryteoError::Cancelled`].
    fn timed_out(&self) -> bool {
        match &self.inner {
            None => false,
            Some(inner) => match inner.deadline_ms {
                Some(deadline) => inner.clock.now_ms() >= deadline,
                None => false,
            },
        }
    }

    /// Return the appropriate typed error if this token has tripped.
    ///
    /// Stages call this between phases and, via [`Self::check_at`], inside
    /// long loops.
    #[inline]
    pub fn check(&self) -> Result<(), SpryteoError> {
        if self.inner.is_none() {
            return Ok(());
        }
        if self.is_cancelled() {
            if self.timed_out() {
                Err(SpryteoError::Timeout)
            } else {
                Err(SpryteoError::Cancelled)
            }
        } else {
            Ok(())
        }
    }

    /// Poll only every [`POLL_INTERVAL`] iterations. Call with the loop
    /// index; the compiler folds this to a cheap mask test plus an early
    /// `None` check when no token is installed.
    #[inline]
    pub fn check_at(&self, iteration: usize) -> Result<(), SpryteoError> {
        if self.inner.is_none() {
            return Ok(());
        }
        if iteration % POLL_INTERVAL == 0 {
            self.check()
        } else {
            Ok(())
        }
    }
}

impl Default for CancelToken {
    fn default() -> Self {
        Self::none()
    }
}

impl std::fmt::Debug for CancelToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CancelToken")
            .field("installed", &self.inner.is_some())
            .field("cancelled", &self.is_cancelled())
            .finish()
    }
}

#[cfg(test)]
#[path = "cancel_tests.rs"]
mod cancel_tests;
