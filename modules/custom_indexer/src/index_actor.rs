use std::sync::Arc;

use acropolis_common::Point;
use tokio::sync::mpsc;

use pallas::ledger::traverse::MultiEraTx;

use crate::{IndexCommand, IndexResult, IndexWrapper};

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
    if wrapper.halted {
        return IndexResult::Failed {
            reason: "index halted previously".to_string(),
        };
    }

    if block.slot <= wrapper.tip.slot_or_default() {
        return IndexResult::Success {
            tip: wrapper.tip.clone(),
        };
    }

    for raw in txs {
        let decoded = match MultiEraTx::decode(raw.as_ref()) {
            Ok(tx) => tx,
            Err(e) => {
                wrapper.halted = true;
                return IndexResult::Failed {
                    reason: format!("tx decode failed: {e}"),
                };
            }
        };

        if let Err(e) = wrapper.index.handle_onchain_tx(&block, &decoded).await {
            wrapper.halted = true;
            return IndexResult::Failed {
                reason: e.to_string(),
            };
        }
    }

    wrapper.tip = Point::Specific {
        hash: block.hash,
        slot: block.slot,
    };

    IndexResult::Success {
        tip: wrapper.tip.clone(),
    }
}

async fn handle_rollback(wrapper: &mut IndexWrapper, point: Point) -> IndexResult {
    if wrapper.halted {
        return IndexResult::Failed {
            reason: "index halted previously".to_string(),
        };
    }

    match wrapper.index.handle_rollback(&point).await {
        Ok(_) => {
            wrapper.tip = point.clone();
            IndexResult::Success {
                tip: wrapper.tip.clone(),
            }
        }
        Err(e) => {
            wrapper.halted = true;
            IndexResult::Failed {
                reason: e.to_string(),
            }
        }
    }
}
