use std::sync::Arc;

use acropolis_common::commands::chain_sync::ChainSyncCommand;
use acropolis_common::messages::{Command, Message};
use acropolis_common::Point;
use anyhow::Result;
use caryatid_sdk::Context;
use tracing::info;

pub async fn change_sync_point(
    point: Point,
    context: Arc<Context<Message>>,
    topic: &String,
) -> Result<()> {
    let msg = Message::Command(Command::ChainSync(ChainSyncCommand::FindIntersect(
        point.clone(),
    )));
    context.publish(topic, Arc::new(msg)).await?;
    info!(
        "Publishing sync command on {} for slot {}",
        topic,
        point.slot()
    );

    Ok(())
}
