use acropolis_common::messages::Message;
use caryatid_sdk::Context;
use std::sync::Arc;

/// Message publisher for SPO State
pub struct SPOStatePublisher {
    /// Module context
    context: Arc<Context<Message>>,

    /// Topic to publish on
    topic: String,
}

impl SPOStatePublisher {
    /// Construct with context and topic to publish on
    pub fn new(context: Arc<Context<Message>>, topic: String) -> Self {
        Self { context, topic }
    }

    /// Publish the DRep Delegation Distribution
    pub async fn publish(&mut self, message: Arc<Message>) -> anyhow::Result<()> {
        self.context.message_bus.publish(&self.topic, message).await
    }
}
