//! Acropolis transaction unpacker module for Caryatid
//! Unpacks transaction bodies into UTXO events

use caryatid_sdk::{Context, Module, module, MessageBusExt};
use acropolis_messages::{TxInput, TxOutput, UTXODelta, UTXODeltasMessage, Message};
use std::sync::Arc;
use anyhow::Result;
use config::Config;
use tracing::{debug, info, error};
use pallas::ledger::traverse::MultiEraTx;

const DEFAULT_SUBSCRIBE_TOPIC: &str = "cardano.txs";
const DEFAULT_PUBLISH_UTXO_DELTAS_TOPIC: &str = "cardano.utxo.deltas";

/// Tx unpacker module
/// Parameterised by the outer message enum used on the bus
#[module(
    message_type(Message),
    name = "tx-unpacker",
    description = "Transaction to UTXO event unpacker"
)]
pub struct TxUnpacker;

impl TxUnpacker
{
    /// Main init function
    pub fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {

        // Subscribe for tx messages
        // Get configuration
        let subscribe_topic = config.get_string("subscribe-topic")
            .unwrap_or(DEFAULT_SUBSCRIBE_TOPIC.to_string());
        info!("Creating subscriber on '{subscribe_topic}'");

        let publish_utxo_deltas_topic = config.get_string("publish-utxo-deltas-topic")
            .unwrap_or(DEFAULT_PUBLISH_UTXO_DELTAS_TOPIC.to_string());
        info!("Publishing UTXO deltas on '{publish_utxo_deltas_topic}'");

        context.clone().message_bus.subscribe(&subscribe_topic, move |message: Arc<Message>| {

            let context = context.clone();
            let publish_utxo_deltas_topic = publish_utxo_deltas_topic.clone();

            async move {
                match message.as_ref() {
                    Message::ReceivedTxs(txs_msg) => {
                        if tracing::enabled!(tracing::Level::DEBUG) {
                            debug!("Received {} txs for slot {}",
                                txs_msg.txs.len(), txs_msg.block.slot);
                        }

                        // Construct message
                        let mut message = UTXODeltasMessage {
                            block: txs_msg.block.clone(),
                            deltas: Vec::new(),
                        };

                        for raw_tx in &txs_msg.txs {
                            // Parse the tx
                            match MultiEraTx::decode(&raw_tx) {
                                Ok(tx) => {
                                    let inputs = tx.consumes();
                                    let outputs = tx.produces();
                                    if tracing::enabled!(tracing::Level::DEBUG) {
                                        debug!("Decoded transaction with {} inputs, {} outputs",
                                           inputs.len(), outputs.len());
                                    }

                                    // Add all the inputs
                                    for input in inputs {  // MultiEraInput

                                        let oref = input.output_ref();

                                        // Construct message
                                        let tx_input = TxInput {
                                            tx_hash: oref.hash().to_vec(),
                                            index: oref.index(),
                                        };

                                        message.deltas.push(UTXODelta::Input(tx_input));
                                    }

                                    // Add all the outputs
                                    for (index, output) in outputs {  // MultiEraOutput

                                        match output.address() {
                                            Ok(address) =>
                                            {
                                                let tx_output = TxOutput {
                                                    tx_hash: tx.hash().to_vec(),
                                                    index: index as u64,
                                                    address: address.to_vec(),
                                                    value: output.value().coin(),
                                                    // !!! datum
                                                };

                                                message.deltas.push(UTXODelta::Output(tx_output));
                                            }

                                            Err(e) => error!("Can't parse output {index} in tx: {e}")
                                        }
                                    }
                                },

                                Err(e) => error!("Can't decode transaction in slot {}: {e}",
                                                 txs_msg.block.slot)
                            }
                        }

                        let message_enum: Message = message.into();
                        context.message_bus.publish(&publish_utxo_deltas_topic,
                                                    Arc::new(message_enum))
                            .await
                            .unwrap_or_else(|e| error!("Failed to publish: {e}"));
                    }

                    _ => error!("Unexpected message type: {message:?}")
                }
            }
        })?;

        Ok(())
    }
}
