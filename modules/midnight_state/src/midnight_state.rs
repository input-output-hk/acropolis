//! Acropolis Midnight state module for Caryatid
//! Indexes data required by `midnight-node`
use acropolis_common::{
    BlockInfo, BlockStatus,
    caryatid::RollbackWrapper,
    declare_cardano_reader,
    messages::{AddressDeltasMessage, CardanoMessage, Message, StateTransitionMessage},
    state_history::{StateHistory, StateHistoryStore},
};
use anyhow::{Result, bail};
use caryatid_sdk::{Context, Subscription, module};
use config::Config;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info, warn};

mod state;
use state::State;
mod types;

declare_cardano_reader!(
    AddressDeltasReader,
    "address-deltas-topic",
    "cardano.address.deltas",
    AddressDeltas,
    AddressDeltasMessage
);

/// Midnight State module
#[module(
    message_type(Message),
    name = "midnight-state",
    description = "Midnight State Indexer"
)]

pub struct MidnightState;

impl MidnightState {
    async fn run(
        history: Arc<Mutex<StateHistory<State>>>,
        mut address_deltas_reader: AddressDeltasReader,
    ) -> Result<()> {
        loop {
            let mut state = {
                let mut h = history.lock().await;
                h.get_or_init_with(State::new)
            };

            match address_deltas_reader.read_with_rollbacks().await? {
                RollbackWrapper::Normal((blk_info, deltas)) => {
                    if blk_info.status == BlockStatus::RolledBack {
                        state = history.lock().await.get_rolled_back_state(blk_info.number);
                        warn!(
                            block_number = blk_info.number,
                            block_hash = %blk_info.hash,
                            "applying rollback"
                        );
                    }

                    if blk_info.new_epoch {
                        if let Some(summary) = state.handle_new_epoch() {
                            info!(
                                epoch = summary.epoch,
                                era = ?summary.era,
                                blocks = summary.blocks,
                                delta_count = summary.delta_count,
                                created_utxos = summary.created_utxos,
                                spent_utxos = summary.spent_utxos,
                                "epoch checkpoint"
                            );

                            if summary.saw_compact {
                                warn!(
                                    epoch = summary.epoch,
                                    "received compact deltas; expected extended mode for midnight"
                                );
                            }
                        }
                    }

                    state.start_block(blk_info.as_ref());
                    state.handle_address_deltas(deltas.as_ref())?;
                    state.finalise_block(blk_info.as_ref());

                    history.lock().await.commit(blk_info.number, state);
                }
                RollbackWrapper::Rollback(point) => {
                    warn!(
                        rollback_point = ?point,
                        "received rollback wrapper message"
                    );
                }
            };
        }
    }

    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        // Subscribe to the `AddressDeltasMessage` publisher
        let address_deltas_reader = AddressDeltasReader::new(&context, &config).await?;

        // Initialize unbounded state history for rollback-safe replay.
        let history = Arc::new(Mutex::new(StateHistory::<State>::new(
            "midnight_state",
            StateHistoryStore::Unbounded,
        )));

        // Start the run task
        context.run(async move {
            Self::run(history, address_deltas_reader)
                .await
                .unwrap_or_else(|e| error!("Failed: {e}"));
        });

        Ok(())
    }
}
