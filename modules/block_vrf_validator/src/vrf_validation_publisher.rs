use acropolis_common::{
    messages::{CardanoMessage, Message},
    ouroboros::vrf_validation::VrfValidationError,
    validation::{ValidationError, ValidationStatus},
    BlockInfo,
};
use caryatid_sdk::Context;
use std::sync::Arc;
use tracing::error;

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

    /// Publish the SPDD
    pub async fn publish_vrf_validation(
        &mut self,
        block: &BlockInfo,
        validation_result: Result<(), VrfValidationError>,
    ) -> anyhow::Result<()> {
        let validation_status = match validation_result {
            Ok(_) => ValidationStatus::Go,
            Err(error) => {
                error!("VRF validation failed: {}", error.clone());
                ValidationStatus::NoGo(ValidationError::from(error))
            }
        };
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
