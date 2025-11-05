use anyhow::Result;
use caryatid_sdk::Context;
use serde::Serialize;
use std::{future::Future, sync::Arc};

use crate::messages::{Message, RESTResponse};

pub async fn query_state<T, F>(
    context: &Arc<Context<Message>>,
    topic: &str,
    request_msg: Arc<Message>,
    extractor: F,
) -> Result<T, anyhow::Error>
where
    F: FnOnce(Message) -> Result<T, anyhow::Error>,
{
    // build message to query
    let raw_msg = context.message_bus.request(topic, request_msg).await?;

    let message = Arc::try_unwrap(raw_msg).unwrap_or_else(|arc| (*arc).clone());

    extractor(message)
}

pub async fn query_state_async<T, F, Fut>(
    context: &Arc<Context<Message>>,
    topic: &str,
    request_msg: Arc<Message>,
    extractor: F,
) -> Result<T, anyhow::Error>
where
    F: FnOnce(Message) -> Fut,
    Fut: Future<Output = Result<T, anyhow::Error>>,
{
    // build message to query
    let raw_msg = context.message_bus.request(topic, request_msg).await?;

    let message = Arc::try_unwrap(raw_msg).unwrap_or_else(|arc| (*arc).clone());

    extractor(message).await
}

/// The outer option in the extractor return value is whether the response was handled by F
pub async fn rest_query_state<T, F>(
    context: &Arc<Context<Message>>,
    topic: &str,
    request_msg: Arc<Message>,
    extractor: F,
) -> Result<RESTResponse>
where
    F: FnOnce(Message) -> Option<Result<Option<T>, anyhow::Error>>,
    T: Serialize,
{
    let result = query_state(context, topic, request_msg, |response| {
        match extractor(response) {
            Some(response) => response,
            None => Err(anyhow::anyhow!(
                "Unexpected response message type while calling {topic}"
            )),
        }
    })
    .await;
    match result {
        Ok(result) => match result {
            Some(result) => match serde_json::to_string(&result) {
                Ok(json) => Ok(RESTResponse::with_json(200, &json)),
                Err(e) => Ok(RESTResponse::with_text(
                    500,
                    &format!("Internal server error while calling {topic}: {e}"),
                )),
            },
            None => Ok(RESTResponse::with_text(404, "Not found")),
        },
        Err(e) => Ok(RESTResponse::with_text(
            500,
            &format!("Internal server error while calling {topic}: {e}"),
        )),
    }
}

pub async fn rest_query_state_async<T, F, Fut>(
    context: &Arc<Context<Message>>,
    topic: &str,
    request_msg: Arc<Message>,
    extractor: F,
) -> Result<RESTResponse>
where
    F: FnOnce(Message) -> Fut,
    Fut: Future<Output = Option<Result<Option<T>, anyhow::Error>>>,
    T: Serialize,
{
    let result = query_state_async(
        context,
        topic,
        request_msg,
        async |response| match extractor(response).await {
            Some(response) => response,
            None => Err(anyhow::anyhow!(
                "Unexpected response message type while calling {topic}"
            )),
        },
    )
    .await;
    match result {
        Ok(result) => match result {
            Some(result) => match serde_json::to_string(&result) {
                Ok(json) => Ok(RESTResponse::with_json(200, &json)),
                Err(e) => Ok(RESTResponse::with_text(
                    500,
                    &format!("Internal server error while calling {topic}: {e}"),
                )),
            },
            None => Ok(RESTResponse::with_text(404, "Not found")),
        },
        Err(e) => Ok(RESTResponse::with_text(
            500,
            &format!("Internal server error while calling {topic}: {e}"),
        )),
    }
}
