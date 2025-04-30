//! Acropolis Stake Delta Filter module
//! Reads address deltas and filters out only stake addresses from it; also resolves pointer addresses.

use std::cmp::max;
use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use caryatid_sdk::{Context, Module, module, MessageBusExt};
use acropolis_common::{messages::Message, Address, AddressNetwork, ShelleyAddressDelegationPart, ShelleyAddressPointer, StakeAddress, StakeCredential, TxCertificate};
use std::sync::Arc;
use anyhow::{anyhow, Result};
use config::Config;
use tracing::{error, info};
use acropolis_common::Credential::AddrKeyHash;
use acropolis_common::messages::TxCertificatesMessage;
use acropolis_common::StakeAddressPayload::{ScriptHash, StakeKeyHash};

const DEFAULT_ADDRESS_DELTA_TOPIC: &str = "cardano.address.delta";
const DEFAULT_CERTIFICATE_TOPIC: &str = "cardano.certificates";
const DEFAULT_BUILD_POINTER_ADDRESS_CACHE: CacheMode = CacheMode::Always;
const DEFAULT_ADDRESS_CACHE_DIR: &str = "downloads";

/// Stake Delta Filter module
#[module(
    message_type(Message),
    name = "stake-delta-filter",
    description = "Retrieves stake addresses from address deltas"
)]
pub struct StakeDeltaFilter;

#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
struct PointerCache {
    pub pointer_map: HashMap<ShelleyAddressPointer, Address>,
    pub max_slot: u64
}

impl PointerCache {
    pub fn new() -> Self {
        Self {
            pointer_map: HashMap::new(),
            max_slot: 0
        }
    }

    pub fn update_max_slot(&mut self, processed_slot: u64) {
        self.max_slot = max(self.max_slot, processed_slot);
    }

    pub fn decode_pointer(&self, pointer: &ShelleyAddressPointer) -> Result<&Address> {
        match self.pointer_map.get(pointer) {
            Some(address) => Ok(address),
            None => Err(anyhow!("Pointer {:?} missing from cache", pointer)),
        }
    }

    pub fn decode_address(&self, address: &Address) -> Result<Address> {
        if let Address::Shelley(shelley_address) = address {
            if let ShelleyAddressDelegationPart::Pointer(ptr) = &shelley_address.delegation {
                if ptr.slot > self.max_slot {
                    return Err(anyhow!("Pointer {:?} is too recent, cache reflects slots up to {}", ptr, self.max_slot));
                }
                return self.decode_pointer(ptr).cloned();
            }
        }
        Ok(address.clone())
    }

    pub fn try_load(file_path: &str) -> Result<Arc<Self>> {
        let file = File::open(file_path)?;
        let reader = BufReader::new(file);
        match serde_json::from_reader::<BufReader<std::fs::File>, PointerCache>(reader) {
            Ok(res) => Ok(Arc::new(res)),
            Err(err) => Err(anyhow!("Error reading json for {}: '{}'", file_path, err))
        }
    }
}

#[derive(Clone, Debug)]
pub enum CacheMode {
    Never, IfAbsent, Always
}

#[derive(Clone, Debug)]
struct StakeDeltaFilterParams {
    address_delta_topic: String,
    tx_certificates_topic: String,
    pointer_address_cache_dir: String,
    genesis_hash: String,
    network: AddressNetwork,
    build_pointer_cache: CacheMode,
    context: Arc<Context<Message>>
}

impl StakeDeltaFilterParams {
    fn new(context: Arc<Context<Message>>) -> Self {
        Self {
            address_delta_topic: "".to_string(),
            tx_certificates_topic: "".to_string(),
            pointer_address_cache_dir: "".to_string(),
            genesis_hash: "".to_string(),
            network: Default::default(),
            build_pointer_cache: CacheMode::Always,
            context,
        }
    }

    fn get_cache_file(&self) -> Result<String> {
        let path = Path::new(&self.pointer_address_cache_dir);
        let full = path.join(&self.genesis_hash);
        let str = full.to_str().ok_or_else(|| anyhow!("Cannot produce cache file name".to_string()))?;
        Ok(str.to_string())
    }
}

impl StakeDeltaFilter
{
    fn decode_build_cache_mode(mode: &str) -> Result<CacheMode> {
        match mode {
            "never" => Ok(CacheMode::Never),
            "if_absent" => Ok(CacheMode::IfAbsent),
            "always" => Ok(CacheMode::Always),
            m => Err(anyhow!("Unknown option value '{}': 'never', 'if_absent' or 'always' expected", m))
        }
    }

    fn prepare_params(context: Arc<Context<Message>>, config: Arc<Config>) -> Result<Arc<StakeDeltaFilterParams>> {
        let mut params = StakeDeltaFilterParams::new(context);

        // Get configuration
        params.address_delta_topic = config.get_string("address-delta-topic")
            .unwrap_or(DEFAULT_ADDRESS_DELTA_TOPIC.to_string());

        params.tx_certificates_topic = config.get_string("certificates-topic")
            .unwrap_or(DEFAULT_ADDRESS_DELTA_TOPIC.to_string());

        params.pointer_address_cache_dir = config.get_string("pointer-address-cache-dir")
            .unwrap_or(DEFAULT_ADDRESS_CACHE_DIR.to_string());
        info!("Reading caches from '{}...'", params.pointer_address_cache_dir);

        params.build_pointer_cache = match config.get_string("build-pointer-address-cache") {
            Ok(c) => Self::decode_build_cache_mode(&c)?,
            Err(_) => DEFAULT_BUILD_POINTER_ADDRESS_CACHE
        };
        info!("Pointer cache mode: {:?}", params.build_pointer_cache);

        params.genesis_hash = match config.get_string("genesis-hash") {
            Ok(h) => h,
            Err(e) => return Err(anyhow!("Reading 'genesis-hash' parameter error: {e}. The parameter is mandatory!"))
        };

        Ok(Arc::new(params))
    }

    pub fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        let params = Self::prepare_params(context, config.clone())?;
        let cache_path = params.get_cache_file()?;

        match params.build_pointer_cache {
            CacheMode::Never => Self::stateless_init(PointerCache::try_load(&cache_path)?, params),

            CacheMode::IfAbsent => match PointerCache::try_load(&cache_path) {
                Ok(cache) => Self::stateless_init(cache, params),
                Err(e) => {
                    info!("Cannot load cache: {}, building from scratch", e);
                    Self::stateful_init(params)
                }
            }

            CacheMode::Always => Self::stateful_init(params)
        }
    }

    fn stateless_init(cache: Arc<PointerCache>, params: Arc<StakeDeltaFilterParams>) -> Result<()> {
    // Subscribe for certificate messages
        info!("Creating subscriber on '{}'", params.address_delta_topic);
        params.context.clone().message_bus.subscribe(&params.clone().address_delta_topic, move |message: Arc<Message>| {
            let params_copy = params.clone();
            let cache_copy = cache.clone();

            async move {
                match message.as_ref() {
                    Message::AddressDeltas(delta) => {
                        for d in delta.deltas.iter() {
                            info!("Address {:?} => {:?}", d.address, cache_copy.decode_address(&d.address))
                        }
                    }

                    _ => error!("Unexpected message type for {}: {message:?}", &params_copy.address_delta_topic)
                }
            }
        })?;

        Ok(())
    }

    fn stateful_init(params: Arc<StakeDeltaFilterParams>) -> Result<()> {
        info!("Creating subscriber on '{}'", params.tx_certificates_topic);
        let params_c = params.clone();
        let params_d = params.clone();

        params.context.clone().message_bus.subscribe(&params.clone().tx_certificates_topic, move |message: Arc<Message>| {
            let params_c = params_c.clone();
            async move {
                let mut pc = PointerCache::new();
                match message.as_ref() {
                    Message::TxCertificates(msg) => {
                        for cert in msg.certificates.iter() {
                            match cert {
                                TxCertificate::StakeRegistration(reg) => {
                                    let ptr = ShelleyAddressPointer {
                                        slot: msg.block.slot,
                                        tx_index: reg.tx_index,
                                        cert_index: reg.cert_index,
                                    };
                                    pc.pointer_map.insert(ptr, Address::Stake(StakeAddress{
                                        network: params_c.network.clone(),
                                        payload: match &reg.stake_credential {
                                            StakeCredential::ScriptHash(h) => ScriptHash(h.clone()),
                                            StakeCredential::AddrKeyHash(k) => StakeKeyHash(k.clone())
                                        }
                                    }));
                                    pc.update_max_slot(msg.block.slot);
                                },
                                _ => ()
                            }
                        }
                    }
                    _ => error!("Unexpected message type for {}: {message:?}", &params_c.tx_certificates_topic)
                }
            }
        })?;

        info!("Creating subscriber on '{}'", params.address_delta_topic);
        params.context.clone().message_bus.subscribe(&params.clone().address_delta_topic, move |message: Arc<Message>| {
            let params_d = params_d.clone();
            async move {
                let pc = PointerCache::new();
                match message.as_ref() {
                    Message::AddressDeltas(delta) => {
                        for d in delta.deltas.iter() {
                            info!("Address {:?} => {:?}", d.address, pc.decode_address(&d.address))
                        }
                    }

                    _ => error!("Unexpected message type for {}: {message:?}", &params_d.address_delta_topic)
                }
            }
        })?;

        Ok(())
    }
}
