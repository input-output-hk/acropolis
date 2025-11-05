use caryatid_sdk::Context;
use serde::Serialize;
use std::sync::Arc;
use crate::app_error::RESTError;
use crate::messages::{Message, RESTResponse};

/// Query state and extract result using a custom extractor function
pub async fn query_state<T, F>(
    context: &Arc<Context<Message>>,
    topic: &str,
    request_msg: Arc<Message>,
    extractor: F,
) -> Result<T, RESTError>
where
    F: FnOnce(Message) -> Result<T, RESTError>,
{
    // Send request and get response
    let raw_msg = context
        .message_bus
        .request(topic, request_msg)
        .await
        .map_err(RESTError::query_failed)?;

    let message = Arc::try_unwrap(raw_msg).unwrap_or_else(|arc| (*arc).clone());

    extractor(message)
}

/// Query state for REST endpoints with automatic JSON serialization
/// Returns None when resource is not found, Some(T) when found
pub async fn rest_query_state<T, F>(
    context: &Arc<Context<Message>>,
    topic: &str,
    request_msg: Arc<Message>,
    extractor: F,
) -> Result<RESTResponse, RESTError>
where
    F: FnOnce(Message) -> Option<Result<Option<T>, RESTError>>,
    T: Serialize,
{
    let result = query_state(context, topic, request_msg, |response| {
        extractor(response).unwrap_or_else(|| {
            Err(RESTError::unexpected_response(&format!(
                "calling {topic}"
            )))
        })
    })
        .await?;

    match result {
        Some(result) => {
            let json = serde_json::to_string_pretty(&result)?;
            Ok(RESTResponse::with_json(200, &json))
        }
        None => Err(RESTError::not_found("Resource")),
    }
}

