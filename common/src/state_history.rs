//! Generic state history
//! Keeps per-block state for rollbacks or per-epoch state for historical lookups
//! Use imbl collections in the state to avoid memory explosion!

use config::Config;
use serde::Serialize;
use std::{
    collections::VecDeque,
    fs::{self, File},
    io::Write,
};
use tracing::info;

use crate::params::SECURITY_PARAMETER_K;

pub const DEFAULT_DUMP_BLOCK: &str = "startup.dump-state-block";
pub const DEFAULT_DUMP_EPOCH: &str = "startup.dump-state-epoch";

pub enum StateHistoryStore {
    Bounded(u64), // Used for rollbacks, bounded at k
    Unbounded,    // Used for historical lookups, unbounded
}

impl StateHistoryStore {
    pub fn default_block_store() -> Self {
        Self::Bounded(SECURITY_PARAMETER_K)
    }
    pub fn default_epoch_store() -> Self {
        Self::Bounded(2)
    }
}

struct HistoryEntry<S> {
    index: u64,
    state: S,
}

/// Generic state history - S is the state to be stored
pub struct StateHistory<S> {
    /// History, one per block or epoch
    history: VecDeque<HistoryEntry<S>>,

    /// Module name
    module: String,

    store: StateHistoryStore,

    dump_index: Option<u64>,

    rolled_back: bool,
}

pub enum StoreType {
    Block,
    Epoch,
}

impl<S: Clone + Default + Serialize> StateHistory<S> {
    /// Construct
    pub fn new(
        module: &str,
        store: StateHistoryStore,
        config: &Config,
        store_type: StoreType,
    ) -> Self {
        let dump_index = match store_type {
            StoreType::Block => config.get::<u64>(DEFAULT_DUMP_BLOCK).ok(),
            StoreType::Epoch => config.get::<u64>(DEFAULT_DUMP_EPOCH).ok(),
        };

        Self {
            history: VecDeque::new(),
            module: module.to_string(),
            store,
            dump_index,
            rolled_back: false,
        }
    }

    /// Get the current state (if any), direct ref
    pub fn current(&self) -> Option<&S> {
        self.history.back().map(|entry| &entry.state)
    }

    /// Get the current state assuming any rollback has been done
    /// Cloned for modification - call commit() when done
    pub fn get_current_state(&self) -> S {
        self.history.back().map(|entry| entry.state.clone()).unwrap_or_default()
    }

    /// Get all the states references in the history
    pub fn values(&self) -> Vec<&S> {
        self.history.iter().map(|entry| &entry.state).collect()
    }

    /// Get the previous state for the given block, handling rollbacks if required
    /// State returned is cloned ready for modification - call commit() when done
    pub fn get_rolled_back_state(&mut self, index: u64) -> S {
        while let Some(entry) = self.history.back() {
            if entry.index >= index {
                info!(
                    "{} rolling back state to {} removing block {}",
                    self.module, index, entry.index
                );
                self.history.pop_back();
            } else {
                break;
            }
        }
        self.rolled_back = true;
        self.get_current_state()
    }

    /// Get the state for a given index (if any), direct ref
    pub fn get_by_index(&self, index: u64) -> Option<&S> {
        self.history.iter().find(|entry| entry.index == index).map(|entry| &entry.state)
    }

    /// Get the most recently stored state at or beore a given index, direct ref
    pub fn get_at_or_before(&self, index: u64) -> Option<&S> {
        self.history.iter().rev().find(|entry| entry.index <= index).map(|entry| &entry.state)
    }

    /// Return a reference to the state at the given block number, if it exists
    pub fn get_by_index_reverse(&self, index: u64) -> Option<&S> {
        self.history.iter().rev().find(|entry| entry.index == index).map(|entry| &entry.state)
    }

    /// Get state history's size
    pub fn len(&self) -> usize {
        self.history.len()
    }

    /// Get if state history is empty
    pub fn is_empty(&self) -> bool {
        self.history.is_empty()
    }

    /// Commit new state without checking the block number
    /// TODO: enhance block number logic to commit state without check (for bootstrapping)
    pub fn commit_forced(&mut self, state: S) {
        self.history.push_back(HistoryEntry { index: 0, state });
    }

    pub fn bootstrap_init_with(&mut self, state: S, index: u64) {
        self.history.push_back(HistoryEntry { index, state });
    }

    /// Commit the new state
    pub fn commit(&mut self, index: u64, state: S) {
        match self.store {
            StateHistoryStore::Bounded(k) => {
                while let Some(entry) = self.history.front() {
                    if (index - entry.index) > k {
                        self.history.pop_front();
                    } else {
                        break;
                    }
                }
                self.history.push_back(HistoryEntry { index, state });
            }
            StateHistoryStore::Unbounded => {
                self.history.push_back(HistoryEntry { index, state });
            }
        }

        if let Some(dump_index) = self.dump_index {
            if index == dump_index {
                if self.rolled_back {
                    if self.compare_states() {
                        tracing::info!("{} rollback validation success", self.module);
                    } else {
                        tracing::error!("{} rollback validation failed", self.module);
                    };
                } else {
                    self.dump_to_file();
                }
            }
        }
    }

    fn dump_to_file(&self) {
        if let Some(entry) = self.history.back() {
            let bytes = match bincode::serialize(&entry.state) {
                Ok(bytes) => bytes,
                Err(e) => {
                    tracing::error!("{}", e);
                    return;
                }
            };

            let mut file = match File::create(self.module.clone()) {
                Ok(file) => file,
                Err(e) => {
                    tracing::error!("{}", e);
                    return;
                }
            };

            match file.write_all(&bytes) {
                Ok(_) => {}
                Err(e) => {
                    tracing::error!("{}", e);
                    return;
                }
            }

            info!(
                "{} dumped state at index {} to {} ({} bytes)",
                self.module,
                entry.index,
                self.module,
                bytes.len()
            );
        } else {
            info!("{} no state to dump", self.module);
        }
    }

    fn compare_states(&mut self) -> bool {
        let bytes_pre = match fs::read(self.module.clone()) {
            Ok(b) => b,
            Err(e) => {
                tracing::error!("failed to read {}: {}", self.module, e);
                return false;
            }
        };

        let Some(after_state) = self.history.back().map(|e| &e.state) else {
            info!("{} no current state to compare", self.module);
            return false;
        };

        let bytes_after = match bincode::serialize(after_state) {
            Ok(b) => b,
            Err(e) => {
                tracing::error!("serialize after failed: {}", e);
                return false;
            }
        };

        bytes_pre == bytes_after
    }
}

/// Helper that lets callers initialize the first state with custom config.
impl<S: Clone> StateHistory<S> {
    pub fn get_or_init_with<F>(&mut self, init: F) -> S
    where
        F: FnOnce() -> S,
    {
        if let Some(current) = self.history.back() {
            current.state.clone()
        } else {
            init()
        }
    }

    /// Clear the history
    pub fn clear(&mut self) {
        self.history.clear();
    }
}
