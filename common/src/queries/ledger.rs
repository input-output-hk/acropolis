#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum LedgerStateQuery {
    GetGenesisInfo,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum LedgerStateQueryResponse {
    GenesisInfo(GenesisInfo),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GenesisInfo {}
