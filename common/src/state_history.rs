//! Generic state history
//! Keeps per-block state for rollbacks or per-epoch state for historical lookups
//! Use imbl collections in the state to avoid memory explosion!

use std::collections::VecDeque;
use tracing::info;

use crate::params::SECURITY_PARAMETER_K;

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
}

impl<S: Clone + Default> StateHistory<S> {
    /// Construct
    pub fn new(module: &str, store: StateHistoryStore) -> Self {
        Self {
            history: VecDeque::new(),
            module: module.to_string(),
            store,
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

    /// Commit new state without checking the block number
    /// TODO: enhance block number logic to commit state without check (for bootstrapping)
    pub fn commit_forced(&mut self, state: S) {
        self.history.push_back(HistoryEntry { index: 0, state });
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
