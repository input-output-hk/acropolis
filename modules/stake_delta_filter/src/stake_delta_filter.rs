//! Acropolis Stake Delta Filter module
//! Reads address deltas and filters out only stake addresses from it; also resolves pointer addresses.

use std::{path::Path, sync::Arc};
use caryatid_sdk::{Context, Module, module, MessageBusExt};
use acropolis_common::{messages::Message, AddressNetwork, Serialiser};
use anyhow::{anyhow, Result};
use config::Config;
use tokio::sync::Mutex;
use tracing::{error, info};

const DEFAULT_ADDRESS_DELTA_TOPIC: (&str,&str) = ("subscription-address-delta-topic", "cardano.address.delta");
const DEFAULT_CERTIFICATES_TOPIC: (&str,&str) = ("subscription-certificates-topic", "cardano.certificates");
const DEFAULT_STAKE_ADDRESS_DELTA_TOPIC: (&str,&str) = ("publishing-stake-delta-topic", "cardano.stake.delta");

/// Directory to put cached shelley address pointers into. Depending on the address 
/// cache mode, these cached pointers can be used instead of tracking current pointer
/// values in blockchain (which can be quite resource-consuming).
const DEFAULT_CACHE_DIR: (&str,&str) = ("cache-dir", "cache");

/// Cache mode, three options: always build cache; always read cache (error if missing); build if missing,
/// read otherwise.
const DEFAULT_CACHE_MODE: (&str,CacheMode) = ("cache-mode", CacheMode::WriteIfAbsent);

/// Network: currently only Main/Test. Parameter is necessary to distinguish caches.
const DEFAULT_NETWORK: (&str,AddressNetwork) = ("network", AddressNetwork::Main);

/// Stake Delta Filter module
#[module(
    message_type(Message),
    name = "stake-delta-filter",
    description = "Retrieves stake addresses from address deltas"
)]
pub struct StakeDeltaFilter;

mod state;
mod utils;

use state::{DeltaPublisher, State};
use utils::{CacheMode, PointerCache, process_message};

#[derive(Clone, Debug)]
struct StakeDeltaFilterParams {
    address_delta_topic: String,
    stake_address_delta_topic: String,
    tx_certificates_topic: String,
    cache_dir: String,
    network: AddressNetwork,
    cache_mode: CacheMode,
    context: Arc<Context<Message>>
}

impl StakeDeltaFilterParams {
    fn get_cache_file_name(&self, modifier: &str) -> Result<String> {
        let path = Path::new(&self.cache_dir);
        let full = path.join(format!("{:?}{}.json", &self.network, modifier).to_lowercase());
        let str = full.to_str().ok_or_else(|| anyhow!("Cannot produce cache file name".to_string()))?;
        Ok(str.to_string())
    }

    fn conf(config: &Arc<Config>, keydef: (&str, &str)) -> String {
        config.get_string(keydef.0).unwrap_or(keydef.1.to_string())
    }

    fn init(context: Arc<Context<Message>>, cfg: Arc<Config>) -> Result<Arc<Self>> {
        let params = Self {
            address_delta_topic: Self::conf(&cfg, DEFAULT_ADDRESS_DELTA_TOPIC),
            tx_certificates_topic: Self::conf(&cfg, DEFAULT_CERTIFICATES_TOPIC),
            stake_address_delta_topic: Self::conf(&cfg, DEFAULT_STAKE_ADDRESS_DELTA_TOPIC),
            cache_dir: Self::conf(&cfg, DEFAULT_CACHE_DIR),
            cache_mode: cfg.get::<CacheMode>(DEFAULT_CACHE_MODE.0).unwrap_or(DEFAULT_CACHE_MODE.1),
            context,
            network: cfg.get::<AddressNetwork>(DEFAULT_NETWORK.0).unwrap_or(DEFAULT_NETWORK.1)
        };

        info!("Reading caches from {}", params.cache_dir);
        info!("Cache mode {:?}", params.cache_mode);

        Ok(Arc::new(params))
    }
}

impl StakeDeltaFilter {
    pub fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        let params = StakeDeltaFilterParams::init(context, config.clone())?;
        let cache_path = params.get_cache_file_name("")?;

        match params.cache_mode {
            CacheMode::Read => Self::stateless_init(PointerCache::try_load(&cache_path)?, params),

            CacheMode::WriteIfAbsent => match PointerCache::try_load(&cache_path) {
                Ok(cache) => Self::stateless_init(cache, params),
                Err(e) => {
                    info!("Cannot load cache: {}, building from scratch", e);
                    Self::stateful_init(params)
                }
            }

            CacheMode::Write => Self::stateful_init(params)
        }
    }

    fn stateless_init(cache: Arc<PointerCache>, params: Arc<StakeDeltaFilterParams>) -> Result<()> {
        info!("Stateless init: using stake pointer cache");

        // Subscribe for certificate messages
        info!("Creating subscriber on '{}'", params.address_delta_topic);
        let cache = cache.clone();
        params.context.clone().message_bus.subscribe(&params.clone().address_delta_topic, move |message: Arc<Message>| {
            let params_copy = params.clone();
            let cache_copy = cache.clone();
            let publisher = DeltaPublisher::new(params.clone());

            async move {
                match message.as_ref() {
                    Message::AddressDeltas(delta) => 
                        match process_message(&cache_copy, delta).await {
                            Err(e) => tracing::error!("Cannot decode and convert stake key for {delta:?}: {e}"),
                            Ok(r) => publisher.publish(r).await.unwrap_or_else(|e| error!("Publish error: {e}"))
                        }

                    msg => error!("Unexpected message type for {}: {msg:?}", &params_copy.address_delta_topic)
                }
            }
        })?;

        Ok(())
    }

    fn stateful_init(params: Arc<StakeDeltaFilterParams>) -> Result<()> {
        info!("Stateful init: creating stake pointer cache");

        // State
        let state = Arc::new(Mutex::new(State::new(params.clone())));

        let serialiser = Arc::new(Mutex::new(Serialiser::new(state.clone(), module_path!())));
        let serialiser_tick = serialiser.clone();

        let serialiser_delta = Arc::new(Mutex::new(Serialiser::new(state.clone(), module_path!())));
        let serialiser_delta_tick = serialiser_delta.clone();
        let state_t = state.clone();
        let params_d = params.clone();

        info!("Creating subscriber on '{}'", params.tx_certificates_topic);
        params.context.clone().message_bus.subscribe(&params.tx_certificates_topic, move |message: Arc<Message>| {
            let serialiser = serialiser.clone();
            async move {
                match message.as_ref() {
                    Message::TxCertificates(tx_cert_msg) => {
                        let mut serialiser = serialiser.lock().await;
                        serialiser.handle(tx_cert_msg.sequence, tx_cert_msg)
                            .await
                            .inspect_err(|e| error!("Messaging handling error: {e}"))
                            .ok();
                    }

                    _ => error!("Unexpected message type: {message:?}")
                }
            }
        })?;

        info!("Creating subscriber on '{}'", params.address_delta_topic);
        params.context.clone().message_bus.subscribe(&params.clone().address_delta_topic, move |message: Arc<Message>| {
            let serialiser = serialiser_delta.clone();
            let params = params_d.clone();
            async move {
                match message.as_ref() {
                    Message::AddressDeltas(delta) => {
                        let mut serialiser = serialiser.lock().await;
                        serialiser.handle(delta.sequence, delta)
                            .await
                            .inspect_err(|e| error!("Messaging handling error: {e}"))
                            .ok();
                    }

                    _ => error!("Unexpected message type for {}: {message:?}", &params.address_delta_topic)
                }
            }
        })?;

        // Ticker to log stats
        params.context.clone().message_bus.subscribe("clock.tick", move |message: Arc<Message>| {
            let serialiser = serialiser_tick.clone();
            let serialiser_delta = serialiser_delta_tick.clone();
            let state = state_t.clone();

            async move {
                if let Message::Clock(message) = message.as_ref() {
                    if (message.number % 60) == 0 {
                        state.lock().await.tick()
                            .await
                            .inspect_err(|e| error!("Tick error: {e}"))
                            .ok();

                        serialiser.lock().await.tick();
                        serialiser_delta.lock().await.tick();
                    }
                }
            }
        })?;

        Ok(())
    }
}
