use acropolis_common::messages::{CardanoMessage, Message, SPOStakeDistributionMessage};
use acropolis_common::{BlockInfo, DelegatedStake, PoolId};
use caryatid_sdk::Context;
use std::collections::BTreeMap;
use std::sync::Arc;

/// Message publisher for Stake Pool Delegation Distribution (SPDD)
pub struct SPODistributionPublisher {
    /// Module context
    context: Arc<Context<Message>>,

    /// Topic to publish on
    topic: String,

    // When did we publish our last non-rollback message
    last_activity_at: Option<u64>,
}

impl SPODistributionPublisher {
    /// Construct with context and topic to publish on
    pub fn new(context: Arc<Context<Message>>, topic: String) -> Self {
        Self {
            context,
            topic,
            last_activity_at: None,
        }
    }

    /// Publish the SPDD
    pub async fn publish_spdd(
        &mut self,
        block: &BlockInfo,
        spos: BTreeMap<PoolId, DelegatedStake>,
    ) -> anyhow::Result<()> {
        self.last_activity_at = Some(block.slot);
        self.context
            .message_bus
            .publish(
                &self.topic,
                Arc::new(Message::Cardano((
                    block.clone(),
                    CardanoMessage::SPOStakeDistribution(SPOStakeDistributionMessage {
                        epoch: block.epoch - 1, // End of the previous epoch
                        spos: spos.into_iter().collect(),
                    }),
                ))),
            )
            .await
    }

    /// Publish a rollback message, if we have anything to roll back
    pub async fn publish_rollback(&mut self, message: Arc<Message>) -> anyhow::Result<()> {
        let Message::Cardano((block_info, CardanoMessage::Rollback(_))) = message.as_ref() else {
            return Ok(());
        };
        if self.last_activity_at.is_none_or(|slot| slot < block_info.slot) {
            return Ok(());
        }
        self.last_activity_at = None;
        self.context.message_bus.publish(&self.topic, message).await
    }
}
