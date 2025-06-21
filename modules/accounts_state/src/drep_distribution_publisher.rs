use acropolis_common::messages::{CardanoMessage, DRepStakeDistributionMessage, Message};
use acropolis_common::BlockInfo;
use caryatid_sdk::Context;
use std::sync::Arc;

use crate::state::DRepDelegationDistribution;

pub struct DRepDistributionPublisher {
    /// Module context
    context: Arc<Context<Message>>,

    /// Topic to publish on
    topic: String,
}

impl DRepDistributionPublisher {
    pub fn new(context: Arc<Context<Message>>, topic: String) -> Self {
        Self { context, topic }
    }

    pub async fn publish_stake(
        &mut self,
        block: &BlockInfo,
        s: DRepDelegationDistribution,
    ) -> anyhow::Result<()> {
        self.context
            .message_bus
            .publish(
                &self.topic,
                Arc::new(Message::Cardano((
                    block.clone(),
                    CardanoMessage::DRepStakeDistribution(DRepStakeDistributionMessage {
                        epoch: block.epoch,
                        abstain: s.abstain,
                        no_confidence: s.no_confidence,
                        dreps: s.dreps,
                    }),
                ))),
            )
            .await
    }
}
