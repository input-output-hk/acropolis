//! Address delta publisher for the UTXO state Acropolis module
use acropolis_common::{
    messages::{AddressDeltasMessage, CardanoMessage, Message},
    Address, AddressDelta, BlockInfo,
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
    /// Module context
    context: Arc<Context<Message>>,

    /// Topic to publish on
    topic: Option<String>,

    /// Accumulating deltas for the current block
    deltas: Mutex<Vec<AddressDelta>>,
}

impl AddressDeltaPublisher {
    /// Create
    pub fn new(context: Arc<Context<Message>>, config: Arc<Config>) -> Self {
        Self {
            context,
            topic: config.get_string("address-delta-topic").ok(),
            deltas: Mutex::new(Vec::new()),
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
    async fn observe_delta(&self, address: &Address, delta: i64) {
        // Accumulate the delta
        self.deltas.lock().await.push(AddressDelta {
            address: address.clone(),
            delta,
        });
    }

    async fn finalise_block(&self, block: &BlockInfo) {
        // Send out the accumulated deltas
        if let Some(topic) = &self.topic {
            let mut deltas = self.deltas.lock().await;
            let message = AddressDeltasMessage {
                deltas: std::mem::take(&mut *deltas),
            };

            let message_enum =
                Message::Cardano((block.clone(), CardanoMessage::AddressDeltas(message)));
            self.context
                .message_bus
                .publish(&topic, Arc::new(message_enum))
                .await
                .unwrap_or_else(|e| error!("Failed to publish: {e}"));
        }
    }
}
