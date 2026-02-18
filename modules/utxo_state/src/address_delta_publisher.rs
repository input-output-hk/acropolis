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

use crate::state::AddressDeltaObserver;

/// Address delta publisher
pub struct AddressDeltaPublisher {
    /// Accumulating compact deltas for the current block
    compact_deltas: Mutex<Vec<AddressDelta>>,

    /// Accumulating extended deltas for the current block
    extended_deltas: Mutex<Vec<ExtendedAddressDelta>>,

    /// Compact publisher
    compact_publisher: Option<Mutex<RollbackAwarePublisher<Message>>>,

    /// Extended publisher
    extended_publisher: Option<Mutex<RollbackAwarePublisher<Message>>>,
}

impl AddressDeltaPublisher {
    /// Create
    pub fn new(context: Arc<Context<Message>>, config: Arc<Config>) -> Self {
        Self {
            compact_deltas: Mutex::new(Vec::new()),
            extended_deltas: Mutex::new(Vec::new()),
            compact_publisher: config
                .get_string("address-delta-topic")
                .ok()
                .map(|topic| Mutex::new(RollbackAwarePublisher::new(context.clone(), topic))),
            extended_publisher: config
                .get_string("address-delta-extended-topic")
                .ok()
                .map(|topic| Mutex::new(RollbackAwarePublisher::new(context, topic))),
        }
    }

    fn to_compact_delta(delta: &ExtendedAddressDelta) -> AddressDelta {
        AddressDelta {
            address: delta.address.clone(),
            tx_identifier: delta.tx_identifier,
            spent_utxos: delta.spent_utxos.iter().map(|utxo| utxo.utxo).collect(),
            created_utxos: delta.created_utxos.iter().map(|utxo| utxo.utxo).collect(),
            sent: delta.sent.clone(),
            received: delta.received.clone(),
        }
    }
}

#[async_trait]
impl AddressDeltaObserver for AddressDeltaPublisher {
    /// Observe a new block
    async fn start_block(&self, _block: &BlockInfo) {
        // Clear the deltas
        self.compact_deltas.lock().await.clear();
        self.extended_deltas.lock().await.clear();
    }

    /// Observe an address delta and publish messages
    async fn observe_delta(&self, delta: &ExtendedAddressDelta) {
        // Accumulate the delta
        self.compact_deltas.lock().await.push(Self::to_compact_delta(delta));
        self.extended_deltas.lock().await.push(delta.clone());
    }

    async fn finalise_block(&self, block: &BlockInfo) {
        let compact_deltas = std::mem::take(&mut *self.compact_deltas.lock().await);
        let extended_deltas = std::mem::take(&mut *self.extended_deltas.lock().await);
        debug!(
            block_number = block.number,
            compact_count = compact_deltas.len(),
            extended_count = extended_deltas.len(),
            "utxo-state finalising address deltas"
        );

        if let Some(publisher) = &self.compact_publisher {
            let message = Message::Cardano((
                block.clone(),
                CardanoMessage::AddressDeltas(AddressDeltasMessage::Deltas(compact_deltas)),
            ));
            publisher
                .lock()
                .await
                .publish(Arc::new(message))
                .await
                .unwrap_or_else(|e| error!("Failed to publish compact address deltas: {e}"));
        }

        if let Some(publisher) = &self.extended_publisher {
            let message = Message::Cardano((
                block.clone(),
                CardanoMessage::AddressDeltas(AddressDeltasMessage::ExtendedDeltas(
                    extended_deltas,
                )),
            ));
            publisher
                .lock()
                .await
                .publish(Arc::new(message))
                .await
                .unwrap_or_else(|e| error!("Failed to publish extended address deltas: {e}"));
        }
    }

    async fn rollback(&self, message: Arc<Message>) {
        if let Some(publisher) = &self.compact_publisher {
            publisher
                .lock()
                .await
                .publish(message.clone())
                .await
                .unwrap_or_else(|e| error!("Failed to publish compact rollback: {e}"));
        }

        if let Some(publisher) = &self.extended_publisher {
            publisher
                .lock()
                .await
                .publish(message)
                .await
                .unwrap_or_else(|e| error!("Failed to publish extended rollback: {e}"));
        }
    }
}
