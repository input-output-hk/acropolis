//! Acropolis Stake Delta Filter module
//! Reads address deltas and filters out only stake addresses from it; also resolves pointer addresses.

use acropolis_common::{
    messages::{CardanoMessage, Message},
    NetworkId,
};
use anyhow::{anyhow, Result};
use caryatid_sdk::{module, Context, Module};
use config::Config;
use serde::Deserialize;
use std::{path::Path, sync::Arc};
use tokio::sync::Mutex;
use tracing::{error, info, info_span, Instrument};

const DEFAULT_ADDRESS_DELTA_TOPIC: (&str, &str) =
    ("subscription-address-delta-topic", "cardano.address.delta");
const DEFAULT_CERTIFICATES_TOPIC: (&str, &str) =
    ("subscription-certificates-topic", "cardano.certificates");
const DEFAULT_STAKE_ADDRESS_DELTA_TOPIC: (&str, &str) =
    ("publishing-stake-delta-topic", "cardano.stake.deltas");

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

#[derive(Clone, Debug)]
struct StakeDeltaFilterParams {
    address_delta_topic: String,
    stake_address_delta_topic: String,
    tx_certificates_topic: String,
    network: NetworkId,

    cache_dir: String,
    cache_mode: CacheMode,
    write_full_cache: bool,

    context: Arc<Context<Message>>,
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
            config
                .get::<T>(keydef.0)
                .or_else(|e| Err(anyhow!("cannot parse {} value: {e}", keydef.0)))
        } else {
            Ok(keydef.1)
        }
    }

    fn init(context: Arc<Context<Message>>, cfg: Arc<Config>) -> Result<Arc<Self>> {
        let params = Self {
            address_delta_topic: Self::conf(&cfg, DEFAULT_ADDRESS_DELTA_TOPIC),
            tx_certificates_topic: Self::conf(&cfg, DEFAULT_CERTIFICATES_TOPIC),
            stake_address_delta_topic: Self::conf(&cfg, DEFAULT_STAKE_ADDRESS_DELTA_TOPIC),
            cache_dir: Self::conf(&cfg, DEFAULT_CACHE_DIR),
            cache_mode: Self::conf_enum::<CacheMode>(&cfg, DEFAULT_CACHE_MODE)?,
            write_full_cache: Self::conf_enum::<bool>(&cfg, DEFAULT_WRITE_FULL_CACHE)?,
            context,
            network: Self::conf_enum::<NetworkId>(&cfg, DEFAULT_NETWORK)?,
        };

        info!("Cache mode {:?}", params.cache_mode);
        if params.cache_mode != CacheMode::Predefined {
            if !Path::new(&params.cache_dir).try_exists()? {
                return Err(anyhow!(
                    "Pointer cache directory '{}' does not exist.",
                    params.cache_dir
                ));
            }
            info!("Reading (writing) caches from (to) {}", params.cache_dir);
        }

        Ok(Arc::new(params))
    }
}

impl StakeDeltaFilter {
    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        let params = StakeDeltaFilterParams::init(context, config.clone())?;
        let cache_path = params.get_cache_file_name(".json")?;

        match params.cache_mode {
            CacheMode::Predefined => {
                Self::stateless_init(
                    PointerCache::try_load_predefined(&params.get_network_name())?,
                    params,
                )
                .await
            }

            CacheMode::Read => {
                Self::stateless_init(PointerCache::try_load(&cache_path)?, params).await
            }

            CacheMode::WriteIfAbsent => match PointerCache::try_load(&cache_path) {
                Ok(cache) => Self::stateless_init(cache, params).await,
                Err(e) => {
                    info!("Cannot load cache: {}, building from scratch", e);
                    Self::stateful_init(params).await
                }
            },

            CacheMode::Write => Self::stateful_init(params).await,
        }
    }

    async fn stateless_init(
        cache: Arc<PointerCache>,
        params: Arc<StakeDeltaFilterParams>,
    ) -> Result<()> {
        info!("Stateless init: using stake pointer cache");

        // Subscribe for certificate messages
        info!("Creating subscriber on '{}'", params.address_delta_topic);
        let mut subscription =
            params.context.subscribe(&params.clone().address_delta_topic).await?;
        params.context.clone().run(async move {
            let publisher = DeltaPublisher::new(params.clone());

            loop {
                let Ok((_, message)) = subscription.read().await else {
                    return;
                };
                match message.as_ref() {
                    Message::Cardano((block_info, CardanoMessage::AddressDeltas(delta))) => {
                        let span = info_span!(
                            "stake_delta_filter_stateless.handle_deltas",
                            block = block_info.number
                        );
                        async {
                            let msg = process_message(&cache, &delta, &block_info, None);
                            publisher
                                .publish(&block_info, msg)
                                .await
                                .unwrap_or_else(|e| error!("Publish error: {e}"))
                        }
                        .instrument(span)
                        .await;
                    }

                    msg => error!(
                        "Unexpected message type for {}: {msg:?}",
                        &params.address_delta_topic
                    ),
                }
            }
        });

        Ok(())
    }

    async fn stateful_init(params: Arc<StakeDeltaFilterParams>) -> Result<()> {
        info!("Stateful init: creating stake pointer cache");

        // State
        let state = Arc::new(Mutex::new(State::new(params.clone())));

        info!("Creating subscriber on '{}'", params.tx_certificates_topic);

        let state_certs = state.clone();
        let mut subscription = params.context.subscribe(&params.tx_certificates_topic).await?;
        params.clone().context.run(async move {
            loop {
                let Ok((_, message)) = subscription.read().await else {
                    return;
                };
                match message.as_ref() {
                    Message::Cardano((block_info, CardanoMessage::TxCertificates(tx_cert_msg))) => {
                        let span = info_span!(
                            "stake_delta_filter.handle_certs",
                            block = block_info.number
                        );
                        async {
                            let mut state = state_certs.lock().await;
                            state
                                .handle_certs(block_info, tx_cert_msg)
                                .await
                                .inspect_err(|e| error!("Messaging handling error: {e}"))
                                .ok();
                        }
                        .instrument(span)
                        .await;
                    }

                    _ => error!("Unexpected message type: {message:?}"),
                }
            }
        });

        info!("Creating subscriber on '{}'", params.address_delta_topic);
        let state_deltas = state.clone();
        let topic = params.address_delta_topic.clone();
        let mut subscription = params.context.subscribe(&topic).await?;
        params.clone().context.run(async move {
            loop {
                let Ok((_, message)) = subscription.read().await else {
                    return;
                };
                match message.as_ref() {
                    Message::Cardano((block_info, CardanoMessage::AddressDeltas(deltas))) => {
                        let span = info_span!(
                            "stake_delta_filter.handle_deltas",
                            block = block_info.number
                        );
                        async {
                            let mut state = state_deltas.lock().await;
                            state
                                .handle_deltas(block_info, deltas)
                                .await
                                .inspect_err(|e| error!("Messaging handling error: {e}"))
                                .ok();
                        }
                        .instrument(span)
                        .await;
                    }

                    _ => error!("Unexpected message type for {}: {message:?}", &topic),
                }
            }
        });

        // Ticker to log stats
        let state_tick = state.clone();
        let mut subscription = params.context.subscribe("clock.tick").await?;
        params.clone().context.run(async move {
            loop {
                let Ok((_, message)) = subscription.read().await else {
                    return;
                };
                if let Message::Clock(message) = message.as_ref() {
                    if (message.number % 60) == 0 {
                        let span = info_span!("stake_delta_filter.tick", number = message.number);
                        async {
                            state_tick
                                .lock()
                                .await
                                .tick()
                                .await
                                .inspect_err(|e| error!("Tick error: {e}"))
                                .ok();
                        }
                        .instrument(span)
                        .await;
                    }
                }
            }
        });

        Ok(())
    }
}
