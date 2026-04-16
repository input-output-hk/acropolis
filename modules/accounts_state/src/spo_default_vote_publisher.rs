use acropolis_common::caryatid::RollbackAwarePublisher;
use acropolis_common::messages::{CardanoMessage, Message, SPODefaultVoteMessage};
use acropolis_common::{BlockInfo, DelegatedStakeDefaultVote, PoolId};
use caryatid_sdk::Context;
use std::collections::BTreeMap;
use std::sync::Arc;

/// Message publisher for Stake Pool Delegation Distribution (SPDD)
pub struct SPODefaultVotePublisher(RollbackAwarePublisher<Message>);

impl SPODefaultVotePublisher {
    /// Construct with context and topic to publish on
    pub fn new(context: Arc<Context<Message>>, topic: String) -> Self {
        Self(RollbackAwarePublisher::new(context, topic))
    }

    /// Publish the SPDD
    pub async fn publish_spo_default_vote(
        &mut self,
        block: &BlockInfo,
        default_vote: BTreeMap<PoolId, DelegatedStakeDefaultVote>,
    ) -> anyhow::Result<()> {
        self.0
            .publish(Arc::new(Message::Cardano((
                block.clone(),
                CardanoMessage::SPODefaultVote(SPODefaultVoteMessage {
                    epoch: block.epoch - 1, // End of the previous epoch
                    default_vote: default_vote.into_iter().collect(),
                }),
            ))))
            .await
    }

    /// Publish a pre-constructed message on the SPDD topic.
    pub async fn publish_message(&mut self, message: Arc<Message>) -> anyhow::Result<()> {
        self.0.publish(message).await
    }
}
