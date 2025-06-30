use std::sync::Arc;

use acropolis_common::{
    ledger_state::LedgerState,
    messages::{Message, SnapshotMessage, SnapshotStateMessage},
};
use anyhow::{Context as AnyhowContext, Result};
use caryatid_sdk::{module, Context, Module};
use config::Config;
use tracing::{error, info};

const DEFAULT_SNAPSHOT_TOPIC: &str = "cardano.snapshot";
const DEFAULT_STARTUP_TOPIC: &str = "cardano.sequence.start";

#[module(
    message_type(Message),
    name = "snapshot-bootstrapper",
    description = "Snapshot Bootstrapper to broadcast state"
)]
pub struct SnapshotBootstrapper;

impl SnapshotBootstrapper {
    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        let file_path = config
            .get_string("snapshot-path")
            .inspect_err(|e| error!("failed to find snapshot-path config: {e}"))?;

        let ledger_state =
            LedgerState::from_directory(file_path).context("failed to load ledger state")?;

        let startup_topic = config
            .get_string("startup-topic")
            .unwrap_or(DEFAULT_STARTUP_TOPIC.to_string());

        let snapshot_topic = config
            .get_string("snapshot-topic")
            .unwrap_or(DEFAULT_SNAPSHOT_TOPIC.to_string());
        info!("Publishing snapshots on '{snapshot_topic}'");

        let mut subscription = context.subscribe(&startup_topic).await?;
        context.clone().run(async move {
            let Ok(_) = subscription.read().await else {
                return;
            };
            info!("Received startup message");

            let spo_state_message = Message::Snapshot(SnapshotMessage::Bootstrap(
                SnapshotStateMessage::SPOState(ledger_state.spo_state),
            ));
            context
                .message_bus
                .publish(&snapshot_topic, Arc::new(spo_state_message))
                .await
                .unwrap_or_else(|e| error!("failed to publish: {e}"));
        });

        Ok(())
    }
}
