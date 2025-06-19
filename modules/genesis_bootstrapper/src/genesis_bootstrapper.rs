//! Acropolis genesis bootstrapper module for Caryatid
//! Reads genesis files and outputs initial UTXO events

use acropolis_common::{
    messages::{CardanoMessage, GenesisCompleteMessage, Message, UTXODeltasMessage},
    Address, BlockInfo, BlockStatus, ByronAddress, Era, TxOutput, UTXODelta,
};
use anyhow::Result;
use caryatid_sdk::{module, Context, Module};
use config::Config;
use pallas::ledger::configs::{byron::genesis_utxos, *};
use std::sync::Arc;
use tracing::{error, info};

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

impl GenesisBootstrapper {
    /// Main init function
    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        let startup_topic = config
            .get_string("startup-topic")
            .unwrap_or(DEFAULT_STARTUP_TOPIC.to_string());
        info!("Creating startup subscriber on '{startup_topic}'");

        let mut subscription = context.subscribe(&startup_topic).await?;
        context.clone().run(async move {
            let Ok(_) = subscription.read().await else {
                return;
            };
            info!("Received startup message");

            let publish_utxo_deltas_topic = config
                .get_string("publish-utxo-deltas-topic")
                .unwrap_or(DEFAULT_PUBLISH_UTXO_DELTAS_TOPIC.to_string());
            info!("Publishing UTXO deltas on '{publish_utxo_deltas_topic}'");

            let completion_topic = config
                .get_string("completion-topic")
                .unwrap_or(DEFAULT_COMPLETION_TOPIC.to_string());
            info!("Completing with '{completion_topic}'");

            // Read genesis data
            let genesis: byron::GenesisFile = serde_json::from_slice(MAINNET_BYRON_GENESIS)
                .expect("Invalid JSON in MAINNET_BYRON_GENESIS file");

            // Construct message
            let block_info = BlockInfo {
                status: BlockStatus::Bootstrap,
                slot: 0,
                number: 0,
                hash: Vec::new(),
                epoch: 0,
                new_epoch: false,
                era: Era::Byron,
            };

            let mut message = UTXODeltasMessage { deltas: Vec::new() };

            // Convert the AVVM distributions into pseudo-UTXOs
            let gen_utxos = genesis_utxos(&genesis);
            for (hash, address, amount) in gen_utxos {
                let tx_output = TxOutput {
                    tx_hash: hash.to_vec(),
                    index: 0,
                    address: Address::Byron(ByronAddress {
                        payload: address.payload.to_vec(),
                    }),
                    value: amount,
                };

                message.deltas.push(UTXODelta::Output(tx_output));
            }

            let message_enum =
                Message::Cardano((block_info.clone(), CardanoMessage::UTXODeltas(message)));
            context
                .publish(&publish_utxo_deltas_topic, Arc::new(message_enum))
                .await
                .unwrap_or_else(|e| error!("Failed to publish: {e}"));

            // Send completion message
            let message_enum = Message::Cardano((
                block_info,
                CardanoMessage::GenesisComplete(GenesisCompleteMessage { 
                    conway_genesis: None 
                }),
            ));
            context
                .message_bus
                .publish(&completion_topic, Arc::new(message_enum))
                .await
                .unwrap_or_else(|e| error!("Failed to publish: {e}"));
        });

        Ok(())
    }
}
