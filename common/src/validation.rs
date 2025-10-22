//! Validation results for Acropolis consensus

// We don't use these types in the acropolis_common crate itself
#![allow(dead_code)]

use thiserror::Error;

/// Validation error
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Error)]
pub enum ValidationError {
    #[error("VRF failure")]
    BadVRF,

    #[error("KES failure")]
    BadKES,

    #[error("Doubly spent UTXO: {0}")]
    DoubleSpendUTXO(String),
}

/// Validation status
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum ValidationStatus {
    /// All good
    Go,

    // Error
    NoGo(ValidationError),
}
