use caryatid_sdk::Context;
use serde::Serialize;
use std::sync::Arc;

use crate::messages::{Message, RESTResponse};
use crate::queries::errors::QueryError;
use crate::rest_error::RESTError;

/// Query state and get typed result or QueryError
/// This is the low-level building block for handlers that need to do more processing
pub async fn query_state<T, F>(
    context: &Arc<Context<Message>>,
    topic: &str,
    request_msg: Arc<Message>,
    extractor: F,
) -> Result<T, QueryError>
where
    F: FnOnce(Message) -> Result<T, QueryError>,
{
    let raw_msg = context
        .message_bus
        .request(topic, request_msg)
        .await
        .map_err(|e| QueryError::query_failed(e.to_string()))?;

    let message = Arc::try_unwrap(raw_msg).unwrap_or_else(|arc| (*arc).clone());

    extractor(message)
}

/// Query state and return a REST response directly
/// This is a convenience function for simple handlers that just fetch and serialize data
pub async fn rest_query_state<T, F>(
    context: &Arc<Context<Message>>,
    topic: &str,
    request_msg: Arc<Message>,
    extractor: F,
) -> Result<RESTResponse, RESTError>
where
    F: FnOnce(Message) -> Option<Result<T, QueryError>>,
    T: Serialize,
{
    let data = query_state(context, topic, request_msg, |response| {
        extractor(response).unwrap_or_else(|| {
            Err(QueryError::query_failed(format!(
                "Unexpected response message type from {topic}"
            )))
        })
    })
    .await?; // QueryError auto-converts to RESTError via From trait

    let json = serde_json::to_string_pretty(&data)?; // Uses From<serde_json::Error> for RESTError
    Ok(RESTResponse::with_json(200, &json))
}
