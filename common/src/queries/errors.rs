use caryatid_module_rest_server::messages::RESTResponse;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Common error type for all state query responses
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum QueryError {
    /// The requested resource was not found
    NotFound {
        resource: String
    },

    /// An error occurred while processing the query
    QueryFailed {
        message: String
    },

    /// Storage backend is disabled in configuration
    StorageDisabled {
        storage_type: String
    },

    /// Invalid request parameters
    InvalidRequest {
        message: String
    },

    /// One or more resources in a batch query were not found
    PartialNotFound {
        message: String,
    },

    /// Query variant is not implemented yet
    NotImplemented {
        query: String,
    },
}

impl QueryError {
    pub fn not_found(resource: impl Into<String>) -> Self {
        Self::NotFound {
            resource: resource.into()
        }
    }

    pub fn query_failed(message: impl Into<String>) -> Self {
        Self::QueryFailed {
            message: message.into()
        }
    }

    pub fn storage_disabled(storage_type: impl Into<String>) -> Self {
        Self::StorageDisabled {
            storage_type: storage_type.into()
        }
    }

    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self::InvalidRequest {
            message: message.into()
        }
    }

    pub fn partial_not_found(message: impl Into<String>) -> Self {
        Self::PartialNotFound {
            message: message.into()
        }
    }

    pub fn not_implemented(query: impl Into<String>) -> Self {
        Self::NotImplemented {
            query: query.into()
        }
    }
}

impl fmt::Display for QueryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound { resource } => write!(f, "Not found: {}", resource),
            Self::QueryFailed { message } => write!(f, "Query failed: {}", message),
            Self::StorageDisabled { storage_type } => write!(f, "{} storage is not enabled", storage_type),
            Self::InvalidRequest { message } => write!(f, "Invalid request: {}", message),
            Self::PartialNotFound { message } => write!(f, "Partial result: {}", message),
            Self::NotImplemented { query } => write!(f, "Query not implemented: {}", query),
        }
    }
}

impl std::error::Error for QueryError {}
