//! Acropolis SPO state module for Caryatid
//! Accepts certificate events and derives the SPO state in memory

use caryatid_sdk::{Context, Module, module, MessageBusExt};
use caryatid_sdk::messages::RESTResponse;
use acropolis_common::{
    messages::Message,
    PoolRegistration,
    TxCertificate,
};
use std::collections::HashMap;
use std::future;
use std::ops::Deref;
use std::sync::Arc;
use anyhow::Result;
use config::Config;
use tokio::sync::Mutex;
use tracing::{debug, error, info};
use serde::{Serialize, Serializer};
use serde_json;
use std::process;
use hex::encode;

const DEFAULT_SUBSCRIBE_TOPIC: &str = "cardano.certificates";
const DEFAULT_HANDLE_TOPIC: &str = "rest.get.spo-state";

/// SPO State module
#[module(
    message_type(Message),
    name = "spo-state",
    description = "In-memory SPO State from certificate events"
)]
pub struct SPOState;

impl SPOState
{
    pub fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {

        // Get configuration
        let subscribe_topic = config.get_string("subscribe-topic")
            .unwrap_or(DEFAULT_SUBSCRIBE_TOPIC.to_string());
        info!("Creating subscriber on '{subscribe_topic}'");

        let handle_topic = config.get_string("handle-topic")
            .unwrap_or(DEFAULT_HANDLE_TOPIC.to_string());
        info!("Creating request handler on '{handle_topic}'");

        let state = Arc::new(Mutex::new(HashMap::<Vec::<u8>, PoolRegistration>::new()));
        let state2 = state.clone();

        // Subscribe for certificate messages
        context.clone().message_bus.subscribe(&subscribe_topic, move |message: Arc<Message>| {
            let state = state.clone();
            async move {
                match message.as_ref() {
                    Message::TxCertificates(tx_cert_msg) => {
                        for tx_cert in tx_cert_msg.certificates.iter() {
                            match tx_cert {
                                TxCertificate::PoolRegistration(reg) => {
                                    state.lock().await.insert(reg.operator.clone(), reg.clone());
                                }
                                _ => ()
                            }
                        }
                    }

                    _ => error!("Unexpected message type: {message:?}")
                }
            }
        })?;

        // Handle requests for SPO state
        context.message_bus.handle(&handle_topic, move |message: Arc<Message>| {
            let state = state2.clone();
            async move {
                let response = match message.as_ref() {
                    Message::RESTRequest(request) => {
                        info!("REST received {} {}", request.method, request.path);
                        let lock = state.lock().await;
                        let map = SPOMapSerializer{ map: lock.deref() };
                        let body = serde_json::to_string(&map).expect("something");
                        RESTResponse {
                            code: 200,
                            body: body,
                        }
                    },
                    _ => {
                        error!("Unexpected message type {:?}", message);
                        RESTResponse {
                            code: 500,
                            body: "Unexpected message in REST request".to_string()
                        }
                    }
                };

                Arc::new(Message::RESTResponse(response))
            }
        })?;

        Ok(())
    }
}

struct SPOMapSerializer<'a> {
    map: &'a HashMap::<Vec::<u8>, PoolRegistration>,
}

impl<'a> Serialize for SPOMapSerializer<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let serialized_map: HashMap<String, PoolRegistration> = self
            .map
            .iter()
            .map(|(key, value)| (hex::encode(key), value.clone()))
            .collect();
        serialized_map.serialize(serializer)
    }
}
