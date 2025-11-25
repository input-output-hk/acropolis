use std::sync::Arc;

use anyhow::Result;
use caryatid_sdk::{async_trait, MessageBounds, Subscription};

use crate::messages::{CardanoMessage, Message};

#[async_trait]
pub trait SubscriptionExt<M: MessageBounds> {
    async fn read_ignoring_rollbacks(&mut self) -> Result<(String, Arc<M>)>;
}

#[async_trait]
impl SubscriptionExt<Message> for Box<dyn Subscription<Message>> {
    async fn read_ignoring_rollbacks(&mut self) -> Result<(String, Arc<Message>)> {
        loop {
            let (stream, message) = self.read().await?;
            if matches!(
                message.as_ref(),
                Message::Cardano((_, CardanoMessage::Rollback(_)))
            ) {
                continue;
            }
            break Ok((stream, message));
        }
    }
}
