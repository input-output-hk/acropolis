use acropolis_common::messages::{CardanoMessage, SPOStakeDistributionMessage, Message};
use acropolis_common::{KeyHash, BlockInfo, DelegatedStake};
use caryatid_sdk::Context;
use std::collections::BTreeMap;
use std::sync::Arc;

/// Message publisher for Stake Pool Delegation Distribution (SPDD)
pub struct SPODistributionPublisher {
    /// Module context
    context: Arc<Context<Message>>,

    /// Topic to publish on
    topic: String,
}

impl SPODistributionPublisher {
    /// Construct with context and topic to publish on
    pub fn new(context: Arc<Context<Message>>, topic: String) -> Self {
        Self { context, topic }
    }

    /// Publish the SPDD
    pub async fn publish_spdd(
        &mut self,
        block: &BlockInfo,
        spos: BTreeMap<KeyHash, DelegatedStake>,
    ) -> anyhow::Result<()> {
        self.context
            .message_bus
            .publish(
                &self.topic,
                Arc::new(Message::Cardano((
                    block.clone(),
                    CardanoMessage::SPOStakeDistribution(SPOStakeDistributionMessage {
                        epoch: block.epoch-1,  // End of previous epoch
                        spos: spos.into_iter().collect(),
                    }),
                ))),
            )
            .await
    }
}
