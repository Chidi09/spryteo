use super::*;

#[test]
fn none_token_never_cancels() {
    let t = CancelToken::none();
    assert!(!t.is_cancelled());
    assert!(t.check().is_ok());
    t.cancel();
    assert!(!t.is_cancelled(), "none() token ignores cancel()");
}

#[test]
fn explicit_cancel_reports_cancelled_not_timeout() {
    let t = CancelToken::new(Arc::new(ManualClock::new()));
    assert!(t.check().is_ok());
    t.cancel();
    assert!(matches!(t.check(), Err(SpryteoError::Cancelled)));
}

#[test]
fn deadline_trips_only_after_clock_advances() {
    let clock = Arc::new(ManualClock::new());
    let t = CancelToken::with_timeout(clock.clone(), 100);
    assert!(t.check().is_ok());
    clock.advance(99);
    assert!(t.check().is_ok(), "99ms < 100ms deadline");
    clock.advance(1);
    assert!(matches!(t.check(), Err(SpryteoError::Timeout)));
}

#[test]
fn zero_timeout_is_immediately_expired() {
    let t = CancelToken::with_timeout(Arc::new(ManualClock::new()), 0);
    assert!(matches!(t.check(), Err(SpryteoError::Timeout)));
}

#[test]
fn check_at_polls_only_on_interval_boundaries() {
    let clock = Arc::new(ManualClock::new());
    let t = CancelToken::with_timeout(clock.clone(), 10);
    clock.advance(50); // deadline is long past

    // Between poll points the token is not consulted at all.
    assert!(t.check_at(1).is_ok());
    assert!(t.check_at(POLL_INTERVAL - 1).is_ok());
    // On a boundary it reports the timeout.
    assert!(matches!(t.check_at(POLL_INTERVAL), Err(SpryteoError::Timeout)));
    assert!(matches!(t.check_at(0), Err(SpryteoError::Timeout)));
}

#[test]
fn check_at_on_none_token_is_ok_at_every_index() {
    let t = CancelToken::none();
    for i in 0..(POLL_INTERVAL * 2 + 1) {
        assert!(t.check_at(i).is_ok());
    }
}

#[test]
fn clones_share_cancellation_state() {
    let t = CancelToken::new(Arc::new(ManualClock::new()));
    let clone = t.clone();
    t.cancel();
    assert!(clone.is_cancelled(), "clone observes cancel via shared Arc");
}

#[test]
fn cancel_wins_over_unexpired_deadline() {
    let clock = Arc::new(ManualClock::new());
    let t = CancelToken::with_timeout(clock, 1_000);
    t.cancel();
    assert!(matches!(t.check(), Err(SpryteoError::Cancelled)));
}

#[test]
fn deadline_reported_as_timeout_even_when_also_cancelled() {
    let clock = Arc::new(ManualClock::new());
    let t = CancelToken::with_timeout(clock.clone(), 10);
    t.cancel();
    clock.advance(10);
    // Deadline passed: timeout is the more specific, more useful error.
    assert!(matches!(t.check(), Err(SpryteoError::Timeout)));
}

#[test]
fn from_timeout_opt_none_is_inert() {
    let t = CancelToken::from_timeout_opt(None);
    assert!(!t.is_cancelled());
}

#[test]
fn from_timeout_opt_zero_trips_immediately() {
    let t = CancelToken::from_timeout_opt(Some(0));
    assert!(matches!(t.check(), Err(SpryteoError::Timeout)));
}

#[test]
fn manual_clock_is_monotonic_under_advance() {
    let c = ManualClock::new();
    let mut last = c.now_ms();
    for _ in 0..10 {
        c.advance(7);
        let now = c.now_ms();
        assert!(now > last);
        last = now;
    }
}
