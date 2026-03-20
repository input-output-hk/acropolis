//! Cold peer set management and peer-sharing rate limiting for the PNI module.
use std::{
    collections::{HashMap, HashSet},
    time::{Duration, Instant},
};

use rand::seq::IteratorRandom;

use crate::network::PeerId;

/// Configuration for `PeerManager`, controlling peer set sizes and discovery behaviour.
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
    /// Per-peer cooldown in seconds between peer-sharing queries.
    pub peer_sharing_cooldown_secs: u64,
}

impl Default for PeerManagerConfig {
    fn default() -> Self {
        Self {
            target_peer_count: 15,
            min_hot_peers: 3,
            peer_sharing_enabled: true,
            churn_interval_secs: 600,
            peer_sharing_timeout_secs: 10,
            peer_sharing_cooldown_secs: 30,
        }
    }
}

/// Manages cold/failed peer sets and rate-limiting for peer-sharing exchanges.
///
/// Cold peers are known addresses not currently connected. Failed peers are session-scoped
/// blacklisted addresses that failed promotion. Sharing cooldown tracks when each hot peer
/// was last queried to prevent excessive polling.
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
    /// address already in the `hot` set. Applies the same cap-and-evict policy as
    /// `add_discovered`.
    pub fn seed(&mut self, addresses: &[String], hot: &HashSet<String>) {
        let cold_cap = self.config.target_peer_count * 4;
        for addr in addresses {
            if !hot.contains(addr)
                && !self.failed_peers.contains(addr)
                && !self.cold_peers.contains(addr.as_str())
            {
                if self.cold_peers.len() >= cold_cap {
                    self.evict_random_cold();
                }
                self.cold_peers.insert(addr.clone());
            }
        }
    }

    /// Add addresses discovered via peer-sharing to the cold set.
    ///
    /// Addresses already in cold, hot, or failed sets are silently skipped.
    /// When the cold set is at capacity (`4 × target_peer_count`), one randomly selected
    /// existing cold peer is evicted before inserting the new address.
    pub fn add_discovered(&mut self, addresses: Vec<String>, hot: &HashSet<String>) -> usize {
        let cold_cap = self.config.target_peer_count * 4;
        let mut added = 0usize;
        for addr in addresses {
            if self.is_known(&addr, hot) {
                continue;
            }
            if self.cold_peers.len() >= cold_cap {
                self.evict_random_cold();
            }
            if self.cold_peers.insert(addr) {
                added += 1;
            }
        }
        added
    }

    /// Remove and return a randomly selected cold peer address for promotion.
    pub fn take_cold_peer(&mut self) -> Option<String> {
        let addr = self.cold_peers.iter().choose(&mut rand::rng()).cloned()?;
        self.cold_peers.remove(&addr);
        Some(addr)
    }

    /// Remove an address from the cold set when it is promoted to a hot connection.
    pub fn mark_as_promoted(&mut self, address: &str) {
        self.cold_peers.remove(address);
    }

    /// Return a previously hot peer to the cold set after churn demotion.
    ///
    /// Unlike `add_discovered`, this skips the `failed_peers` check: a peer that was
    /// successfully running as hot should not remain blacklisted just because an earlier
    /// cold promotion from the same session failed. The peer is also removed from
    /// `failed_peers` if present.  Cap-and-evict applies as normal.
    pub fn demote_to_cold(&mut self, address: String, hot: &HashSet<String>) {
        self.failed_peers.remove(&address);
        if !self.cold_peers.contains(&address) && !hot.contains(&address) {
            let cold_cap = self.config.target_peer_count * 4;
            if self.cold_peers.len() >= cold_cap {
                self.evict_random_cold();
            }
            self.cold_peers.insert(address);
        }
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
    /// peer exists.
    pub fn needs_discovery(&self, hot_count: usize) -> bool {
        self.config.peer_sharing_enabled && hot_count > 0
    }

    /// Returns true if the peer has not been queried within the cooldown window.
    pub fn can_query(&self, peer_id: PeerId) -> bool {
        match self.sharing_cooldown.get(&peer_id) {
            None => true,
            Some(&last) => {
                last.elapsed() >= Duration::from_secs(self.config.peer_sharing_cooldown_secs)
            }
        }
    }

    /// Record a peer-sharing query for `peer_id`, starting its cooldown.
    /// Must be called BEFORE the async exchange starts.
    pub fn record_query(&mut self, peer_id: PeerId) {
        self.sharing_cooldown.insert(peer_id, Instant::now());
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
            tracing::info!(
                evicted_addr = %victim,
                cold_count = self.cold_peers.len(),
                "evicted cold peer"
            );
        }
    }
}
