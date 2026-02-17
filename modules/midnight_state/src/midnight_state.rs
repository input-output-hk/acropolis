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
use tracing::{error, info};
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

const DEFAULT_ENABLED_STATUS: (&str, bool) = ("enable-indexer", false);

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
            // Get a mutable state
            let mut state = {
                let mut h = history.lock().await;
                h.get_or_init_with(State::new)
            };

            match address_deltas_reader.read_with_rollbacks().await? {
                RollbackWrapper::Normal((blk_info, deltas)) => {
                    if blk_info.status == BlockStatus::RolledBack {
                        state = history.lock().await.get_rolled_back_state(blk_info.number);
                    }

                    if blk_info.new_epoch {
                        state.handle_new_epoch()?;
                    }

                    state.handle_address_deltas(&deltas)?;

                    history.lock().await.commit(blk_info.number, state);
                }
                RollbackWrapper::Rollback(_) => {}
            };
        }
    }

    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        // Get configuration
        let is_enabled =
            config.get_bool(DEFAULT_ENABLED_STATUS.0).unwrap_or(DEFAULT_ENABLED_STATUS.1);

        // Early return if indexing is disabled
        if !is_enabled {
            return Ok(());
        }

        // Subscribe to the `AddressDeltasMessage` publisher
        let address_deltas_reader = AddressDeltasReader::new(&context, &config).await?;

        // Initalize unbounded state history
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
