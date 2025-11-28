use std::sync::Arc;

use anyhow::Result;
use caryatid_sdk::{async_trait, Context, MessageBounds, Subscription};

use crate::messages::{CardanoMessage, Message, StateTransitionMessage};

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
                Message::Cardano((
                    _,
                    CardanoMessage::StateTransition(StateTransitionMessage::Rollback(_))
                ))
            ) {
                continue;
            }
            break Ok((stream, message));
        }
    }
}

/// A utility to publish messages, which will only publish rollback messages if some work has been rolled back
pub struct RollbackAwarePublisher<M: MessageBounds> {
    /// Module context
    context: Arc<Context<M>>,

    /// Topic to publish on
    topic: String,

    // At which slot did we publish our last non-rollback message
    last_activity_at: Option<u64>,
}

impl RollbackAwarePublisher<Message> {
    pub fn new(context: Arc<Context<Message>>, topic: String) -> Self {
        Self {
            context,
            topic,
            last_activity_at: None,
        }
    }

    pub async fn publish(&mut self, message: Arc<Message>) -> Result<()> {
        match message.as_ref() {
            Message::Cardano((
                block,
                CardanoMessage::StateTransition(StateTransitionMessage::Rollback(_)),
            )) => {
                if self.last_activity_at.is_some_and(|slot| slot >= block.slot) {
                    self.last_activity_at = None;
                    self.context.publish(&self.topic, message).await?;
                }
                Ok(())
            }
            Message::Cardano((block, _)) => {
                self.last_activity_at = Some(block.slot);
                self.context.publish(&self.topic, message).await
            }
            _ => self.context.publish(&self.topic, message).await,
        }
    }
}
