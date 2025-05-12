use caryatid_sdk::Context;
use acropolis_common::{messages::{Message, Sequence}, SerialisedHandler, serialiser::Serialisable};
use anyhow::Result;
use async_trait::async_trait;
use tracing::error;
use std::sync::Arc;

pub struct Sender<T: Serialisable> {
    context: Arc<Context<Message>>,
    topic: String,
    prev_sequence: Option<u64>,
    msg_builder: Box<dyn for<'a, 'b> Fn(&'a Sequence, &'b T) -> Message + Send + 'static>,
}

impl<T: Serialisable> Sender<T> {
    pub fn new<F>(context: Arc<Context<Message>>, topic: String, prev_sequence: Option<u64>, msg_builder: F) -> Self
    where
        F: for<'a, 'b> Fn(&'a Sequence, &'b T) -> Message + Send + 'static,
    {
        Sender {
            context,
            topic,
            prev_sequence,
            msg_builder: Box::new(msg_builder),
        }
    }

}

#[async_trait]
impl<T: Serialisable> SerialisedHandler<Option<T>> for Sender<T> {
    async fn handle(&mut self, sequence: u64, data: &Option<T>) -> Result<()> {
        match data {
            Some(data) => {
                let sequence = Sequence::new(sequence, self.prev_sequence);
                self.prev_sequence = Some(sequence.number);
                let message = (self.msg_builder)(&sequence, &data);
                self.context.message_bus.publish(&self.topic, Arc::new(message))
                    .await
                    .unwrap_or_else(|e| error!("Failed to publish: {e}"));
            },
            _ => (),
        }
        Ok(())
    }
}
