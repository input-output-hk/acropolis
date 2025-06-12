use acropolis_common::messages::{CardanoMessage, DRepStakeDistributionMessage, Message};
use acropolis_common::{BlockInfo, DRepCredential, Lovelace};
use caryatid_sdk::Context;
use std::sync::Arc;

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

    pub async fn publish_stake(
        &mut self,
        block: &BlockInfo,
        s: Option<Vec<(DRepCredential, Lovelace)>>,
    ) -> anyhow::Result<()> {
        self.context
            .message_bus
            .publish(
                &self.topic,
                Arc::new(Message::Cardano((
                    block.clone(),
                    CardanoMessage::DRepStakeDistribution(DRepStakeDistributionMessage { data: s }),
                ))),
            )
            .await
    }
}
