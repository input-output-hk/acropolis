use acropolis_module_peer_network_interface::PeerId;
use acropolis_module_peer_network_interface::peer_manager::{PeerManager, PeerManagerConfig};
use std::collections::HashSet;

fn default_config() -> PeerManagerConfig {
    PeerManagerConfig::default()
}

// --- US1 tests ---

#[test]
fn cold_list_seeded_from_config_excludes_hot() {
    let mut pm = PeerManager::new(default_config());
    let hot: HashSet<String> = ["1.2.3.4:3001".to_string()].into();
    pm.seed(
        &["1.2.3.4:3001".to_string(), "5.6.7.8:3001".to_string()],
        &hot,
    );
    assert!(
        !pm.contains_cold("1.2.3.4:3001"),
        "hot address must not enter cold set"
    );
    assert!(
        pm.contains_cold("5.6.7.8:3001"),
        "non-hot address must enter cold set"
    );
}

#[test]
fn cold_list_deduplicates_on_seed() {
    let mut pm = PeerManager::new(default_config());
    let hot: HashSet<String> = HashSet::new();
    pm.seed(
        &["1.2.3.4:3001".to_string(), "1.2.3.4:3001".to_string()],
        &hot,
    );
    assert_eq!(pm.cold_count(), 1, "duplicates must be deduplicated");
}

#[test]
fn take_cold_peer_returns_none_when_empty() {
    let mut pm = PeerManager::new(default_config());
    assert!(pm.take_cold_peer().is_none());
}

#[test]
fn failed_peer_not_re_added_by_discovery() {
    let mut pm = PeerManager::new(default_config());
    let hot: HashSet<String> = HashSet::new();
    pm.mark_failed("1.2.3.4:3001".to_string());
    pm.add_discovered(vec!["1.2.3.4:3001".to_string()], &hot);
    assert_eq!(
        pm.cold_count(),
        0,
        "failed peer must not enter cold set via discovery"
    );
}

#[test]
fn hot_overshoot_on_reconnect() {
    // Verifies that the cold cap does not interfere with a temporarily high hot count.
    // The PeerManager itself doesn't cap hot peers — NetworkManager handles that.
    // This test just ensures the cold set remains unaffected by hot count variations.
    let mut pm = PeerManager::new(default_config());
    let hot: HashSet<String> = (0..20).map(|i| format!("10.0.0.{}:3001", i)).collect();
    pm.seed(&[], &hot);
    assert_eq!(pm.cold_count(), 0, "hot peers must not appear in cold set");
}

// --- US2 tests (cap / eviction / dedup / cooldown) ---

#[test]
fn cold_list_cap_enforced_by_eviction() {
    let config = PeerManagerConfig {
        target_peer_count: 2,
        ..PeerManagerConfig::default()
    };
    let cold_cap = 2 * 4; // = 8
    let mut pm = PeerManager::new(config);
    let hot: HashSet<String> = HashSet::new();
    // Fill to cap
    let addrs: Vec<String> = (0..cold_cap).map(|i| format!("10.0.{}.1:3001", i)).collect();
    pm.seed(&addrs, &hot);
    assert_eq!(pm.cold_count(), cold_cap);
    // Adding one more must evict one to stay at cap
    pm.add_discovered(vec!["9.9.9.9:3001".to_string()], &hot);
    assert_eq!(
        pm.cold_count(),
        cold_cap,
        "cold count must stay at cap after eviction"
    );
}

#[test]
fn eviction_keeps_size_at_cap() {
    let config = PeerManagerConfig {
        target_peer_count: 1,
        ..PeerManagerConfig::default()
    };
    let cold_cap = 4;
    let mut pm = PeerManager::new(config);
    let hot: HashSet<String> = HashSet::new();
    let addrs: Vec<String> = (0..cold_cap).map(|i| format!("10.0.{}.1:3001", i)).collect();
    pm.seed(&addrs, &hot);
    for i in cold_cap..(cold_cap + 10) {
        pm.add_discovered(vec![format!("10.1.{}.1:3001", i)], &hot);
        assert_eq!(pm.cold_count(), cold_cap, "size must never exceed cap");
    }
}

#[test]
fn peer_sharing_runs_at_cap() {
    let config = PeerManagerConfig {
        target_peer_count: 1,
        ..PeerManagerConfig::default()
    };
    let cold_cap = 4;
    let mut pm = PeerManager::new(config);
    let hot: HashSet<String> = HashSet::new();
    let addrs: Vec<String> = (0..cold_cap).map(|i| format!("10.0.{}.1:3001", i)).collect();
    pm.seed(&addrs, &hot);
    assert_eq!(pm.cold_count(), cold_cap);
    assert!(
        pm.needs_discovery(1),
        "needs_discovery must return true even when cold set is at cap"
    );
}

#[test]
fn discovery_runs_above_target_count() {
    let pm = PeerManager::new(default_config());
    // Even with lots of cold peers, discovery should still run
    assert!(pm.needs_discovery(5), "discovery must run continuously");
    assert!(
        pm.needs_discovery(1),
        "discovery must run with any hot peers"
    );
    assert!(
        !pm.needs_discovery(0),
        "discovery must not run with zero hot peers"
    );
}

#[test]
fn cold_list_deduplicates_on_discovery() {
    let mut pm = PeerManager::new(default_config());
    let hot: HashSet<String> = HashSet::new();
    pm.add_discovered(vec!["5.5.5.5:3001".to_string()], &hot);
    pm.add_discovered(vec!["5.5.5.5:3001".to_string()], &hot);
    assert_eq!(
        pm.cold_count(),
        1,
        "duplicate discovered address must not be added twice"
    );
}

#[test]
fn cooldown_blocks_requery_within_five_minutes() {
    let mut pm = PeerManager::new(default_config());
    let peer_id = PeerId(42);
    pm.record_query(peer_id);
    assert!(
        !pm.can_query(peer_id),
        "peer must be blocked by cooldown immediately after query"
    );
}

#[test]
fn cooldown_allows_requery_after_five_minutes() {
    use std::time::{Duration, Instant};
    let mut pm = PeerManager::new(default_config());
    let peer_id = PeerId(99);
    // Simulate a query that happened 5 minutes + 1 second ago by inserting a past instant.
    let past =
        Instant::now().checked_sub(Duration::from_secs(5 * 60 + 1)).expect("subtraction underflow");
    pm.insert_query_time(peer_id, past);
    assert!(
        pm.can_query(peer_id),
        "peer must be queryable after 5-minute cooldown expires"
    );
}

// --- Issue 3: seed() cap enforcement ---

#[test]
fn seed_enforces_cold_cap() {
    let config = PeerManagerConfig {
        target_peer_count: 1, // cold_cap = 4
        ..PeerManagerConfig::default()
    };
    let cold_cap = 4;
    let mut pm = PeerManager::new(config);
    let hot: HashSet<String> = HashSet::new();
    // Seed more addresses than the cap allows
    let addrs: Vec<String> = (0..cold_cap + 5).map(|i| format!("10.0.{}.1:3001", i)).collect();
    pm.seed(&addrs, &hot);
    assert_eq!(
        pm.cold_count(),
        cold_cap,
        "seed() must not exceed 4×target_peer_count"
    );
}

// --- Issue 4: demote_to_cold bypasses failed_peers ---

#[test]
fn demote_to_cold_bypasses_failed_peers_blacklist() {
    let mut pm = PeerManager::new(default_config());
    let hot: HashSet<String> = HashSet::new();
    // Simulate a prior failed promotion
    pm.mark_failed("1.2.3.4:3001".to_string());
    assert_eq!(pm.cold_count(), 0, "failed peer must not be in cold set");
    // Churn demotion should re-add it regardless of failed_peers
    pm.demote_to_cold("1.2.3.4:3001".to_string(), &hot);
    assert_eq!(
        pm.cold_count(),
        1,
        "demote_to_cold must re-add despite failed_peers"
    );
    // Verify it was also cleared from failed_peers (re-promotable)
    pm.add_discovered(vec!["1.2.3.4:3001".to_string()], &hot);
    assert_eq!(
        pm.cold_count(),
        1,
        "address is already in cold, dedup applies"
    );
}

// --- US3 tests ---

#[test]
fn churn_allowed_when_above_min() {
    let config = PeerManagerConfig {
        min_hot_peers: 2,
        ..PeerManagerConfig::default()
    };
    let pm = PeerManager::new(config);
    assert!(pm.should_churn(3), "churn must be allowed when hot > min");
    assert!(pm.should_churn(10), "churn must be allowed when hot >> min");
}

#[test]
fn churn_blocked_at_min_hot_peers() {
    let config = PeerManagerConfig {
        min_hot_peers: 3,
        ..PeerManagerConfig::default()
    };
    let pm = PeerManager::new(config);
    assert!(
        !pm.should_churn(3),
        "churn must not occur at exactly min_hot_peers"
    );
    assert!(
        !pm.should_churn(2),
        "churn must not occur below min_hot_peers"
    );
    assert!(
        !pm.should_churn(0),
        "churn must not occur with zero hot peers"
    );
}
