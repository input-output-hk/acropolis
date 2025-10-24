use serde_with::{hex::Hex, serde_as};

use crate::TxHash;

#[serde_as]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum TransactionsCommand {
    Submit {
        #[serde_as(as = "Hex")]
        cbor: Vec<u8>,
    },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum TransactionsCommandResponse {
    Submitted { id: TxHash },
    Error(String),
}
