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

    // Prune blocks at k and epochs at 2 flag
    keep_all: bool,
}

impl<S: Clone + Default> StateHistory<S> {
    /// Construct
    pub fn new(module: &str, kind: HistoryKind, keep_all: bool) -> Self {
        Self {
            history: VecDeque::new(),
            module: module.to_string(),
            kind,
            keep_all,
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

    /// Get the most recently stored state at or beore a given index, direct ref
    pub fn get_at_or_before(&self, index: u64) -> Option<&S> {
        self.history.iter().rev().find(|entry| entry.index <= index).map(|entry| &entry.state)
    }

    pub fn len(&self) -> usize {
        self.history.len()
    }

    /// Commit the new state
    pub fn commit(&mut self, index: u64, state: S) {
        match self.kind {
            HistoryKind::BlockState => {
                if !self.keep_all {
                    while self.history.len() >= SECURITY_PARAMETER_K as usize {
                        self.history.pop_front();
                    }
                }
                self.history.push_back(HistoryEntry { index, state });
            }
            HistoryKind::EpochState => {
                if !self.keep_all {
                    while self.history.len() >= 2 {
                        self.history.pop_front();
                    }
                }
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
}
