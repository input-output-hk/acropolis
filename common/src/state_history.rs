//! Generic state history
//! Keeps per-block state to allow for rollbacks
//! Use imbl collections in the state to avoid memory explosion!

use crate::params::SECURITY_PARAMETER_K;
use crate::types::BlockInfo;
use std::collections::VecDeque;
use tracing::info;

/// Entry in the history - S is the state to be stored
struct HistoryEntry<S> {
    /// Block number this state is for
    block: u64,

    /// State to store
    state: S,
}

/// Generic state history - S is the state to be stored
pub struct StateHistory<S> {
    /// History, one per block
    history: VecDeque<HistoryEntry<S>>,

    /// Module name
    module: String,
}

impl<S: Clone + Default> StateHistory<S> {
    /// Construct
    pub fn new(module: &str) -> Self {
        Self {
            history: VecDeque::new(),
            module: module.to_string(),
        }
    }

    /// Get the current state (if any), direct ref
    pub fn current(&self) -> Option<&S> {
        match self.history.back() {
            Some(entry) => Some(&entry.state),
            None => None,
        }
    }

    /// Get all the states references in the history
    pub fn values(&self) -> Vec<&S> {
        self.history.iter().map(|entry| &entry.state).collect()
    }

    /// Get state history's size
    pub fn len(&self) -> usize {
        self.history.len()
    }

    /// Get the previous state for the given block, handling rollbacks if required
    /// State returned is cloned ready for modification - call commit() when done
    pub fn get_rolled_back_state(&mut self, block: &BlockInfo) -> S {
        loop {
            match self.history.back() {
                Some(state) => {
                    if state.block >= block.number {
                        info!(
                            "{} rolling back state to {} removing block {}",
                            self.module, block.number, state.block
                        );
                        self.history.pop_back();
                    } else {
                        break;
                    }
                }
                _ => break,
            }
        }

        self.get_current_state()
    }

    /// Get the current state assuming any rollback has been done
    /// Cloned for modification - call commit() when done
    pub fn get_current_state(&mut self) -> S {
        if let Some(current) = self.history.back() {
            current.state.clone()
        } else {
            S::default()
        }
    }

    /// Return a reference to the state at the given block number, if it exists
    pub fn inspect_previous_state(&self, block_number: u64) -> Option<&S> {
        for state in self.history.iter().rev() {
            if state.block == block_number {
                return Some(&state.state);
            }
        }
        None
    }

    /// Commit new state without checking the block number
    /// TODO: enhance block number logic to commit state without check (for bootstrapping)
    pub fn commit_forced(&mut self, state: S) {
        self.history.push_back(HistoryEntry { block: 0, state });
    }

    /// Commit the new state
    pub fn commit(&mut self, block: &BlockInfo, state: S) {
        // Prune beyond 'k'
        loop {
            if block.number < SECURITY_PARAMETER_K as u64 {
                break;
            }
            if let Some(entry) = self.history.front() {
                if entry.block < block.number - SECURITY_PARAMETER_K as u64 {
                    self.history.pop_front();
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        self.history.push_back(HistoryEntry {
            block: block.number,
            state,
        });
    }

    /// Clear the history
    pub fn clear(&mut self) {
        self.history.clear();
    }
}
