use acropolis_common::messages::{CardanoMessage, Message, StakeAddressDiffsMessage};
use acropolis_common::{BlockInfo, StakeAddressDiff};
use caryatid_sdk::Context;
use std::sync::Arc;

/// Message publisher for Stake Address Diffs
pub struct StakeDiffsPublisher {
    /// Module context
    context: Arc<Context<Message>>,

    /// Topic to publish on
    topic: String,
}

impl StakeDiffsPublisher {
    /// Construct with context and topic to publish on
    pub fn new(context: Arc<Context<Message>>, topic: String) -> Self {
        Self { context, topic }
    }

    /// Publish the Stake Diffs
    pub async fn publish_stake_diffs(
        &mut self,
        block: &BlockInfo,
        stake_diffs: Vec<StakeAddressDiff>,
    ) -> anyhow::Result<()> {
        self.context
            .message_bus
            .publish(
                &self.topic,
                Arc::new(Message::Cardano((
                    block.clone(),
                    CardanoMessage::StakeAddressDiffs(StakeAddressDiffsMessage {
                        diffs: stake_diffs,
                    }),
                ))),
            )
            .await
    }
}
