//! Acropolis genesis bootstrapper module for Caryatid
//! Reads genesis files and outputs initial UTXO events

use acropolis_common::{
    messages::{
        CardanoMessage, GenesisCompleteMessage, Message, PotDeltasMessage, UTXODeltasMessage,
    },
    Address, BlockInfo, BlockStatus, ByronAddress, Era, Lovelace, LovelaceDelta, Pot, PotDelta,
    TxOutput, UTXODelta,
};
use anyhow::Result;
use caryatid_sdk::{module, Context, Module};
use config::Config;
use pallas::ledger::configs::{byron::genesis_utxos, *};
use std::sync::Arc;
use tracing::{error, info, info_span, Instrument};

const DEFAULT_STARTUP_TOPIC: &str = "cardano.sequence.start";
const DEFAULT_PUBLISH_UTXO_DELTAS_TOPIC: &str = "cardano.utxo.deltas";
const DEFAULT_PUBLISH_POT_DELTAS_TOPIC: &str = "cardano.pot.deltas";
const DEFAULT_COMPLETION_TOPIC: &str = "cardano.sequence.bootstrapped";
const DEFAULT_NETWORK_NAME: &str = "mainnet";

// Include genesis data (downloaded by build.rs)
const MAINNET_BYRON_GENESIS: &[u8] = include_bytes!("../downloads/mainnet-byron-genesis.json");
const SANCHONET_BYRON_GENESIS: &[u8] = include_bytes!("../downloads/sanchonet-byron-genesis.json");

// Initial reserves (=maximum ever Lovelace supply)
const INITIAL_RESERVES: Lovelace = 45_000_000_000_000_000;

/// Genesis bootstrapper module
#[module(
    message_type(Message),
    name = "genesis-bootstrapper",
    description = "Genesis bootstrap UTXO event generator"
)]
pub struct GenesisBootstrapper;

impl GenesisBootstrapper {
    /// Main init function
    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        let startup_topic =
            config.get_string("startup-topic").unwrap_or(DEFAULT_STARTUP_TOPIC.to_string());
        info!("Creating startup subscriber on '{startup_topic}'");

        let mut subscription = context.subscribe(&startup_topic).await?;
        context.clone().run(async move {
            let Ok(_) = subscription.read().await else {
                return;
            };
            let span = info_span!("genesis_bootstrapper", block = 0);
            async {
                info!("Received startup message");

                let publish_utxo_deltas_topic = config
                    .get_string("publish-utxo-deltas-topic")
                    .unwrap_or(DEFAULT_PUBLISH_UTXO_DELTAS_TOPIC.to_string());
                info!("Publishing UTXO deltas on '{publish_utxo_deltas_topic}'");

                let publish_pot_deltas_topic = config
                    .get_string("publish-pot-deltas-topic")
                    .unwrap_or(DEFAULT_PUBLISH_POT_DELTAS_TOPIC.to_string());
                info!("Publishing pot deltas on '{publish_pot_deltas_topic}'");

                let completion_topic = config
                    .get_string("completion-topic")
                    .unwrap_or(DEFAULT_COMPLETION_TOPIC.to_string());
                info!("Completing with '{completion_topic}'");

                let network_name =
                    config.get_string("network-name").unwrap_or(DEFAULT_NETWORK_NAME.to_string());

                let genesis = match network_name.as_ref() {
                    "mainnet" => MAINNET_BYRON_GENESIS,
                    "sanchonet" => SANCHONET_BYRON_GENESIS,
                    _ => {
                        error!("Cannot find genesis for {network_name}");
                        return;
                    }
                };
                info!("Reading genesis for '{network_name}'");

                // Read genesis data
                let genesis: byron::GenesisFile =
                    serde_json::from_slice(genesis).expect("Invalid JSON in BYRON_GENESIS file");

                // Construct messages
                let block_info = BlockInfo {
                    status: BlockStatus::Bootstrap,
                    slot: 0,
                    number: 0,
                    hash: Vec::new(),
                    epoch: 0,
                    new_epoch: false,
                    era: Era::Byron,
                };

                let mut utxo_deltas_message = UTXODeltasMessage { deltas: Vec::new() };

                // Convert the AVVM distributions into pseudo-UTXOs
                let gen_utxos = genesis_utxos(&genesis);
                let mut total_allocated: u64 = 0;
                for (hash, address, amount) in gen_utxos.iter() {
                    let tx_output = TxOutput {
                        tx_hash: hash.to_vec(),
                        index: 0,
                        address: Address::Byron(ByronAddress {
                            payload: address.payload.to_vec(),
                        }),
                        value: *amount,
                    };

                    utxo_deltas_message.deltas.push(UTXODelta::Output(tx_output));
                    total_allocated += amount;
                }

                info!(
                    total_allocated,
                    count = gen_utxos.len(),
                    "AVVM genesis UTXOs"
                );

                let message_enum = Message::Cardano((
                    block_info.clone(),
                    CardanoMessage::UTXODeltas(utxo_deltas_message),
                ));
                context
                    .publish(&publish_utxo_deltas_topic, Arc::new(message_enum))
                    .await
                    .unwrap_or_else(|e| error!("Failed to publish: {e}"));

                // Send the pot update message with the remaining reserves
                let mut pot_deltas_message = PotDeltasMessage { deltas: Vec::new() };
                pot_deltas_message.deltas.push(PotDelta {
                    pot: Pot::Reserves,
                    delta: (INITIAL_RESERVES - total_allocated) as LovelaceDelta,
                });

                let message_enum = Message::Cardano((
                    block_info.clone(),
                    CardanoMessage::PotDeltas(pot_deltas_message),
                ));
                context
                    .publish(&publish_pot_deltas_topic, Arc::new(message_enum))
                    .await
                    .unwrap_or_else(|e| error!("Failed to publish: {e}"));

                // Send completion message
                let message_enum = Message::Cardano((
                    block_info,
                    CardanoMessage::GenesisComplete(GenesisCompleteMessage {}),
                ));
                context
                    .message_bus
                    .publish(&completion_topic, Arc::new(message_enum))
                    .await
                    .unwrap_or_else(|e| error!("Failed to publish: {e}"));
            }
            .instrument(span)
            .await;
        });

        Ok(())
    }
}
