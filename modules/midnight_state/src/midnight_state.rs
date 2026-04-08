//! Acropolis Midnight state module for Caryatid
//! Indexes data required by `midnight-node`
use acropolis_common::{
    caryatid::{PrimaryRead, RollbackWrapper},
    declare_cardano_reader,
    messages::{
        AddressDeltasMessage, CardanoMessage, Message, ProtocolParamsMessage,
        StateTransitionMessage,
    },
    protocol_params::Nonce,
    state_history::{StateHistory, StateHistoryStore, StoreType},
};
use anyhow::{bail, Result};
use caryatid_sdk::{module, Context, Subscription};
use config::Config;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info, warn};
mod epoch_totals;

mod configuration;
mod grpc;
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

declare_cardano_reader!(
    EpochNonceReader,
    "epoch-nonce-topic",
    "cardano.epoch.nonce",
    EpochNonce,
    Option<Nonce>
);

declare_cardano_reader!(
    ProtocolParamsReader,
    "publish-parameters-topic",
    "cardano.protocol.parameters",
    ProtocolParams,
    ProtocolParamsMessage
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
        mut epoch_nonce_reader: EpochNonceReader,
        mut protocol_params_reader: ProtocolParamsReader,
    ) -> Result<()> {
        loop {
            let mut state = {
                let mut h = history.lock().await;
                h.get_or_init_with(|| State::new(config.clone()))
            };

            let primary =
                PrimaryRead::from_read(address_deltas_reader.read_with_rollbacks().await?);

            if primary.is_rollback() {
                state = history.lock().await.get_rolled_back_state(primary.block_info().number);
                warn!(
                    block_number = primary.block_info().number,
                    block_hash = %primary.block_info().hash,
                    "applying rollback"
                );
            }

            if primary.should_read_epoch_messages() {
                match protocol_params_reader.read_with_rollbacks().await? {
                    RollbackWrapper::Normal((_, protocol_params)) => {
                        state.update_stable_block_window_bounds(&protocol_params.params)?;
                    }
                    RollbackWrapper::Rollback(_) => {}
                }
                match epoch_nonce_reader.read_with_rollbacks().await? {
                    RollbackWrapper::Normal((_, nonce)) => {
                        state.handle_new_epoch(
                            primary.block_info().as_ref(),
                            nonce.as_ref().clone(),
                        );
                    }
                    RollbackWrapper::Rollback(_) => {}
                }
            }

            if let Some(deltas) = primary.message() {
                state.handle_address_deltas(primary.block_info(), deltas.as_ref())?;
                history.lock().await.commit(primary.block_info().number, state);
            }
        }
    }

    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        // Get the config
        let cfg = MidnightConfig::try_load(&config)?;
        let addr = cfg.grpc_socket_addr()?;

        // Subscribe to the `AddressDeltasMessage` publisher
        let address_deltas_reader = AddressDeltasReader::new(&context, &config).await?;
        let epoch_nonce_reader = EpochNonceReader::new(&context, &config).await?;
        let protocol_params_reader = ProtocolParamsReader::new(&context, &config).await?;

        // Initialize unbounded state history for rollback-safe replay.
        let history = Arc::new(Mutex::new(StateHistory::<State>::new(
            "midnight_state",
            StateHistoryStore::Unbounded,
            &config,
            StoreType::Block,
        )));
        let grpc_history = history.clone();
        let grpc_context = context.clone();
        // Start the main run loop
        context.run(async move {
            Self::run(
                history,
                cfg,
                address_deltas_reader,
                epoch_nonce_reader,
                protocol_params_reader,
            )
            .await
            .unwrap_or_else(|e| error!("Failed: {e}"));
        });

        // Start the gRPC server
        context.run(async move {
            crate::grpc::server::run(grpc_history, grpc_context, addr)
                .await
                .unwrap_or_else(|e| error!("gRPC server failed: {e}"));
        });

        Ok(())
    }
}
