use crate::{BlockHash, Lovelace, NativeAsset, TxHash};
use serde::ser::{Serialize, SerializeStruct, Serializer};
use serde_with::{DisplayFromStr, serde_as};

pub const DEFAULT_TRANSACTIONS_QUERY_TOPIC: (&str, &str) = (
    "transactions-state-query-topic",
    "cardano.query.transactions",
);

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum TransactionsStateQuery {
    GetTransactionInfo { tx_hash: TxHash },
    GetTransactionUTxOs,
    GetTransactionStakeCertificates,
    GetTransactionDelegationCertificates,
    GetTransactionWithdrawals,
    GetTransactionMIRs,
    GetTransactionPoolUpdateCertificates,
    GetTransactionPoolRetirementCertificates,
    GetTransactionMetadata,
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
    NotFound,
    Error(String),
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
            },
            TransactionOutputAmount::Asset(asset) => {
                state.serialize_field("unit", &asset.name)?;
                state.serialize_field("amount", &asset.amount.to_string())?;
            },
        }
        state.end()
    }
}

#[serde_as]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TransactionInfo {
    pub hash: TxHash,
    #[serde(rename = "block")]
    pub block_hash: BlockHash,
    #[serde(rename = "height")]
    pub block_number: u64,
    #[serde(rename = "time")]
    pub block_time: u64,
    pub slot: u64,
    pub index: u64,
    #[serde(rename = "order_amount")]
    pub output_amounts: Vec<TransactionOutputAmount>,
    #[serde(rename = "fees")]
    #[serde_as(as = "DisplayFromStr")]
    pub fee: u64,
    pub deposit: u64,
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
pub struct TransactionStakeCertificates {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TransactionDelegationCertificates {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TransactionWithdrawals {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ScriptDatumJSON {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TransactionMIRs {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TransactionPoolUpdateCertificates {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TransactionPoolRetirementCertificates {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TransactionMetadata {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TransactionMetadataCBOR {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TransactionRedeemers {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TransactionRequiredSigners {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TransactionCBOR {}
