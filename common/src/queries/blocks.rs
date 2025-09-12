use cryptoxide::hashing::blake2b::Blake2b;
use crate::{BlockHash, KeyHash, TxHash, serialization::Bech32WithHrp};
use serde::ser::{Serialize, SerializeStruct, Serializer};

pub const DEFAULT_BLOCKS_QUERY_TOPIC: (&str, &str) =
    ("blocks-state-query-topic", "cardano.query.blocks");

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum BlocksStateQuery {
    GetLatestBlock,
    GetLatestBlockTransactions,
    GetLatestBlockTransactionsCBOR,
    GetBlockInfo {
        block_key: BlockKey,
    },
    GetNextBlocks {
        block_key: BlockKey,
        limit: u64,
        skip: u64,
    },
    GetPreviousBlocks {
        block_key: BlockKey,
        limit: u64,
        skip: u64,
    },
    GetBlockBySlot {
        slot: u64,
    },
    GetBlockByEpochSlot {
        epoch: u64,
        slot: u64,
    },
    GetBlockTransactions {
        block_key: BlockKey,
    },
    GetBlockTransactionsCBOR {
        block_key: BlockKey,
    },
    GetBlockInvolvedAddresses {
        block_key: BlockKey,
    },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum BlockKey {
    Hash(Vec<u8>),
    Number(u64),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum BlocksStateQueryResponse {
    LatestBlock(BlockInfo),
    LatestBlockTransactions(BlockTransactions),
    LatestBlockTransactionsCBOR(BlockTransactionsCBOR),
    BlockInfo(BlockInfo),
    NextBlocks(NextBlocks),
    PreviousBlocks(PreviousBlocks),
    BlockBySlot(BlockInfo),
    BlockByEpochSlot(BlockInfo),
    BlockTransactions(BlockTransactions),
    BlockTransactionsCBOR(BlockTransactionsCBOR),
    BlockInvolvedAddresses(BlockInvolvedAddresses),
    NotFound,
    Error(String),
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct BlockInfo {
    pub timestamp: u64,
    pub number: u64,
    pub hash: BlockHash,
    pub slot: u64,
    pub epoch: u64,
    pub epoch_slot: u64,
    pub issuer_vkey: Option<KeyHash>,
    pub size: u64,
    pub tx_count: u64,
    pub output: Option<u64>,
    pub fees: Option<u64>,
    pub block_vrf: Option<Vec<u8>>,
    pub op_cert: Option<KeyHash>,
    pub op_cert_counter: Option<u64>,
    pub previous_block: Option<BlockHash>,
    pub next_block: Option<BlockHash>,
    pub confirmations: u64,
}

impl Serialize for BlockInfo {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer
    {
        let mut state = serializer.serialize_struct("BlockInfo", 17)?;
        state.serialize_field("time", &self.timestamp)?;
        state.serialize_field("height", &self.number)?;
        state.serialize_field("slot", &self.slot)?;
        state.serialize_field("epoch", &self.epoch)?;
        state.serialize_field("epoch_slot", &self.epoch_slot)?;
        state.serialize_field("slot_issuer", &self.issuer_vkey.clone().map(|vkey| -> String {
            let mut context = Blake2b::<224>::new();
            context.update_mut(&vkey);
            let digest = context.finalize().as_slice().to_owned();
            digest.to_bech32_with_hrp("pool").unwrap_or(String::new())
        }))?;
        state.serialize_field("size", &self.size)?;
        state.serialize_field("tx_count", &self.tx_count)?;
        state.serialize_field("output", &self.output)?;
        state.serialize_field("fees", &self.fees)?;
        state.serialize_field("block_vrf", &self.block_vrf.clone().map(|v| hex::encode(v)))?;
        state.serialize_field("op_cert", &self.op_cert.clone().map(|v| hex::encode(v)))?;
        state.serialize_field("op_cert_counter", &self.op_cert_counter)?;
        state.serialize_field("previous_block", &self.previous_block)?;
        state.serialize_field("next_block", &self.next_block)?;
        state.serialize_field("confirmations", &self.confirmations)?;
        state.end()
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NextBlocks {
    pub blocks: Vec<BlockInfo>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PreviousBlocks {
    pub blocks: Vec<BlockInfo>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BlockTransactions {
    pub hashes: Vec<TxHash>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BlockTransactionsCBOR {
    pub txs: Vec<BlockTransaction>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BlockTransaction {
    pub hash: TxHash,
    pub cbor: Vec<u8>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BlockInvolvedAddresses {}
