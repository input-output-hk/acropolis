mod stores;

use acropolis_codec::{block::map_to_block_issuer, map_parameters};
use acropolis_common::{
    crypto::keyhash_224,
    messages::{CardanoMessage, Message, StateQuery, StateQueryResponse},
    queries::blocks::{
        BlockHashes, BlockInfo, BlockInvolvedAddress, BlockInvolvedAddresses, BlockKey,
        BlockTransaction, BlockTransactions, BlockTransactionsCBOR, BlocksStateQuery,
        BlocksStateQueryResponse, NextBlocks, PreviousBlocks, TransactionHashes,
        DEFAULT_BLOCKS_QUERY_TOPIC,
    },
    queries::misc::Order,
    state_history::{StateHistory, StateHistoryStore},
    BechOrdAddress, BlockHash, GenesisDelegate, HeavyDelegate, TxHash, VRFKey,
};
use anyhow::{bail, Result};
use caryatid_sdk::{module, Context, Module};
use config::Config;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::error;

use crate::stores::{fjall::FjallStore, Block, Store};

const DEFAULT_BLOCKS_TOPIC: &str = "cardano.block.available";
const DEFAULT_PROTOCOL_PARAMETERS_TOPIC: &str = "cardano.protocol.parameters";
const DEFAULT_STORE: &str = "fjall";

#[module(
    message_type(Message),
    name = "chain-store",
    description = "Block and TX state"
)]
pub struct ChainStore;

impl ChainStore {
    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        let new_blocks_topic =
            config.get_string("blocks-topic").unwrap_or(DEFAULT_BLOCKS_TOPIC.to_string());
        let params_topic = config
            .get_string("protocol-parameters-topic")
            .unwrap_or(DEFAULT_PROTOCOL_PARAMETERS_TOPIC.to_string());
        let block_queries_topic = config
            .get_string(DEFAULT_BLOCKS_QUERY_TOPIC.0)
            .unwrap_or(DEFAULT_BLOCKS_QUERY_TOPIC.1.to_string());

        let store_type = config.get_string("store").unwrap_or(DEFAULT_STORE.to_string());
        let store: Arc<dyn Store> = match store_type.as_str() {
            "fjall" => Arc::new(FjallStore::new(config.clone())?),
            _ => bail!("Unknown store type {store_type}"),
        };

        let history = Arc::new(Mutex::new(StateHistory::<State>::new(
            "chain_store",
            StateHistoryStore::default_epoch_store(),
        )));
        history.lock().await.commit_forced(State::new());

        let query_store = store.clone();
        let query_history = history.clone();
        context.handle(&block_queries_topic, move |req| {
            let query_store = query_store.clone();
            let query_history = query_history.clone();
            async move {
                let Message::StateQuery(StateQuery::Blocks(query)) = req.as_ref() else {
                    return Arc::new(Message::StateQueryResponse(StateQueryResponse::Blocks(
                        BlocksStateQueryResponse::Error("Invalid message for blocks-state".into()),
                    )));
                };
                let Some(state) = query_history.lock().await.current().cloned() else {
                    return Arc::new(Message::StateQueryResponse(StateQueryResponse::Blocks(
                        BlocksStateQueryResponse::Error("unitialised state".to_string()),
                    )));
                };
                let res = Self::handle_blocks_query(&query_store, &state, query)
                    .unwrap_or_else(|err| BlocksStateQueryResponse::Error(err.to_string()));
                Arc::new(Message::StateQueryResponse(StateQueryResponse::Blocks(res)))
            }
        });

        let mut new_blocks_subscription = context.subscribe(&new_blocks_topic).await?;
        let mut params_subscription = context.subscribe(&params_topic).await?;
        context.run(async move {
            // Get promise of params message so the params queue is cleared and
            // the message is ready as soon as possible when we need it
            let mut params_message = params_subscription.read();
            loop {
                let Ok((_, message)) = new_blocks_subscription.read().await else {
                    return;
                };

                if let Err(err) = Self::handle_new_block(&store, &message) {
                    error!("Could not insert block: {err}");
                }

                if let Message::Cardano((block_info, _)) = message.as_ref() {
                    if block_info.new_epoch {
                        let Ok((_, message)) = params_message.await else {
                            return;
                        };
                        let mut history = history.lock().await;
                        let mut state = history.get_current_state();
                        if Self::handle_new_params(&mut state, message).is_err() {
                            return;
                        };
                        history.commit(block_info.number, state);
                        // Have the next params message ready for the next epoch
                        params_message = params_subscription.read();
                    }
                }
            }
        });

        Ok(())
    }

    fn handle_new_block(store: &Arc<dyn Store>, message: &Message) -> Result<()> {
        let Message::Cardano((info, CardanoMessage::BlockAvailable(raw_block))) = message else {
            bail!("Unexpected message type: {message:?}");
        };

        store.insert_block(info, &raw_block.body)
    }

    fn handle_blocks_query(
        store: &Arc<dyn Store>,
        state: &State,
        query: &BlocksStateQuery,
    ) -> Result<BlocksStateQueryResponse> {
        match query {
            BlocksStateQuery::GetLatestBlock => {
                let Some(block) = store.get_latest_block()? else {
                    return Ok(BlocksStateQueryResponse::NotFound);
                };
                let info = Self::to_block_info(block, store, state, true)?;
                Ok(BlocksStateQueryResponse::LatestBlock(info))
            }
            BlocksStateQuery::GetLatestBlockTransactions { limit, skip, order } => {
                let Some(block) = store.get_latest_block()? else {
                    return Ok(BlocksStateQueryResponse::NotFound);
                };
                let txs = Self::to_block_transactions(block, limit, skip, order)?;
                Ok(BlocksStateQueryResponse::LatestBlockTransactions(txs))
            }
            BlocksStateQuery::GetLatestBlockTransactionsCBOR { limit, skip, order } => {
                let Some(block) = store.get_latest_block()? else {
                    return Ok(BlocksStateQueryResponse::NotFound);
                };
                let txs = Self::to_block_transactions_cbor(block, limit, skip, order)?;
                Ok(BlocksStateQueryResponse::LatestBlockTransactionsCBOR(txs))
            }
            BlocksStateQuery::GetBlockInfo { block_key } => {
                let Some(block) = Self::get_block_by_key(store, block_key)? else {
                    return Ok(BlocksStateQueryResponse::NotFound);
                };
                let info = Self::to_block_info(block, store, state, false)?;
                Ok(BlocksStateQueryResponse::BlockInfo(info))
            }
            BlocksStateQuery::GetBlockBySlot { slot } => {
                let Some(block) = store.get_block_by_slot(*slot)? else {
                    return Ok(BlocksStateQueryResponse::NotFound);
                };
                let info = Self::to_block_info(block, store, state, false)?;
                Ok(BlocksStateQueryResponse::BlockBySlot(info))
            }
            BlocksStateQuery::GetBlockByEpochSlot { epoch, slot } => {
                let Some(block) = store.get_block_by_epoch_slot(*epoch, *slot)? else {
                    return Ok(BlocksStateQueryResponse::NotFound);
                };
                let info = Self::to_block_info(block, store, state, false)?;
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
                let Some(block) = Self::get_block_by_key(store, block_key)? else {
                    return Ok(BlocksStateQueryResponse::NotFound);
                };
                let number = match block_key {
                    BlockKey::Number(number) => *number,
                    _ => Self::get_block_number(&block)?,
                };
                let min_number = number + 1 + skip;
                let max_number = min_number + limit - 1;
                let blocks = store.get_blocks_by_number_range(min_number, max_number)?;
                let info = Self::to_block_info_bulk(blocks, store, state, false)?;
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
                let Some(block) = Self::get_block_by_key(store, block_key)? else {
                    return Ok(BlocksStateQueryResponse::NotFound);
                };
                let number = match block_key {
                    BlockKey::Number(number) => *number,
                    _ => Self::get_block_number(&block)?,
                };
                let Some(max_number) = number.checked_sub(1 + skip) else {
                    return Ok(BlocksStateQueryResponse::PreviousBlocks(PreviousBlocks {
                        blocks: vec![],
                    }));
                };
                let min_number = max_number.saturating_sub(limit - 1);
                let blocks = store.get_blocks_by_number_range(min_number, max_number)?;
                let info = Self::to_block_info_bulk(blocks, store, state, false)?;
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
                let Some(block) = Self::get_block_by_key(store, block_key)? else {
                    return Ok(BlocksStateQueryResponse::NotFound);
                };
                let txs = Self::to_block_transactions(block, limit, skip, order)?;
                Ok(BlocksStateQueryResponse::BlockTransactions(txs))
            }
            BlocksStateQuery::GetBlockTransactionsCBOR {
                block_key,
                limit,
                skip,
                order,
            } => {
                let Some(block) = Self::get_block_by_key(store, block_key)? else {
                    return Ok(BlocksStateQueryResponse::NotFound);
                };
                let txs = Self::to_block_transactions_cbor(block, limit, skip, order)?;
                Ok(BlocksStateQueryResponse::BlockTransactionsCBOR(txs))
            }
            BlocksStateQuery::GetBlockInvolvedAddresses {
                block_key,
                limit,
                skip,
            } => {
                let Some(block) = Self::get_block_by_key(store, block_key)? else {
                    return Ok(BlocksStateQueryResponse::NotFound);
                };
                let addresses = Self::to_block_involved_addresses(block, limit, skip)?;
                Ok(BlocksStateQueryResponse::BlockInvolvedAddresses(addresses))
            }
            BlocksStateQuery::GetBlockHashes { block_numbers } => {
                let mut block_hashes = HashMap::new();
                for block_number in block_numbers {
                    if let Ok(Some(block)) = store.get_block_by_number(*block_number) {
                        if let Ok(hash) = Self::get_block_hash(&block) {
                            block_hashes.insert(*block_number, hash);
                        }
                    }
                }
                Ok(BlocksStateQueryResponse::BlockHashes(BlockHashes {
                    block_hashes,
                }))
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
                            if let Ok(hashes) = Self::to_block_transaction_hashes(&block) {
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
        }
    }

    fn get_block_by_key(store: &Arc<dyn Store>, block_key: &BlockKey) -> Result<Option<Block>> {
        match block_key {
            BlockKey::Hash(hash) => store.get_block_by_hash(hash.as_ref()),
            BlockKey::Number(number) => store.get_block_by_number(*number),
        }
    }

    fn get_block_number(block: &Block) -> Result<u64> {
        Ok(pallas_traverse::MultiEraBlock::decode(&block.bytes)?.number())
    }

    fn get_block_hash(block: &Block) -> Result<BlockHash> {
        Ok(BlockHash(
            *pallas_traverse::MultiEraBlock::decode(&block.bytes)?.hash(),
        ))
    }

    fn to_block_info(
        block: Block,
        store: &Arc<dyn Store>,
        state: &State,
        is_latest: bool,
    ) -> Result<BlockInfo> {
        let blocks = vec![block];
        let mut info = Self::to_block_info_bulk(blocks, store, state, is_latest)?;
        Ok(info.remove(0))
    }

    fn to_block_info_bulk(
        blocks: Vec<Block>,
        store: &Arc<dyn Store>,
        state: &State,
        final_block_is_latest: bool,
    ) -> Result<Vec<BlockInfo>> {
        if blocks.is_empty() {
            return Ok(vec![]);
        }
        let mut decoded_blocks = vec![];
        for block in &blocks {
            decoded_blocks.push(pallas_traverse::MultiEraBlock::decode(&block.bytes)?);
        }

        let (latest_number, latest_hash) = if final_block_is_latest {
            let latest = decoded_blocks.last().unwrap();
            (latest.number(), latest.hash())
        } else {
            let raw_latest = store.get_latest_block()?.unwrap();
            let latest = pallas_traverse::MultiEraBlock::decode(&raw_latest.bytes)?;
            (latest.number(), latest.hash())
        };

        let mut next_hash = if final_block_is_latest {
            None
        } else {
            let next_number = decoded_blocks.last().unwrap().number() + 1;
            if next_number > latest_number {
                None
            } else if next_number == latest_number {
                Some(latest_hash)
            } else {
                let raw_next = store.get_block_by_number(next_number)?;
                if let Some(raw_next) = raw_next {
                    let next = pallas_traverse::MultiEraBlock::decode(&raw_next.bytes)?;
                    Some(next.hash())
                } else {
                    None
                }
            }
        };

        let mut block_info = vec![];
        for (block, decoded) in blocks.iter().zip(decoded_blocks).rev() {
            let header = decoded.header();
            let mut output = None;
            let mut fees = None;
            for tx in decoded.txs() {
                if let Some(new_fee) = tx.fee() {
                    fees = Some(fees.unwrap_or_default() + new_fee);
                }
                for o in tx.outputs() {
                    output = Some(output.unwrap_or_default() + o.value().coin())
                }
            }
            let (op_cert_hot_vkey, op_cert_counter) = match &header {
                pallas_traverse::MultiEraHeader::BabbageCompatible(h) => {
                    let cert = &h.header_body.operational_cert;
                    (
                        Some(&cert.operational_cert_hot_vkey),
                        Some(cert.operational_cert_sequence_number),
                    )
                }
                pallas_traverse::MultiEraHeader::ShelleyCompatible(h) => (
                    Some(&h.header_body.operational_cert_hot_vkey),
                    Some(h.header_body.operational_cert_sequence_number),
                ),
                _ => (None, None),
            };
            let op_cert = op_cert_hot_vkey.map(|vkey| keyhash_224(vkey));

            block_info.push(BlockInfo {
                timestamp: block.extra.timestamp,
                number: header.number(),
                hash: BlockHash(*header.hash()),
                slot: header.slot(),
                epoch: block.extra.epoch,
                epoch_slot: block.extra.epoch_slot,
                issuer: map_to_block_issuer(
                    &header,
                    &state.byron_heavy_delegates,
                    &state.shelley_genesis_delegates,
                ),
                size: block.bytes.len() as u64,
                tx_count: decoded.tx_count() as u64,
                output,
                fees,
                block_vrf: header.vrf_vkey().map(|key| VRFKey::try_from(key).ok().unwrap()),
                op_cert,
                op_cert_counter,
                previous_block: header.previous_hash().map(|h| BlockHash(*h)),
                next_block: next_hash.map(|h| BlockHash(*h)),
                confirmations: latest_number - header.number(),
            });

            next_hash = Some(header.hash());
        }

        block_info.reverse();
        Ok(block_info)
    }

    fn to_block_transaction_hashes(block: &Block) -> Result<Vec<TxHash>> {
        let decoded = pallas_traverse::MultiEraBlock::decode(&block.bytes)?;
        let txs = decoded.txs();
        Ok(txs.iter().map(|tx| TxHash(*tx.hash())).collect())
    }

    fn to_block_transactions(
        block: Block,
        limit: &u64,
        skip: &u64,
        order: &Order,
    ) -> Result<BlockTransactions> {
        let decoded = pallas_traverse::MultiEraBlock::decode(&block.bytes)?;
        let txs = decoded.txs();
        let txs_iter: Box<dyn Iterator<Item = _>> = match *order {
            Order::Asc => Box::new(txs.iter()),
            Order::Desc => Box::new(txs.iter().rev()),
        };
        let hashes = txs_iter
            .skip(*skip as usize)
            .take(*limit as usize)
            .map(|tx| TxHash(*tx.hash()))
            .collect();
        Ok(BlockTransactions { hashes })
    }

    fn to_block_transactions_cbor(
        block: Block,
        limit: &u64,
        skip: &u64,
        order: &Order,
    ) -> Result<BlockTransactionsCBOR> {
        let decoded = pallas_traverse::MultiEraBlock::decode(&block.bytes)?;
        let txs = decoded.txs();
        let txs_iter: Box<dyn Iterator<Item = _>> = match *order {
            Order::Asc => Box::new(txs.iter()),
            Order::Desc => Box::new(txs.iter().rev()),
        };
        let txs = txs_iter
            .skip(*skip as usize)
            .take(*limit as usize)
            .map(|tx| {
                let hash = TxHash(*tx.hash());
                let cbor = tx.encode();
                BlockTransaction { hash, cbor }
            })
            .collect();
        Ok(BlockTransactionsCBOR { txs })
    }

    fn to_block_involved_addresses(
        block: Block,
        limit: &u64,
        skip: &u64,
    ) -> Result<BlockInvolvedAddresses> {
        let decoded = pallas_traverse::MultiEraBlock::decode(&block.bytes)?;
        let mut addresses = BTreeMap::new();
        for tx in decoded.txs() {
            let hash = TxHash(*tx.hash());
            for output in tx.outputs() {
                if let Ok(pallas_address) = output.address() {
                    if let Ok(address) = map_parameters::map_address(&pallas_address) {
                        addresses
                            .entry(BechOrdAddress(address))
                            .or_insert_with(Vec::new)
                            .push(hash);
                    }
                }
            }
        }
        let addresses: Vec<BlockInvolvedAddress> = addresses
            .into_iter()
            .skip(*skip as usize)
            .take(*limit as usize)
            .map(|(address, txs)| BlockInvolvedAddress {
                address: address.0,
                txs,
            })
            .collect();
        Ok(BlockInvolvedAddresses { addresses })
    }

    fn handle_new_params(state: &mut State, message: Arc<Message>) -> Result<()> {
        if let Message::Cardano((_, CardanoMessage::ProtocolParams(params))) = message.as_ref() {
            if let Some(byron) = &params.params.byron {
                state.byron_heavy_delegates = byron.heavy_delegation.clone();
            }
            if let Some(shelley) = &params.params.shelley {
                state.shelley_genesis_delegates = shelley.gen_delegs.clone();
            }
        }
        Ok(())
    }
}

#[derive(Default, Debug, Clone)]
pub struct State {
    pub byron_heavy_delegates: HashMap<Vec<u8>, HeavyDelegate>,
    pub shelley_genesis_delegates: HashMap<Vec<u8>, GenesisDelegate>,
}

impl State {
    pub fn new() -> Self {
        Self {
            byron_heavy_delegates: HashMap::new(),
            shelley_genesis_delegates: HashMap::new(),
        }
    }
}
