use acropolis_common::caryatid::RollbackAwarePublisher;
use acropolis_common::messages::{CardanoMessage, Message, SPORewardsMessage};
use acropolis_common::{BlockInfo, PoolId, SPORewards};
use caryatid_sdk::Context;
use std::sync::Arc;

/// Message publisher for Stake Pool Delegation Distribution (SPDD)
pub struct SPORewardsPublisher(RollbackAwarePublisher<Message>);

impl SPORewardsPublisher {
    /// Construct with context and topic to publish on
    pub fn new(context: Arc<Context<Message>>, topic: String) -> Self {
        Self(RollbackAwarePublisher::new(context, topic))
    }

    /// Publish the SPO rewards
    pub async fn publish_spo_rewards(
        &mut self,
        block: &BlockInfo,
        spo_rewards: Vec<(PoolId, SPORewards)>,
    ) -> anyhow::Result<()> {
        self.0
            .publish(Arc::new(Message::Cardano((
                block.clone(),
                CardanoMessage::SPORewards(SPORewardsMessage {
                    epoch: block.epoch - 1, // End of previous epoch
                    spos: spo_rewards.into_iter().collect(),
                }),
            ))))
            .await
    }

    /// Publish a rollback message, if we have anything to roll back
    pub async fn publish_rollback(&mut self, message: Arc<Message>) -> anyhow::Result<()> {
        self.0.publish(message).await
    }
}
