//! Snapshot parsing error types

use std::fmt;

/// Errors that can occur during snapshot parsing
#[derive(Debug)]
pub enum SnapshotError {
    /// File not found or inaccessible
    FileNotFound(String),

    /// Structural decoding error (unexpected CBOR structure)
    StructuralDecode(String),

    /// CBOR parsing error
    Cbor(minicbor::decode::Error),

    /// I/O error
    IoError(String),

    /// Era mismatch between expected and actual
    EraMismatch { expected: String, actual: String },

    /// Integrity check failed (hash mismatch)
    IntegrityMismatch { expected: String, actual: String },

    /// JSON parsing error
    Json(serde_json::Error),
}

impl fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SnapshotError::FileNotFound(msg) => write!(f, "File not found: {msg}"),
            SnapshotError::StructuralDecode(msg) => write!(f, "Structural decode error: {msg}"),
            SnapshotError::Cbor(e) => write!(f, "CBOR error: {e}"),
            SnapshotError::IoError(msg) => write!(f, "I/O error: {msg}"),
            SnapshotError::EraMismatch { expected, actual } => {
                write!(f, "Era mismatch: expected {expected}, got {actual}")
            }
            SnapshotError::IntegrityMismatch { expected, actual } => {
                write!(f, "Integrity mismatch: expected {expected}, got {actual}")
            }
            SnapshotError::Json(e) => write!(f, "JSON error: {e}"),
        }
    }
}

impl std::error::Error for SnapshotError {}

impl From<std::io::Error> for SnapshotError {
    fn from(e: std::io::Error) -> Self {
        SnapshotError::IoError(e.to_string())
    }
}

impl From<minicbor::decode::Error> for SnapshotError {
    fn from(e: minicbor::decode::Error) -> Self {
        SnapshotError::Cbor(e)
    }
}

impl From<serde_json::Error> for SnapshotError {
    fn from(e: serde_json::Error) -> Self {
        SnapshotError::Json(e)
    }
}
