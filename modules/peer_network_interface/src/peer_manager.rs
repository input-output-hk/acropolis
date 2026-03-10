//! Cold peer set management and peer-sharing rate limiting for the PNI module.
#![deny(missing_docs)]

use std::{
    collections::{HashMap, HashSet},
    time::{Duration, Instant},
};

use rand::seq::IteratorRandom;

use crate::network::PeerId;

/// The per-peer cooldown between peer-sharing queries (5 minutes, hardcoded).
const PEER_SHARING_COOLDOWN: Duration = Duration::from_secs(5 * 60);

/// Configuration for `PeerManager`, controlling peer set sizes and discovery behaviour.
///
/// # TODO(warm-peers): Add `max_warm_peers: usize` (default 10) to limit the warm tier
/// once the warm peer pool is added. The warm pool sits between cold and hot — peers are
/// promoted cold→warm (probe connect only) before being elevated to hot (full protocols).
#[derive(Clone, Debug)]
pub struct PeerManagerConfig {
    /// Target peer count; also defines the cold cap (`cold_cap = 4 × target_peer_count`).
    pub target_peer_count: usize,
    /// Minimum number of active hot connections to maintain.
    pub min_hot_peers: usize,
    /// Whether peer-sharing discovery is enabled.
    pub peer_sharing_enabled: bool,
    /// Seconds between random hot-peer churn events.
    pub churn_interval_secs: u64,
    /// Timeout in seconds for a full peer-sharing exchange.
    pub peer_sharing_timeout_secs: u64,
}

impl Default for PeerManagerConfig {
    fn default() -> Self {
        Self {
            target_peer_count: 15,
            min_hot_peers: 3,
            peer_sharing_enabled: true,
            churn_interval_secs: 600,
            peer_sharing_timeout_secs: 10,
        }
    }
}

/// Manages cold/failed peer sets and rate-limiting for peer-sharing exchanges.
///
/// Cold peers are known addresses not currently connected. Failed peers are session-scoped
/// blacklisted addresses that failed promotion. Sharing cooldown tracks when each hot peer
/// was last queried to prevent excessive polling.
///
/// # TODO(warm-peers): When warm tier is added, this struct will need a `warm_peers` set
/// and methods to track warm→hot and warm→cold transitions. The cold cap enforcement
/// and `add_discovered` logic extend naturally to a warm pool.
///
/// # TODO(ledger-peers): A `ledger_peers: HashSet<String>` field and `seed_from_ledger()`
/// method can be added here to accept relay addresses from SPO State epoch boundaries.
/// The `add_discovered` method serves as the template for `seed_from_ledger`.
pub struct PeerManager {
    /// Known peer addresses not currently connected.
    cold_peers: HashSet<String>,
    /// Session blacklist: addresses that failed cold→hot promotion.
    failed_peers: HashSet<String>,
    /// Per-hot-peer cooldown: last peer-sharing query time.
    sharing_cooldown: HashMap<PeerId, Instant>,
    config: PeerManagerConfig,
}

impl PeerManager {
    /// Create a new `PeerManager` with the given configuration.
    pub fn new(config: PeerManagerConfig) -> Self {
        Self {
            cold_peers: HashSet::new(),
            failed_peers: HashSet::new(),
            sharing_cooldown: HashMap::new(),
            config,
        }
    }

    /// Seed the cold peer set from the static `node_addresses` config, excluding any
    /// address already in the `hot` set.
    pub fn seed(&mut self, addresses: &[String], hot: &HashSet<String>) {
        for addr in addresses {
            if !hot.contains(addr) && !self.failed_peers.contains(addr) {
                self.cold_peers.insert(addr.clone());
            }
        }
    }

    /// Add addresses discovered via peer-sharing to the cold set.
    ///
    /// Addresses already in cold, hot, or failed sets are silently skipped.
    /// When the cold set is at capacity (`4 × target_peer_count`), one randomly selected
    /// existing cold peer is evicted before inserting the new address (D-010).
    ///
    /// # TODO(ledger-peers): `seed_from_ledger(addresses, hot)` follows the same pattern —
    /// add a sibling method that accepts relay addresses from `SPOStateMessage` and applies
    /// the same deduplication and cap enforcement.
    pub fn add_discovered(&mut self, addresses: Vec<String>, hot: &HashSet<String>) {
        let cold_cap = self.config.target_peer_count * 4;
        for addr in addresses {
            if self.is_known(&addr, hot) {
                continue;
            }
            if self.cold_peers.len() >= cold_cap {
                self.evict_random_cold();
            }
            self.cold_peers.insert(addr);
        }
    }

    /// Remove and return a randomly selected cold peer address for promotion.
    /// Returns `None` if the cold set is empty.
    pub fn take_cold_peer(&mut self) -> Option<String> {
        let addr = self.cold_peers.iter().choose(&mut rand::rng()).cloned()?;
        self.cold_peers.remove(&addr);
        Some(addr)
    }

    /// Remove an address from the cold set when it is promoted to a hot connection.
    pub fn mark_as_promoted(&mut self, address: &str) {
        self.cold_peers.remove(address);
    }

    /// Mark an address as failed (session blacklist).
    ///
    /// The address is removed from the cold set (if present) and added to `failed_peers`.
    /// Failed peers are never promoted or re-discovered until process restart.
    pub fn mark_failed(&mut self, address: String) {
        self.cold_peers.remove(&address);
        self.failed_peers.insert(address);
    }

    /// Returns true if peer-sharing discovery should run.
    ///
    /// Discovery runs continuously whenever `peer_sharing_enabled` and at least one hot
    /// peer exists. It is NOT gated on the cold peer count (D-015).
    pub fn needs_discovery(&self, hot_count: usize) -> bool {
        self.config.peer_sharing_enabled && hot_count > 0
    }

    /// Returns true if the peer has not been queried within the 5-minute cooldown window.
    pub fn can_query(&self, peer_id: PeerId) -> bool {
        match self.sharing_cooldown.get(&peer_id) {
            None => true,
            Some(&last) => last.elapsed() >= PEER_SHARING_COOLDOWN,
        }
    }

    /// Record a peer-sharing query for `peer_id`, starting its cooldown.
    /// Must be called BEFORE the async exchange starts (D-006).
    pub fn record_query(&mut self, peer_id: PeerId) {
        self.sharing_cooldown.insert(peer_id, Instant::now());
    }

    /// Test helper: insert a specific query time for a peer (used to simulate elapsed cooldowns).
    #[doc(hidden)]
    pub fn insert_query_time(&mut self, peer_id: PeerId, at: Instant) {
        self.sharing_cooldown.insert(peer_id, at);
    }

    /// Returns true if a churn demotion is allowed (hot count exceeds `min_hot_peers`).
    pub fn should_churn(&self, hot_count: usize) -> bool {
        hot_count > self.config.min_hot_peers
    }

    /// Returns the number of cold peers currently known.
    pub fn cold_count(&self) -> usize {
        self.cold_peers.len()
    }

    /// Returns true if `addr` is in the cold set.
    pub fn contains_cold(&self, addr: &str) -> bool {
        self.cold_peers.contains(addr)
    }

    /// Returns a reference to the current config.
    pub fn config(&self) -> &PeerManagerConfig {
        &self.config
    }

    fn is_known(&self, addr: &str, hot: &HashSet<String>) -> bool {
        self.cold_peers.contains(addr) || hot.contains(addr) || self.failed_peers.contains(addr)
    }

    fn evict_random_cold(&mut self) {
        if let Some(victim) = self.cold_peers.iter().choose(&mut rand::rng()).cloned() {
            self.cold_peers.remove(&victim);
        }
    }
}
