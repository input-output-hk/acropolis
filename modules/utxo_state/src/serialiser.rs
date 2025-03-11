//! Acropolis UTXO state - serialiser
//! Takes potentially out-of-order UTXO delta messages and reorders them
//! before presenting to the state

use std::collections::BinaryHeap;
use std::cmp::Ordering;
use tracing::{debug, info};

use acropolis_messages::{UTXODelta, UTXODeltasMessage};
use crate::state::State;

/// Pending queue entry
struct PendingEntry {
    /// Block number
    number: u64,

    /// Deltas message
    message: UTXODeltasMessage,
}

// Ord and Eq implementations to make it a min-heap on block number
impl Ord for PendingEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        other.number.cmp(&self.number)  // Note reverse order
    }
}

impl PartialOrd for PendingEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Eq for PendingEntry {}

impl PartialEq for PendingEntry {
    fn eq(&self, other: &Self) -> bool {
        self.number == other.number
    }
}

/// UTXO delta serialiser
pub struct Serialiser {
    /// UTXO state
    state: State,

    /// Pending queue, presents messages in order, implemented as a reversed max-heap
    pending: BinaryHeap<PendingEntry>,
}

impl Serialiser {
    /// Constructor
    pub fn new() -> Self {
        Self {
            state: State::new(),
            pending: BinaryHeap::new(),
        }
    }

    /// Process all the deltas in a message
    /// Using a static here to avoid mutable self borrow horrors below
    fn process_deltas(state: &mut State, deltas: &UTXODeltasMessage) {

        for delta in &deltas.deltas {  // UTXODelta
            let number = deltas.block.number;

            match delta {
                UTXODelta::Input(tx_input) => {
                    state.observe_input(&tx_input, number);
                }, 

                UTXODelta::Output(tx_output) => {
                    state.observe_output(&tx_output, number);
                },

                _ => {}
            }
        }

    }

    /// Handle a UTXO delta message
    pub fn observe_utxo_deltas(&mut self, deltas: &UTXODeltasMessage) {

        // Observe block for stats and rollbacks
        if self.state.observe_block(&deltas.block) {

            // Accepted - it's in order
            Self::process_deltas(&mut self.state, deltas);

            // See if any pending now work
            while let Some(next_pending) = self.pending.peek() {
                if self.state.observe_block(&next_pending.message.block) {
                    if tracing::enabled!(tracing::Level::DEBUG) {
                        debug!("Now accepted block {}", next_pending.number);
                    }
                    Self::process_deltas(&mut self.state, &next_pending.message);
                    self.pending.pop();
                } else {
                    break;
                }
            }
        } else {
            // Not accepted, it's out of order, queue it
            if tracing::enabled!(tracing::Level::DEBUG) {
                debug!("Queueing out-of-order block {}", deltas.block.number);
            }
            self.pending.push(PendingEntry {
                number: deltas.block.number,
                message: deltas.clone()
            });
        }
    }

    /// Periodic tick for background logging and pruning
    pub fn tick(&mut self) {
        self.state.prune();
        self.state.log_stats();
        info!(pending = self.pending.len());
    }
}

