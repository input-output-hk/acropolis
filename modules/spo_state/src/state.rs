//! Acropolis SPOState: State storage
use acropolis_common::{
    messages::TxCertificatesMessage,
    PoolRegistration,
    SerialisedMessageHandler,
    TxCertificate,
};
use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;
use tracing::{error, info};
use serde_with::{serde_as, hex::Hex};

#[serde_as]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct State {
    #[serde_as(as = "HashMap<Hex, _>")]
    spos: HashMap<Vec::<u8>, PoolRegistration>,
}

impl State {
    pub fn new() -> Self {
        Self {
            spos: HashMap::<Vec::<u8>, PoolRegistration>::new(),
        }
    }

    pub fn get(&self, operator: &Vec<u8>) -> Option<&PoolRegistration> {
        self.spos.get(operator)
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
                TxCertificate::PoolRetirement(ret) => {
                    match self.spos.remove(&ret.operator) {
                        None => error!("Retirement requested for unregistered SPO {}", hex::encode(&ret.operator)),
                        _ => (),
                    };
                }
                _ => ()
            }
        }

        Ok(())
    }
}
