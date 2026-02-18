//! Address delta publisher for the UTXO state Acropolis module
use acropolis_common::{
    caryatid::RollbackAwarePublisher,
    messages::{AddressDeltasMessage, CardanoMessage, Message},
    AddressDelta, BlockInfo, ExtendedAddressDelta,
};
use async_trait::async_trait;
use caryatid_sdk::Context;
use config::Config;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, error};

use crate::state::{AddressDeltaObserver, AddressDeltaPublishMode, ObservedAddressDelta};

enum AddressDeltaAccumulator {
    Compact(Mutex<Vec<AddressDelta>>),
    Extended(Mutex<Vec<ExtendedAddressDelta>>),
}

/// Address delta publisher
pub struct AddressDeltaPublisher {
    /// Selected publish mode for this runtime.
    mode: AddressDeltaPublishMode,

    /// Accumulating mode-aligned deltas for the current block.
    deltas: AddressDeltaAccumulator,

    /// Single publisher for `address-delta-topic`.
    publisher: Option<Mutex<RollbackAwarePublisher<Message>>>,
}

impl AddressDeltaPublisher {
    /// Create
    pub fn new(
        context: Arc<Context<Message>>,
        config: Arc<Config>,
        mode: AddressDeltaPublishMode,
    ) -> Self {
        let deltas = match mode {
            AddressDeltaPublishMode::Compact => {
                AddressDeltaAccumulator::Compact(Mutex::new(Vec::new()))
            }
            AddressDeltaPublishMode::Extended => {
                AddressDeltaAccumulator::Extended(Mutex::new(Vec::new()))
            }
        };

        Self {
            mode,
            deltas,
            publisher: config
                .get_string("address-delta-topic")
                .ok()
                .map(|topic| Mutex::new(RollbackAwarePublisher::new(context, topic))),
        }
    }
}

#[async_trait]
impl AddressDeltaObserver for AddressDeltaPublisher {
    /// Observe a new block
    async fn start_block(&self, _block: &BlockInfo) {
        // Clear the deltas
        match &self.deltas {
            AddressDeltaAccumulator::Compact(deltas) => deltas.lock().await.clear(),
            AddressDeltaAccumulator::Extended(deltas) => deltas.lock().await.clear(),
        }
    }

    /// Observe an address delta and publish messages
    async fn observe_delta(&self, delta: ObservedAddressDelta) {
        // Accumulate the delta
        match (&self.mode, &self.deltas, delta) {
            (
                AddressDeltaPublishMode::Compact,
                AddressDeltaAccumulator::Compact(deltas),
                ObservedAddressDelta::Compact(delta),
            ) => deltas.lock().await.push(delta),
            (
                AddressDeltaPublishMode::Extended,
                AddressDeltaAccumulator::Extended(deltas),
                ObservedAddressDelta::Extended(delta),
            ) => deltas.lock().await.push(delta),
            (mode, _, _) => error!(
                mode = ?mode,
                "address delta mode mismatch between state emission and publisher mode"
            ),
        }
    }

    async fn finalise_block(&self, block: &BlockInfo) {
        if let Some(publisher) = &self.publisher {
            let message = match &self.deltas {
                AddressDeltaAccumulator::Compact(deltas) => {
                    let compact_deltas = std::mem::take(&mut *deltas.lock().await);
                    debug!(
                        block_number = block.number,
                        mode = "compact",
                        delta_count = compact_deltas.len(),
                        "utxo-state finalising address deltas"
                    );
                    Message::Cardano((
                        block.clone(),
                        CardanoMessage::AddressDeltas(AddressDeltasMessage::Deltas(compact_deltas)),
                    ))
                }
                AddressDeltaAccumulator::Extended(deltas) => {
                    let extended_deltas = std::mem::take(&mut *deltas.lock().await);
                    debug!(
                        block_number = block.number,
                        mode = "extended",
                        delta_count = extended_deltas.len(),
                        "utxo-state finalising address deltas"
                    );
                    Message::Cardano((
                        block.clone(),
                        CardanoMessage::AddressDeltas(AddressDeltasMessage::ExtendedDeltas(
                            extended_deltas,
                        )),
                    ))
                }
            };
            publisher
                .lock()
                .await
                .publish(Arc::new(message))
                .await
                .unwrap_or_else(|e| error!("Failed to publish address deltas: {e}"));
        }
    }

    async fn rollback(&self, message: Arc<Message>) {
        if let Some(publisher) = &self.publisher {
            publisher
                .lock()
                .await
                .publish(message)
                .await
                .unwrap_or_else(|e| error!("Failed to publish rollback: {e}"));
        }
    }
}
