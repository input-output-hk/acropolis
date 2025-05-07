//! Acropolis SPOState: State storage
use acropolis_common::{
    messages::TxCertificatesMessage,
    PoolRegistration,
    SerialisedHandler,
    TxCertificate,
};
use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;
use tracing::{error, info};
use serde_with::{serde_as, hex::Hex};

const TECHNICAL_PARAMETER_POOL_RETIRE_MAX_EPOCH: u64 = 18;

#[serde_as]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct State {
    epoch: u64,

    #[serde_as(as = "HashMap<Hex, _>")]
    spos: HashMap<Vec::<u8>, PoolRegistration>,

    #[serde_as(as = "HashMap<_, Vec<Hex>>")]
    pending_deregistrations: HashMap<u64, Vec<Vec<u8>>>,
}

impl State {
    pub fn new() -> Self {
        Self {
            epoch: 0,
            spos: HashMap::<Vec::<u8>, PoolRegistration>::new(),
            pending_deregistrations: HashMap::<u64, Vec::<Vec::<u8>>>::new(),
        }
    }

    pub fn get(&self, operator: &Vec<u8>) -> Option<&PoolRegistration> {
        self.spos.get(operator)
    }

    async fn log_stats(&self) {
        info!(
            num_spos = self.spos.keys().len(),
            num_pending_deregistrations = self.pending_deregistrations.values().map(|d| d.len()).sum::<usize>(),
        );
    }

    pub async fn tick(&self) -> Result<()> {
        self.log_stats().await;
        Ok(())
    }
}

#[async_trait]
impl SerialisedHandler<TxCertificatesMessage> for State {
    async fn handle(&mut self, _sequence: u64, tx_cert_msg: &TxCertificatesMessage) -> Result<()> {
        if tx_cert_msg.block.epoch > self.epoch {
            self.epoch = tx_cert_msg.block.epoch;
            let deregistrations = self.pending_deregistrations.remove(&self.epoch);
            match deregistrations {
                Some(deregistrations) => {
                    for dr in deregistrations {
                        match self.spos.remove(&dr) {
                        None => error!("Retirement requested for unregistered SPO {}", hex::encode(&dr)),
                        _ => (),
                    };
                    }
                },
                None => (),
            };
        }
        for tx_cert in tx_cert_msg.certificates.iter() {
            match tx_cert {
                TxCertificate::PoolRegistration(reg) => {
                    self.spos.insert(reg.operator.clone(), reg.clone());
                }
                TxCertificate::PoolRetirement(ret) => {
                    if ret.epoch <= self.epoch {
                        error!("SPO retirement received for current or past epoch {} for SPO {}", ret.epoch, hex::encode(&ret.operator));
                    } else if ret.epoch > self.epoch + TECHNICAL_PARAMETER_POOL_RETIRE_MAX_EPOCH {
                        error!("SPO retirement received for epoch {} that exceeds future limit for SPO {}", ret.epoch, hex::encode(&ret.operator));
                    } else {
                        // Replace any existing queued deregistrations
                        for (epoch, deregistrations) in &mut self.pending_deregistrations {
                            let len = deregistrations.len();
                            deregistrations.retain(|d| *d != ret.operator);
                            if deregistrations.len() < len {
                                info!("Removed pending deregistration of SPO {} from epoch {}", hex::encode(&ret.operator), epoch);
                            }
                        }
                        self.pending_deregistrations.entry(ret.epoch).or_default().push(ret.operator.clone());
                    }
                }
                _ => ()
            }
        }

        Ok(())
    }
}
