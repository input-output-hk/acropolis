use std::sync::Arc;
use caryatid_sdk::Context;
use acropolis_common::{DRepCredential, Lovelace, BlockInfo};
use acropolis_common::messages::{DRepStakeDistributionMessage, Message, CardanoMessage};

pub struct DRepDistributionPublisher {
    /// Module context
    context: Arc<Context<Message>>,

    /// Topic to publish on
    topic: String,
}

impl DRepDistributionPublisher {
    pub fn new(context: Arc<Context<Message>>, topic: String) -> Self {
        Self { context, topic }
    }

    pub async fn publish_stake(&mut self, block: &BlockInfo,
                               s: Option<Vec<(DRepCredential, Lovelace)>>) -> anyhow::Result<()> {
        self.context.message_bus.publish(
            &self.topic,
            Arc::new(Message::Cardano((
                block.clone(),
                CardanoMessage::DRepStakeDistribution(DRepStakeDistributionMessage {
                    data: s
                })
            )))
        ).await
    }
}
