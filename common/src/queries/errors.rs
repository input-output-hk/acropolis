use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Common error type for all state query responses
#[derive(Debug, Clone, Error, Serialize, Deserialize)]
pub enum QueryError {
    /// The requested resource was not found
    #[error("Not found: {resource}")]
    NotFound { resource: String },

    /// An error occurred while processing the query
    #[error("Internal error: {message}")]
    Internal { message: String },

    /// Storage backend is disabled in configuration
    #[error("{storage_type} storage is not enabled")]
    StorageDisabled { storage_type: String },

    /// Invalid request parameters
    #[error("Invalid request: {message}")]
    InvalidRequest { message: String },

    /// Query variant is not implemented yet
    #[error("Query not implemented: {query}")]
    NotImplemented { query: String },
}

impl QueryError {
    pub fn not_found(resource: impl Into<String>) -> Self {
        Self::NotFound {
            resource: resource.into(),
        }
    }

    pub fn internal_error(message: impl Into<String>) -> Self {
        Self::Internal {
            message: message.into(),
        }
    }

    pub fn storage_disabled(storage_type: impl Into<String>) -> Self {
        Self::StorageDisabled {
            storage_type: storage_type.into(),
        }
    }

    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self::InvalidRequest {
            message: message.into(),
        }
    }

    pub fn not_implemented(query: impl Into<String>) -> Self {
        Self::NotImplemented {
            query: query.into(),
        }
    }
}
