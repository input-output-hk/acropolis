use anyhow::Result;
use caryatid_sdk::Context;
use std::sync::Arc;

use crate::messages::Message;

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

    Ok(extractor(message)?)
}
