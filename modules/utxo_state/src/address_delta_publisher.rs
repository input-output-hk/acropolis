//! Address delta publisher for the UTXO state Acropolis module
use acropolis_common::{
    caryatid::RollbackAwarePublisher,
    messages::{AddressDeltasMessage, CardanoMessage, Message},
    AddressDelta, BlockInfo,
};
use async_trait::async_trait;
use caryatid_sdk::Context;
use config::Config;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::error;

use crate::state::AddressDeltaObserver;

/// Address delta publisher
pub struct AddressDeltaPublisher {
    /// Accumulating deltas for the current block
    deltas: Mutex<Vec<AddressDelta>>,

    /// Publisher
    publisher: Option<Mutex<RollbackAwarePublisher<Message>>>,
}

impl AddressDeltaPublisher {
    /// Create
    pub fn new(context: Arc<Context<Message>>, config: Arc<Config>) -> Self {
        Self {
            deltas: Mutex::new(Vec::new()),
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
        self.deltas.lock().await.clear();
    }

    /// Observe an address delta and publish messages
    async fn observe_delta(&self, delta: &AddressDelta) {
        // Accumulate the delta
        self.deltas.lock().await.push(delta.clone());
    }

    async fn finalise_block(&self, block: &BlockInfo) {
        // Send out the accumulated deltas
        if let Some(publisher) = &self.publisher {
            let mut deltas = self.deltas.lock().await;
            let message = AddressDeltasMessage {
                deltas: std::mem::take(&mut *deltas),
            };

            let message_enum =
                Message::Cardano((block.clone(), CardanoMessage::AddressDeltas(message)));
            publisher
                .lock()
                .await
                .publish(Arc::new(message_enum))
                .await
                .unwrap_or_else(|e| error!("Failed to publish: {e}"));
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
