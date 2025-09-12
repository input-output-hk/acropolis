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
}

impl DRepDistributionPublisher {
    /// Construct with context and topic to publish on
    pub fn new(context: Arc<Context<Message>>, topic: String) -> Self {
        Self { context, topic }
    }

    /// Publish the DRep Delegation Distribution
    pub async fn publish_drdd(
        &mut self,
        block: &BlockInfo,
        drdd: DRepDelegationDistribution,
    ) -> anyhow::Result<()> {
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
}
