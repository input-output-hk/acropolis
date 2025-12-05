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

pub enum IndexResult {
    Success { entry: CursorEntry },
    DecodeError { reason: String },
    HandleError { reason: String },
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
                    reason: e.to_string(),
                };
            }
        };
        if let Err(e) = wrapper.index.handle_onchain_tx(&block, &decoded).await {
            wrapper.halted = true;
            return IndexResult::HandleError {
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
