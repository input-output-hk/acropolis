// Unit tests for health state transitions

use spec_test::state::{HealthStatus, State};

#[test]
fn test_initial_state() {
    let state = State::new();
    assert_eq!(state.status, HealthStatus::Starting);
    assert_eq!(state.tip_height, 0);
    assert_eq!(state.era, "conway");
}

#[test]
fn test_transition_starting_to_loading() {
    let mut state = State::new();
    state.start_loading();
    assert_eq!(state.status, HealthStatus::LoadingSnapshot);
}

#[test]
fn test_transition_loading_to_ready() {
    let mut state = State::new();
    state.start_loading();
    state.set_ready();
    assert_eq!(state.status, HealthStatus::Ready);
    assert!(state.ready_at.is_some());
}

#[test]
fn test_transition_loading_to_catching_up_to_ready() {
    let mut state = State::new();
    state.start_loading();
    state.start_catching_up();
    assert_eq!(state.status, HealthStatus::CatchingUp);
    state.set_ready();
    assert_eq!(state.status, HealthStatus::Ready);
}

#[test]
fn test_boot_duration() {
    let mut state = State::new();
    state.start_loading();
    state.set_ready();

    let duration = state.boot_duration();
    assert!(duration.is_some());
    assert!(duration.unwrap().as_secs() < 1); // Should be very fast in tests
}

#[test]
fn test_error_transition() {
    let mut state = State::new();
    state.set_error();
    assert_eq!(state.status, HealthStatus::Error);
}
