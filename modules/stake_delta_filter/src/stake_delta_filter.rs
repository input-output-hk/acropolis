//! Acropolis Stake Delta Filter module
//! Reads address deltas and filters out only stake addresses from it; also resolves pointer addresses.

use acropolis_common::{
    NetworkId, caryatid::{PrimaryRead, RollbackWrapper, ValidationContext}, configuration::StartupMode, declare_cardano_reader, messages::{
        AddressDeltasMessage, CardanoMessage, Message, StateQuery, StateQueryResponse,
        StateTransitionMessage, TxCertificatesMessage,
    }, queries::{
        errors::QueryError,
        stake_deltas::{
            DEFAULT_STAKE_DELTAS_QUERY_TOPIC, StakeDeltaQuery, StakeDeltaQueryResponse
        },
    }, state_history::{StateHistory, StateHistoryStore, StoreType}
};
use anyhow::{anyhow, bail, Result};
use caryatid_sdk::{module, Context, Subscription};
use config::Config;
use serde::Deserialize;
use std::{path::Path, sync::Arc};
use tokio::sync::Mutex;
use tracing::{error, info, info_span, Instrument};

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

const DEFAULT_STAKE_ADDRESS_DELTA_TOPIC: (&str, &str) =
    ("publishing-stake-delta-topic", "cardano.stake.deltas");
const DEFAULT_VALIDATION_TOPIC: (&str, &str) = (
    "publishing-validation-topic",
    "cardano.validation.stake.filter",
);

/// Directory to put cached shelley address pointers into. Depending on the address
/// cache mode, these cached pointers can be used instead of tracking current pointer
/// values in blockchain (which can be quite resource-consuming).
const DEFAULT_CACHE_DIR: (&str, &str) = ("cache-dir", "cache");

/// Cache mode: use built-in; always build cache; always read cache (error if missing);
/// build if missing, read otherwise.
const DEFAULT_CACHE_MODE: (&str, CacheMode) = ("cache-mode", CacheMode::Predefined);

/// Cache remembers all stake addresses that could potentially be referenced by pointers. However
/// only a few addressed are actually referenced by pointers in real blockchain.
/// `true` means that all possible addresses should be written to disk (as potential pointers).
/// `false` means that only addresses used in actual pointers should be written to disk.
const DEFAULT_WRITE_FULL_CACHE: (&str, bool) = ("write-full-cache", false);

/// Network: currently only Main/Test. Parameter is necessary to distinguish caches.
const DEFAULT_NETWORK: (&str, NetworkId) = ("network", NetworkId::Mainnet);

/// Stake Delta Filter module
#[module(
    message_type(Message),
    name = "stake-delta-filter",
    description = "Retrieves stake addresses from address deltas"
)]
pub struct StakeDeltaFilter;

mod predefined;
mod state;
mod utils;

use state::{DeltaPublisher, State};
use utils::{process_message, CacheMode, PointerCache, Tracker};

#[derive(Clone, Debug, Default, serde::Serialize)]
struct StakeDeltaFilterParams {
    stake_address_delta_topic: String,
    validation_topic: String,
    network: NetworkId,

    cache_dir: String,
    cache_mode: CacheMode,
    write_full_cache: bool,
}

impl StakeDeltaFilterParams {
    fn get_cache_file_name(&self, modifier: &str) -> Result<String> {
        let path = Path::new(&self.cache_dir);
        let full = path.join(format!("{}{}", self.get_network_name(), modifier).to_lowercase());
        let str =
            full.to_str().ok_or_else(|| anyhow!("Cannot produce cache file name".to_string()))?;
        Ok(str.to_string())
    }

    fn get_network_name(&self) -> String {
        format!("{:?}", self.network)
    }

    fn conf(config: &Arc<Config>, keydef: (&str, &str)) -> String {
        config.get_string(keydef.0).unwrap_or(keydef.1.to_string())
    }

    fn conf_enum<'a, T: Deserialize<'a>>(config: &Arc<Config>, keydef: (&str, T)) -> Result<T> {
        if config.get_string(keydef.0).is_ok() {
            config.get::<T>(keydef.0).map_err(|e| anyhow!("cannot parse {} value: {e}", keydef.0))
        } else {
            Ok(keydef.1)
        }
    }

    fn init(cfg: Arc<Config>) -> Result<Arc<Self>> {
        let params = Self {
            stake_address_delta_topic: Self::conf(&cfg, DEFAULT_STAKE_ADDRESS_DELTA_TOPIC),
            validation_topic: Self::conf(&cfg, DEFAULT_VALIDATION_TOPIC),
            cache_dir: Self::conf(&cfg, DEFAULT_CACHE_DIR),
            cache_mode: Self::conf_enum::<CacheMode>(&cfg, DEFAULT_CACHE_MODE)?,
            write_full_cache: Self::conf_enum::<bool>(&cfg, DEFAULT_WRITE_FULL_CACHE)?,
            network: Self::conf_enum::<NetworkId>(&cfg, DEFAULT_NETWORK)?,
        };

        info!("Cache mode {:?}", params.cache_mode);
        if params.cache_mode == CacheMode::Read {
            if !Path::new(&params.cache_dir).try_exists()? {
                return Err(anyhow!(
                    "Pointer cache directory '{}' does not exist.",
                    params.cache_dir
                ));
            }
            info!("Reading (writing) caches from (to) {}", params.cache_dir);
        } else if params.cache_mode != CacheMode::Predefined {
            std::fs::create_dir_all(&params.cache_dir)?;
        }

        Ok(Arc::new(params))
    }
}

impl StakeDeltaFilter {
    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        let address_delta_reader = AddressDeltasReader::new(&context, &config).await?;
        let params = StakeDeltaFilterParams::init(config.clone())?;
        let is_snapshot_mode = StartupMode::from_config(config.as_ref()).is_snapshot();
        let cache_path = params.get_cache_file_name(".json")?;
        let publisher = DeltaPublisher::new(context.clone(), params.clone());

        match params.cache_mode {
            CacheMode::Predefined => {
                Self::stateless_init(
                    PointerCache::try_load_predefined(&params.get_network_name())?,
                    context,
                    publisher,
                    address_delta_reader,
                    is_snapshot_mode,
                )
                .await
            }

            CacheMode::Read => {
                Self::stateless_init(
                    PointerCache::try_load(&cache_path)?,
                    context,
                    publisher,
                    address_delta_reader,
                    is_snapshot_mode,
                )
                .await
            }

            CacheMode::WriteIfAbsent => match PointerCache::try_load(&cache_path) {
                Ok(cache) => {
                    Self::stateless_init(
                        cache,
                        context,
                        publisher,
                        address_delta_reader,
                        is_snapshot_mode,
                    )
                    .await
                }
                Err(e) => {
                    info!("Cannot load cache: {}, building from scratch", e);
                    let certs_reader = CertsReader::new(&context, &config).await?;
                    Self::stateful_init(
                        params,
                        context,
                        certs_reader,
                        address_delta_reader,
                        publisher,
                        is_snapshot_mode,
                    )
                    .await
                }
            },

            CacheMode::Write => {
                let certs_reader = CertsReader::new(&context, &config).await?;
                Self::stateful_init(
                    params,
                    context,
                    certs_reader,
                    address_delta_reader,
                    publisher,
                    is_snapshot_mode,
                )
                .await
            }
        }
    }

    /// Register a query handler that resolves pointer addresses using the given cache.
    fn register_query_handler(
        context: &Arc<Context<Message>>,
        config: &Arc<Config>,
        cache: Arc<PointerCache>,
    ) {
        let query_topic = config
            .get_string(DEFAULT_STAKE_DELTAS_QUERY_TOPIC.0)
            .unwrap_or(DEFAULT_STAKE_DELTAS_QUERY_TOPIC.1.to_string());
        info!("Registering query handler on '{query_topic}'");

        context.handle(&query_topic, move |message| {
            let cache = cache.clone();
            async move {
                let Message::StateQuery(StateQuery::StakeDeltas(query)) = message.as_ref() else {
                    return Arc::new(Message::StateQueryResponse(
                        StateQueryResponse::StakeDeltas(StakeDeltaQueryResponse::Error(
                            QueryError::internal_error("Invalid message for stake-delta-filter"),
                        )),
                    ));
                };

                let response = match query {
                    StakeDeltaQuery::ResolvePointers { pointers } => {
                        let mut resolved = std::collections::HashMap::new();
                        for ptr in pointers {
                            if let Some(Some(stake_addr)) = cache.decode_pointer(ptr) {
                                resolved.insert(ptr.clone(), stake_addr.clone());
                            }
                        }
                        StakeDeltaQueryResponse::ResolvedPointers(resolved)
                    }
                };

                Arc::new(Message::StateQueryResponse(
                    StateQueryResponse::StakeDeltas(response),
                ))
            }
        });
    }

    async fn stateless_init(
        cache: Arc<PointerCache>,
        context: Arc<Context<Message>>,
        publisher: DeltaPublisher,
        address_delta_reader: AddressDeltasReader,
        is_snapshot_mode: bool,
    ) -> Result<()> {
        info!("Stateless init: using stake pointer cache");

        // Register query handler for pointer resolution
        Self::register_query_handler(&context, &context.config, cache.clone());

        context.clone().run(Self::stateless_run(
            cache,
            publisher,
            address_delta_reader,
            is_snapshot_mode,
        ));

        Ok(())
    }

    async fn stateless_run(
        cache: Arc<PointerCache>,
        mut publisher: DeltaPublisher,
        mut address_delta_reader: AddressDeltasReader,
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

    /// Register a query handler for stateful mode, where the cache is behind a Mutex.
    fn register_query_handler_stateful(
        context: &Arc<Context<Message>>,
        config: &Arc<Config>,
        history: Arc<Mutex<StateHistory<State>>>,
    ) {
        let query_topic = config
            .get_string(DEFAULT_STAKE_DELTAS_QUERY_TOPIC.0)
            .unwrap_or(DEFAULT_STAKE_DELTAS_QUERY_TOPIC.1.to_string());
        info!("Registering stateful query handler on '{query_topic}'");

        context.handle(&query_topic, move |message| {
            let history = history.clone();
            async move {
                let Message::StateQuery(StateQuery::StakeDeltas(query)) = message.as_ref() else {
                    return Arc::new(Message::StateQueryResponse(
                        StateQueryResponse::StakeDeltas(StakeDeltaQueryResponse::Error(
                            QueryError::internal_error("Invalid message for stake-delta-filter"),
                        )),
                    ));
                };

                let locked = history.lock().await;
                let state = match locked.current() {
                    Some(state) => state,
                    None => {
                        return Arc::new(Message::StateQueryResponse(
                            StateQueryResponse::StakeDeltas(StakeDeltaQueryResponse::Error(
                                QueryError::internal_error(
                                    "Invalid message for stake-delta-filter",
                                ),
                            )),
                        ))
                    }
                };

                let response = match query {
                    StakeDeltaQuery::ResolvePointers { pointers } => {
                        let mut resolved = std::collections::HashMap::new();
                        for ptr in pointers {
                            if let Some(Some(stake_addr)) = state.pointer_cache.decode_pointer(ptr)
                            {
                                resolved.insert(ptr.clone(), stake_addr.clone());
                            }
                        }
                        StakeDeltaQueryResponse::ResolvedPointers(resolved)
                    }
                };

                Arc::new(Message::StateQueryResponse(
                    StateQueryResponse::StakeDeltas(response),
                ))
            }
        });
    }

    async fn stateful_init(
        params: Arc<StakeDeltaFilterParams>,
        context: Arc<Context<Message>>,
        certs_reader: CertsReader,
        address_deltas_reader: AddressDeltasReader,
        publisher: DeltaPublisher,
        is_snapshot_mode: bool,
    ) -> Result<()> {
        info!("Stateful init: creating stake pointer cache");

        // State
        let history = Arc::new(Mutex::new(StateHistory::<State>::new(
            "stake_delta_filter",
            StateHistoryStore::default_block_store(),
            &context.config,
            StoreType::Block,
        )));
        let history_tick = history.clone();

        // Register query handler for pointer resolution (stateful)
        Self::register_query_handler_stateful(&context, &context.config, history.clone());

        let context_run = context.clone();
        context.run(Self::stateful_run(
            history,
            certs_reader,
            address_deltas_reader,
            publisher,
            params,
            context_run,
            is_snapshot_mode,
        ));

        // Ticker to log stats
        let mut subscription = context.subscribe("clock.tick").await?;
        context.run(async move {
            loop {
                let Ok((_, message)) = subscription.read().await else {
                    return;
                };
                if let Message::Clock(message) = message.as_ref() {
                    if (message.number % 60) == 0 {
                        let span = info_span!("stake_delta_filter.tick", number = message.number);
                        async {
                            let history = history_tick.lock().await;
                            if let Some(state) = history.current() {
                                state.tick().await.inspect_err(|e| error!("Tick error: {e}")).ok();
                            }
                        }
                        .instrument(span)
                        .await;
                    }
                }
            }
        });

        Ok(())
    }

    async fn stateful_run(
        history: Arc<Mutex<StateHistory<State>>>,
        mut certs_reader: CertsReader,
        mut address_deltas_reader: AddressDeltasReader,
        mut publisher: DeltaPublisher,
        params: Arc<StakeDeltaFilterParams>,
        context: Arc<Context<Message>>,
        is_snapshot_mode: bool,
    ) -> Result<()> {
        if !is_snapshot_mode {
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
                state.save()?;
                history.lock().await.commit(block_info.number, state);

                if primary.do_validation() {
                    ctx.publish().await;
                }
            }
        }
    }
}
