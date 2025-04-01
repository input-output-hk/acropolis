//! Acropolis common library - message serialiser
//! Serialises messages based on block number

use std::collections::BinaryHeap;
use std::cmp::Ordering;
use tracing::{debug, info};
use std::sync::Arc;
use tokio::sync::Mutex;
use async_trait::async_trait;
use caryatid_sdk::MessageBounds;
use anyhow::Result;

/// Pending queue entry
struct PendingEntry<MSG: MessageBounds> {
    /// Sequence number
    sequence: u64,

    /// Message
    message: MSG,
}

// Ord and Eq implementations to make it a min-heap on block number
impl<MSG: MessageBounds> Ord for PendingEntry<MSG> {
    fn cmp(&self, other: &Self) -> Ordering {
        other.sequence.cmp(&self.sequence)  // Note reverse order
    }
}

impl<MSG: MessageBounds> PartialOrd for PendingEntry<MSG> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<MSG: MessageBounds> Eq for PendingEntry<MSG> {}

impl<MSG: MessageBounds> PartialEq for PendingEntry<MSG> {
    fn eq(&self, other: &Self) -> bool {
        self.sequence == other.sequence
    }
}


/// Message handler (once serialised)
#[async_trait]
pub trait SerialisedMessageHandler<MSG: MessageBounds>: Send + Sync {

    /// Handle a message
    async fn handle(&mut self, message: &MSG) -> Result<()>;
}

/// Message serialiser
pub struct Serialiser<'a, MSG: MessageBounds> {
    /// Pending queue, presents messages in order, implemented as a reversed max-heap
    pending: BinaryHeap<PendingEntry<MSG>>,

    /// Next sequence expected
    next_sequence: u64,

    /// Message handler
    handler: Arc<Mutex<dyn SerialisedMessageHandler<MSG>>>,

    /// Module path using it (for logging)
    module_name: &'a str,
}

impl <'a, MSG: MessageBounds> Serialiser<'a, MSG> {
    /// Constructor
    pub fn new(handler: Arc<Mutex<dyn SerialisedMessageHandler<MSG>>>,
               module_name: &'a str, first_sequence: u64) -> Self {
        Self {
            pending: BinaryHeap::new(),
            next_sequence: first_sequence,
            handler,
            module_name,
        }
    }

    /// Process a message
    async fn process_message(&mut self, sequence: u64, message: &MSG) -> Result<()> {
        // Pass to the handler
        self.handler.lock().await.handle(message).await?;

        // Update sequence
        self.next_sequence = sequence + 1;

        Ok(())
    }

    /// Handle a message
    pub async fn handle_message(&mut self, sequence: u64, message: &MSG) -> Result<()> {

        // Is it in order?
        if sequence == self.next_sequence {

            self.process_message(sequence, &message).await?;

            // See if any pending now work
            while let Some(next) = self.pending.peek() {
                if next.sequence == self.next_sequence {

                    if tracing::enabled!(tracing::Level::DEBUG) {
                        debug!("Now accepted event {}", next.sequence);
                    }

                    if let Some(next) = self.pending.pop() {
                        self.process_message(next.sequence, &next.message).await?;
                    }
                } else {
                    break;
                }
            }
        } else {
            // Not accepted, it's out of order, queue it
            if tracing::enabled!(tracing::Level::DEBUG) {
                debug!("Queueing out-of-order event {}", sequence);
            }
            self.pending.push(PendingEntry {
                sequence,
                message: message.clone(),
            });
        }

        Ok(())
    }

    /// Periodic tick for background logging
    pub fn tick(&mut self) {
        if self.pending.len() != 0 {
            info!(module = self.module_name, pending = self.pending.len());
        }
    }
}

// -- Tests --
#[cfg(test)]
mod tests {
    use super::*;

    // Mock message handler to track received messages
    struct MockMessageHandler {
        received: Vec<u64>,
    }

    impl MockMessageHandler {
        pub fn new() -> Self {
            Self {
                received: Vec::new()
            }
        }
    }

    // Test message
    #[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
    pub struct TestMessage {
        index: u64
    }

    #[async_trait]
    impl SerialisedMessageHandler<TestMessage> for MockMessageHandler {
        async fn handle(&mut self, message: &TestMessage) -> Result<()> {
            self.received.push(message.index);
            Ok(())
        }
    }

    // Simple in-order test
    #[tokio::test]
    async fn messages_in_order_pass_through() {
        let handler = Arc::new(Mutex::new(MockMessageHandler::new()));
        let handler2 = handler.clone();
        let mut serialiser = Serialiser::new(handler, "test", 0);

        let message0 = TestMessage { index: 0 };
        serialiser.handle_message(0, &message0).await.unwrap();

        let message1 = TestMessage { index: 1 };
        serialiser.handle_message(1, &message1).await.unwrap();

        let message2 = TestMessage { index: 2 };
        serialiser.handle_message(2, &message2).await.unwrap();

        let handler = handler2.lock().await;
        assert_eq!(3, handler.received.len());
        assert_eq!(0, handler.received[0]);
        assert_eq!(1, handler.received[1]);
        assert_eq!(2, handler.received[2]);
    }

    // Simple out-of-order test
    #[tokio::test]
    async fn messages_out_of_order_are_reordered() {
        let handler = Arc::new(Mutex::new(MockMessageHandler::new()));
        let handler2 = handler.clone();
        let mut serialiser = Serialiser::new(handler, "test", 42);

        let message1 = TestMessage { index: 1 };
        serialiser.handle_message(43, &message1).await.unwrap();

        let message0 = TestMessage { index: 0 };
        serialiser.handle_message(42, &message0).await.unwrap();

        let message2 = TestMessage { index: 2 };
        serialiser.handle_message(44, &message2).await.unwrap();

        let handler = handler2.lock().await;
        assert_eq!(3, handler.received.len());
        assert_eq!(0, handler.received[0]);
        assert_eq!(1, handler.received[1]);
        assert_eq!(2, handler.received[2]);
    }

}
