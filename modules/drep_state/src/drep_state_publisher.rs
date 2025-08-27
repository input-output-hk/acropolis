use acropolis_common::{
    messages::{CardanoMessage, DRepStateMessage, Message},
    BlockInfo, Credential,
};
use caryatid_sdk::Context;
use std::sync::Arc;

/// Message publisher for DRep State
pub struct DRepStatePublisher {
    /// Module context
    context: Arc<Context<Message>>,

    /// Topic to publish on
    topic: String,
}

impl DRepStatePublisher {
    /// Construct with context and topic to publish on
    pub fn new(context: Arc<Context<Message>>, topic: String) -> Self {
        Self { context, topic }
    }

    /// Publish the DRep state
    pub async fn publish_drep_state(
        &mut self,
        block: &BlockInfo,
        dreps: Vec<(Credential, u64)>,
    ) -> anyhow::Result<()> {
        self.context
            .message_bus
            .publish(
                &self.topic,
                Arc::new(Message::Cardano((
                    block.clone(),
                    CardanoMessage::DRepState(DRepStateMessage {
                        epoch: block.epoch,
                        dreps,
                    }),
                ))),
            )
            .await
    }
}
