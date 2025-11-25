//! Address delta publisher for the UTXO state Acropolis module
use acropolis_common::{
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
    /// Module context
    context: Arc<Context<Message>>,

    /// Topic to publish on
    topic: Option<String>,

    /// Accumulating deltas for the current block
    deltas: Mutex<Vec<AddressDelta>>,

    // When did we publish our last non-rollback message
    last_activity_at: Mutex<Option<u64>>,
}

impl AddressDeltaPublisher {
    /// Create
    pub fn new(context: Arc<Context<Message>>, config: Arc<Config>) -> Self {
        Self {
            context,
            topic: config.get_string("address-delta-topic").ok(),
            deltas: Mutex::new(Vec::new()),
            last_activity_at: Mutex::new(None),
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
        *self.last_activity_at.lock().await = Some(block.slot);
        if let Some(topic) = &self.topic {
            let mut deltas = self.deltas.lock().await;
            let message = AddressDeltasMessage {
                deltas: std::mem::take(&mut *deltas),
            };

            let message_enum =
                Message::Cardano((block.clone(), CardanoMessage::AddressDeltas(message)));
            self.context
                .message_bus
                .publish(topic, Arc::new(message_enum))
                .await
                .unwrap_or_else(|e| error!("Failed to publish: {e}"));
        }
    }

    async fn rollback(&self, message: Arc<Message>) {
        let Message::Cardano((block_info, CardanoMessage::Rollback(_))) = message.as_ref() else {
            return;
        };
        let mut last_activity_at = self.last_activity_at.lock().await;
        if last_activity_at.is_none_or(|slot| slot < block_info.slot) {
            return;
        }
        *last_activity_at = None;
        if let Some(topic) = &self.topic {
            self.context
                .message_bus
                .publish(topic, message)
                .await
                .unwrap_or_else(|e| error!("Failed to publish rollback: {e}"));
        }
    }
}
