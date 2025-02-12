//! Acropolis genesis bootstrapper module for Caryatid
//! Reads genesis files and outputs initial UTXO events

use caryatid_sdk::{Context, Module, module, MessageBusExt};
use acropolis_messages::{TxOutput, UTXODelta, UTXODeltasMessage, Message};
use std::sync::Arc;
use anyhow::Result;
use config::Config;
use tracing::{debug, info, error};

const DEFAULT_PUBLISH_UTXO_DELTAS_TOPIC: &str = "cardano.utxo.deltas";

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

        // TODO read genesis files
        // TODO read outputs and generate delta message
        // TODO output bootstrap done message for miniprotocols to take over

        // Construct message
        let mut message = UTXODeltasMessage {
            slot: 0,
            deltas: Vec::new(),
        };

        // For each initial UTXO
        //let tx_output = TxOutput {
        //  tx_hash: tx.hash().to_vec(),
        //  index: index,
        //  address: address.to_vec(),
        //  value: output.value().coin(),
        // };

        // message.deltas.push(UTXODelta::Output(tx_output));
        // index += 1;

        debug!("Genesis bootstrapper sending {:?}", message);
        let message_enum: Message = message.into();

        tokio::spawn(async move {
            context.message_bus.publish(&publish_utxo_deltas_topic,
                                        Arc::new(message_enum))
                .await
                .unwrap_or_else(|e| error!("Failed to publish: {e}"));
        });

        Ok(())
    }
}
