use acropolis_common::caryatid::RollbackAwarePublisher;
use acropolis_common::messages::{CardanoMessage, Message, StakeRewardDeltasMessage};
use acropolis_common::{BlockInfo, StakeRewardDelta};
use caryatid_sdk::Context;
use std::sync::Arc;

/// Message publisher for Stake Reward Deltas
pub struct StakeRewardDeltasPublisher(RollbackAwarePublisher<Message>);

impl StakeRewardDeltasPublisher {
    /// Construct with context and topic to publish on
    pub fn new(context: Arc<Context<Message>>, topic: String) -> Self {
        Self(RollbackAwarePublisher::new(context, topic))
    }

    /// Publish the Stake Diffs
    pub async fn publish_stake_reward_deltas(
        &mut self,
        block: &BlockInfo,
        stake_reward_deltas: Vec<StakeRewardDelta>,
    ) -> anyhow::Result<()> {
        self.0
            .publish(Arc::new(Message::Cardano((
                block.clone(),
                CardanoMessage::StakeRewardDeltas(StakeRewardDeltasMessage {
                    deltas: stake_reward_deltas,
                }),
            ))))
            .await
    }

    /// Publish a rollback message, if we have anything to roll back
    pub async fn publish_rollback(&mut self, message: Arc<Message>) -> anyhow::Result<()> {
        self.0.publish(message).await
    }
}
