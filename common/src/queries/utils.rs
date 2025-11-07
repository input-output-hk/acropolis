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
    let raw_msg = context.message_bus.request(topic, request_msg).await?;
    let message = Arc::try_unwrap(raw_msg).unwrap_or_else(|arc| (*arc).clone());

    extractor(message)
}

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
        extractor(response).ok_or_else(|| {
            QueryError::internal_error(format!(
                "Unexpected response message type while calling {topic}"
            ))
        })?
    })
    .await?;

    let json = serde_json::to_string_pretty(&data)?;
    Ok(RESTResponse::with_json(200, &json))
}
