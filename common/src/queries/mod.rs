use crate::messages::Message;
use caryatid_sdk::Context;
use std::sync::Arc;

pub mod accounts;
pub mod addresses;
pub mod assets;
pub mod blocks;
pub mod epochs;
pub mod governance;
pub mod ledger;
pub mod mempool;
pub mod metadata;
pub mod misc;
pub mod network;
pub mod parameters;
pub mod pools;
pub mod scripts;
pub mod spdd;
pub mod transactions;
pub mod utils;
pub mod utxos;
pub mod errors;

pub fn get_query_topic(context: Arc<Context<Message>>, topic: (&str, &str)) -> String {
    context.config.get_string(topic.0).unwrap_or_else(|_| topic.1.to_string())
}
