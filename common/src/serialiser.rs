//! Acropolis common library - serialiser
//! Serialises based on a gapless sequence number

use std::collections::BinaryHeap;
use std::cmp::Ordering;
use tracing::{debug, info};
use std::sync::Arc;
use tokio::sync::Mutex;
use async_trait::async_trait;
use anyhow::Result;
use crate::messages::Sequence;

pub trait Serialisable: Clone + Sync {}
impl<T: Clone + Sync> Serialisable for T {}

/// Pending queue entry
struct PendingEntry<T: Serialisable> {
    /// Sequence
    sequence: Sequence,

    /// Data
    data: T,
}

// Ord and Eq implementations to make it a min-heap on sequence number
impl<T: Serialisable> Ord for PendingEntry<T> {
    fn cmp(&self, other: &Self) -> Ordering {
        other.sequence.number.cmp(&self.sequence.number)  // Note reverse order
    }
}

impl<T: Serialisable> PartialOrd for PendingEntry<T> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<T: Serialisable> Eq for PendingEntry<T> {}

impl<T: Serialisable> PartialEq for PendingEntry<T> {
    fn eq(&self, other: &Self) -> bool {
        self.sequence.number == other.sequence.number
    }
}


/// Data handler (once serialised)
#[async_trait]
pub trait SerialisedHandler<T: Serialisable>: Send {
    /// Handle a message
    async fn handle(&mut self, sequence: u64, data: &T) -> Result<()>;
}

/// Serialiser
pub struct Serialiser<'a, T: Serialisable> {
    /// Pending queue, presents data in order, implemented as a reversed max-heap
    pending: BinaryHeap<PendingEntry<T>>,

    /// Previous sequence expected
    prev_sequence: Option<u64>,

    /// Message handler
    handler: Arc<Mutex<dyn SerialisedHandler<T>>>,

    /// Module path using it (for logging)
    module_name: &'a str,
}

impl <'a, T: Serialisable> Serialiser<'a, T> {
    /// Constructor
    pub fn new(handler: Arc<Mutex<dyn SerialisedHandler<T>>>,
               module_name: &'a str) -> Self {
        Self {
            pending: BinaryHeap::new(),
            prev_sequence: None,
            handler,
            module_name,
        }
    }
    pub fn new_from(handler: Arc<Mutex<dyn SerialisedHandler<T>>>,
                    module_name: &'a str,
                    prev_sequence: Option<u64>) -> Self {
        Self {
            pending: BinaryHeap::new(),
            prev_sequence,
            handler,
            module_name,
        }
    }

    /// Process data
    async fn process(&mut self, sequence: Sequence, data: &T) -> Result<()> {
        // Pass to the handler
        self.handler.lock().await.handle(sequence.number, data).await?;

        // Update sequence
        self.prev_sequence = Some(sequence.number);

        Ok(())
    }

    /// Handle data
    pub async fn handle(&mut self, sequence: Sequence, data: &T) -> Result<()> {

        // Is it in order?
        if sequence.previous == self.prev_sequence {

            self.process(sequence, &data).await?;

            // See if any pending now work
            while let Some(next) = self.pending.peek() {
                if next.sequence.previous == self.prev_sequence {

                    if tracing::enabled!(tracing::Level::DEBUG) {
                        debug!("Now accepted event {:?}", next.sequence);
                    }

                    if let Some(next) = self.pending.pop() {
                        self.process(next.sequence, &next.data).await?;
                    }
                } else {
                    break;
                }
            }
        } else {
            // Not accepted, it's out of order, queue it
            if tracing::enabled!(tracing::Level::DEBUG) {
                debug!("Queueing out-of-order event {:?}", sequence);
            }
            self.pending.push(PendingEntry {
                sequence,
                data: data.clone(),
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
    struct MockHandler {
        received: Vec<u64>,
    }

    impl MockHandler {
        pub fn new() -> Self {
            Self {
                received: Vec::new()
            }
        }
    }

    // Test data
    #[derive(Clone)]
    pub struct TestData {
        index: u64
    }

    #[async_trait]
    impl SerialisedHandler<TestData> for MockHandler {
        async fn handle(&mut self, _sequence: u64, data: & TestData) -> Result<()> {
            self.received.push(data.index);
            Ok(())
        }
    }

    // Simple in-order test
    #[tokio::test]
    async fn messages_in_order_pass_through() {
        let handler = Arc::new(Mutex::new(MockHandler::new()));
        let handler2 = handler.clone();
        let mut serialiser = Serialiser::new(handler, "test");

        let message0 = TestData { index: 0 };
        serialiser.handle(Sequence::new(0, None), &message0).await.unwrap();

        let message1 = TestData { index: 1 };
        serialiser.handle(Sequence::new(1, Some(0)), &message1).await.unwrap();

        let message2 = TestData { index: 2 };
        serialiser.handle(Sequence::new(2, Some(1)), &message2).await.unwrap();

        let handler = handler2.lock().await;
        assert_eq!(3, handler.received.len());
        assert_eq!(0, handler.received[0]);
        assert_eq!(1, handler.received[1]);
        assert_eq!(2, handler.received[2]);
    }

    // Simple out-of-order test
    #[tokio::test]
    async fn messages_out_of_order_are_reordered() {
        let handler = Arc::new(Mutex::new(MockHandler::new()));
        let handler2 = handler.clone();
        let mut serialiser = Serialiser::new(handler, "test");

        let message1 = TestData { index: 1 };
        serialiser.handle(Sequence::new(43, Some(42)), &message1).await.unwrap();

        let message0 = TestData { index: 0 };
        serialiser.handle(Sequence::new(42, None), &message0).await.unwrap();

        let message2 = TestData { index: 2 };
        serialiser.handle(Sequence::new(44, Some(43)), &message2).await.unwrap();

        let handler = handler2.lock().await;
        assert_eq!(3, handler.received.len());
        assert_eq!(0, handler.received[0]);
        assert_eq!(1, handler.received[1]);
        assert_eq!(2, handler.received[2]);
    }

}
