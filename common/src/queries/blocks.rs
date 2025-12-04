use crate::queries::errors::QueryError;
use crate::{
    queries::misc::Order,
    serialization::{Bech32Conversion, Bech32WithHrp},
    Address, BlockHash, GenesisDelegate, HeavyDelegate, KeyHash, TxHash, TxIdentifier,
    VrfKeyHash,
};
use cryptoxide::hashing::blake2b::Blake2b;
use serde::ser::{Serialize, SerializeStruct, Serializer};
use serde_with::{hex::Hex, serde_as};
use std::collections::HashMap;

pub const DEFAULT_BLOCKS_QUERY_TOPIC: (&str, &str) =
    ("blocks-state-query-topic", "cardano.query.blocks");

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum BlocksStateQuery {
    GetLatestBlock,
    GetLatestBlockTransactions {
        limit: u64,
        skip: u64,
        order: Order,
    },
    GetLatestBlockTransactionsCBOR {
        limit: u64,
        skip: u64,
        order: Order,
    },
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
        limit: u64,
        skip: u64,
        order: Order,
    },
    GetBlockTransactionsCBOR {
        block_key: BlockKey,
        limit: u64,
        skip: u64,
        order: Order,
    },
    GetBlockInvolvedAddresses {
        block_key: BlockKey,
        limit: u64,
        skip: u64,
    },
    GetBlockHashes {
        block_numbers: Vec<u64>,
    },
    GetBlockHashesByNumberRange {
        min_number: u64,
        max_number: u64,
    },
    GetBlockHashesAndIndexOfTransactionHashes {
        tx_hashes: Vec<TxHash>,
    },
    GetTransactionHashes {
        tx_ids: Vec<TxIdentifier>,
    },
    GetTransactionHashesAndTimestamps {
        tx_ids: Vec<TxIdentifier>,
    },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum BlockKey {
    Hash(BlockHash),
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
    BlockHashes(BlockHashes),
    BlockHashesByNumberRange(Vec<BlockHash>),
    BlockHashesAndIndexOfTransactionHashes(Vec<BlockHashAndTxIndex>),
    TransactionHashes(TransactionHashes),
    TransactionHashesAndTimestamps(TransactionHashesAndTimeStamps),
    Error(QueryError),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum BlockIssuer {
    HeavyDelegate(HeavyDelegate),
    GenesisDelegate(GenesisDelegate),
    SPO(Vec<u8>),
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct BlockInfo {
    pub timestamp: u64,
    pub number: u64,
    pub hash: BlockHash,
    pub slot: u64,
    pub epoch: u64,
    pub epoch_slot: u64,
    pub issuer: Option<BlockIssuer>,
    pub size: u64,
    pub tx_count: u64,
    pub output: Option<u64>,
    pub fees: Option<u64>,
    pub block_vrf: Option<VrfKeyHash>,
    pub op_cert: Option<KeyHash>,
    pub op_cert_counter: Option<u64>,
    pub previous_block: Option<BlockHash>,
    pub next_block: Option<BlockHash>,
    pub confirmations: u64,
}

impl Serialize for BlockInfo {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("BlockInfo", 17)?;
        state.serialize_field("time", &self.timestamp)?;
        state.serialize_field("height", &self.number)?;
        state.serialize_field("hash", &self.hash)?;
        state.serialize_field("slot", &self.slot)?;
        state.serialize_field("epoch", &self.epoch)?;
        state.serialize_field("epoch_slot", &self.epoch_slot)?;
        state.serialize_field(
            "slot_issuer",
            &self.issuer.clone().map(|vkey| -> String {
                match vkey {
                    BlockIssuer::HeavyDelegate(_) => "Byron genesis slot issuer".to_string(),
                    BlockIssuer::GenesisDelegate(_) => "Shelley genesis slot issuer".to_string(),
                    BlockIssuer::SPO(vkey) => {
                        let mut context = Blake2b::<224>::new();
                        context.update_mut(&vkey);
                        let digest = context.finalize().as_slice().to_owned();
                        digest.to_bech32_with_hrp("pool").unwrap_or(String::new())
                    }
                }
            }),
        )?;
        state.serialize_field("size", &self.size)?;
        state.serialize_field("tx_count", &self.tx_count)?;
        state.serialize_field("output", &self.output)?;
        state.serialize_field("fees", &self.fees)?;
        state.serialize_field(
            "block_vrf",
            &self.block_vrf.and_then(|vkey| vkey.to_bech32().ok()),
        )?;
        state.serialize_field("op_cert", &self.op_cert.map(hex::encode))?;
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

#[serde_as]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BlockTransaction {
    pub hash: TxHash,
    #[serde_as(as = "Hex")]
    pub cbor: Vec<u8>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BlockInvolvedAddresses {
    pub addresses: Vec<BlockInvolvedAddress>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct BlockInvolvedAddress {
    pub address: Address,
    pub txs: Vec<TxHash>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BlockHashAndTxIndex {
    pub block_hash: BlockHash,
    pub tx_index: u16,
}

impl Serialize for BlockInvolvedAddress {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("BlockInvolvedAddress", 2)?;
        state.serialize_field(
            "address",
            &self.address.to_string().unwrap_or("<invalid or unknown address type>".to_string()),
        )?;
        state.serialize_field("transactions", &self.txs)?;
        state.end()
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BlockHashes {
    pub block_hashes: HashMap<u64, BlockHash>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TransactionHashes {
    pub tx_hashes: HashMap<TxIdentifier, TxHash>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TransactionHashesAndTimeStamps {
    pub tx_hashes: Vec<TxHash>,
    pub timestamps: Vec<u64>,
}
