use acropolis_common::caryatid::RollbackAwarePublisher;
use acropolis_common::messages::{
    CardanoMessage, DRepDelegationDistribution, DRepStakeDistributionMessage, Message,
};
use acropolis_common::BlockInfo;
use caryatid_sdk::Context;
use std::sync::Arc;

/// Message publisher for DRep Delegation Distribution (DRDD)
pub struct DRepDistributionPublisher(RollbackAwarePublisher<Message>);

impl DRepDistributionPublisher {
    /// Construct with context and topic to publish on
    pub fn new(context: Arc<Context<Message>>, topic: String) -> Self {
        Self(RollbackAwarePublisher::new(context, topic))
    }

    /// Publish the DRep Delegation Distribution
    pub async fn publish_drdd(
        &mut self,
        block: &BlockInfo,
        drdd: DRepDelegationDistribution,
    ) -> anyhow::Result<()> {
        self.0
            .publish(Arc::new(Message::Cardano((
                block.clone(),
                CardanoMessage::DRepStakeDistribution(DRepStakeDistributionMessage {
                    epoch: block.epoch,
                    drdd,
                }),
            ))))
            .await
    }

    /// Publish a rollback message, if we have anything to roll back
    pub async fn publish_rollback(&mut self, message: Arc<Message>) -> anyhow::Result<()> {
        self.0.publish(message).await
    }
}
