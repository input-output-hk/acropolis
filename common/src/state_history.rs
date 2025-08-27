//! Generic state history
//! Keeps per-block state for rollbacks or per-epoch state for historical lookups
//! Use imbl collections in the state to avoid memory explosion!

use crate::params::SECURITY_PARAMETER_K;
use std::collections::VecDeque;
use tracing::info;

pub enum HistoryKind {
    BlockState, // Used for rollbacks, bounded at k
    EpochState, // Used for historical lookups, unbounded
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

    // Block or Epoch based history
    kind: HistoryKind,
}

impl<S: Clone + Default> StateHistory<S> {
    /// Construct
    pub fn new(module: &str, kind: HistoryKind) -> Self {
        Self {
            history: VecDeque::new(),
            module: module.to_string(),
            kind: kind,
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
        match self.kind {
            HistoryKind::BlockState => {
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
            }
            HistoryKind::EpochState => {
                while let Some(entry) = self.history.back() {
                    if entry.index >= index {
                        info!(
                            "{} rolling back epoch state to {} removing epoch {}",
                            self.module, index, entry.index
                        );
                        self.history.pop_back();
                    } else {
                        break;
                    }
                }
            }
        }

        self.get_current_state()
    }

    /// Get the state for a given index (if any), direct ref
    pub fn get_by_index(&self, index: u64) -> Option<&S> {
        self.history.iter().find(|entry| entry.index == index).map(|entry| &entry.state)
    }

    /// Get state history's size
    pub fn len(&self) -> usize {
        self.history.len()
    }

    /// Return a reference to the state at the given block number, if it exists
    pub fn inspect_previous_state(&self, index: u64) -> Option<&S> {
        for state in self.history.iter().rev() {
            if state.index == index {
                return Some(&state.state);
            }
        }
        None
    }

    /// Commit new state without checking the block number
    /// TODO: enhance block number logic to commit state without check (for bootstrapping)
    pub fn commit_forced(&mut self, state: S) {
        self.history.push_back(HistoryEntry { index: 0, state });
    }

    /// Commit the new state
    pub fn commit(&mut self, index: u64, state: S) {
        match self.kind {
            HistoryKind::BlockState => {
                while self.history.len() >= SECURITY_PARAMETER_K as usize {
                    self.history.pop_front();
                }
                self.history.push_back(HistoryEntry { index, state });
            }
            HistoryKind::EpochState => {
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
