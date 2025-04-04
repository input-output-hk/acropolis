//! Acropolis DRepState: State storage
use acropolis_common::{
    messages::TxCertificatesMessage,
    SerialisedMessageHandler,
    TxCertificate,
};
use anyhow::Result;
use async_trait::async_trait;
use tracing::info;
use serde_with::serde_as;

#[serde_as]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct State {
    dreps_count: i32
}

impl State {
    pub fn new() -> Self {
        Self {
            dreps_count: 0
            //spos: HashMap::<Vec::<u8>, PoolRegistration>::new(),
        }
    }

    pub fn get_count(&self) -> i32 {
        self.dreps_count
    }

    async fn log_stats(&self) {
        info!(count = self.dreps_count);
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
                TxCertificate::DRepRegistration(_reg) => {
                    //info!("DRep registration: {:?}, deposit: {}, anchor: {:?}", reg.credential, reg.deposit, reg.anchor);
                    self.dreps_count += 1;
                },
                TxCertificate::DRepDeregistration(_reg) => {
                    //info!("DRep deregistration: {:?}, refund: {}", reg.credential, reg.refund);
                    self.dreps_count -= 1;
                }
                TxCertificate::DRepUpdate(_reg) => {
                    //info!("DRep deregistration: {:?}, anchor: {:?}", reg.credential, reg.anchor);
                }
                _ => ()
            }
        }

        Ok(())
    }
}
