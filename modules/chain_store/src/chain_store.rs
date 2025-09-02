mod stores;

use acropolis_common::messages::Message;
use caryatid_sdk::{module, Context, Module};
use config::Config;
use std::sync::Arc;

#[module(
    message_type(Message),
    name = "chain-store",
    description = "Block and TX state"
)]
pub struct ChainStore;
