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
}

impl fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SnapshotError::FileNotFound(msg) => write!(f, "File not found: {}", msg),
            SnapshotError::StructuralDecode(msg) => write!(f, "Structural decode error: {}", msg),
            SnapshotError::Cbor(e) => write!(f, "CBOR error: {}", e),
            SnapshotError::IoError(msg) => write!(f, "I/O error: {}", msg),
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
