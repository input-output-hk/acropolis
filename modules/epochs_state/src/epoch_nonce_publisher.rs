use acropolis_common::{
    messages::{CardanoMessage, EpochNonceMessage, Message},
    protocol_params::Nonces,
    BlockInfo,
};
use caryatid_sdk::Context;
use std::sync::Arc;

/// Message publisher for Epoch Nonce Message
pub struct EpochNoncePublisher {
    /// Module context
    context: Arc<Context<Message>>,

    /// Topic to publish on
    topic: String,
}

impl EpochNoncePublisher {
    /// Construct with context and topic to publish on
    pub fn new(context: Arc<Context<Message>>, topic: String) -> Self {
        Self { context, topic }
    }

    /// Publish the Epoch Nonce Message
    pub async fn publish(
        &mut self,
        block_info: &BlockInfo,
        nonces: Option<Nonces>,
    ) -> anyhow::Result<()> {
        let active_nonce = nonces.map(|nonces| nonces.active);
        self.context
            .message_bus
            .publish(
                &self.topic,
                Arc::new(Message::Cardano((
                    block_info.clone(),
                    CardanoMessage::EpochNonce(EpochNonceMessage {
                        nonce: active_nonce,
                    }),
                ))),
            )
            .await
    }
}
