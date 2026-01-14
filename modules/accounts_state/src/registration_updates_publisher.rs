use acropolis_common::caryatid::RollbackAwarePublisher;
use acropolis_common::messages::{CardanoMessage, Message, StakeRegistrationUpdatesMessage};
use acropolis_common::{BlockInfo, StakeRegistrationUpdate};
use caryatid_sdk::Context;
use std::sync::Arc;

/// Message publisher for Stake Registration Updates
pub struct StakeRegistrationUpdatesPublisher(RollbackAwarePublisher<Message>);

impl StakeRegistrationUpdatesPublisher {
    /// Construct with context and topic to publish on
    pub fn new(context: Arc<Context<Message>>, topic: String) -> Self {
        Self(RollbackAwarePublisher::new(context, topic))
    }

    /// Publish the Stake Registration Updates
    pub async fn publish(
        &mut self,
        block: &BlockInfo,
        stake_registration_updates: Vec<StakeRegistrationUpdate>,
    ) -> anyhow::Result<()> {
        self.0
            .publish(Arc::new(Message::Cardano((
                block.clone(),
                CardanoMessage::StakeRegistrationUpdates(StakeRegistrationUpdatesMessage {
                    updates: stake_registration_updates,
                }),
            ))))
            .await
    }

    /// Publish a rollback message, if we have anything to roll back
    pub async fn publish_rollback(&mut self, message: Arc<Message>) -> anyhow::Result<()> {
        self.0.publish(message).await
    }
}
