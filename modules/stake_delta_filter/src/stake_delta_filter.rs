//! Acropolis Stake Delta Filter module
//! Reads address deltas and filters out only stake addresses from it; also resolves pointer addresses.

use caryatid_sdk::{Context, Module, module, MessageBusExt};
use acropolis_common::{messages::Message, Address, ShelleyAddressDelegationPart};
use std::sync::Arc;
use anyhow::Result;
use config::Config;
//use tokio::sync::Mutex;
use tracing::{error, info};
//use acropolis_common::messages::{AddressDeltasMessage, RESTResponse};

const DEFAULT_ADDRESS_DELTA_TOPIC: &str = "cardano.address.delta";
const DEFAULT_ADDRESS_CACHE_DIR: &str = "downloads";

/// Stake Delta Filter module
#[module(
    message_type(Message),
    name = "stake-delta-filter",
    description = "Retrieves stake addresses from address deltas"
)]
pub struct StakeDeltaFilter;

impl StakeDeltaFilter
{
    pub fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {

        // Get configuration
        let subscribe_topic = config.get_string("address-delta-topic")
            .unwrap_or(DEFAULT_ADDRESS_DELTA_TOPIC.to_string());
        info!("Creating subscriber on '{subscribe_topic}'");

        let pointer_address_cache_dir = config.get_string("pointer-address-cache-dir")
            .unwrap_or(DEFAULT_ADDRESS_CACHE_DIR.to_string());
        info!("Reading caches from '{pointer_address_cache_dir}...'");

        // Subscribe for certificate messages
        context.clone().message_bus.subscribe(&subscribe_topic, move |message: Arc<Message>| {
            async move {
                match message.as_ref() {
                    Message::AddressDeltas(delta) => {
                        for d in delta.deltas.iter() {
                            match d.address {
                                Address::None => (),
                                Address::Byron(_) => (),
                                Address::Shelley(ref sh) => {
                                    match &sh.delegation {
                                        ShelleyAddressDelegationPart::None => (),
                                        ShelleyAddressDelegationPart::StakeKeyHash(_) => (),
                                        ShelleyAddressDelegationPart::ScriptHash(_) => (),
                                        ShelleyAddressDelegationPart::Pointer(p) => {
                                            info!("Pointer: to slot {}, index {}, cert_index {}",
                                                p.slot, p.tx_index, p.cert_index
                                            );
                                        }
                                    }
                                }
                                Address::Stake(_) => ()
                            }
                        }
                    }

                    _ => error!("Unexpected message type: {message:?}")
                }
            }
        })?;

        Ok(())
    }
}
