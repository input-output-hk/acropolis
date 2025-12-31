use std::{collections::VecDeque, sync::Arc};

use acropolis_common::{BlockInfo, Point};
use anyhow::{Context, Result};
use tokio::sync::{mpsc, oneshot};
use tracing::warn;

use crate::{chain_index::ChainIndex, cursor_store::CursorEntry};

enum IndexCommand {
    ApplyTx {
        block: Arc<BlockInfo>,
        tx: Arc<[u8]>,
        response_tx: oneshot::Sender<Result<()>>,
    },
    Rollback {
        point: Point,
        response_tx: oneshot::Sender<Result<()>>,
    },
}

pub struct IndexActor {
    pub name: String,
    tx: mpsc::Sender<IndexCommand>,
    points: VecDeque<Point>,
    next_tx: Option<u64>,
    halted: bool,
    security_param: u64,
}

impl IndexActor {
    pub fn new(
        name: String,
        index: Box<dyn ChainIndex>,
        cursor: &CursorEntry,
        security_param: u64,
    ) -> Self {
        let (tx, rx) = mpsc::channel(128);
        tokio::spawn(index_actor(index, rx));
        Self {
            name,
            tx,
            points: cursor.points.clone(),
            next_tx: cursor.next_tx,
            halted: false,
            security_param,
        }
    }

    pub fn update_cursor(&self, cursor: &mut CursorEntry) {
        cursor.next_tx = self.next_tx;
        let (Some(first), Some(last)) = (self.points.front(), self.points.back()) else {
            cursor.points.clear();
            return;
        };
        while cursor.points.front().is_some_and(|p| p.slot() < first.slot()) {
            // we pruned our history, do that to the cursor as well
            cursor.points.pop_front();
        }
        while cursor.points.len() > self.points.len() {
            // we rolled back, roll back the cursor as well
            cursor.points.pop_back();
        }
        if cursor.points.len() == self.points.len()
            && cursor.points.back().is_some_and(|l| l != last)
        {
            // after rolling back, we must have rolled forward
            cursor.points.pop_back();
        }
        if cursor.points.len() < self.points.len() {
            // we only roll forward one block at a time,
            // so the cursor can only be missing the most recent block.
            cursor.points.push_back(last.clone());
        }
    }

    pub async fn apply_txs(&mut self, block: Arc<BlockInfo>, txs: &[Arc<[u8]>]) {
        if self.points.front().is_none_or(|p| p.slot() > block.slot) {
            // this block is from before our recent history
            return;
        }
        let tip = self.points.back().unwrap();
        let tip_slot = tip.slot();
        if tip_slot >= block.slot {
            // This block is new enough to be in our history, but isn't new enough to be a new tip.
            // Check if we need to roll back.
            let rollback_before_idx =
                match self.points.binary_search_by_key(&block.slot, |p| p.slot()) {
                    Ok(pos) => {
                        let point = self.points.get(pos).unwrap();
                        if point.hash() == Some(&block.hash) {
                            // This point was in our history, no need to roll back
                            None
                        } else {
                            // We saw a different block in this slot.
                            // Roll back to before that block.
                            Some(pos)
                        }
                    }
                    Err(pos) => {
                        // We never saw a block in this slot.
                        // Roll back to before whichever block came after this slot.
                        Some(pos)
                    }
                };
            if let Some(idx) = rollback_before_idx {
                // We need to roll back
                let Some(rollback_to_idx) = idx.checked_sub(1) else {
                    self.halted = true;
                    warn!(index = self.name, "rolled back farther than known history");
                    return;
                };
                let point = self.points.get(rollback_to_idx).unwrap().clone();
                self.rollback(point).await;
            }
            if tip_slot < block.slot || self.next_tx.is_none() {
                // Either this block is from before our tip,
                // or this block is at our tip but we have already applied all of its TXs.
                // Either way, we're done here.
                return;
            }
        }

        if self.halted {
            return;
        }

        if tip_slot < block.slot {
            self.points.push_back(Point::Specific {
                hash: block.hash,
                slot: block.slot,
            });
            while self.points.len() > self.security_param as usize {
                self.points.pop_front();
            }
        }

        for (idx, tx) in txs.iter().enumerate() {
            if self.next_tx.is_some_and(|i| i as usize > idx) {
                continue;
            }

            if let Err(error) = self.call_apply_tx(block.clone(), tx.clone()).await {
                self.next_tx = Some(idx as u64);
                self.halted = true;
                warn!(index = self.name, "error applying tx: {error:#}");
                return;
            }
        }
        self.next_tx = None;
    }

    pub async fn rollback(&mut self, point: Point) {
        let mut new_points = self.points.clone();
        let mut new_tx_index = self.next_tx;
        let mut new_halted = self.halted;
        while new_points.back().is_some_and(|p| p.slot() > point.slot()) {
            new_points.pop_back();
            new_tx_index = None;
            new_halted = false;
        }
        if new_points.back().is_none_or(|p| p != &point) {
            self.halted = true;
            warn!(index = self.name, "rolled back farther than known history");
            return;
        }
        match self.call_rollback(point).await {
            Ok(()) => {
                self.points = new_points;
                self.next_tx = new_tx_index;
                self.halted = new_halted;
            }
            Err(e) => {
                self.halted = true;
                warn!(index = self.name, "error when rolling back: {e:#}");
            }
        }
    }

    async fn call_apply_tx(&self, block: Arc<BlockInfo>, tx: Arc<[u8]>) -> Result<()> {
        let (response_tx, response_rx) = oneshot::channel();
        let cmd = IndexCommand::ApplyTx {
            block,
            tx,
            response_tx,
        };
        self.tx.send(cmd).await.context("channel closed")?;
        response_rx.await.context("sender closed")?
    }

    async fn call_rollback(&self, point: Point) -> Result<()> {
        let (response_tx, response_rx) = oneshot::channel();
        let cmd = IndexCommand::Rollback { point, response_tx };
        self.tx.send(cmd).await.context("channel closed")?;
        response_rx.await.context("sender closed")?
    }
}

async fn index_actor(mut index: Box<dyn ChainIndex>, mut rx: mpsc::Receiver<IndexCommand>) {
    while let Some(cmd) = rx.recv().await {
        match cmd {
            IndexCommand::ApplyTx {
                block,
                tx,
                response_tx,
            } => {
                let res = index.handle_onchain_tx_bytes(&block, &tx).await;
                let _ = response_tx.send(res);
            }
            IndexCommand::Rollback { point, response_tx } => {
                let res = index.handle_rollback(&point).await;
                let _ = response_tx.send(res);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::VecDeque, sync::Arc};

    use acropolis_common::{
        params::SECURITY_PARAMETER_K, BlockInfo, BlockIntent, BlockStatus, Era, Point,
    };
    use caryatid_sdk::async_trait;
    use pallas::ledger::traverse::MultiEraTx;

    use crate::{chain_index::ChainIndex, cursor_store::CursorEntry, index_actor::IndexActor};

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
            hash: [slot as u8; 32].into(),
            epoch: 0,
            epoch_slot: 0,
            new_epoch: false,
            tip_slot: None,
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

    fn new_cursor(slot: u64) -> CursorEntry {
        let mut points = VecDeque::new();
        let hash = [slot as u8; 32].into();
        points.push_back(Point::Specific { hash, slot });
        CursorEntry {
            points,
            next_tx: None,
        }
    }

    #[tokio::test]
    async fn apply_txs_handle_error_sets_halt() {
        let mock = MockIndex {
            on_tx: Some(Box::new(|| Err(anyhow::anyhow!("handle error response")))),
            ..Default::default()
        };

        let block = Arc::new(test_block(1));
        let txs = vec![valid_tx()];
        let mut cursor = new_cursor(0);

        let mut actor = IndexActor::new(mock.name(), Box::new(mock), &cursor, SECURITY_PARAMETER_K);
        actor.apply_txs(block.clone(), &txs).await;
        actor.update_cursor(&mut cursor);

        assert!(actor.halted);
        assert_eq!(
            cursor.points.back(),
            Some(&Point::Specific {
                hash: block.hash,
                slot: block.slot
            })
        );
        assert_eq!(cursor.next_tx, Some(0));
    }

    #[tokio::test]
    async fn apply_txs_skips_when_halted() {
        let mock = MockIndex {
            on_tx: Some(Box::new(|| Err(anyhow::anyhow!("handle error response")))),
            ..Default::default()
        };

        let b1 = Arc::new(test_block(1));
        let txs = vec![valid_tx()];
        let mut cursor = new_cursor(0);

        let mut actor = IndexActor::new(mock.name(), Box::new(mock), &cursor, SECURITY_PARAMETER_K);
        actor.apply_txs(b1.clone(), &txs).await;
        actor.update_cursor(&mut cursor);

        assert!(actor.halted);
        assert_eq!(
            cursor.points.back(),
            Some(&Point::Specific {
                hash: b1.hash,
                slot: b1.slot
            })
        );
        assert_eq!(cursor.next_tx, Some(0));

        let b2 = Arc::new(test_block(2));
        actor.apply_txs(b2.clone(), &txs).await;
        actor.update_cursor(&mut cursor);

        assert!(actor.halted);
        assert_eq!(
            cursor.points.back(),
            Some(&Point::Specific {
                hash: b1.hash,
                slot: b1.slot
            })
        );
        assert_eq!(cursor.next_tx, Some(0));
    }

    #[tokio::test]
    async fn apply_txs_updates_tip_on_success() {
        let mock = MockIndex {
            on_tx: Some(Box::new(|| Ok(()))),
            ..Default::default()
        };

        let b1 = Arc::new(test_block(1));
        let txs = vec![valid_tx()];
        let mut cursor = new_cursor(0);

        let mut actor = IndexActor::new(mock.name(), Box::new(mock), &cursor, SECURITY_PARAMETER_K);
        actor.apply_txs(b1.clone(), &txs).await;
        actor.update_cursor(&mut cursor);

        assert!(!actor.halted);
        assert_eq!(
            cursor.points.back(),
            Some(&Point::Specific {
                hash: b1.hash,
                slot: b1.slot
            })
        );
        assert_eq!(cursor.next_tx, None);
    }

    #[tokio::test]
    async fn rollback_updates_tip_and_clears_halt_on_success() {
        let mock = MockIndex {
            on_tx: Some(Box::new(|| Err(anyhow::anyhow!("boom")))),
            on_rollback: Some(Box::new(|| Ok(()))),
            ..Default::default()
        };

        let b0 = test_block(123);
        let b1 = Arc::new(test_block(200));
        let txs = vec![valid_tx()];
        let mut cursor = new_cursor(123);

        let mut actor = IndexActor::new(mock.name(), Box::new(mock), &cursor, SECURITY_PARAMETER_K);
        actor.apply_txs(b1.clone(), &txs).await;
        actor.update_cursor(&mut cursor);

        assert!(actor.halted);
        assert_eq!(
            cursor.points.back(),
            Some(&Point::Specific {
                hash: b1.hash,
                slot: b1.slot
            })
        );
        assert_eq!(cursor.next_tx, Some(0));

        let rollback_point = Point::Specific {
            hash: [123u8; 32].into(),
            slot: 123,
        };
        actor.rollback(rollback_point).await;
        actor.update_cursor(&mut cursor);
        assert!(!actor.halted);
        assert_eq!(
            cursor.points.back(),
            Some(&Point::Specific {
                hash: b0.hash,
                slot: b0.slot
            })
        );
        assert_eq!(cursor.next_tx, None);
    }
}
