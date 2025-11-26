use acropolis_common::{
    caryatid::RollbackAwarePublisher,
    messages::{CardanoMessage, DRepStateMessage, Message},
    BlockInfo, Credential,
};
use caryatid_sdk::Context;
use std::sync::Arc;

/// Message publisher for DRep State
pub struct DRepStatePublisher(RollbackAwarePublisher<Message>);

impl DRepStatePublisher {
    /// Construct with context and topic to publish on
    pub fn new(context: Arc<Context<Message>>, topic: String) -> Self {
        Self(RollbackAwarePublisher::new(context, topic))
    }

    /// Publish the DRep state
    pub async fn publish_drep_state(
        &mut self,
        block: &BlockInfo,
        dreps: Vec<(Credential, u64)>,
    ) -> anyhow::Result<()> {
        self.0
            .publish(Arc::new(Message::Cardano((
                block.clone(),
                CardanoMessage::DRepState(DRepStateMessage {
                    epoch: block.epoch,
                    dreps,
                }),
            ))))
            .await
    }

    /// Publish a rollback message, if we have anything to roll back
    pub async fn publish_rollback(&mut self, message: Arc<Message>) -> anyhow::Result<()> {
        self.0.publish(message).await
    }
}
