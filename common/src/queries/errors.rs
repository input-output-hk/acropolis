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

/// Convert QueryError to RESTResponse with appropriate status codes
/// Not entirely sure where this should go
impl From<QueryError> for RESTResponse {
    fn from(error: QueryError) -> Self {
        match &error {
            QueryError::NotFound { .. } => RESTResponse::with_text(404, &error.to_string()),
            QueryError::QueryFailed { .. } => RESTResponse::with_text(500, &error.to_string()),
            QueryError::StorageDisabled { .. } => RESTResponse::with_text(501, &error.to_string()),
            QueryError::NotImplemented { .. } => RESTResponse::with_text(501, &error.to_string()),
            QueryError::InvalidRequest { .. } => RESTResponse::with_text(400, &error.to_string()),
            QueryError::PartialNotFound { .. } => RESTResponse::with_text(206, &error.to_string()),
        }
    }
}

impl std::error::Error for QueryError {}
