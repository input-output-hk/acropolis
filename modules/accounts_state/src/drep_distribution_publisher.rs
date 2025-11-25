use acropolis_common::messages::{
    CardanoMessage, DRepDelegationDistribution, DRepStakeDistributionMessage, Message,
};
use acropolis_common::BlockInfo;
use caryatid_sdk::Context;
use std::sync::Arc;

/// Message publisher for DRep Delegation Distribution (DRDD)
pub struct DRepDistributionPublisher {
    /// Module context
    context: Arc<Context<Message>>,

    /// Topic to publish on
    topic: String,

    // When did we publish our last non-rollback message
    last_activity_at: Option<u64>,
}

impl DRepDistributionPublisher {
    /// Construct with context and topic to publish on
    pub fn new(context: Arc<Context<Message>>, topic: String) -> Self {
        Self {
            context,
            topic,
            last_activity_at: None,
        }
    }

    /// Publish the DRep Delegation Distribution
    pub async fn publish_drdd(
        &mut self,
        block: &BlockInfo,
        drdd: DRepDelegationDistribution,
    ) -> anyhow::Result<()> {
        self.last_activity_at = Some(block.slot);
        self.context
            .message_bus
            .publish(
                &self.topic,
                Arc::new(Message::Cardano((
                    block.clone(),
                    CardanoMessage::DRepStakeDistribution(DRepStakeDistributionMessage {
                        epoch: block.epoch,
                        drdd,
                    }),
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
