use std::sync::Arc;
use caryatid_sdk::Context;
use tokio::sync::Mutex;
use acropolis_common::{AddressDelta, DRepCredential, Lovelace};
use acropolis_common::messages::{DrepStakeDistributionMessage, Message};

pub struct DrepVotingStakePublisher {
    /// Module context
    context: Arc<Context<Message>>,

    /// Topic to publish on
    topic: String,

    sequence: u64,
}

impl DrepVotingStakePublisher {
    pub fn new(context: Arc<Context<Message>>, topic: String) -> Self {
        Self { context, topic, sequence: 1 }
    }

    pub async fn publish_stake(&mut self, s: Vec<(DRepCredential, Lovelace)>) -> anyhow::Result<()> {
        match self.context.message_bus.publish(
            &self.topic,
            Arc::new(Message::DrepStakeDistribution(DrepStakeDistributionMessage {
                sequence: self.sequence,
                data: s
            }))
        ).await {
            Ok(()) => { self.sequence += 1; Ok(()) },
            err => err
        }
    }
}
