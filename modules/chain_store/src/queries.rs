use std::{collections::HashMap, sync::Arc};

use acropolis_common::{
    queries::{
        blocks::{
            BlockHashAndTxIndex, BlockHashes, BlockInfo, BlockKey, BlocksStateQuery,
            BlocksStateQueryResponse, NextBlocks, PreviousBlocks, TransactionHashes,
            TransactionHashesAndTimeStamps,
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
    stores::{Block, Store},
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
                        )));
                    }
                    Err(e) => {
                        return Ok(BlocksStateQueryResponse::Error(QueryError::not_found(
                            format!("Failed to lookup tx hash {}: {e}", tx_hash),
                        )));
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
                        )));
                    }
                    Err(e) => {
                        return Ok(BlocksStateQueryResponse::Error(QueryError::not_found(
                            format!("Failed to fetch block {}: {e}", tx.block_number()),
                        )));
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
                        )));
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
                        )));
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
                        )));
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
        BlocksStateQuery::GetLatestStableBlockAsOf {
            stability_offset,
            min_block_timestamp_unix_millis,
            max_block_timestamp_unix_millis,
        } => Ok(BlocksStateQueryResponse::LatestStableBlockAsOf(
            get_latest_stable_block_as_of(
                store,
                state,
                *stability_offset,
                *min_block_timestamp_unix_millis,
                *max_block_timestamp_unix_millis,
            )?,
        )),
        BlocksStateQuery::GetStableBlockByHashAsOf {
            block_hash,
            stability_offset,
            min_block_timestamp_unix_millis,
            max_block_timestamp_unix_millis,
        } => Ok(BlocksStateQueryResponse::StableBlockByHashAsOf(
            get_stable_block_by_hash_as_of(
                store,
                state,
                *block_hash,
                *stability_offset,
                *min_block_timestamp_unix_millis,
                *max_block_timestamp_unix_millis,
            )?,
        )),
    }
}

fn get_latest_stable_block_as_of(
    store: &Arc<dyn Store>,
    state: &State,
    stability_offset: u32,
    min_block_timestamp_unix_millis: u64,
    max_block_timestamp_unix_millis: u64,
) -> Result<Option<BlockInfo>> {
    let tip = store.get_tip_block_number();
    let stable_boundary_number = tip.saturating_sub(stability_offset as u64);
    let Some(stable_boundary_block) = store.get_block_by_number(stable_boundary_number)? else {
        return Ok(None);
    };

    if block_is_within_timestamp_window(
        &stable_boundary_block,
        min_block_timestamp_unix_millis,
        max_block_timestamp_unix_millis,
    ) {
        return Ok(Some(to_block_info(
            stable_boundary_block,
            store,
            state,
            false,
        )?));
    }

    if block_is_older_than_timestamp_window(&stable_boundary_block, min_block_timestamp_unix_millis)
    {
        return Ok(None);
    }

    let Some(earliest_block_number) = store.get_earliest_block_number()? else {
        return Ok(None);
    };

    let Some(candidate) = find_latest_block_at_or_before_timestamp(
        store,
        earliest_block_number,
        stable_boundary_number.saturating_sub(1),
        max_block_timestamp_unix_millis,
    )?
    else {
        return Ok(None);
    };

    if block_is_older_than_timestamp_window(&candidate, min_block_timestamp_unix_millis) {
        return Ok(None);
    }

    Ok(Some(to_block_info(candidate, store, state, false)?))
}

fn get_stable_block_by_hash_as_of(
    store: &Arc<dyn Store>,
    state: &State,
    block_hash: BlockHash,
    stability_offset: u32,
    min_block_timestamp_unix_millis: u64,
    max_block_timestamp_unix_millis: u64,
) -> Result<Option<BlockInfo>> {
    let tip = store.get_tip_block_number();
    let stable_boundary_number = tip.saturating_sub(stability_offset as u64);
    let Some(block) = store.get_block_by_hash(block_hash.as_slice())? else {
        return Ok(None);
    };

    if !block_is_within_timestamp_window(
        &block,
        min_block_timestamp_unix_millis,
        max_block_timestamp_unix_millis,
    ) {
        return Ok(None);
    }

    let block_info = to_block_info(block, store, state, false)?;
    Ok((block_info.number <= stable_boundary_number).then_some(block_info))
}

fn find_latest_block_at_or_before_timestamp(
    store: &Arc<dyn Store>,
    lower_block_number: u64,
    upper_block_number: u64,
    max_block_timestamp_unix_millis: u64,
) -> Result<Option<Block>> {
    if lower_block_number > upper_block_number {
        return Ok(None);
    }

    let mut low = lower_block_number;
    let mut high = upper_block_number;
    let mut best = None;

    while low <= high {
        let mid = low + (high - low) / 2;
        let Some(block) = store.get_block_by_number(mid)? else {
            anyhow::bail!("Block {mid} not found during stable block search");
        };

        if block_timestamp_unix_millis(&block) <= max_block_timestamp_unix_millis {
            best = Some(block);
            low = mid.saturating_add(1);
        } else if mid == 0 {
            break;
        } else {
            high = mid - 1;
        }
    }

    Ok(best)
}

fn block_timestamp_unix_millis(block: &Block) -> u64 {
    block.extra.timestamp.saturating_mul(1000)
}

fn block_is_within_timestamp_window(
    block: &Block,
    min_block_timestamp_unix_millis: u64,
    max_block_timestamp_unix_millis: u64,
) -> bool {
    let timestamp = block_timestamp_unix_millis(block);
    min_block_timestamp_unix_millis <= timestamp && timestamp <= max_block_timestamp_unix_millis
}

fn block_is_older_than_timestamp_window(
    block: &Block,
    min_block_timestamp_unix_millis: u64,
) -> bool {
    block_timestamp_unix_millis(block) < min_block_timestamp_unix_millis
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

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, HashMap};

    use super::*;
    use crate::stores::{fjall::FjallStore, Block, ExtraBlockData, Store, Tx, TxBlockReference};
    use anyhow::{anyhow, Result};
    use config::Config;
    use tempfile::TempDir;

    fn init_store_with_blocks(
        count: usize,
    ) -> (TempDir, Arc<dyn Store>, Vec<acropolis_common::BlockInfo>) {
        let dir = tempfile::tempdir().unwrap();
        let config = Config::builder()
            .set_default("database-path", dir.path().to_str().unwrap())
            .unwrap()
            .build()
            .unwrap();
        let store = Arc::new(FjallStore::new(Arc::new(config)).unwrap()) as Arc<dyn Store>;

        let blocks = crate::stores::fjall::tests::test_block_range_bytes(count);
        let infos = blocks
            .iter()
            .map(|bytes| crate::stores::fjall::tests::test_block_info(bytes))
            .collect::<Vec<_>>();

        for (info, bytes) in infos.iter().zip(blocks.iter()) {
            store.insert_block(info, bytes).unwrap();
        }

        (dir, store, infos)
    }

    fn assert_block_matches_info(block: &BlockInfo, info: &acropolis_common::BlockInfo) {
        assert_eq!(block.timestamp, info.timestamp);
        assert_eq!(block.number, info.number);
        assert_eq!(block.hash, info.hash);
        assert_eq!(block.slot, info.slot);
        assert_eq!(block.epoch, info.epoch);
        assert_eq!(block.epoch_slot, info.epoch_slot);
    }

    fn build_store_block(
        info: &acropolis_common::BlockInfo,
        bytes: &[u8],
        timestamp: u64,
    ) -> Block {
        Block {
            bytes: bytes.to_vec(),
            extra: ExtraBlockData {
                epoch: info.epoch,
                epoch_slot: info.epoch_slot,
                timestamp,
            },
        }
    }

    fn copy_block(block: &Block) -> Block {
        Block {
            bytes: block.bytes.clone(),
            extra: ExtraBlockData {
                epoch: block.extra.epoch,
                epoch_slot: block.extra.epoch_slot,
                timestamp: block.extra.timestamp,
            },
        }
    }

    struct MemoryStore {
        blocks_by_number: BTreeMap<u64, Block>,
        blocks_by_hash: HashMap<Vec<u8>, Block>,
    }

    impl MemoryStore {
        fn new(indexed_blocks: impl IntoIterator<Item = (u64, Block)>) -> Self {
            let mut blocks_by_number = BTreeMap::new();
            let mut blocks_by_hash = HashMap::new();

            for (number, block) in indexed_blocks {
                let hash = BlockHash::from(
                    *pallas_traverse::MultiEraBlock::decode(&block.bytes).unwrap().hash(),
                );
                blocks_by_hash.insert(hash.to_vec(), copy_block(&block));
                blocks_by_number.insert(number, block);
            }

            Self {
                blocks_by_number,
                blocks_by_hash,
            }
        }
    }

    impl Store for MemoryStore {
        fn insert_block(&self, _info: &acropolis_common::BlockInfo, _block: &[u8]) -> Result<()> {
            Err(anyhow!("MemoryStore does not support inserts"))
        }

        fn rollback(&self, _info: &acropolis_common::BlockInfo) -> Result<()> {
            Ok(())
        }

        fn should_persist(&self, _block_number: u64) -> bool {
            false
        }

        fn get_earliest_block_number(&self) -> Result<Option<u64>> {
            Ok(self.blocks_by_number.first_key_value().map(|(number, _)| *number))
        }

        fn get_tip_block_number(&self) -> u64 {
            self.blocks_by_number.last_key_value().map(|(number, _)| *number).unwrap_or_default()
        }

        fn get_block_by_hash(&self, hash: &[u8]) -> Result<Option<Block>> {
            Ok(self.blocks_by_hash.get(hash).map(copy_block))
        }

        fn get_block_by_slot(&self, _slot: u64) -> Result<Option<Block>> {
            Ok(None)
        }

        fn get_block_by_number(&self, number: u64) -> Result<Option<Block>> {
            Ok(self.blocks_by_number.get(&number).map(copy_block))
        }

        fn get_blocks_by_number_range(
            &self,
            min_number: u64,
            max_number: u64,
        ) -> Result<Vec<Block>> {
            Ok(self
                .blocks_by_number
                .range(min_number..=max_number)
                .map(|(_, block)| copy_block(block))
                .collect())
        }

        fn get_block_by_epoch_slot(&self, _epoch: u64, _epoch_slot: u64) -> Result<Option<Block>> {
            Ok(None)
        }

        fn get_latest_block(&self) -> Result<Option<Block>> {
            Ok(self.blocks_by_number.last_key_value().map(|(_, block)| copy_block(block)))
        }

        fn get_tx_by_hash(&self, _hash: &[u8]) -> Result<Option<Tx>> {
            Ok(None)
        }

        fn get_tx_block_ref_by_hash(&self, _hash: &[u8]) -> Result<Option<TxBlockReference>> {
            Ok(None)
        }
    }

    #[test]
    fn should_return_latest_stable_block_when_boundary_is_within_window() {
        let (_dir, store, infos) = init_store_with_blocks(6);
        let state = State::new();
        let expected = infos[4].clone();

        let response = handle_blocks_query(
            &store,
            &state,
            &BlocksStateQuery::GetLatestStableBlockAsOf {
                stability_offset: 1,
                min_block_timestamp_unix_millis: expected.timestamp.saturating_mul(1000),
                max_block_timestamp_unix_millis: expected.timestamp.saturating_mul(1000),
            },
        )
        .unwrap();

        match response {
            BlocksStateQueryResponse::LatestStableBlockAsOf(Some(block)) => {
                assert_block_matches_info(&block, &expected);
            }
            other => panic!("unexpected response: {other:?}"),
        }
    }

    #[test]
    fn should_return_latest_earlier_stable_block_when_boundary_is_too_new() {
        let (_dir, store, infos) = init_store_with_blocks(6);
        let state = State::new();
        let expected = infos[3].clone();

        let response = handle_blocks_query(
            &store,
            &state,
            &BlocksStateQuery::GetLatestStableBlockAsOf {
                stability_offset: 1,
                min_block_timestamp_unix_millis: expected.timestamp.saturating_mul(1000),
                max_block_timestamp_unix_millis: expected.timestamp.saturating_mul(1000),
            },
        )
        .unwrap();

        match response {
            BlocksStateQueryResponse::LatestStableBlockAsOf(Some(block)) => {
                assert_block_matches_info(&block, &expected);
            }
            other => panic!("unexpected response: {other:?}"),
        }
    }

    #[test]
    fn should_return_block_stored_at_zero_when_it_is_only_matching_candidate() {
        let state = State::new();
        let blocks = crate::stores::fjall::tests::test_block_range_bytes(2);
        let infos = blocks
            .iter()
            .map(|bytes| crate::stores::fjall::tests::test_block_info(bytes))
            .collect::<Vec<_>>();
        let zero_block = build_store_block(&infos[0], &blocks[0], 5);
        let tip_block = build_store_block(&infos[1], &blocks[1], 10);
        let store = Arc::new(MemoryStore::new([(0, zero_block), (1, tip_block)])) as Arc<dyn Store>;

        let response = handle_blocks_query(
            &store,
            &state,
            &BlocksStateQuery::GetLatestStableBlockAsOf {
                stability_offset: 0,
                min_block_timestamp_unix_millis: 5_000,
                max_block_timestamp_unix_millis: 5_000,
            },
        )
        .unwrap();

        match response {
            BlocksStateQueryResponse::LatestStableBlockAsOf(Some(block)) => {
                assert_eq!(block.hash, infos[0].hash);
                assert_eq!(block.timestamp, 5);
            }
            other => panic!("unexpected response: {other:?}"),
        }
    }

    #[test]
    fn should_search_from_oldest_retained_block_for_latest_stable_block_when_store_starts_after_genesis(
    ) {
        let state = State::new();
        let blocks = crate::stores::fjall::tests::test_block_range_bytes(3);
        let infos = blocks
            .iter()
            .map(|bytes| crate::stores::fjall::tests::test_block_info(bytes))
            .collect::<Vec<_>>();
        let store = Arc::new(MemoryStore::new([
            (100, build_store_block(&infos[0], &blocks[0], 5)),
            (101, build_store_block(&infos[1], &blocks[1], 6)),
            (102, build_store_block(&infos[2], &blocks[2], 10)),
        ])) as Arc<dyn Store>;

        let response = handle_blocks_query(
            &store,
            &state,
            &BlocksStateQuery::GetLatestStableBlockAsOf {
                stability_offset: 0,
                min_block_timestamp_unix_millis: 6_000,
                max_block_timestamp_unix_millis: 6_000,
            },
        )
        .unwrap();

        match response {
            BlocksStateQueryResponse::LatestStableBlockAsOf(Some(block)) => {
                assert_eq!(block.hash, infos[1].hash);
                assert_eq!(block.timestamp, 6);
            }
            other => panic!("unexpected response: {other:?}"),
        }
    }

    #[test]
    fn should_return_no_latest_stable_block_when_no_block_is_within_window() {
        let state = State::new();
        let blocks = crate::stores::fjall::tests::test_block_range_bytes(2);
        let infos = blocks
            .iter()
            .map(|bytes| crate::stores::fjall::tests::test_block_info(bytes))
            .collect::<Vec<_>>();
        let store = Arc::new(MemoryStore::new([
            (0, build_store_block(&infos[0], &blocks[0], 5)),
            (1, build_store_block(&infos[1], &blocks[1], 10)),
        ])) as Arc<dyn Store>;

        let response = handle_blocks_query(
            &store,
            &state,
            &BlocksStateQuery::GetLatestStableBlockAsOf {
                stability_offset: 1,
                min_block_timestamp_unix_millis: 4_000,
                max_block_timestamp_unix_millis: 4_000,
            },
        )
        .unwrap();

        match response {
            BlocksStateQueryResponse::LatestStableBlockAsOf(None) => {}
            other => panic!("unexpected response: {other:?}"),
        }
    }

    #[test]
    fn should_return_stable_block_by_hash_when_within_boundary_and_window() {
        let (_dir, store, infos) = init_store_with_blocks(6);
        let state = State::new();
        let expected = infos[3].clone();

        let response = handle_blocks_query(
            &store,
            &state,
            &BlocksStateQuery::GetStableBlockByHashAsOf {
                block_hash: expected.hash,
                stability_offset: 1,
                min_block_timestamp_unix_millis: expected.timestamp.saturating_mul(1000),
                max_block_timestamp_unix_millis: expected.timestamp.saturating_mul(1000),
            },
        )
        .unwrap();

        match response {
            BlocksStateQueryResponse::StableBlockByHashAsOf(Some(block)) => {
                assert_block_matches_info(&block, &expected);
            }
            other => panic!("unexpected response: {other:?}"),
        }
    }

    #[test]
    fn should_reject_stable_block_by_hash_outside_boundary_or_window() {
        let (_dir, store, infos) = init_store_with_blocks(6);
        let state = State::new();
        let unstable_block = infos[5].clone();
        let windowed_block = infos[3].clone();

        let outside_boundary = handle_blocks_query(
            &store,
            &state,
            &BlocksStateQuery::GetStableBlockByHashAsOf {
                block_hash: unstable_block.hash,
                stability_offset: 1,
                min_block_timestamp_unix_millis: unstable_block.timestamp.saturating_mul(1000),
                max_block_timestamp_unix_millis: unstable_block.timestamp.saturating_mul(1000),
            },
        )
        .unwrap();

        match outside_boundary {
            BlocksStateQueryResponse::StableBlockByHashAsOf(None) => {}
            other => panic!("unexpected response: {other:?}"),
        }

        let outside_window = handle_blocks_query(
            &store,
            &state,
            &BlocksStateQuery::GetStableBlockByHashAsOf {
                block_hash: windowed_block.hash,
                stability_offset: 1,
                min_block_timestamp_unix_millis: windowed_block
                    .timestamp
                    .saturating_mul(1000)
                    .saturating_add(1),
                max_block_timestamp_unix_millis: windowed_block
                    .timestamp
                    .saturating_mul(1000)
                    .saturating_add(1),
            },
        )
        .unwrap();

        match outside_window {
            BlocksStateQueryResponse::StableBlockByHashAsOf(None) => {}
            other => panic!("unexpected response: {other:?}"),
        }
    }
}
