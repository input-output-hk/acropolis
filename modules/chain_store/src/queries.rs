use std::{collections::HashMap, sync::Arc};

use acropolis_common::{
    queries::{
        blocks::{
            BlockHashAndTxIndex, BlockHashes, BlockKey, BlocksStateQuery, BlocksStateQueryResponse,
            NextBlocks, PreviousBlocks, TransactionHashes, TransactionHashesAndTimeStamps,
        },
        errors::QueryError,
        transactions::{
            TransactionDelegationCertificates, TransactionMIRs, TransactionMetadata,
            TransactionPoolRetirementCertificates, TransactionPoolUpdateCertificates,
            TransactionStakeCertificates, TransactionWithdrawals, TransactionsStateQuery,
            TransactionsStateQueryResponse,
        },
    },
    BlockHash, NetworkId,
};
use anyhow::Result;

use crate::{
    helpers::{
        get_block_by_key, get_block_hash, get_block_number, to_block_info, to_block_info_bulk,
        to_block_involved_addresses, to_block_transaction_hashes, to_block_transactions,
        to_block_transactions_cbor, to_tx_delegations, to_tx_info, to_tx_metadata, to_tx_mirs,
        to_tx_pool_retirements, to_tx_pool_updates, to_tx_stakes, to_tx_withdrawals,
    },
    state::State,
    stores::Store,
};

pub fn handle_blocks_query(
    store: &Arc<dyn Store>,
    state: &State,
    query: &BlocksStateQuery,
) -> Result<BlocksStateQueryResponse> {
    match query {
        BlocksStateQuery::GetLatestBlock => {
            let Some(block) = store.get_latest_block()? else {
                return Ok(BlocksStateQueryResponse::Error(QueryError::not_found(
                    "Latest block not found",
                )));
            };
            let info = to_block_info(block, store, state, true)?;
            Ok(BlocksStateQueryResponse::LatestBlock(info))
        }
        BlocksStateQuery::GetLatestBlockTransactions { limit, skip, order } => {
            let Some(block) = store.get_latest_block()? else {
                return Ok(BlocksStateQueryResponse::Error(QueryError::not_found(
                    "Latest block not found",
                )));
            };
            let txs = to_block_transactions(block, limit, skip, order)?;
            Ok(BlocksStateQueryResponse::LatestBlockTransactions(txs))
        }
        BlocksStateQuery::GetLatestBlockTransactionsCBOR { limit, skip, order } => {
            let Some(block) = store.get_latest_block()? else {
                return Ok(BlocksStateQueryResponse::Error(QueryError::not_found(
                    "Latest block not found",
                )));
            };
            let txs = to_block_transactions_cbor(block, limit, skip, order)?;
            Ok(BlocksStateQueryResponse::LatestBlockTransactionsCBOR(txs))
        }
        BlocksStateQuery::GetBlockInfo { block_key } => {
            let Some(block) = get_block_by_key(store, block_key)? else {
                return Ok(BlocksStateQueryResponse::Error(QueryError::not_found(
                    format!("Block {:?} not found", block_key),
                )));
            };
            let info = to_block_info(block, store, state, false)?;
            Ok(BlocksStateQueryResponse::BlockInfo(info))
        }
        BlocksStateQuery::GetBlockBySlot { slot } => {
            let Some(block) = store.get_block_by_slot(*slot)? else {
                return Ok(BlocksStateQueryResponse::Error(QueryError::not_found(
                    format!("Block at slot {} not found", slot),
                )));
            };
            let info = to_block_info(block, store, state, false)?;
            Ok(BlocksStateQueryResponse::BlockBySlot(info))
        }
        BlocksStateQuery::GetBlockByHash { block_hash } => {
            let Some(block) = store.get_block_by_hash(block_hash.as_ref())? else {
                return Ok(BlocksStateQueryResponse::Error(QueryError::not_found(
                    format!("{} not found", block_hash),
                )));
            };

            let info = to_block_info(block, store, state, false)?;
            Ok(BlocksStateQueryResponse::BlockByHash(info))
        }
        BlocksStateQuery::GetBlockByEpochSlot { epoch, slot } => {
            let Some(block) = store.get_block_by_epoch_slot(*epoch, *slot)? else {
                return Ok(BlocksStateQueryResponse::Error(QueryError::not_found(
                    format!("Block at epoch {} slot {} not found", epoch, slot),
                )));
            };
            let info = to_block_info(block, store, state, false)?;
            Ok(BlocksStateQueryResponse::BlockByEpochSlot(info))
        }
        BlocksStateQuery::GetNextBlocks {
            block_key,
            limit,
            skip,
        } => {
            if *limit == 0 {
                return Ok(BlocksStateQueryResponse::NextBlocks(NextBlocks {
                    blocks: vec![],
                }));
            }
            let Some(block) = get_block_by_key(store, block_key)? else {
                return Ok(BlocksStateQueryResponse::Error(QueryError::not_found(
                    format!("Block {:?} not found", block_key),
                )));
            };
            let number = match block_key {
                BlockKey::Number(number) => *number,
                _ => get_block_number(&block)?,
            };
            let min_number = number + 1 + skip;
            let max_number = min_number + limit - 1;
            let blocks = store.get_blocks_by_number_range(min_number, max_number)?;
            let info = to_block_info_bulk(blocks, store, state, false)?;
            Ok(BlocksStateQueryResponse::NextBlocks(NextBlocks {
                blocks: info,
            }))
        }
        BlocksStateQuery::GetPreviousBlocks {
            block_key,
            limit,
            skip,
        } => {
            if *limit == 0 {
                return Ok(BlocksStateQueryResponse::PreviousBlocks(PreviousBlocks {
                    blocks: vec![],
                }));
            }
            let Some(block) = get_block_by_key(store, block_key)? else {
                return Ok(BlocksStateQueryResponse::Error(QueryError::not_found(
                    format!("Block {:?} not found", block_key),
                )));
            };
            let number = match block_key {
                BlockKey::Number(number) => *number,
                _ => get_block_number(&block)?,
            };
            let Some(max_number) = number.checked_sub(1 + skip) else {
                return Ok(BlocksStateQueryResponse::PreviousBlocks(PreviousBlocks {
                    blocks: vec![],
                }));
            };
            let min_number = max_number.saturating_sub(limit - 1);
            let blocks = store.get_blocks_by_number_range(min_number, max_number)?;
            let info = to_block_info_bulk(blocks, store, state, false)?;
            Ok(BlocksStateQueryResponse::PreviousBlocks(PreviousBlocks {
                blocks: info,
            }))
        }
        BlocksStateQuery::GetBlockTransactions {
            block_key,
            limit,
            skip,
            order,
        } => {
            let Some(block) = get_block_by_key(store, block_key)? else {
                return Ok(BlocksStateQueryResponse::Error(QueryError::not_found(
                    format!("Block {:?} not found", block_key),
                )));
            };
            let txs = to_block_transactions(block, limit, skip, order)?;
            Ok(BlocksStateQueryResponse::BlockTransactions(txs))
        }
        BlocksStateQuery::GetBlockTransactionsCBOR {
            block_key,
            limit,
            skip,
            order,
        } => {
            let Some(block) = get_block_by_key(store, block_key)? else {
                return Ok(BlocksStateQueryResponse::Error(QueryError::not_found(
                    format!("Block {:?} not found", block_key),
                )));
            };
            let txs = to_block_transactions_cbor(block, limit, skip, order)?;
            Ok(BlocksStateQueryResponse::BlockTransactionsCBOR(txs))
        }
        BlocksStateQuery::GetBlockInvolvedAddresses {
            block_key,
            limit,
            skip,
        } => {
            let Some(block) = get_block_by_key(store, block_key)? else {
                return Ok(BlocksStateQueryResponse::Error(QueryError::not_found(
                    format!("Block {:?} not found", block_key),
                )));
            };
            let addresses = to_block_involved_addresses(block, limit, skip)?;
            Ok(BlocksStateQueryResponse::BlockInvolvedAddresses(addresses))
        }
        BlocksStateQuery::GetBlockHashes { block_numbers } => {
            let mut block_hashes = HashMap::new();
            for block_number in block_numbers {
                if let Ok(Some(block)) = store.get_block_by_number(*block_number) {
                    if let Ok(hash) = get_block_hash(&block) {
                        block_hashes.insert(*block_number, hash);
                    }
                }
            }
            Ok(BlocksStateQueryResponse::BlockHashes(BlockHashes {
                block_hashes,
            }))
        }
        BlocksStateQuery::GetBlockHashesByNumberRange {
            min_number,
            max_number,
        } => {
            if *max_number < *min_number {
                return Ok(BlocksStateQueryResponse::Error(
                    QueryError::invalid_request("Invalid number range"),
                ));
            }
            let mut block_hashes = Vec::new();
            let blocks = store.get_blocks_by_number_range(*min_number, *max_number)?;
            for block in blocks {
                if let Ok(hash) = get_block_hash(&block) {
                    block_hashes.push(hash);
                }
            }
            Ok(BlocksStateQueryResponse::BlockHashesByNumberRange(
                block_hashes,
            ))
        }
        BlocksStateQuery::GetTransactionHashes { tx_ids } => {
            let mut block_ids: HashMap<_, Vec<_>> = HashMap::new();
            for tx_id in tx_ids {
                block_ids.entry(tx_id.block_number()).or_default().push(tx_id);
            }
            let mut tx_hashes = HashMap::new();
            for (block_number, tx_ids) in block_ids {
                if let Ok(Some(block)) = store.get_block_by_number(block_number.into()) {
                    for tx_id in tx_ids {
                        if let Ok(hashes) = to_block_transaction_hashes(&block) {
                            if let Some(hash) = hashes.get(tx_id.tx_index() as usize) {
                                tx_hashes.insert(*tx_id, *hash);
                            }
                        }
                    }
                }
            }
            Ok(BlocksStateQueryResponse::TransactionHashes(
                TransactionHashes { tx_hashes },
            ))
        }
        BlocksStateQuery::GetBlockHashesAndIndexOfTransactionHashes { tx_hashes } => {
            let mut block_hashes_and_indexes = Vec::with_capacity(tx_hashes.len());

            for tx_hash in tx_hashes {
                match store.get_tx_block_ref_by_hash(tx_hash.as_inner()) {
                    Ok(Some(tx_block_ref)) => {
                        let Ok(block_hash) =
                            BlockHash::try_from(tx_block_ref.block_hash.as_slice())
                        else {
                            return Ok(BlocksStateQueryResponse::Error(
                                QueryError::internal_error(
                                    "Failed to instantiate BlockHash from record".to_string(),
                                ),
                            ));
                        };
                        block_hashes_and_indexes.push(BlockHashAndTxIndex {
                            block_hash,
                            tx_index: tx_block_ref.index as u16,
                        })
                    }
                    Ok(None) => {
                        return Ok(BlocksStateQueryResponse::Error(QueryError::not_found(
                            format!("TxHash {} not found", tx_hash),
                        )))
                    }
                    Err(e) => {
                        return Ok(BlocksStateQueryResponse::Error(QueryError::not_found(
                            format!("Failed to lookup tx hash {}: {e}", tx_hash),
                        )))
                    }
                }
            }

            Ok(
                BlocksStateQueryResponse::BlockHashesAndIndexOfTransactionHashes(
                    block_hashes_and_indexes,
                ),
            )
        }
        BlocksStateQuery::GetTransactionHashesAndTimestamps { tx_ids } => {
            let mut tx_hashes = Vec::with_capacity(tx_ids.len());
            let mut timestamps = Vec::with_capacity(tx_ids.len());

            for tx in tx_ids {
                let block = match store.get_block_by_number(tx.block_number().into()) {
                    Ok(Some(b)) => b,
                    Ok(None) => {
                        return Ok(BlocksStateQueryResponse::Error(QueryError::not_found(
                            format!("Block {} not found", tx.block_number()),
                        )))
                    }
                    Err(e) => {
                        return Ok(BlocksStateQueryResponse::Error(QueryError::not_found(
                            format!("Failed to fetch block {}: {e}", tx.block_number()),
                        )))
                    }
                };

                let hashes_in_block = match to_block_transaction_hashes(&block) {
                    Ok(h) => h,
                    Err(e) => {
                        return Ok(BlocksStateQueryResponse::Error(QueryError::not_found(
                            format!(
                                "Failed to extract tx hashes for block {}: {e}",
                                tx.block_number()
                            ),
                        )))
                    }
                };

                let tx_hash = match hashes_in_block.get(tx.tx_index() as usize) {
                    Some(h) => h,
                    None => {
                        return Ok(BlocksStateQueryResponse::Error(QueryError::not_found(
                            format!(
                                "tx_index {} out of bounds for block {}",
                                tx.tx_index(),
                                tx.block_number()
                            ),
                        )))
                    }
                };

                let block_info = match to_block_info(block, store, state, false) {
                    Ok(info) => info,
                    Err(e) => {
                        return Ok(BlocksStateQueryResponse::Error(QueryError::not_found(
                            format!(
                                "Failed to build block info for block {}: {e}",
                                tx.block_number()
                            ),
                        )))
                    }
                };

                tx_hashes.push(*tx_hash);
                timestamps.push(block_info.timestamp);
            }

            Ok(BlocksStateQueryResponse::TransactionHashesAndTimestamps(
                TransactionHashesAndTimeStamps {
                    tx_hashes,
                    timestamps,
                },
            ))
        }
        BlocksStateQuery::GetBlockByTipOffset { offset } => {
            let tip = store.get_tip_block_number();

            let stable_block_number = tip.saturating_sub(*offset as u64);

            let block_opt = match store.get_block_by_number(stable_block_number) {
                Ok(b) => b,
                Err(e) => return Ok(BlocksStateQueryResponse::Error(e.into())),
            };

            let block_info_opt =
                match block_opt.map(|b| to_block_info(b, store, state, false)).transpose() {
                    Ok(v) => v,
                    Err(e) => return Ok(BlocksStateQueryResponse::Error(e.into())),
                };

            Ok(BlocksStateQueryResponse::BlockByTipOffset(block_info_opt))
        }
        BlocksStateQuery::GetStableBlockByHash { block_hash, offset } => {
            let tip = store.get_tip_block_number();

            let stable_boundary = tip.saturating_sub(*offset as u64);

            let block_opt = match store.get_block_by_hash(block_hash.as_slice()) {
                Ok(b) => b,
                Err(e) => return Ok(BlocksStateQueryResponse::Error(e.into())),
            };

            let block_info_opt =
                match block_opt.map(|b| to_block_info(b, store, state, false)).transpose() {
                    Ok(v) => v,
                    Err(e) => return Ok(BlocksStateQueryResponse::Error(e.into())),
                };

            let stable_block = block_info_opt.filter(|b| b.number <= stable_boundary);

            Ok(BlocksStateQueryResponse::StableBlockByHash(stable_block))
        }
    }
}

pub fn handle_txs_query(
    store: &Arc<dyn Store>,
    query: &TransactionsStateQuery,
    network_id: NetworkId,
) -> Result<TransactionsStateQueryResponse> {
    match query {
        TransactionsStateQuery::GetTransactionInfo { tx_hash } => {
            let Some(tx) = store.get_tx_by_hash(tx_hash.as_ref())? else {
                return Ok(TransactionsStateQueryResponse::Error(
                    QueryError::not_found("Transaction not found"),
                ));
            };
            Ok(TransactionsStateQueryResponse::TransactionInfo(to_tx_info(
                &tx,
            )?))
        }
        TransactionsStateQuery::GetTransactionStakeCertificates { tx_hash } => {
            let Some(tx) = store.get_tx_by_hash(tx_hash.as_ref())? else {
                return Ok(TransactionsStateQueryResponse::Error(
                    QueryError::not_found("Transaction not found"),
                ));
            };
            Ok(
                TransactionsStateQueryResponse::TransactionStakeCertificates(
                    TransactionStakeCertificates {
                        certificates: to_tx_stakes(&tx, network_id)?,
                    },
                ),
            )
        }
        TransactionsStateQuery::GetTransactionDelegationCertificates { tx_hash } => {
            let Some(tx) = store.get_tx_by_hash(tx_hash.as_ref())? else {
                return Ok(TransactionsStateQueryResponse::Error(
                    QueryError::not_found("Transaction not found"),
                ));
            };
            Ok(
                TransactionsStateQueryResponse::TransactionDelegationCertificates(
                    TransactionDelegationCertificates {
                        certificates: to_tx_delegations(&tx, network_id)?,
                    },
                ),
            )
        }
        TransactionsStateQuery::GetTransactionWithdrawals { tx_hash } => {
            let Some(tx) = store.get_tx_by_hash(tx_hash.as_ref())? else {
                return Ok(TransactionsStateQueryResponse::Error(
                    QueryError::not_found("Transaction not found"),
                ));
            };
            Ok(TransactionsStateQueryResponse::TransactionWithdrawals(
                TransactionWithdrawals {
                    withdrawals: to_tx_withdrawals(&tx)?,
                },
            ))
        }
        TransactionsStateQuery::GetTransactionMIRs { tx_hash } => {
            let Some(tx) = store.get_tx_by_hash(tx_hash.as_ref())? else {
                return Ok(TransactionsStateQueryResponse::Error(
                    QueryError::not_found("Transaction not found"),
                ));
            };
            Ok(TransactionsStateQueryResponse::TransactionMIRs(
                TransactionMIRs {
                    mirs: to_tx_mirs(&tx, network_id)?,
                },
            ))
        }
        TransactionsStateQuery::GetTransactionPoolUpdateCertificates { tx_hash } => {
            let Some(tx) = store.get_tx_by_hash(tx_hash.as_ref())? else {
                return Ok(TransactionsStateQueryResponse::Error(
                    QueryError::not_found("Transaction not found"),
                ));
            };
            Ok(
                TransactionsStateQueryResponse::TransactionPoolUpdateCertificates(
                    TransactionPoolUpdateCertificates {
                        pool_updates: to_tx_pool_updates(&tx, network_id)?,
                    },
                ),
            )
        }
        TransactionsStateQuery::GetTransactionPoolRetirementCertificates { tx_hash } => {
            let Some(tx) = store.get_tx_by_hash(tx_hash.as_ref())? else {
                return Ok(TransactionsStateQueryResponse::Error(
                    QueryError::not_found("Transaction not found"),
                ));
            };
            Ok(
                TransactionsStateQueryResponse::TransactionPoolRetirementCertificates(
                    TransactionPoolRetirementCertificates {
                        pool_retirements: to_tx_pool_retirements(&tx)?,
                    },
                ),
            )
        }
        TransactionsStateQuery::GetTransactionMetadata { tx_hash } => {
            let Some(tx) = store.get_tx_by_hash(tx_hash.as_ref())? else {
                return Ok(TransactionsStateQueryResponse::Error(
                    QueryError::not_found("Transaction not found"),
                ));
            };
            Ok(TransactionsStateQueryResponse::TransactionMetadata(
                TransactionMetadata {
                    metadata: to_tx_metadata(&tx)?,
                },
            ))
        }
        _ => Ok(TransactionsStateQueryResponse::Error(
            QueryError::not_implemented("Unimplemented".to_string()),
        )),
    }
}
