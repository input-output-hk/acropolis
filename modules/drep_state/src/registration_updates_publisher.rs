use acropolis_common::{caryatid::RollbackAwarePublisher, messages::Message};
use caryatid_sdk::Context;
use std::sync::Arc;

/// Message publisher for DRep Registration Updates
pub struct DRepRegistrationUpdatesPublisher(RollbackAwarePublisher<Message>);

impl DRepRegistrationUpdatesPublisher {
    /// Construct with context and topic to publish on
    pub fn new(context: Arc<Context<Message>>, topic: String) -> Self {
        Self(RollbackAwarePublisher::new(context, topic))
    }

    /// Publish the DRep Registration Updates
    pub async fn publish(&mut self, message: Arc<Message>) -> anyhow::Result<()> {
        self.0.publish(message).await
    }

    /// Publish a rollback message, if we have anything to roll back
    pub async fn publish_rollback(&mut self, message: Arc<Message>) -> anyhow::Result<()> {
        self.0.publish(message).await
    }
}
