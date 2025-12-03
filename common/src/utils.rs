use anyhow::bail;
use std::sync::Arc;
use caryatid_sdk::Context;
use tracing::error;
use crate::BlockInfo;
use crate::messages::CardanoMessage::BlockValidation;
use crate::messages::Message;
use crate::validation::{ValidationError, ValidationStatus};

#[macro_export] macro_rules! declare_cardano_reader {
    ($name:ident, $msg_constructor:ident, $msg_type:ty) => {
        async fn $name(s: &mut Box<dyn Subscription<Message>>) -> Result<(BlockInfo, $msg_type)> {
            info!("Waiting in topic {}", stringify!($msg_constructor));
            match s.read_ignoring_rollbacks().await?.1.as_ref() {
                Message::Cardano((blk, CardanoMessage::$msg_constructor(body))) => {
                    Ok((blk.clone(), body.clone()))
                }
                msg => Err(anyhow!(
                    "Unexpected message {msg:?} for {} topic", stringify!($msg_constructor)
                )),
            }
        }
    };
}

pub struct ValidationOutcomes {
    outcomes: Vec<ValidationError>,
}

impl ValidationOutcomes {
    pub fn new() -> Self {
        Self { outcomes: Vec::new() }
    }

    pub fn merge(&mut self, with: &mut ValidationOutcomes) {
        self.outcomes.append(&mut with.outcomes);
    }

    pub fn push(&mut self, outcome: ValidationError) {
        self.outcomes.push(outcome);
    }

    pub fn push_anyhow(&mut self, error: anyhow::Error) {
        self.outcomes.push(ValidationError::Unclassified(format!("{}", error)));
    }

    pub async fn publish(
        &mut self,
        context: &Arc<Context<Message>>,
        topic_field: &str,
        block: &BlockInfo,
    ) -> anyhow::Result<()> {
        if block.intent.do_validation() {
            let status = if let Some(result) = self.outcomes.get(0) {
                // TODO: add multiple responses / decide that they're not necessary
                ValidationStatus::NoGo(result.clone())
            }
            else {
                ValidationStatus::Go
            };

            let outcome_msg = Arc::new(
                Message::Cardano((block.clone(), BlockValidation(status)))
            );

            context.message_bus.publish(topic_field, outcome_msg).await?;
        }
        else if !self.outcomes.is_empty() {
            error!("Error in validation, block {block:?}: outcomes {:?}", self.outcomes);
        }
        self.outcomes.clear();
        Ok(())
    }

    #[allow(dead_code)]
    pub fn as_result(&self) -> anyhow::Result<()> {
        if self.outcomes.is_empty() {
            return Ok(());
        }

        let res = self.outcomes.iter().map(
            |e| format!("{}; ", e)
        ).collect::<String>();

        bail!("Validation failed: {}", res)
    }
}
