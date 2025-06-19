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

    /// Commit the new state
    pub fn commit(&mut self, block: &BlockInfo, state: S) {
        // Prune beyond 'k'
        while self.history.len() >= SECURITY_PARAMETER_K as usize {
            self.history.pop_front();
        }

        self.history.push_back(HistoryEntry {
            block: block.number,
            state,
        });
    }
}
