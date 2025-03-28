//! Acropolis SPOState: State storage
use acropolis_common::{
    messages::TxCertificatesMessage,
    PoolRegistration,
    SerialisedMessageHandler,
    TxCertificate,
};
use anyhow::Result;
use async_trait::async_trait;
use serde::{Serialize, Serializer};
use std::collections::HashMap;
use tracing::info;

pub struct State {
    spos: HashMap<Vec::<u8>, PoolRegistration>,
}

impl State {
    pub fn new() -> Self {
        Self {
            spos: HashMap::<Vec::<u8>, PoolRegistration>::new(),
        }
    }

    async fn log_stats(&self) {
        info!(number = self.spos.keys().len());
    }

    pub async fn tick(&self) -> Result<()> {
        self.log_stats().await;
        Ok(())
    }
}

#[async_trait]
impl SerialisedMessageHandler<TxCertificatesMessage> for State {
    async fn handle(&mut self, tx_cert_msg: &TxCertificatesMessage) -> Result<()> {
        for tx_cert in tx_cert_msg.certificates.iter() {
            match tx_cert {
                TxCertificate::PoolRegistration(reg) => {
                    self.spos.insert(reg.operator.clone(), reg.clone());
                }
                TxCertificate::PoolRetirement(reg) => {
                    self.spos.remove(&reg.operator);
                }
                _ => ()
            }
        }

        Ok(())
    }
}

impl Serialize for State {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let serialized_map: HashMap<String, PoolRegistration> = self
            .spos
            .iter()
            .map(|(key, value)| (hex::encode(key), value.clone()))
            .collect();
        serialized_map.serialize(serializer)
    }
}
