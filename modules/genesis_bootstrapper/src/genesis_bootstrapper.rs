//! Acropolis genesis bootstrapper module for Caryatid
//! Reads genesis files and outputs initial UTXO events

use caryatid_sdk::{Context, Module, module, MessageBusExt};
use acropolis_messages::{TxOutput, UTXODelta, UTXODeltasMessage, Message};
use std::sync::Arc;
use anyhow::Result;
use config::Config;
use tracing::{debug, info, error};
use pallas::ledger::configs::byron::{GenesisFile, genesis_utxos};

const DEFAULT_PUBLISH_UTXO_DELTAS_TOPIC: &str = "cardano.utxo.deltas";

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

        let publish_utxo_deltas_topic = config.get_string("publish-utxo-deltas-topic")
            .unwrap_or(DEFAULT_PUBLISH_UTXO_DELTAS_TOPIC.to_string());
        info!("Publishing UTXO deltas on '{publish_utxo_deltas_topic}'");

        // Read genesis data
        let genesis: GenesisFile = serde_json::from_slice(MAINNET_BYRON_GENESIS)
            .expect("Invalid JSON in {MAINNET_BYRON_GENESIS}");

        // Construct message
        let mut message = UTXODeltasMessage {
            slot: 0,
            deltas: Vec::new(),
        };

        // Convert the AVVM distributions into pseudo-UTXOs
        let gen_utxos = genesis_utxos(&genesis);
        let mut index: u64 = 0;
        for (hash, address, amount) in gen_utxos {
            let tx_output = TxOutput {
                tx_hash: hash.to_vec(),
                index: index,
                address: address.to_vec(),
                value: amount
            };

            message.deltas.push(UTXODelta::Output(tx_output));
            index += 1;
        }

        debug!("Genesis bootstrapper sending {:?}", message);
        let message_enum: Message = message.into();

        tokio::spawn(async move {
            context.message_bus.publish(&publish_utxo_deltas_topic,
                                        Arc::new(message_enum))
                .await
                .unwrap_or_else(|e| error!("Failed to publish: {e}"));

            // TODO output bootstrap done message for miniprotocols to take over
        });

        Ok(())
    }
}
