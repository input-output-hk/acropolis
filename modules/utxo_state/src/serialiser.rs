//! Acropolis UTXO state - serialiser
//! Takes potentially out-of-order UTXO delta messages and reorders them
//! before presenting to the state

use acropolis_messages::{UTXODelta, UTXODeltasMessage};
use crate::state::State;

/// UTXO delta serialiser
pub struct Serialiser {
    state: State,
}

impl Serialiser {
    /// Constructor
    pub fn new() -> Self {
        Self {
            state: State::new(),
        }
    }

    /// Handle a UTXO delta message
    pub fn observe_utxo_deltas(&mut self, deltas: &UTXODeltasMessage) {

        // Observe block for stats and rollbacks
        self.state.observe_block(&deltas.block);

        // Observe each delta
        for delta in &deltas.deltas {  // UTXODelta
            let number = deltas.block.number;

            match delta {
                UTXODelta::Input(tx_input) => {
                    self.state.observe_input(&tx_input, number);
                },

                UTXODelta::Output(tx_output) => {
                    self.state.observe_output(&tx_output, number);
                },

                _ => {}
            }
        }
    }

    /// Periodic tick for background logging and pruning
    pub fn tick(&mut self) {
        self.state.prune();
        self.state.log_stats();
    }
}

