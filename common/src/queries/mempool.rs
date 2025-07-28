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
    NotFound,
    Error(String),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MempoolList {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MempoolTransaction {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MempoolTransactionByAddress {}
