use caryatid_sdk::Context;
use serde::Serialize;
use std::sync::Arc;

use crate::messages::{Message, RESTResponse};
use crate::queries::errors::QueryError;
use crate::rest_error::RESTError;

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
        .map_err(|e| QueryError::internal_error(format!("Failed to query '{topic}': {e:#}")))?;

    let message = Arc::try_unwrap(raw_msg).unwrap_or_else(|arc| (*arc).clone());

    extractor(message)
}

/// The outer option in the extractor return value is whether the response was handled by F
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
            Err(QueryError::internal_error(format!(
                "Unexpected response message type from {topic}"
            )))
        })
    })
    .await?;

    let json = serde_json::to_string_pretty(&data)?;
    Ok(RESTResponse::with_json(200, &json))
}
