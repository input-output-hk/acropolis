//! Address delta publisher for the UTXO state Acropolis module
use caryatid_sdk::Context;
use config::Config;
use acropolis_messages::{
    Message, BlockInfo,
    AddressDeltasMessage, AddressDelta, Address};
use std::sync::Arc;
use tracing::error;

use crate::state::AddressDeltaObserver;

/// Address delta publisher
pub struct AddressDeltaPublisher {

    /// Module context
    context: Arc<Context<Message>>,

    /// Topic to publish on
    topic: Option<String>,
}

impl AddressDeltaPublisher {

    /// Create
    pub fn new(context: Arc<Context<Message>>, config: Arc<Config>) -> Self {
        Self { 
            context, 
            topic: config.get_string("address-delta-topic").ok(),
        }
    }
}

impl AddressDeltaObserver for AddressDeltaPublisher {

    /// Observe an address delta and publish messages
    fn observe_delta(&mut self, block: &BlockInfo, address: &Address, delta: i64) {

        if let Some(topic) = &self.topic {

            // TODO accumulate multiple from a single block!
            let mut message = AddressDeltasMessage {
                block: block.clone(),
                deltas: Vec::new(),
            };

            message.deltas.push(AddressDelta {
                address: address.clone(),
                delta,
            });

            let context = self.context.clone();
            let topic = topic.clone();
            tokio::spawn(async move {
                let message_enum: Message = message.into();
                context.message_bus.publish(&topic,
                                            Arc::new(message_enum))
                    .await
                    .unwrap_or_else(|e| error!("Failed to publish: {e}")); 
            });
        }
    }
}
