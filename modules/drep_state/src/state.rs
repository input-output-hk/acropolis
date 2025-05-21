//! Acropolis DRepState: State storage

use std::collections::HashMap;
use acropolis_common::{
    messages::TxCertificatesMessage,
    TxCertificate,
    Anchor, DRepCredential, Lovelace,
};
use anyhow::{anyhow, Result};
use tracing::info;
use serde_with::serde_as;
use crate::{drep_distribution_publisher::DRepDistributionPublisher};

#[serde_as]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DRepRecord {
    pub deposit: Lovelace,
    pub anchor: Option<Anchor>
}

impl DRepRecord {
    pub fn new(deposit: Lovelace, anchor: Option<Anchor>) -> Self {
        Self { deposit, anchor }
    }
}

pub struct State {
    certificate_info_update_slot: u64,
    dreps: HashMap::<DRepCredential, DRepRecord>,

    drep_distribution_publisher: Option<DRepDistributionPublisher>
}

impl State {
    pub fn new(drep_distribution_publisher: Option<DRepDistributionPublisher>) -> Self {
        Self {
            certificate_info_update_slot: 1,
            dreps: HashMap::new(),
            drep_distribution_publisher
        }
    }

    #[allow(dead_code)]
    pub fn get_count(&self) -> usize {
        self.dreps.len()
    }

    pub fn get_drep(&self, credential: &DRepCredential) -> Option<&DRepRecord> {
        self.dreps.get(credential)
    }

    pub fn list(&self) -> Vec<DRepCredential> {
        self.dreps.keys().map(|x| x.clone()).collect()
    }

    async fn log_stats(&self) {
        info!(count = self.dreps.len(), "count");
    }

    pub async fn tick(&self) -> Result<()> {
        self.log_stats().await;
        Ok(())
    }
}

impl State {
    fn process_one_certificate(&mut self, tx_cert: &TxCertificate, tx_slot: u64) -> Result<()> {
        match tx_cert {
            TxCertificate::DRepRegistration(reg) => {
                match self.dreps.get_mut(&reg.credential) {
                    Some(ref mut drep) => {
                        if reg.deposit != 0 {
                            return Err(anyhow!("DRep registartion {:?}: replacement requires deposit = 0, instead of {}",
                                    reg.credential, reg.deposit
                                ));
                        } else {
                            drep.anchor = reg.anchor.clone();
                        }
                    },
                    None => { self.dreps.insert(reg.credential.clone(), DRepRecord::new(reg.deposit, reg.anchor.clone())); }
                }
            },
            TxCertificate::DRepDeregistration(reg) => {
                if self.dreps.remove(&reg.credential).is_none() {
                    return Err(anyhow!("DRep registartion {:?}: internal error, credential not found", reg.credential))
                }
            },
            TxCertificate::DRepUpdate(reg) => {
                match self.dreps.get_mut(&reg.credential) {
                    Some(ref mut drep) => drep.anchor = reg.anchor.clone(),
                    None => { return Err(anyhow!("DRep registartion {:?}: internal error, credential not found", reg.credential)); }
                }
            },
            _ => return Ok(())
        }

        // Fall through for all branches, where votes distribution had changed
        self.certificate_info_update_slot = tx_slot;
        Ok(())
    }

    pub fn new_drep_distribution(&self) -> Vec<(DRepCredential, Lovelace)> {
        let mut distribution = Vec::new();
        for (drep, drep_info) in self.dreps.iter() {
            distribution.push((drep.clone(), drep_info.deposit));
        }
        distribution
    }

    pub async fn handle(&mut self, tx_cert_msg: &TxCertificatesMessage) -> Result<()> {
        let tx_slot = tx_cert_msg.block.slot;

        for tx_cert in tx_cert_msg.certificates.iter() {
            if let Err(e) = self.process_one_certificate(tx_cert, tx_slot) {
                tracing::error!("Error processing tx_cert {}", e);
            }
        }

        if self.certificate_info_update_slot == tx_slot && 
            self.drep_distribution_publisher.is_some() 
        {
            let d = self.new_drep_distribution();
            info!("New vote distribution at slot = {}: len = {}", tx_slot, d.len());
            if let Some(ref mut publisher) = self.drep_distribution_publisher {
                if let Err(e) = publisher.publish_stake(d).await {
                    tracing::error!("Error publishing drep voting stake distribution: {e}");
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use acropolis_common::{DRepRegistration, TxCertificate, Credential};
    use crate::state::{DRepRecord, State};

    #[test]
    fn test_drep_process_one_certificate() {
        let tx_cred = Credential::AddrKeyHash([123, 222, 247, 170, 243, 201, 37, 233, 124, 164, 45, 54, 241, 25, 176, 70, 154, 18, 204, 164, 161, 126, 207, 239, 198, 144, 3, 80].to_vec());
        let tx_cert = TxCertificate::DRepRegistration( DRepRegistration{
            credential: tx_cred.clone(),
            deposit: 500000000,
            anchor: None
        });
        let mut state = State::new(None);
        state.process_one_certificate(&tx_cert, 1).unwrap();
        assert_eq!(state.get_count(), 1);
        let tx_cert_record = DRepRecord{ deposit: 500000000, anchor: None };
        assert_eq!(state.get_drep(&tx_cred).unwrap().deposit, tx_cert_record.deposit);
    }
}
