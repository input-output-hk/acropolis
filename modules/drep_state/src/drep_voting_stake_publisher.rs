use std::sync::Arc;
use caryatid_sdk::Context;
use acropolis_common::{AddressDelta, DRepCredential, Lovelace};
use acropolis_common::messages::{DrepStakeDistributionMessage, Message, Sequence};

pub struct DrepVotingStakePublisher {
    /// Module context
    context: Arc<Context<Message>>,

    /// Topic to publish on
    topic: String,

    sequence: Sequence,
}

impl DrepVotingStakePublisher {
    pub fn new(context: Arc<Context<Message>>, topic: String) -> Self {
        Self { context, topic, sequence: Sequence::new(1, None) }
    }

    pub async fn publish_stake(&mut self, s: Vec<(DRepCredential, Lovelace)>) -> anyhow::Result<()> {
        match self.context.message_bus.publish(
            &self.topic,
            Arc::new(Message::DrepStakeDistribution(DrepStakeDistributionMessage {
                sequence: self.sequence,
                data: s
            }))
        ).await {
            Ok(()) => { self.sequence.inc(); Ok(()) },
            err => err
        }
    }
}
