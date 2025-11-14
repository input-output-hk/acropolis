use crate::{
    BlockHash, InstantaneousRewardSource, Lovelace, Metadata, NativeAsset, PoolId,
    PoolRegistration, StakeAddress, TxHash,
};
use serde::ser::{Serialize, SerializeStruct, Serializer};
use serde_with::serde_as;

pub const DEFAULT_TRANSACTIONS_QUERY_TOPIC: (&str, &str) = (
    "transactions-state-query-topic",
    "cardano.query.transactions",
);
use crate::queries::errors::QueryError;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum TransactionsStateQuery {
    GetTransactionInfo { tx_hash: TxHash },
    GetTransactionUTxOs,
    GetTransactionStakeCertificates { tx_hash: TxHash },
    GetTransactionDelegationCertificates { tx_hash: TxHash },
    GetTransactionWithdrawals { tx_hash: TxHash },
    GetTransactionMIRs { tx_hash: TxHash },
    GetTransactionPoolUpdateCertificates { tx_hash: TxHash },
    GetTransactionPoolRetirementCertificates { tx_hash: TxHash },
    GetTransactionMetadata { tx_hash: TxHash },
    GetTransactionMetadataCBOR,
    GetTransactionRedeemers,
    GetTransactionRequiredSigners,
    GetTransactionCBOR,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum TransactionsStateQueryResponse {
    TransactionInfo(TransactionInfo),
    TransactionUTxOs(TransactionUTxOs),
    TransactionStakeCertificates(TransactionStakeCertificates),
    TransactionDelegationCertificates(TransactionDelegationCertificates),
    TransactionWithdrawals(TransactionWithdrawals),
    TransactionMIRs(TransactionMIRs),
    TransactionPoolUpdateCertificates(TransactionPoolUpdateCertificates),
    TransactionPoolRetirementCertificates(TransactionPoolRetirementCertificates),
    TransactionMetadata(TransactionMetadata),
    TransactionMetadataCBOR(TransactionMetadataCBOR),
    TransactionRedeemers(TransactionRedeemers),
    TransactionRequiredSigners(TransactionRequiredSigners),
    TransactionCBOR(TransactionCBOR),
    Error(QueryError),
}

#[derive(Debug, Clone, serde::Deserialize)]
pub enum TransactionOutputAmount {
    Lovelace(Lovelace),
    Asset(NativeAsset),
}

impl Serialize for TransactionOutputAmount {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("TransactionOutputAmount", 2)?;
        match self {
            TransactionOutputAmount::Lovelace(lovelace) => {
                state.serialize_field("unit", "lovelace")?;
                state.serialize_field("amount", &lovelace.to_string())?;
            }
            TransactionOutputAmount::Asset(asset) => {
                state.serialize_field("unit", &asset.name)?;
                state.serialize_field("amount", &asset.amount.to_string())?;
            }
        }
        state.end()
    }
}

#[serde_as]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TransactionInfo {
    pub hash: TxHash,
    pub block_hash: BlockHash,
    pub block_number: u64,
    pub block_time: u64,
    pub epoch: u64,
    pub slot: u64,
    pub index: u64,
    pub output_amounts: Vec<TransactionOutputAmount>,
    pub recorded_fee: Option<u64>,
    pub size: u64,
    pub invalid_before: Option<u64>,
    pub invalid_after: Option<u64>,
    pub utxo_count: u64,
    pub withdrawal_count: u64,
    pub mir_cert_count: u64,
    pub delegation_count: u64,
    pub stake_cert_count: u64,
    pub pool_update_count: u64,
    pub pool_retire_count: u64,
    pub asset_mint_or_burn_count: u64,
    pub redeemer_count: u64,
    pub valid_contract: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TransactionUTxOs {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TransactionStakeCertificate {
    pub index: u64,
    pub address: StakeAddress,
    pub registration: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TransactionStakeCertificates {
    pub certificates: Vec<TransactionStakeCertificate>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TransactionDelegationCertificate {
    pub index: u64,
    pub address: StakeAddress,
    pub pool_id: PoolId,
    pub active_epoch: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TransactionDelegationCertificates {
    pub certificates: Vec<TransactionDelegationCertificate>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TransactionWithdrawal {
    pub address: StakeAddress,
    pub amount: Lovelace,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TransactionWithdrawals {
    pub withdrawals: Vec<TransactionWithdrawal>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ScriptDatumJSON {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TransactionMIR {
    pub cert_index: u64,
    pub pot: InstantaneousRewardSource,
    pub address: StakeAddress,
    pub amount: Lovelace,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TransactionMIRs {
    pub mirs: Vec<TransactionMIR>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TransactionPoolUpdateCertificate {
    pub cert_index: u64,
    pub pool_reg: PoolRegistration,
    pub active_epoch: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TransactionPoolUpdateCertificates {
    pub pool_updates: Vec<TransactionPoolUpdateCertificate>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TransactionPoolRetirementCertificate {
    pub cert_index: u64,
    pub pool_id: PoolId,
    pub retirement_epoch: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TransactionPoolRetirementCertificates {
    pub pool_retirements: Vec<TransactionPoolRetirementCertificate>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TransactionMetadataItem {
    pub label: String,
    pub json_metadata: Metadata,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TransactionMetadata {
    pub metadata: Vec<TransactionMetadataItem>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TransactionMetadataCBOR {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TransactionRedeemers {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TransactionRequiredSigners {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TransactionCBOR {}
