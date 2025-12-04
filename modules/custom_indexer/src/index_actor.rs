use acropolis_common::Point;
use tokio::sync::mpsc;
use tracing::error;

use crate::{IndexCommand, IndexResult, IndexWrapper};

pub async fn index_actor(mut wrapper: IndexWrapper, mut rx: mpsc::Receiver<IndexCommand>) {
    while let Some(cmd) = rx.recv().await {
        match cmd {
            IndexCommand::ApplyTxs {
                block,
                txs,
                response_tx,
            } => {
                let result = if wrapper.halted {
                    IndexResult::Failed {
                        reason: "index halted previously".to_string(),
                    }
                } else if block.slot <= wrapper.tip.slot_or_default() {
                    IndexResult::Success {
                        tip: wrapper.tip.clone(),
                    }
                } else {
                    let mut failed_reason = None;

                    for tx in &txs {
                        if let Err(e) = wrapper.index.handle_onchain_tx(&block, &tx).await {
                            error!(
                                "Index '{}' failed on block {}: {e:#}",
                                wrapper.name, block.number
                            );
                            wrapper.halted = true;
                            failed_reason = Some(e.to_string());
                            break;
                        }
                    }

                    if let Some(reason) = failed_reason {
                        IndexResult::Failed { reason }
                    } else {
                        wrapper.tip = Point::Specific {
                            hash: block.hash,
                            slot: block.slot,
                        };

                        IndexResult::Success {
                            tip: wrapper.tip.clone(),
                        }
                    }
                };

                let _ = response_tx.send(result);
            }

            IndexCommand::Rollback { point, response_tx } => {
                let result = if wrapper.halted {
                    IndexResult::Failed {
                        reason: "index halted previously".to_string(),
                    }
                } else {
                    match wrapper.index.handle_rollback(&point).await {
                        Ok(_) => {
                            wrapper.tip = point.clone();
                            IndexResult::Success {
                                tip: wrapper.tip.clone(),
                            }
                        }
                        Err(e) => {
                            error!("Rollback error in index '{}': {e:#}", wrapper.name);
                            wrapper.halted = true;
                            IndexResult::Failed {
                                reason: e.to_string(),
                            }
                        }
                    }
                };

                let _ = response_tx.send(result);
            }
        }
    }
}
