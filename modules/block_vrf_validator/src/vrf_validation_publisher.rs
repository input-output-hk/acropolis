use acropolis_common::{
    messages::{CardanoMessage, Message},
    validation::{ValidationStatus, VrfValidationError},
    BlockInfo,
};
use caryatid_sdk::Context;
use std::sync::Arc;
use tracing::debug;

/// Message publisher for Block header Vrf Validation Result
pub struct VrfValidationPublisher {
    /// Module context
    context: Arc<Context<Message>>,

    /// Topic to publish on
    topic: String,
}

impl VrfValidationPublisher {
    /// Construct with context and topic to publish on
    pub fn new(context: Arc<Context<Message>>, topic: String) -> Self {
        Self { context, topic }
    }

    pub async fn publish_vrf_validation(
        &mut self,
        block: &BlockInfo,
        validation_result: Result<(), VrfValidationError>,
    ) -> anyhow::Result<()> {
        // TODO: Re-enable VRF validation - currently accepting all blocks
        if let Err(error) = &validation_result {
            debug!(
                "VRF validation would have failed (ignored): {} of block {}",
                error, block.number
            );
        }
        let validation_status = ValidationStatus::Go;
        self.context
            .message_bus
            .publish(
                &self.topic,
                Arc::new(Message::Cardano((
                    block.clone(),
                    CardanoMessage::BlockValidation(validation_status),
                ))),
            )
            .await
    }
}
