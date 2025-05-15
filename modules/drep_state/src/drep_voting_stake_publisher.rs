use std::sync::Arc;
use caryatid_sdk::Context;
use acropolis_common::{DRepCredential, Lovelace};
use acropolis_common::messages::{DrepStakeDistributionMessage, Message};

pub struct DrepVotingStakePublisher {
    /// Module context
    context: Arc<Context<Message>>,

    /// Topic to publish on
    topic: String,
}

impl DrepVotingStakePublisher {
    pub fn new(context: Arc<Context<Message>>, topic: String) -> Self {
        Self { context, topic }
    }

    pub async fn publish_stake(&mut self, s: Vec<(DRepCredential, Lovelace)>) -> anyhow::Result<()> {
        self.context.message_bus.publish(
            &self.topic,
            Arc::new(Message::DrepStakeDistribution(DrepStakeDistributionMessage {
                data: s
            }))).await
    }
}
