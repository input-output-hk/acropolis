//! Acropolis transaction unpacker module for Caryatid
//! Unpacks transaction bodies into UTXO events

use caryatid_sdk::{Context, Module, module, MessageBusExt};
use acropolis_messages::{OutputMessage, Message};
use std::sync::Arc;
use anyhow::Result;
use config::Config;
use tracing::{debug, info, error};

use pallas::{
    ledger::traverse::MultiEraTx,
};

const DEFAULT_SUBSCRIBE_TOPIC: &str = "cardano.tx";
const DEFAULT_PUBLISH_TOPIC: &str = "cardano.utxo";

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

        let publish_topic = config.get_string("publish-topic")
            .unwrap_or(DEFAULT_PUBLISH_TOPIC.to_string());
        info!("Publishing on '{publish_topic}'");

        context.clone().message_bus.subscribe(&subscribe_topic,
                                      move |message: Arc<Message>| {
           match message.as_ref() {
               Message::Tx(tx_msg) => {
                   info!("Received tx {}:{}", tx_msg.slot, tx_msg.index);

                   // Parse the tx
                   match MultiEraTx::decode(&tx_msg.raw) {
                       Ok(tx) => {
                           let outputs = tx.outputs();
                           info!("Decoded transaction with {} outputs", outputs.len());

                           // Publish all the outputs
                           let mut index: u32 = 0;
                           for output in outputs {  // MultiEraOutput

                               match output.address() {
                                   Ok(address) =>
                                   {
                                       // Construct message
                                       let message = OutputMessage {
                                           slot: tx_msg.slot,
                                           tx_index: tx_msg.index,
                                           index: index,
                                           address: address.to_vec()
                                       };

                                       debug!("Tx unpacker sending output {:?}", message);
                                       let message_enum: Message = message.into();

                                       let context = context.clone();
                                       let topic = publish_topic.clone() + "spent";
                                       tokio::spawn(async move {
                                           context.message_bus.publish(&topic,
                                                                       Arc::new(message_enum))
                                               .await
                                               .unwrap_or_else(|e| error!("Failed to publish: {e}"));
                                       });
                                   },

                                   Err(e) => error!("Can't parse output {index} in tx: {e}")
                               }

                               index += 1;
                           }
                       },

                       Err(e) => error!("Can't decode transaction {}:{}: {e}",
                                        tx_msg.slot, tx_msg.index)
                   }
               }

               _ => error!("Unexpected message type: {message:?}")
           }
        })?;

        Ok(())
    }
}
