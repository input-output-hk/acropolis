//! Acropolis ledger state module for Caryatid
//! Accepts UTXO events and derives the current ledger state in memory

use caryatid_sdk::{Context, Module, module, MessageBusExt};
use acropolis_messages::Message;
use std::sync::Arc;
use anyhow::Result;
use config::Config;
use tracing::{info, error};
use hex::encode;

const DEFAULT_SUBSCRIBE_TOPIC: &str = "cardano.utxo.#";

/// Ledger state module
#[module(
    message_type(Message),
    name = "ledger-state",
    description = "In-memory ledger state from UTXO events"
)]
pub struct LedgerState;

impl LedgerState
{
    /// Main init function
    pub fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {

        // Subscribe for UTXO messages
        // Get configuration
        let subscribe_topic = config.get_string("subscribe-topic")
            .unwrap_or(DEFAULT_SUBSCRIBE_TOPIC.to_string());
        info!("Creating subscriber on '{subscribe_topic}'");

        context.clone().message_bus.subscribe(&subscribe_topic,
                                      move |message: Arc<Message>| {
           match message.as_ref() {
               Message::Input(input_msg) => {
                   info!("Received input {}:{}", encode(&input_msg.ref_hash), input_msg.ref_index);
               }

               Message::Output(output_msg) => {
                   info!("Received output {}:{}", encode(&output_msg.tx_hash), output_msg.index);
               }

               _ => error!("Unexpected message type: {message:?}")
           }
        })?;

        Ok(())
    }
}
