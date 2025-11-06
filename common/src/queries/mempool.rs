use crate::queries::errors::QueryError;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum MempoolStateQuery {
    GetMempoolList,
    GetMempoolTransaction,
    GetMempoolTransactionByAddress,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum MempoolStateQueryResponse {
    MempoolList(MempoolList),
    MempoolTransaction(MempoolTransaction),
    MempoolTransactionByAddress(MempoolTransactionByAddress),
    Error(QueryError),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MempoolList {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MempoolTransaction {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MempoolTransactionByAddress {}
