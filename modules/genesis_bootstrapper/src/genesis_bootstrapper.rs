//! Acropolis genesis bootstrapper module for Caryatid
//! Reads genesis files and outputs initial UTXO events

use caryatid_sdk::{Context, Module, module, MessageBusExt};
use acropolis_common::{
    BlockInfo, BlockStatus, Address, ByronAddress,
    TxOutput, UTXODelta, 
    messages::{
        UTXODeltasMessage, Message,
    }
};
use std::sync::Arc;
use anyhow::Result;
use config::Config;
use tracing::{info, error};
use pallas::ledger::configs::byron::{GenesisFile, genesis_utxos};

const DEFAULT_STARTUP_TOPIC: &str = "cardano.sequence.start";
const DEFAULT_PUBLISH_UTXO_DELTAS_TOPIC: &str = "cardano.utxo.deltas";
const DEFAULT_COMPLETION_TOPIC: &str = "cardano.sequence.bootstrapped";

// Include genesis data (downloaded by build.rs)
const MAINNET_BYRON_GENESIS: &[u8] = include_bytes!("../downloads/mainnet-byron-genesis.json");

/// Genesis bootstrapper module
#[module(
    message_type(Message),
    name = "genesis-bootstrapper",
    description = "Genesis bootstrap UTXO event generator"
)]
pub struct GenesisBootstrapper;

impl GenesisBootstrapper
{
    /// Main init function
    pub fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {

        let startup_topic = config.get_string("startup-topic")
            .unwrap_or(DEFAULT_STARTUP_TOPIC.to_string());
        info!("Creating startup subscriber on '{startup_topic}'");

        context.clone().message_bus.subscribe(&startup_topic, 
            move |_message: Arc<Message>| {
                let context = context.clone();
                let config = config.clone();
                info!("Received startup message");

                tokio::spawn(async move {
                    let publish_utxo_deltas_topic = config.get_string("publish-utxo-deltas-topic")
                        .unwrap_or(DEFAULT_PUBLISH_UTXO_DELTAS_TOPIC.to_string());
                    info!("Publishing UTXO deltas on '{publish_utxo_deltas_topic}'");

                    let completion_topic = config.get_string("completion-topic")
                        .unwrap_or(DEFAULT_COMPLETION_TOPIC.to_string());
                    info!("Completing with '{completion_topic}'");

                    // Read genesis data
                    let genesis: GenesisFile = serde_json::from_slice(MAINNET_BYRON_GENESIS)
                        .expect("Invalid JSON in {MAINNET_BYRON_GENESIS}");

                    // Construct message
                    let mut message = UTXODeltasMessage {
                        block: BlockInfo {
                            status: BlockStatus::Bootstrap,
                            slot: 0,
                            number: 0,
                            hash: Vec::new(),
                        },
                        deltas: Vec::new(),
                    };

                    // Convert the AVVM distributions into pseudo-UTXOs
                    let gen_utxos = genesis_utxos(&genesis);
                    for (hash, address, amount) in gen_utxos {
                        let tx_output = TxOutput {
                            tx_hash: hash.to_vec(),
                            index: 0,
                            address: Address::Byron(ByronAddress{
                                payload: address.payload.to_vec(),
                            }),
                            value: amount
                        };

                        message.deltas.push(UTXODelta::Output(tx_output));
                    }

                    let message_enum = Message::UTXODeltas(message);
                    context.message_bus.publish(&publish_utxo_deltas_topic,
                                                Arc::new(message_enum))
                        .await
                        .unwrap_or_else(|e| error!("Failed to publish: {e}"));

                    // Send completion message
                    context.message_bus.publish(&completion_topic, Arc::new(Message::None(())))
                        .await
                        .unwrap_or_else(|e| error!("Failed to publish: {e}"));
                });

                async {}
            }
        )?;

        Ok(())
    }
}
