use acropolis_common::caryatid::RollbackAwarePublisher;
use acropolis_common::messages::{CardanoMessage, Message, SPOStakeDistributionMessage};
use acropolis_common::{BlockInfo, DelegatedStake, PoolId};
use caryatid_sdk::Context;
use std::collections::BTreeMap;
use std::sync::Arc;

/// Message publisher for Stake Pool Delegation Distribution (SPDD)
pub struct SPODistributionPublisher(RollbackAwarePublisher<Message>);

impl SPODistributionPublisher {
    /// Construct with context and topic to publish on
    pub fn new(context: Arc<Context<Message>>, topic: String) -> Self {
        Self(RollbackAwarePublisher::new(context, topic))
    }

    /// Publish the SPDD
    pub async fn publish_spdd(
        &mut self,
        block: &BlockInfo,
        spos: BTreeMap<PoolId, DelegatedStake>,
    ) -> anyhow::Result<()> {
        self.0
            .publish(Arc::new(Message::Cardano((
                block.clone(),
                CardanoMessage::SPOStakeDistribution(SPOStakeDistributionMessage {
                    epoch: block.epoch - 1, // End of the previous epoch
                    spos: spos.into_iter().collect(),
                }),
            ))))
            .await
    }

    /// Publish a rollback message, if we have anything to roll back
    pub async fn publish_rollback(&mut self, message: Arc<Message>) -> anyhow::Result<()> {
        self.0.publish(message).await
    }
}
