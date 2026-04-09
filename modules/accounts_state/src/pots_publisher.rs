use acropolis_common::caryatid::RollbackAwarePublisher;
use acropolis_common::messages::{CardanoMessage, Message};
use acropolis_common::{BlockInfo, Pots};
use caryatid_sdk::Context;
use std::sync::Arc;

/// Message publisher for Current Pots
pub struct PotsPublisher(RollbackAwarePublisher<Message>);

impl PotsPublisher {
    /// Construct with context and topic to publish on
    pub fn new(context: Arc<Context<Message>>, topic: String) -> Self {
        Self(RollbackAwarePublisher::new(context, topic))
    }

    /// Publish the Pots
    pub async fn publish_pots(&mut self, block: &BlockInfo, pots: Pots) -> anyhow::Result<()> {
        self.0
            .publish(Arc::new(Message::Cardano((
                block.clone(),
                CardanoMessage::Pots(pots),
            ))))
            .await
    }

    /// Publish a pre-constructed message on the Pots topic.
    pub async fn publish_message(&mut self, message: Arc<Message>) -> anyhow::Result<()> {
        self.0.publish(message).await
    }
}
