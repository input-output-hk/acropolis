use anyhow::Error as AnyhowError;
use std::fmt;
use std::num::ParseIntError;
use caryatid_module_rest_server::messages::RESTResponse;

/// Standard error types for the application
#[derive(Debug)]
pub enum RESTError {
    BadRequest(String),
    NotFound(String),
    InternalServerError(String),
    NotImplemented(String),
}

impl RESTError {
    /// Get the HTTP status code for this error
    pub fn status_code(&self) -> u16 {
        match self {
            RESTError::BadRequest(_) => 400,
            RESTError::NotFound(_) => 404,
            RESTError::InternalServerError(_) => 500,
            RESTError::NotImplemented(_) => 501,
        }
    }

    /// Get the error message
    pub fn message(&self) -> String {
        match self {
            RESTError::BadRequest(msg) => msg.clone(),
            RESTError::NotFound(msg) => msg.clone(),
            RESTError::InternalServerError(msg) => msg.clone(),
            RESTError::NotImplemented(msg) => msg.clone(),
        }
    }

    /// Parameter missing error
    pub fn param_missing(param_name: &str) -> Self {
        RESTError::BadRequest(format!("{} parameter is missing", param_name))
    }

    /// Feature hasn't been implemented error
    pub fn not_implemented(feature: &str) -> Self {
        RESTError::NotImplemented(format!("{} not yet implemented", feature))
    }

    /// Storage disabled error
    pub fn storage_disabled(storage_type: &str) -> Self {
        RESTError::NotImplemented(format!("{} storage is disabled in config", storage_type))
    }

    /// Invalid hex string error
    pub fn invalid_hex() -> Self {
        RESTError::BadRequest("Invalid hex string".to_string())
    }

    /// Invalid parameter error
    pub fn invalid_param(param_name: &str, reason: &str) -> Self {
        RESTError::BadRequest(format!("Invalid {}: {}", param_name, reason))
    }


    /// Resource wasn't found error
    pub fn not_found(resource: &str) -> Self {
        RESTError::NotFound(format!("{} not found", resource))
    }



    /// Unexpected response error
    pub fn unexpected_response(context: &str) -> Self {
        RESTError::InternalServerError(format!("Unexpected response while {}", context))
    }

    /// Query failed error
    pub fn query_failed(error: impl fmt::Display) -> Self {
        RESTError::InternalServerError(format!("Query failed: {}", error))
    }

    /// Serialization failed error
    pub fn serialization_failed(what: &str, error: impl fmt::Display) -> Self {
        RESTError::InternalServerError(format!("Failed to serialize {}: {}", what, error))
    }

    /// Encoding failed error
    pub fn encoding_failed(what: &str) -> Self {
        RESTError::InternalServerError(format!("Failed to encode {}", what))
    }
}

impl fmt::Display for RESTError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message())
    }
}

impl std::error::Error for RESTError {}

/// Convert RESTError to RESTResponse
impl From<RESTError> for RESTResponse {
    fn from(error: RESTError) -> Self {
        RESTResponse::with_text(error.status_code(), &error.message())
    }
}

/// Convert anyhow::Error to RESTError (default to 500)
impl From<AnyhowError> for RESTError {
    fn from(error: AnyhowError) -> Self {
        RESTError::InternalServerError(error.to_string())
    }
}

/// Convert ParseIntError to RESTError (400 Bad Request)
impl From<ParseIntError> for RESTError {
    fn from(error: ParseIntError) -> Self {
        RESTError::BadRequest(error.to_string())
    }
}

/// Convert hex decode errors to RESTError (400 Bad Request)
impl From<hex::FromHexError> for RESTError {
    fn from(error: hex::FromHexError) -> Self {
        RESTError::BadRequest(format!("Invalid hex string: {}", error))
    }
}

/// Convert bech32 decode errors to RESTError (400 Bad Request)
impl From<bech32::DecodeError> for RESTError {
    fn from(error: bech32::DecodeError) -> Self {
        RESTError::BadRequest(format!("Invalid bech32 encoding: {}", error))
    }
}

/// Convert bech32 encode errors to RESTError (500 Internal Server Error)
impl From<bech32::EncodeError> for RESTError {
    fn from(error: bech32::EncodeError) -> Self {
        RESTError::InternalServerError(format!("Failed to encode bech32: {}", error))
    }
}

/// Convert serde_json errors to RESTError (500 Internal Server Error)
impl From<serde_json::Error> for RESTError {
    fn from(error: serde_json::Error) -> Self {
        RESTError::InternalServerError(format!("JSON serialization failed: {}", error))
    }
}

pub type AppResult<T> = Result<T, RESTError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bad_request_error() {
        let error = RESTError::BadRequest("Invalid parameter".to_string());
        assert_eq!(error.status_code(), 400);
        assert_eq!(error.message(), "Invalid parameter");
    }

    #[test]
    fn test_not_found_error() {
        let error = RESTError::NotFound("Account not found".to_string());
        assert_eq!(error.status_code(), 404);
        assert_eq!(error.message(), "Account not found");
    }

    #[test]
    fn test_internal_error() {
        let error = RESTError::InternalServerError("Database connection failed".to_string());
        assert_eq!(error.status_code(), 500);
        assert_eq!(error.message(), "Database connection failed");
    }

    #[test]
    fn test_from_anyhow() {
        let anyhow_err = anyhow::anyhow!("Something went wrong");
        let app_error = RESTError::from(anyhow_err);
        assert_eq!(app_error.status_code(), 500);
    }

    #[test]
    fn test_from_parse_int_error() {
        let result: Result<u64, _> = "not_a_number".parse();
        let app_error: RESTError = result.unwrap_err().into();
        assert_eq!(app_error.status_code(), 400);
    }

    #[test]
    fn test_from_hex_error() {
        let result = hex::decode("not_hex_gg");
        let app_error: RESTError = result.unwrap_err().into();
        assert_eq!(app_error.status_code(), 400);
    }

    #[test]
    fn test_to_rest_response() {
        let error = RESTError::BadRequest("Invalid stake address".to_string());
        let response: RESTResponse = error.into();
        assert_eq!(response.code, 400);
        assert_eq!(response.body, "Invalid stake address");
    }

    #[test]
    fn test_convenience_constructors() {
        assert_eq!(RESTError::invalid_hex().status_code(), 400);
        assert_eq!(RESTError::not_found("Asset").message(), "Asset not found");
        assert_eq!(RESTError::not_implemented("Feature").status_code(), 501);
    }
}