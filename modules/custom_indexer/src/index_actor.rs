use std::sync::Arc;

use acropolis_common::{BlockInfo, Point};
use tokio::sync::{mpsc, oneshot};

use pallas::ledger::traverse::MultiEraTx;

use crate::{cursor_store::CursorEntry, IndexWrapper};

pub enum IndexCommand {
    ApplyTxs {
        block: BlockInfo,
        txs: Vec<Arc<[u8]>>,
        response_tx: oneshot::Sender<IndexResult>,
    },
    Rollback {
        point: Point,
        response_tx: oneshot::Sender<IndexResult>,
    },
}

#[derive(Debug)]
pub enum IndexResult {
    Success { entry: CursorEntry },
    DecodeError { entry: CursorEntry, reason: String },
    HandleError { entry: CursorEntry, reason: String },
    Reset { entry: CursorEntry },
    Halted,
    FatalResetError { entry: CursorEntry, reason: String },
}

pub async fn index_actor(mut wrapper: IndexWrapper, mut rx: mpsc::Receiver<IndexCommand>) {
    while let Some(cmd) = rx.recv().await {
        match cmd {
            IndexCommand::ApplyTxs {
                block,
                txs,
                response_tx,
            } => {
                let result = handle_apply_txs(&mut wrapper, block, txs).await;
                let _ = response_tx.send(result);
            }

            IndexCommand::Rollback { point, response_tx } => {
                let result = handle_rollback(&mut wrapper, point).await;
                let _ = response_tx.send(result);
            }
        }
    }
}

async fn handle_apply_txs(
    wrapper: &mut IndexWrapper,
    block: acropolis_common::BlockInfo,
    txs: Vec<Arc<[u8]>>,
) -> IndexResult {
    // If the index is halted early return and continue waiting for a rollback event
    if wrapper.halted {
        return IndexResult::Halted;
    }

    // If the index has a tip greater than the current set of transactions early return and continue waiting for chainsync to catch up
    if block.slot <= wrapper.tip.slot() {
        return IndexResult::Success {
            entry: CursorEntry {
                tip: wrapper.tip.clone(),
                halted: false,
            },
        };
    }

    // Decode the transactions and call handle_onchain_tx for each, halting if decode or the handler return an error
    for raw in txs {
        let decoded = match MultiEraTx::decode(raw.as_ref()) {
            Ok(tx) => tx,
            Err(e) => {
                wrapper.halted = true;
                return IndexResult::DecodeError {
                    entry: CursorEntry {
                        tip: wrapper.tip.clone(),
                        halted: true,
                    },
                    reason: e.to_string(),
                };
            }
        };
        if let Err(e) = wrapper.index.handle_onchain_tx(&block, &decoded).await {
            wrapper.halted = true;
            return IndexResult::HandleError {
                entry: CursorEntry {
                    tip: wrapper.tip.clone(),
                    halted: true,
                },
                reason: e.to_string(),
            };
        }
    }

    // Update index tip and return success
    wrapper.tip = Point::Specific {
        hash: block.hash,
        slot: block.slot,
    };
    IndexResult::Success {
        entry: CursorEntry {
            tip: wrapper.tip.clone(),
            halted: false,
        },
    }
}

async fn handle_rollback(wrapper: &mut IndexWrapper, point: Point) -> IndexResult {
    match wrapper.index.handle_rollback(&point).await {
        Ok(_) => {
            // If the rollback is successful, remove the halt (if any), update the tip, and return success
            wrapper.halted = false;
            wrapper.tip = point.clone();
            IndexResult::Success {
                entry: CursorEntry {
                    tip: wrapper.tip.clone(),
                    halted: false,
                },
            }
        }
        // If the rollback failed, attempt to reset the index
        Err(_) => match wrapper.index.reset(&wrapper.default_start).await {
            // If reset successful, remove the halt (if any), update the tip and return reset so the manager can send a FindIntersect command
            Ok(point) => {
                wrapper.tip = point;
                wrapper.halted = false;
                IndexResult::Reset {
                    entry: CursorEntry {
                        tip: wrapper.tip.clone(),
                        halted: false,
                    },
                }
            }
            // If the reset fails, return a fatal error to remove the index from the manager (On next run the index will attempt to reset again)
            Err(e) => IndexResult::FatalResetError {
                entry: CursorEntry {
                    tip: wrapper.tip.clone(),
                    halted: true,
                },
                reason: e.to_string(),
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use acropolis_common::{BlockHash, BlockInfo, BlockIntent, BlockStatus, Era, Point};
    use caryatid_sdk::async_trait;
    use pallas::ledger::traverse::MultiEraTx;
    use tokio::sync::{mpsc, oneshot};

    use crate::{
        chain_index::ChainIndex,
        cursor_store::InMemoryCursorStore,
        index_actor::{IndexCommand, IndexResult},
        CustomIndexer,
    };

    #[derive(Default)]
    pub struct MockIndex {
        pub on_tx: Option<Box<dyn Fn() -> anyhow::Result<()> + Send + Sync>>,
        pub on_rollback: Option<Box<dyn Fn() -> anyhow::Result<()> + Send + Sync>>,
        pub on_reset: Option<Box<dyn Fn() -> anyhow::Result<Point> + Send + Sync>>,
    }

    #[async_trait]
    impl ChainIndex for MockIndex {
        fn name(&self) -> String {
            "mock-index".into()
        }

        async fn handle_onchain_tx(
            &mut self,
            _info: &BlockInfo,
            _tx: &MultiEraTx<'_>,
        ) -> anyhow::Result<()> {
            if let Some(f) = &self.on_tx {
                f()
            } else {
                Ok(())
            }
        }

        async fn handle_rollback(&mut self, _point: &Point) -> anyhow::Result<()> {
            if let Some(f) = &self.on_rollback {
                f()
            } else {
                Ok(())
            }
        }
        async fn reset(&mut self, start: &Point) -> anyhow::Result<Point> {
            if let Some(f) = &self.on_reset {
                f()
            } else {
                Ok(start.clone())
            }
        }
    }

    fn test_block(slot: u64) -> BlockInfo {
        BlockInfo {
            status: BlockStatus::Volatile,
            intent: BlockIntent::none(),
            slot,
            number: 1,
            hash: BlockHash::default(),
            epoch: 0,
            epoch_slot: 0,
            new_epoch: false,
            timestamp: 0,
            era: Era::Conway,
        }
    }

    fn valid_tx() -> Arc<[u8]> {
        let raw_tx = hex::decode(
            "84a600d9010281825820565573dcde964aa30e7e307531ee6c6f8e47279dcbade4b4301e9ef291b6791601018282583901b786e57fa44f9707d023719c60b712a3ebbaf89a932ee87ea4de39ce65f459f57e462edc82d90225fac6162f4757c226ad50a7adf230e4c81b0000000ac336383982583901b786e57fa44f9707d023719c60b712a3ebbaf89a932ee87ea4de39ce65f459f57e462edc82d90225fac6162f4757c226ad50a7adf230e4c81a004c4b40021a0002aac1031a0a0d7b1705a1581de165f459f57e462edc82d90225fac6162f4757c226ad50a7adf230e4c81a42fa31010801a100d9010282825820ed67aef668355b2f6220aeb7b5118adeb31b7cf0de7d9a4bb4ea0aac7bdfea5a58406718e1a35b9fae1c91d0ca08b90c0270bcd0e98b9df2b826b0ea6b9742b93631e0f2c43d098a9a8fdd58f1ba44c649d397ca32bd207a9d3fa784611694184904825820086b567b1b34bd97e1a79c46533ed4e771e170848a50983297605f1d7fe6acb8584040fe7d3108c4eaca8484ef9590a52214dae09af501aa84cba4f093c590acdd2c9c15977fc381c0224306567e775d2c7e62a65319fcf504657221e7648411bd0af5f6"
        ).unwrap();
        Arc::from(raw_tx.as_slice())
    }

    async fn setup_indexer(
        mock: MockIndex,
    ) -> (
        Arc<CustomIndexer<InMemoryCursorStore>>,
        mpsc::Sender<IndexCommand>,
    ) {
        let cursor_store = InMemoryCursorStore::new();
        let indexer = Arc::new(CustomIndexer::new(cursor_store));

        indexer.add_index(mock, Point::Origin, false).await.expect("add_index failed");

        let sender = {
            let senders = indexer.senders.lock().await;
            senders.get("mock-index").expect("index not registered").clone()
        };

        (indexer, sender)
    }

    async fn send_apply(
        sender: &mpsc::Sender<IndexCommand>,
        block: BlockInfo,
        txs: Vec<Arc<[u8]>>,
    ) -> IndexResult {
        let (tx, rx) = oneshot::channel();
        sender
            .send(IndexCommand::ApplyTxs {
                block,
                txs,
                response_tx: tx,
            })
            .await
            .expect("actor dropped");
        rx.await.expect("oneshot dropped")
    }

    async fn send_rollback(sender: &mpsc::Sender<IndexCommand>, point: Point) -> IndexResult {
        let (tx, rx) = oneshot::channel();
        sender
            .send(IndexCommand::Rollback {
                point,
                response_tx: tx,
            })
            .await
            .expect("actor dropped");
        rx.await.expect("oneshot dropped")
    }

    #[tokio::test]
    async fn apply_txs_handle_error_sets_halt() {
        let mock = MockIndex {
            on_tx: Some(Box::new(|| Err(anyhow::anyhow!("handle error response")))),
            ..Default::default()
        };

        let (_indexer, sender) = setup_indexer(mock).await;
        let (resp_tx, resp_rx) = oneshot::channel();

        sender
            .send(IndexCommand::ApplyTxs {
                block: test_block(1),
                txs: vec![valid_tx()],
                response_tx: resp_tx,
            })
            .await
            .expect("actor dropped unexpectedly");

        let result = resp_rx.await.expect("oneshot dropped");

        match result {
            IndexResult::HandleError { entry, reason } => {
                assert!(entry.halted);
                assert!(reason.contains("handle error response"));
            }
            other => panic!("Expected HandleError, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn apply_txs_decode_error_sets_halt() {
        let mock = MockIndex {
            on_tx: None,
            ..Default::default()
        };

        let (_indexer, sender) = setup_indexer(mock).await;

        match send_apply(&sender, test_block(1), vec![Arc::from([0u8; 1].as_slice())]).await {
            IndexResult::DecodeError { entry, reason } => {
                assert!(entry.halted);
                assert!(!reason.is_empty());
            }
            other => panic!("Expected DecodeError, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn apply_txs_skips_when_halted() {
        let mock = MockIndex {
            on_tx: Some(Box::new(|| Err(anyhow::anyhow!("handle error response")))),
            ..Default::default()
        };

        let (_indexer, sender) = setup_indexer(mock).await;

        match send_apply(&sender, test_block(1), vec![valid_tx()]).await {
            IndexResult::HandleError { entry, .. } => {
                assert!(entry.halted);
            }
            other => panic!("Expected HandleError on first call, got {:?}", other),
        }

        match send_apply(&sender, test_block(2), vec![valid_tx()]).await {
            IndexResult::Halted => {}
            other => panic!("Expected Halted, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn apply_txs_updates_tip_on_success() {
        let mock = MockIndex {
            on_tx: Some(Box::new(|| Ok(()))),
            ..Default::default()
        };

        let (_indexer, sender) = setup_indexer(mock).await;

        match send_apply(&sender, test_block(50), vec![valid_tx()]).await {
            IndexResult::Success { entry } => {
                assert_eq!(entry.tip.slot(), 50);
                assert!(!entry.halted, "index should not be halted on success");
            }
            other => panic!("Expected Success, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn rollback_updates_tip_and_clears_halt_on_success() {
        let mock = MockIndex {
            on_tx: Some(Box::new(|| Err(anyhow::anyhow!("boom")))),
            on_rollback: Some(Box::new(|| Ok(()))),
            ..Default::default()
        };

        let (_indexer, sender) = setup_indexer(mock).await;

        match send_apply(&sender, test_block(1), vec![valid_tx()]).await {
            IndexResult::HandleError { entry, .. } => assert!(entry.halted),
            other => panic!("Expected HandleError, got {:?}", other),
        }

        let rollback_point = Point::Specific {
            hash: [9u8; 32].into(),
            slot: 12345,
        };

        match send_rollback(&sender, rollback_point.clone()).await {
            IndexResult::Success { entry } => {
                assert_eq!(entry.tip, rollback_point);
                assert!(!entry.halted);
            }
            other => panic!("Expected Success, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn rollback_fails_then_reset_succeeds_clears_halt_and_updates_tip() {
        let mock = MockIndex {
            on_tx: Some(Box::new(|| Err(anyhow::anyhow!("fail tx")))),
            on_rollback: Some(Box::new(|| Err(anyhow::anyhow!("rollback failed")))),
            on_reset: Some(Box::new(|| {
                Ok(Point::Specific {
                    hash: [3u8; 32].into(),
                    slot: 123,
                })
            })),
        };

        let (_indexer, sender) = setup_indexer(mock).await;

        match send_apply(&sender, test_block(1), vec![valid_tx()]).await {
            IndexResult::HandleError { entry, .. } => assert!(entry.halted),
            other => panic!("Expected HandleError, got {:?}", other),
        }

        match send_rollback(
            &sender,
            Point::Specific {
                hash: [7u8; 32].into(),
                slot: 123,
            },
        )
        .await
        {
            IndexResult::Reset { entry } => {
                assert_eq!(entry.tip.slot(), 123);
                assert!(!entry.halted);
            }
            other => panic!("Expected Reset, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn rollback_fails_then_reset_fails_halts() {
        let mock = MockIndex {
            on_tx: Some(Box::new(|| Err(anyhow::anyhow!("tx boom")))),
            on_rollback: Some(Box::new(|| Err(anyhow::anyhow!("rollback boom")))),
            on_reset: Some(Box::new(|| Err(anyhow::anyhow!("reset boom")))),
        };

        let (_indexer, sender) = setup_indexer(mock).await;

        match send_apply(&sender, test_block(1), vec![valid_tx()]).await {
            IndexResult::HandleError { entry, .. } => assert!(entry.halted),
            other => panic!("Expected HandleError, got {:?}", other),
        }

        match send_rollback(
            &sender,
            Point::Specific {
                hash: [9u8; 32].into(),
                slot: 123,
            },
        )
        .await
        {
            IndexResult::FatalResetError { entry, reason } => {
                assert!(entry.halted, "halt must remain true after failed reset");
                assert!(
                    reason.contains("reset boom"),
                    "expected reset failure reason in: {reason}"
                );
            }
            other => panic!("Expected FatalResetError, got {:?}", other),
        }
    }
}
