use acropolis_common::{
    messages::{CardanoMessage, EpochActivityMessage, Message},
    BlockInfo,
};
use caryatid_sdk::Context;
use std::sync::Arc;

/// Message publisher for Epoch Activity Message
pub struct EpochActivityPublisher {
    /// Module context
    context: Arc<Context<Message>>,

    /// Topic to publish on
    topic: String,

    // Whether the last message we published was a rollback
    last_activity_at: Option<u64>,
}

impl EpochActivityPublisher {
    /// Construct with context and topic to publish on
    pub fn new(context: Arc<Context<Message>>, topic: String) -> Self {
        Self {
            context,
            topic,
            last_activity_at: None,
        }
    }

    /// Publish the Epoch Activity Message
    pub async fn publish(
        &mut self,
        block_info: &BlockInfo,
        ea: EpochActivityMessage,
    ) -> anyhow::Result<()> {
        self.last_activity_at = Some(block_info.slot);
        self.context
            .message_bus
            .publish(
                &self.topic,
                Arc::new(Message::Cardano((
                    block_info.clone(),
                    CardanoMessage::EpochActivity(ea),
                ))),
            )
            .await
    }

    /// Publish a rollback message, if we have anything to roll back
    pub async fn publish_rollback(&mut self, message: Arc<Message>) -> anyhow::Result<()> {
        let Message::Cardano((block_info, CardanoMessage::Rollback(_))) = message.as_ref() else {
            return Ok(());
        };
        if self.last_activity_at.is_none_or(|slot| slot < block_info.slot) {
            return Ok(());
        }
        self.last_activity_at = None;
        self.context.message_bus.publish(&self.topic, message).await
    }
}
