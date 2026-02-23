//! Acropolis Midnight state module for Caryatid
//! Indexes data required by `midnight-node`
use acropolis_common::{
    caryatid::RollbackWrapper,
    declare_cardano_reader,
    messages::{AddressDeltasMessage, CardanoMessage, Message, StateTransitionMessage},
    state_history::{StateHistory, StateHistoryStore},
    BlockInfo, BlockStatus,
};
use anyhow::{bail, Result};
use caryatid_sdk::{module, Context, Subscription};
use config::Config;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info, warn};
mod epoch_totals;

mod configuration;
mod indexes;
mod state;
mod types;

use crate::configuration::MidnightConfig;
use state::State;

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
        config: MidnightConfig,
        mut address_deltas_reader: AddressDeltasReader,
    ) -> Result<()> {
        loop {
            let mut state = {
                let mut h = history.lock().await;
                h.get_or_init_with(|| State::new(config.clone()))
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
                        let summary = state.handle_new_epoch(blk_info.as_ref())?;
                        info!(
                            epoch = summary.epoch,
                            era = ?summary.era,
                            indexed_night_utxo_creations = summary.indexed_night_utxo_creations,
                            indexed_night_utxo_spends = summary.indexed_night_utxo_spends,
                            "epoch checkpoint"
                        );
                    }

                    state.handle_address_deltas(&blk_info, deltas.as_ref())?;
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
        // Get the config
        let cfg = MidnightConfig::try_load(&config)?;

        // Subscribe to the `AddressDeltasMessage` publisher
        let address_deltas_reader = AddressDeltasReader::new(&context, &config).await?;

        // Initialize unbounded state history for rollback-safe replay.
        let history = Arc::new(Mutex::new(StateHistory::<State>::new(
            "midnight_state",
            StateHistoryStore::Unbounded,
        )));

        // Start the run task
        context.run(async move {
            Self::run(history, cfg, address_deltas_reader)
                .await
                .unwrap_or_else(|e| error!("Failed: {e}"));
        });

        Ok(())
    }
}
