//! Acropolis Stake Delta Filter module
//! Reads address deltas and filters out only stake addresses from it; also resolves pointer addresses.

use acropolis_common::{
    caryatid::{PrimaryRead, RollbackWrapper, ValidationContext},
    declare_cardano_reader,
    messages::{
        AddressDeltasMessage, CardanoMessage, GenesisCompleteMessage, Message,
        StateTransitionMessage, TxCertificatesMessage,
    },
    state_history::{StateHistory, StateHistoryStore},
    NetworkId,
};
use anyhow::{bail, Result};
use caryatid_sdk::{module, Context, Subscription};
use config::Config;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info};

declare_cardano_reader!(
    AddressDeltasReader,
    "subscription-address-delta-topic",
    "cardano.address.deltas",
    AddressDeltas,
    AddressDeltasMessage
);
declare_cardano_reader!(
    CertsReader,
    "subscription-certificates-topic",
    "cardano.certificates",
    TxCertificates,
    TxCertificatesMessage
);
declare_cardano_reader!(
    GenesisReader,
    "genesis-subscribe-topic",
    "cardano.sequence.bootstrapped",
    GenesisComplete,
    GenesisCompleteMessage
);

/// Stake Delta Filter module
#[module(
    message_type(Message),
    name = "stake-delta-filter",
    description = "Retrieves stake addresses from address deltas"
)]
pub struct StakeDeltaFilter;

mod configuration;
mod predefined;
mod queries;
mod state;
mod utils;

use state::{DeltaPublisher, State};
use utils::{process_message, CacheMode, PointerCache, Tracker};

use crate::{
    configuration::StakeDeltaFilterParams,
    queries::{register_query_handler, register_query_handler_stateful},
    utils::get_network_name,
};

impl StakeDeltaFilter {
    async fn run(
        context: Arc<Context<Message>>,
        mut genesis_reader: GenesisReader,
        address_delta_reader: AddressDeltasReader,
        certs_reader: CertsReader,
        publisher: DeltaPublisher,
        params: Arc<StakeDeltaFilterParams>,
    ) -> Result<()> {
        let network_id = match genesis_reader.read_with_rollbacks().await? {
            RollbackWrapper::Normal((_, genesis_values)) => genesis_values.values.network_id(),
            RollbackWrapper::Rollback(_) => {
                bail!("Unexpected rollback while reading genesis values")
            }
        };

        match params.as_ref().cache_mode {
            CacheMode::Predefined => {
                let cache = PointerCache::try_load_predefined(&get_network_name(network_id))?;
                register_query_handler(&context, &context.config, cache.clone());

                Self::stateless_run(
                    cache,
                    publisher,
                    address_delta_reader,
                    certs_reader,
                    params.is_snapshot_mode,
                )
                .await?;
            }
            CacheMode::Read => {
                let cache =
                    PointerCache::try_load(&params.get_cache_file_name(".json", &network_id)?)?;
                register_query_handler(&context, &context.config, cache.clone());

                Self::stateless_run(
                    cache,
                    publisher,
                    address_delta_reader,
                    certs_reader,
                    params.is_snapshot_mode,
                )
                .await?;
            }

            CacheMode::WriteIfAbsent => {
                match PointerCache::try_load(&params.get_cache_file_name(".json", &network_id)?) {
                    Ok(cache) => {
                        register_query_handler(&context, &context.config, cache.clone());

                        Self::stateless_run(
                            cache,
                            publisher,
                            address_delta_reader,
                            certs_reader,
                            params.is_snapshot_mode,
                        )
                        .await?;
                    }
                    Err(e) => {
                        info!("Cannot load cache: {}, building from scratch", e);
                        let history = Arc::new(Mutex::new(StateHistory::<State>::new(
                            "stake_delta_filter",
                            StateHistoryStore::default_block_store(),
                        )));
                        register_query_handler_stateful(&context, &context.config, history.clone());

                        Self::stateful_run(
                            history,
                            certs_reader,
                            address_delta_reader,
                            publisher,
                            network_id,
                            params,
                            context,
                        )
                        .await?;
                    }
                };
            }

            CacheMode::Write => {
                let history = Arc::new(Mutex::new(StateHistory::<State>::new(
                    "stake_delta_filter",
                    StateHistoryStore::default_block_store(),
                )));
                register_query_handler_stateful(&context, &context.config, history.clone());

                Self::stateful_run(
                    history,
                    certs_reader,
                    address_delta_reader,
                    publisher,
                    network_id,
                    params,
                    context,
                )
                .await?;
            }
        }

        Ok(())
    }

    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        let genesis_reader = GenesisReader::new(&context, &config).await?;
        let address_delta_reader = AddressDeltasReader::new(&context, &config).await?;
        let certs_reader = CertsReader::new(&context, &config).await?;

        let params = StakeDeltaFilterParams::init(config.clone())?;
        let publisher = DeltaPublisher::new(context.clone(), params.clone());

        // Start run task
        let run_ctx = context.clone();
        context.run(async move {
            Self::run(
                run_ctx,
                genesis_reader,
                address_delta_reader,
                certs_reader,
                publisher,
                params,
            )
            .await
            .unwrap_or_else(|e| error!("Failed: {e}"));
        });

        Ok(())
    }

    async fn stateless_run(
        cache: Arc<PointerCache>,
        mut publisher: DeltaPublisher,
        mut address_delta_reader: AddressDeltasReader,
        mut certs_reader: CertsReader,
        is_snapshot_mode: bool,
    ) -> Result<()> {
        if !is_snapshot_mode {
            match address_delta_reader.read_with_rollbacks().await? {
                RollbackWrapper::Normal(_) => {}
                RollbackWrapper::Rollback(_) => {
                    bail!("Unexpected rollback while reading initial deltas message");
                }
            }
        }
        loop {
            let primary = PrimaryRead::from_read(address_delta_reader.read_with_rollbacks().await?);

            // Read certs to keep messages aligned
            certs_reader.read_with_rollbacks().await?;

            if let Some(address_deltas) = primary.message() {
                let msg = process_message(&cache, address_deltas, primary.block_info(), None);
                publisher
                    .publish(primary.block_info(), msg)
                    .await
                    .unwrap_or_else(|e| error!("Publish error: {e}"))
            } else if let Some(message) = primary.rollback_message() {
                // Publish rollbacks downstream
                publisher
                    .publish_message(message.clone())
                    .await
                    .unwrap_or_else(|e| error!("Publish error: {e}"));
            }
        }
    }

    async fn stateful_run(
        history: Arc<Mutex<StateHistory<State>>>,
        mut certs_reader: CertsReader,
        mut address_deltas_reader: AddressDeltasReader,
        mut publisher: DeltaPublisher,
        network: NetworkId,
        params: Arc<StakeDeltaFilterParams>,
        context: Arc<Context<Message>>,
    ) -> Result<()> {
        if !params.is_snapshot_mode {
            match address_deltas_reader.read_with_rollbacks().await? {
                RollbackWrapper::Normal(_) => {}
                RollbackWrapper::Rollback(_) => {
                    bail!("Unexpected rollback while reading initial deltas message");
                }
            }
        }
        loop {
            let mut ctx =
                ValidationContext::new(&context, &params.validation_topic, "stake_delta_filter");

            let mut state = history.lock().await.get_or_init_with(|| State::new(params.clone()));

            let primary = PrimaryRead::from_sync(
                &mut ctx,
                "certs",
                certs_reader.read_with_rollbacks().await,
            )?;

            if let Some(tx_cert_msg) = primary.message() {
                state
                    .handle_certs(primary.block_info(), tx_cert_msg)
                    .await
                    .inspect_err(|e| error!("Messaging handling error: {e}"))
                    .ok();
            } else if let Some(message) = primary.rollback_message() {
                // Handle rollbacks on this topic only
                state = history.lock().await.get_rolled_back_state(primary.block_info().number);

                // Publish rollbacks downstream
                publisher.publish_message(message.clone()).await?;
            }

            match ctx.consume_sync(
                "address deltas",
                address_deltas_reader.read_with_rollbacks().await,
            )? {
                RollbackWrapper::Normal((block_info, deltas)) => {
                    let msg = state.handle_deltas(&block_info, &deltas);
                    publisher.publish(&block_info, msg).await?;
                }
                RollbackWrapper::Rollback(_) => {}
            }

            if primary.message().is_some() {
                let block_info = primary.block_info();
                state.save(&network)?;
                history.lock().await.commit(block_info.number, state);

                if primary.do_validation() {
                    ctx.publish().await;
                }
            }
        }
    }
}
