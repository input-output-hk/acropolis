use acropolis_common::{
    messages::{CardanoMessage, Message},
    validation::{TransactionValidationError, ValidationError, ValidationStatus},
    BlockInfo,
};
use caryatid_sdk::Context;
use std::sync::Arc;
use tracing::error;

/// Message publisher for Block header Tx Validation Result
pub struct TxValidationPublisher {
    /// Module context
    context: Arc<Context<Message>>,

    /// Topic to publish on
    topic: String,
}

impl TxValidationPublisher {
    /// Construct with context and topic to publish on
    pub fn new(context: Arc<Context<Message>>, topic: String) -> Self {
        Self { context, topic }
    }

    pub async fn publish_tx_validation(
        &self,
        block: &BlockInfo,
        tx_errors: Vec<(u16, TransactionValidationError)>,
    ) -> anyhow::Result<()> {
        let validation_status = if tx_errors.is_empty() {
            ValidationStatus::Go
        } else {
            error!(
                "Tx validation failed: block={}, tx_index={}, error={}",
                block.number,
                tx_errors[0].0,
                tx_errors[0].1.clone(),
            );
            ValidationStatus::NoGo(ValidationError::BadTransaction {
                tx_index: tx_errors[0].0,
                error: tx_errors[0].1.clone(),
            })
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
