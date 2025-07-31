#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum MetadataStateQuery {
    GetMetadataLabels,
    GetTransactionMetadataJSON,
    GetTransactionMetadataCBOR,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum MetadataStateQueryResponse {
    MetadataLabels(MetadataLabels),
    TransactionMetadataJSON(TransactionMetadataJSON),
    TransactionMetadataCBOR(TransactionMetadataCBOR),
    NotFound,
    Error(String),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MetadataLabels {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TransactionMetadataJSON {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TransactionMetadataCBOR {}
