use acropolis_common::{
    ledger_state::LedgerState,
    messages::{
        CardanoMessage, Message, RawTxsMessage, SnapshotDumpMessage, SnapshotMessage,
        SnapshotStateMessage,
    },
    BlockHash, BlockInfo, BlockIntent, BlockStatus, Era,
};
use anyhow::{Context as AnyhowContext, Result};
use caryatid_sdk::{module, Context};
use config::Config;
use std::sync::Arc;

const DEFAULT_TRANSACTIONS_TOPIC: &str = "cardano.txs";
const DEFAULT_SNAPSHOT_TOPIC: &str = "cardano.snapshot";

#[module(
    message_type(Message),
    name = "test-module",
    description = "Test module that orchestrates sending blocks and comparing output state to expected state"
)]
pub struct TestModule;

impl TestModule {
    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        // TODO: we need to somehow get test data into the context so this module can unpack it all
        // Currently just *assuming* it exists in the context as a string
        let transactions_topic = config
            .get_string("transactions-topic")
            .unwrap_or(DEFAULT_TRANSACTIONS_TOPIC.to_string());

        let snapshot_topic =
            config.get_string("snapshot-topic").unwrap_or(DEFAULT_SNAPSHOT_TOPIC.to_string());

        let tx_bytes = hex::decode(
            config.get_string("transaction").with_context(|| "no transaction provided for test")?,
        )
        .with_context(|| "failed to decode transaction hex")?;

        let transaction_message = Message::Cardano((
            BlockInfo {
                status: BlockStatus::Volatile,
                intent: BlockIntent::Apply,
                slot: 1,
                number: 1,
                hash: BlockHash::default(),
                epoch: 1,
                epoch_slot: 1,
                new_epoch: false,
                is_new_era: false,
                timestamp: 1,
                tip_slot: None,
                era: Era::Conway,
            },
            CardanoMessage::ReceivedTxs(RawTxsMessage {
                txs: vec![tx_bytes],
            }),
        ));

        context
            .message_bus
            .publish(&transactions_topic, Arc::new(transaction_message))
            .await
            .with_context(|| "failed to publish transactions message")?;

        let ledger_state_directory =
            config.get_string("final-state").with_context(|| "no final state provided for test")?;

        let expected_final_state = LedgerState::from_directory(ledger_state_directory)?;

        // TODO: We need to enforce a timeout on this logic, at the end of which we validate the state
        let mut snapshot_subscription = context.subscribe(&snapshot_topic).await?;
        let dump_message = Message::Snapshot(SnapshotMessage::DumpRequest(SnapshotDumpMessage {
            block_height: 1,
        }));

        context
            .message_bus
            .publish(&snapshot_topic, Arc::new(dump_message))
            .await
            .with_context(|| "failed to publish dump message")?;

        context.clone().run(async move {
            loop {
                let Ok((_, message)) = snapshot_subscription.read().await else {
                    return;
                };

                if let Message::Snapshot(SnapshotMessage::Dump(SnapshotStateMessage::SPOState(
                    spo_bootstrap,
                ))) = message.as_ref()
                {
                    assert_eq!(&expected_final_state.spo_state, &spo_bootstrap.spo_state);
                    super::signal_test_completion();
                }
            }
        });

        Ok(())
    }
}
