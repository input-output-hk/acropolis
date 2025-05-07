//! Address delta publisher for the UTXO state Acropolis module
use caryatid_sdk::Context;
use config::Config;
use acropolis_common::{
    BlockInfo, AddressDelta, Address,
        messages::{
        AddressDeltasMessage,
        Message,
        Sequence,
     },
};
use std::sync::Arc;
use tokio::sync::Mutex;
use async_trait::async_trait;
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

    async fn finalise_block(&self, block: &BlockInfo, sequence: Sequence) {

        // Send out the accumulated deltas
        if let Some(topic) = &self.topic {

            let mut deltas = self.deltas.lock().await;
            let message = AddressDeltasMessage {
                sequence,
                block: block.clone(),
                deltas: std::mem::take(&mut *deltas),
            };

            let context = self.context.clone();
            let topic = topic.clone();
            tokio::spawn(async move {
                let message_enum = Message::AddressDeltas(message);
                context.message_bus.publish(&topic,
                                            Arc::new(message_enum))
                    .await
                    .unwrap_or_else(|e| error!("Failed to publish: {e}")); 
            });
        }
    }
}
