use std::sync::Arc;

use acropolis_common::{
    caryatid::RollbackAwarePublisher,
    messages::{BlockTxsMessage, CardanoMessage, Message},
    BlockInfo,
};
use async_trait::async_trait;
use caryatid_sdk::Context;
use config::Config;
use tokio::sync::Mutex;

use crate::publish_observer_message;
use crate::state::BlockTotalsObserver;

pub struct BlockTotalsPublisher {
    state: Mutex<BlockTotalsState>,
    publisher: Option<Mutex<RollbackAwarePublisher<Message>>>,
}

#[derive(Default)]
struct BlockTotalsState {
    tx_count: u64,
    total_output: u128,
    total_fees: u64,
}

#[async_trait]
impl BlockTotalsObserver for BlockTotalsPublisher {
    /// Observe a new block
    async fn start_block(&self, _block: &BlockInfo) {
        let mut state = self.state.lock().await;
        state.tx_count = 0;
        state.total_output = 0;
        state.total_fees = 0;
    }

    async fn observe_tx(&self, output: u64, fee: u64) {
        let mut state = self.state.lock().await;
        state.tx_count += 1;
        state.total_output += output as u128;
        state.total_fees += fee;
    }

    async fn finalise_block(&self, block: &BlockInfo) {
        if self.publisher.is_none() {
            return;
        }

        let state = self.state.lock().await;

        // Send out the accumulated totals
        let message = BlockTxsMessage {
            total_txs: state.tx_count,
            total_output: state.total_output,
            total_fees: state.total_fees,
        };
        let message_enum =
            Message::Cardano((block.clone(), CardanoMessage::BlockInfoMessage(message)));

        publish_observer_message(&self.publisher, Arc::new(message_enum), "Failed to publish")
            .await;
    }

    async fn rollback(&self, message: Arc<Message>) {
        publish_observer_message(&self.publisher, message, "Failed to publish rollback").await;
    }
}

impl BlockTotalsPublisher {
    pub fn new(context: Arc<Context<Message>>, config: Arc<Config>) -> Self {
        Self {
            state: Mutex::new(BlockTotalsState::default()),
            publisher: config
                .get_string("block-totals-topic")
                .ok()
                .map(|topic| Mutex::new(RollbackAwarePublisher::new(context, topic))),
        }
    }
}
