use acropolis_common::messages::{CardanoMessage, Message, StakeRewardDeltasMessage};
use acropolis_common::{BlockInfo, StakeRewardDelta};
use caryatid_sdk::Context;
use std::sync::Arc;

/// Message publisher for Stake Reward Deltas
pub struct StakeRewardDeltasPublisher {
    /// Module context
    context: Arc<Context<Message>>,

    /// Topic to publish on
    topic: String,
}

impl StakeRewardDeltasPublisher {
    /// Construct with context and topic to publish on
    pub fn new(context: Arc<Context<Message>>, topic: String) -> Self {
        Self { context, topic }
    }

    /// Publish the Stake Diffs
    pub async fn publish_stake_reward_deltas(
        &mut self,
        block: &BlockInfo,
        stake_reward_deltas: Vec<StakeRewardDelta>,
    ) -> anyhow::Result<()> {
        self.context
            .message_bus
            .publish(
                &self.topic,
                Arc::new(Message::Cardano((
                    block.clone(),
                    CardanoMessage::StakeRewardDeltas(StakeRewardDeltasMessage {
                        deltas: stake_reward_deltas,
                    }),
                ))),
            )
            .await
    }
}
