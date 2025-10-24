use acropolis_common::{
    messages::{CardanoMessage, EpochNoncesMessage, Message},
    protocol_params::Nonces,
    BlockInfo,
};
use caryatid_sdk::Context;
use std::sync::Arc;

/// Message publisher for Epoch Nonces Message
pub struct EpochNoncesPublisher {
    /// Module context
    context: Arc<Context<Message>>,

    /// Topic to publish on
    topic: String,
}

impl EpochNoncesPublisher {
    /// Construct with context and topic to publish on
    pub fn new(context: Arc<Context<Message>>, topic: String) -> Self {
        Self { context, topic }
    }

    /// Publish the Epoch Nonces Message
    pub async fn publish(&mut self, block_info: &BlockInfo, nonces: Nonces) -> anyhow::Result<()> {
        self.context
            .message_bus
            .publish(
                &self.topic,
                Arc::new(Message::Cardano((
                    block_info.clone(),
                    CardanoMessage::EpochNonces(EpochNoncesMessage { nonces }),
                ))),
            )
            .await
    }
}
