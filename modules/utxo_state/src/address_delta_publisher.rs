//! Address delta publisher for the UTXO state Acropolis module
use caryatid_sdk::Context;
use config::Config;
use acropolis_common::{
    BlockInfo, AddressDelta, Address,
        messages::{
        AddressDeltasMessage,
        Message
     }, 
};
use std::sync::Arc;
use tracing::error;

use crate::state::AddressDeltaObserver;

/// Address delta publisher
pub struct AddressDeltaPublisher {

    /// Module context
    context: Arc<Context<Message>>,

    /// Topic to publish on
    topic: Option<String>,

    /// Accumulating deltas for the current block
    deltas: Vec<AddressDelta>,
}

impl AddressDeltaPublisher {

    /// Create
    pub fn new(context: Arc<Context<Message>>, config: Arc<Config>) -> Self {
        Self { 
            context, 
            topic: config.get_string("address-delta-topic").ok(),
            deltas: Vec::new(),
        }
    }
}

impl AddressDeltaObserver for AddressDeltaPublisher {

    /// Observe a new block
    fn start_block(&mut self, _block: &BlockInfo) {
        // Clear the deltas
        self.deltas.clear();
    }

    /// Observe an address delta and publish messages
    fn observe_delta(&mut self, address: &Address, delta: i64) {
        // Accumulate the delta
        self.deltas.push(AddressDelta {
            address: address.clone(),
            delta,
        });
    }

    fn finalise_block(&mut self, block: &BlockInfo, sequence: u64) {

        // Send out the accumulated deltas
        if let Some(topic) = &self.topic {

            let message = AddressDeltasMessage {
                sequence,
                block: block.clone(),
                deltas: std::mem::take(&mut self.deltas),
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
