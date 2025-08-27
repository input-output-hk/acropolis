use acropolis_common::messages::{CardanoMessage, Message, SPORewardsMessage};
use acropolis_common::{BlockInfo, KeyHash, SPORewards};
use caryatid_sdk::Context;
use std::sync::Arc;

/// Message publisher for Stake Pool Delegation Distribution (SPDD)
pub struct SPORewardsPublisher {
    /// Module context
    context: Arc<Context<Message>>,

    /// Topic to publish on
    topic: String,
}

impl SPORewardsPublisher {
    /// Construct with context and topic to publish on
    pub fn new(context: Arc<Context<Message>>, topic: String) -> Self {
        Self { context, topic }
    }

    /// Publish the SPO rewards
    pub async fn publish_spo_rewards(
        &mut self,
        block: &BlockInfo,
        spo_rewards: Vec<(KeyHash, SPORewards)>,
    ) -> anyhow::Result<()> {
        self.context
            .message_bus
            .publish(
                &self.topic,
                Arc::new(Message::Cardano((
                    block.clone(),
                    CardanoMessage::SPORewards(SPORewardsMessage {
                        epoch: block.epoch - 1, // End of previous epoch
                        spos: spo_rewards.into_iter().collect(),
                    }),
                ))),
            )
            .await
    }
}
