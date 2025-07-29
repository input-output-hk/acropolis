#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum TransactionsStateQuery {
    GetTransactionInfo,
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TransactionInfo {}

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
