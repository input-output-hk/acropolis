use acropolis_common::messages::{CardanoMessage, Message, SPORewardStateMessage};
use acropolis_common::{BlockInfo, KeyHash, SPORewardState};
use caryatid_sdk::Context;
use std::collections::BTreeMap;
use std::sync::Arc;

/// Message publisher for Stake Pool Reward State
pub struct SPORewardStatePublisher {
    /// Module context
    context: Arc<Context<Message>>,

    /// Topic to publish on
    topic: String,
}

impl SPORewardStatePublisher {
    /// Construct with context and topic to publish on
    pub fn new(context: Arc<Context<Message>>, topic: String) -> Self {
        Self { context, topic }
    }

    /// Publish the SPO reward state
    pub async fn publish_spo_reward_state(
        &mut self,
        block: &BlockInfo,
        spo_rewards: BTreeMap<KeyHash, SPORewardState>,
    ) -> anyhow::Result<()> {
        self.context
            .message_bus
            .publish(
                &self.topic,
                Arc::new(Message::Cardano((
                    block.clone(),
                    CardanoMessage::SPORewardState(SPORewardStateMessage {
                        epoch: block.epoch - 1,
                        spos: spo_rewards.into_iter().collect(),
                    }),
                ))),
            )
            .await
    }
}
