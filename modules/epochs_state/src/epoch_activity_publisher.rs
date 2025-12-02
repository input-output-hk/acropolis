use acropolis_common::{
    caryatid::RollbackAwarePublisher,
    messages::{CardanoMessage, EpochActivityMessage, Message},
    BlockInfo,
};
use caryatid_sdk::Context;
use std::sync::Arc;

/// Message publisher for Epoch Activity Message
pub struct EpochActivityPublisher(RollbackAwarePublisher<Message>);

impl EpochActivityPublisher {
    /// Construct with context and topic to publish on
    pub fn new(context: Arc<Context<Message>>, topic: String) -> Self {
        Self(RollbackAwarePublisher::new(context, topic))
    }

    /// Publish the Epoch Activity Message
    pub async fn publish(
        &mut self,
        block_info: &BlockInfo,
        ea: EpochActivityMessage,
    ) -> anyhow::Result<()> {
        self.0
            .publish(Arc::new(Message::Cardano((
                block_info.clone(),
                CardanoMessage::EpochActivity(ea),
            ))))
            .await
    }

    /// Publish a rollback message, if we have anything to roll back
    pub async fn publish_rollback(&mut self, message: Arc<Message>) -> anyhow::Result<()> {
        self.0.publish(message).await
    }
}
