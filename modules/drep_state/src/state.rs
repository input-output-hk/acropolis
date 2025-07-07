//! Acropolis DRepState: State storage

use acropolis_common::{
    messages::TxCertificatesMessage, Anchor, DRepCredential, Lovelace, TxCertificate,
};
use anyhow::{anyhow, Result};
use serde_with::serde_as;
use std::collections::HashMap;
use tracing::info;

#[serde_as]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DRepRecord {
    pub deposit: Lovelace,
    pub anchor: Option<Anchor>,
}

impl DRepRecord {
    pub fn new(deposit: Lovelace, anchor: Option<Anchor>) -> Self {
        Self { deposit, anchor }
    }
}

pub struct State {
    dreps: HashMap<DRepCredential, DRepRecord>,
}

impl State {
    pub fn new() -> Self {
        Self {
            dreps: HashMap::new(),
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
        self.dreps.keys().cloned().collect()
    }

    async fn log_stats(&self) {
        info!(count = self.dreps.len());
    }

    pub async fn tick(&self) -> Result<()> {
        self.log_stats().await;
        Ok(())
    }
}

impl State {
    fn process_one_certificate(&mut self, tx_cert: &TxCertificate) -> Result<bool> {
        match tx_cert {
            TxCertificate::DRepRegistration(reg) => match self.dreps.get_mut(&reg.credential) {
                Some(ref mut drep) => {
                    if reg.deposit != 0 {
                        Err(anyhow!("DRep registration {:?}: replacement requires deposit = 0, instead of {}",
                                reg.credential, reg.deposit
                            ))
                    } else {
                        drep.anchor = reg.anchor.clone();
                        Ok(false)
                    }
                }
                None => {
                    self.dreps.insert(
                        reg.credential.clone(),
                        DRepRecord::new(reg.deposit, reg.anchor.clone()),
                    );
                    Ok(true)
                }
            },
            TxCertificate::DRepDeregistration(reg) => {
                if self.dreps.remove(&reg.credential).is_none() {
                    Err(anyhow!(
                        "DRep registration {:?}: internal error, credential not found",
                        reg.credential
                    ))
                } else {
                    Ok(true)
                }
            }
            TxCertificate::DRepUpdate(reg) => match self.dreps.get_mut(&reg.credential) {
                Some(ref mut drep) => {
                    drep.anchor = reg.anchor.clone();
                    Ok(false)
                }
                None => Err(anyhow!(
                    "DRep registration {:?}: internal error, credential not found",
                    reg.credential
                )),
            },
            _ => Ok(false),
        }
    }

    pub fn active_drep_list(&self) -> Vec<(DRepCredential, Lovelace)> {
        let mut distribution = Vec::new();
        for (drep, drep_info) in self.dreps.iter() {
            distribution.push((drep.clone(), drep_info.deposit));
        }
        distribution
    }

    pub async fn handle(&mut self, tx_cert_msg: &TxCertificatesMessage) -> Result<()> {
        for tx_cert in tx_cert_msg.certificates.iter() {
            if let Err(e) = self.process_one_certificate(tx_cert) {
                tracing::error!("Error processing tx_cert {}", e);
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::state::{DRepRecord, State};
    use acropolis_common::{
        Anchor, Credential, DRepDeregistration, DRepRegistration, DRepUpdate, TxCertificate,
    };

    const CRED_1: [u8; 28] = [
        123, 222, 247, 170, 243, 201, 37, 233, 124, 164, 45, 54, 241, 25, 176, 70, 154, 18, 204,
        164, 161, 126, 207, 239, 198, 144, 3, 80,
    ];
    const CRED_2: [u8; 28] = [
        124, 223, 248, 171, 244, 202, 38, 234, 125, 165, 46, 55, 242, 26, 177, 71, 155, 19, 205,
        165, 162, 127, 208, 240, 199, 145, 4, 81,
    ];

    #[test]
    fn test_drep_process_one_certificate() {
        let tx_cred = Credential::AddrKeyHash(CRED_1.to_vec());
        let tx_cert = TxCertificate::DRepRegistration(DRepRegistration {
            credential: tx_cred.clone(),
            deposit: 500000000,
            anchor: None,
        });
        let mut state = State::new();
        assert_eq!(state.process_one_certificate(&tx_cert).unwrap(), true);
        assert_eq!(state.get_count(), 1);
        let tx_cert_record = DRepRecord {
            deposit: 500000000,
            anchor: None,
        };
        assert_eq!(
            state.get_drep(&tx_cred).unwrap().deposit,
            tx_cert_record.deposit
        );
    }

    #[test]
    fn test_drep_do_not_replace_existing_certificate() {
        let tx_cred = Credential::AddrKeyHash(CRED_1.to_vec());
        let tx_cert = TxCertificate::DRepRegistration(DRepRegistration {
            credential: tx_cred.clone(),
            deposit: 500000000,
            anchor: None,
        });
        let mut state = State::new();
        assert_eq!(state.process_one_certificate(&tx_cert).unwrap(), true);

        let bad_tx_cert = TxCertificate::DRepRegistration(DRepRegistration {
            credential: tx_cred.clone(),
            deposit: 600000000,
            anchor: None,
        });
        assert!(state.process_one_certificate(&bad_tx_cert).is_err());

        assert_eq!(state.get_count(), 1);
        let tx_cert_record = DRepRecord {
            deposit: 500000000,
            anchor: None,
        };
        assert_eq!(
            state.get_drep(&tx_cred).unwrap().deposit,
            tx_cert_record.deposit
        );
    }

    #[test]
    fn test_drep_update_certificate() {
        let tx_cred = Credential::AddrKeyHash(CRED_1.to_vec());
        let tx_cert = TxCertificate::DRepRegistration(DRepRegistration {
            credential: tx_cred.clone(),
            deposit: 500000000,
            anchor: None,
        });
        let mut state = State::new();
        assert_eq!(state.process_one_certificate(&tx_cert).unwrap(), true);

        let anchor = Anchor {
            url: "https://poop.bike".into(),
            data_hash: vec![0x13, 0x37],
        };
        let update_anchor_tx_cert = TxCertificate::DRepUpdate(DRepUpdate {
            credential: tx_cred.clone(),
            anchor: Some(anchor.clone()),
        });

        assert_eq!(
            state.process_one_certificate(&update_anchor_tx_cert).unwrap(),
            false
        );

        assert_eq!(state.get_count(), 1);
        let tx_cert_record = DRepRecord {
            deposit: 500000000,
            anchor: Some(anchor),
        };
        assert_eq!(
            state.get_drep(&tx_cred).unwrap().anchor,
            tx_cert_record.anchor
        );
    }

    #[test]
    fn test_drep_do_not_update_nonexistent_certificate() {
        let tx_cred = Credential::AddrKeyHash(CRED_1.to_vec());
        let tx_cert = TxCertificate::DRepRegistration(DRepRegistration {
            credential: tx_cred.clone(),
            deposit: 500000000,
            anchor: None,
        });
        let mut state = State::new();
        assert_eq!(state.process_one_certificate(&tx_cert).unwrap(), true);

        let anchor = Anchor {
            url: "https://poop.bike".into(),
            data_hash: vec![0x13, 0x37],
        };
        let update_anchor_tx_cert = TxCertificate::DRepUpdate(DRepUpdate {
            credential: Credential::AddrKeyHash(CRED_2.to_vec()),
            anchor: Some(anchor.clone()),
        });

        assert!(state.process_one_certificate(&update_anchor_tx_cert).is_err());

        assert_eq!(state.get_count(), 1);
        let tx_cert_record = DRepRecord {
            deposit: 500000000,
            anchor: Some(anchor),
        };
        assert_eq!(
            state.get_drep(&tx_cred).unwrap().deposit,
            tx_cert_record.deposit
        );
    }

    #[test]
    fn test_drep_deregister() {
        let tx_cred = Credential::AddrKeyHash(CRED_1.to_vec());
        let tx_cert = TxCertificate::DRepRegistration(DRepRegistration {
            credential: tx_cred.clone(),
            deposit: 500000000,
            anchor: None,
        });
        let mut state = State::new();
        assert_eq!(state.process_one_certificate(&tx_cert).unwrap(), true);

        let unregister_tx_cert = TxCertificate::DRepDeregistration(DRepDeregistration {
            credential: tx_cred.clone(),
            refund: 500000000,
        });
        assert_eq!(
            state.process_one_certificate(&unregister_tx_cert).unwrap(),
            true
        );
        assert_eq!(state.get_count(), 0);
        assert!(state.get_drep(&tx_cred).is_none());
    }

    #[test]
    fn test_drep_do_not_deregister_nonexistent_cert() {
        let tx_cred = Credential::AddrKeyHash(CRED_1.to_vec());
        let tx_cert = TxCertificate::DRepRegistration(DRepRegistration {
            credential: tx_cred.clone(),
            deposit: 500000000,
            anchor: None,
        });
        let mut state = State::new();
        assert_eq!(state.process_one_certificate(&tx_cert).unwrap(), true);

        let unregister_tx_cert = TxCertificate::DRepDeregistration(DRepDeregistration {
            credential: Credential::AddrKeyHash(CRED_2.to_vec()),
            refund: 500000000,
        });
        assert!(state.process_one_certificate(&unregister_tx_cert).is_err());
        assert_eq!(state.get_count(), 1);
        assert_eq!(state.get_drep(&tx_cred).unwrap().deposit, 500000000);
    }
}
