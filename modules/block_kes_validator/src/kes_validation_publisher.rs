use acropolis_common::{
    messages::{CardanoMessage, Message},
    validation::{KesValidationError, ValidationError, ValidationStatus},
    BlockInfo, PoolId,
};
use caryatid_sdk::Context;
use std::sync::Arc;
use tracing::error;

/// Message publisher for Block header KES Validation Result
pub struct KesValidationPublisher {
    /// Module context
    context: Arc<Context<Message>>,

    /// Topic to publish on
    topic: String,
}

impl KesValidationPublisher {
    /// Construct with context and topic to publish on
    pub fn new(context: Arc<Context<Message>>, topic: String) -> Self {
        Self { context, topic }
    }

    pub async fn publish_kes_validation(
        &self,
        block: &BlockInfo,
        validation_result: Result<Option<(PoolId, u64)>, KesValidationError>,
    ) -> anyhow::Result<()> {
        let validation_status = match validation_result {
            Ok(_) => ValidationStatus::Go,
            Err(error) => {
                error!(
                    "KES validation failed: {} of block {}",
                    error.clone(),
                    block.number
                );
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
