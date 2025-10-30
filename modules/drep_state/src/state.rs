//! Acropolis DRepState: State storage

use acropolis_common::{
    messages::{Message, StateQuery, StateQueryResponse},
    queries::{
        accounts::{AccountsStateQuery, AccountsStateQueryResponse, DEFAULT_ACCOUNTS_QUERY_TOPIC},
        get_query_topic,
        governance::{DRepActionUpdate, DRepUpdateEvent, VoteRecord},
    },
    Anchor, DRepChoice, DRepCredential, Lovelace, StakeAddress, TxCertificate,
    TxCertificateWithPos, TxHash, Voter, VotingProcedures,
};
use anyhow::{anyhow, Result};
use caryatid_sdk::Context;
use serde_with::serde_as;
use std::{collections::HashMap, sync::Arc};
use tracing::{error, info};

#[serde_as]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DRepRecord {
    pub deposit: Lovelace,
    pub anchor: Option<Anchor>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HistoricalDRepState {
    // Populated from the reg field in:
    // - DRepRegistration
    // - DRepDeregistration
    // - DRepUpdate
    pub info: Option<DRepRecordExtended>,
    pub updates: Option<Vec<DRepUpdateEvent>>,
    pub metadata: Option<Option<Anchor>>,

    // Populated from the drep and credential fields in:
    // - VoteDelegation
    // - StakeAndVoteDelegation
    // - StakeRegistrationAndVoteDelegation
    // - StakeRegistrationAndStakeAndVoteDelegation
    pub delegators: Option<Vec<StakeAddress>>,

    // Populated from voting_procedures in GovernanceProceduresMessage
    pub votes: Option<Vec<VoteRecord>>,
}

impl HistoricalDRepState {
    pub fn from_config(cfg: &DRepStorageConfig) -> Self {
        Self {
            info: cfg.store_info.then(DRepRecordExtended::default),
            updates: cfg.store_updates.then(Vec::new),
            metadata: cfg.store_metadata.then_some(None),
            delegators: cfg.store_delegators.then(Vec::new),
            votes: cfg.store_votes.then(Vec::new),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct DRepRecordExtended {
    pub deposit: Lovelace,
    pub expired: bool,
    pub retired: bool,
    pub active_epoch: Option<u64>,
    pub last_active_epoch: u64,
}

impl DRepRecord {
    pub fn new(deposit: Lovelace, anchor: Option<Anchor>) -> Self {
        Self { deposit, anchor }
    }
}

#[derive(Debug, Copy, Clone, Default)]
pub struct DRepStorageConfig {
    pub store_info: bool,
    pub store_delegators: bool,
    pub store_metadata: bool,
    pub store_updates: bool,
    pub store_votes: bool,
}

impl DRepStorageConfig {
    fn any_enabled(&self) -> bool {
        self.store_info
            || self.store_delegators
            || self.store_metadata
            || self.store_updates
            || self.store_votes
    }
}

#[derive(Debug, Default, Clone)]
pub struct State {
    pub config: DRepStorageConfig,
    pub dreps: HashMap<DRepCredential, DRepRecord>,
    pub historical_dreps: Option<HashMap<DRepCredential, HistoricalDRepState>>,
}

impl State {
    pub fn new(config: DRepStorageConfig) -> Self {
        Self {
            config,
            dreps: HashMap::new(),
            historical_dreps: if config.any_enabled() {
                Some(HashMap::new())
            } else {
                None
            },
        }
    }

    #[allow(dead_code)]
    pub fn get_count(&self) -> usize {
        self.dreps.len()
    }

    #[allow(dead_code)]
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

    pub fn get_drep_info(
        &self,
        credential: &DRepCredential,
    ) -> Result<Option<&DRepRecordExtended>, &'static str> {
        let hist = self
            .historical_dreps
            .as_ref()
            .ok_or("Historical DRep storage is disabled by configuration.")?;
        match hist.get(credential) {
            Some(e) => {
                e.info.as_ref().ok_or("DRep info storage is disabled by configuration.").map(Some)
            }
            None => Ok(None),
        }
    }

    pub fn get_drep_delegators(
        &self,
        credential: &DRepCredential,
    ) -> Result<Option<&Vec<StakeAddress>>, &'static str> {
        let hist = self
            .historical_dreps
            .as_ref()
            .ok_or("Historical DRep storage is disabled by configuration.")?;
        match hist.get(credential) {
            Some(e) => e
                .delegators
                .as_ref()
                .ok_or("DRep delegator storage is disabled by configuration.")
                .map(Some),
            None => Ok(None),
        }
    }

    pub fn get_drep_anchor(
        &self,
        credential: &DRepCredential,
    ) -> Result<Option<&Anchor>, &'static str> {
        let hist = self
            .historical_dreps
            .as_ref()
            .ok_or("Historical DRep storage is disabled by configuration.")?;
        match hist.get(credential) {
            Some(e) => e.metadata.as_ref().ok_or("DRep metadata not found").map(|m| m.as_ref()),
            None => Ok(None),
        }
    }

    pub fn get_drep_updates(
        &self,
        credential: &DRepCredential,
    ) -> Result<Option<&Vec<DRepUpdateEvent>>, &'static str> {
        let hist = self
            .historical_dreps
            .as_ref()
            .ok_or("Historical DRep storage is disabled by configuration.")?;
        match hist.get(credential) {
            Some(e) => e
                .updates
                .as_ref()
                .ok_or("DRep updates storage is disabled by configuration.")
                .map(Some),
            None => Ok(None),
        }
    }

    pub fn get_drep_votes(
        &self,
        credential: &DRepCredential,
    ) -> Result<Option<&Vec<VoteRecord>>, &'static str> {
        let hist = self
            .historical_dreps
            .as_ref()
            .ok_or("Historical DRep storage is disabled by configuration.")?;
        match hist.get(credential) {
            Some(e) => {
                e.votes.as_ref().ok_or("DRep votes storage is disabled by configuration.").map(Some)
            }
            None => Ok(None),
        }
    }
}

impl State {
    pub async fn process_certificates(
        &mut self,
        context: Arc<Context<Message>>,
        tx_certs: &Vec<TxCertificateWithPos>,
        epoch: u64,
    ) -> Result<()> {
        let mut batched_delegators = Vec::new();
        let store_delegators = self.config.store_delegators;

        for tx_cert in tx_certs {
            if store_delegators {
                if let Some((delegator, drep)) = Self::extract_delegation_fields(&tx_cert.cert) {
                    batched_delegators.push((delegator, drep));
                    continue;
                }
            }

            if let Err(e) = self.process_one_cert(tx_cert, epoch) {
                error!("Error processing tx_cert: {e}");
            }
        }

        // Batched delegations to reduce redundant queries to accounts_state
        if store_delegators && !batched_delegators.is_empty() {
            if let Err(e) = self.update_delegators(&context, &batched_delegators).await {
                error!("Error processing batched delegators: {e}");
            }
        }

        Ok(())
    }

    pub fn process_votes(
        &mut self,
        voting_procedures: &[(TxHash, VotingProcedures)],
    ) -> Result<()> {
        let Some(hist_map) = self.historical_dreps.as_mut() else {
            return Ok(());
        };

        let cfg = self.config;
        for (tx_hash, voting_procedures) in voting_procedures {
            for (voter, single_votes) in &voting_procedures.votes {
                let drep_cred = match voter {
                    Voter::DRepKey(k) => DRepCredential::AddrKeyHash(k.to_vec()),
                    Voter::DRepScript(s) => DRepCredential::ScriptHash(s.to_vec()),
                    _ => continue,
                };

                let entry = hist_map
                    .entry(drep_cred)
                    .or_insert_with(|| HistoricalDRepState::from_config(&cfg));

                let votes = entry.votes.as_mut().unwrap();

                for vp in single_votes.voting_procedures.values() {
                    votes.push(VoteRecord {
                        tx_hash: *tx_hash,
                        vote_index: vp.vote_index,
                        vote: vp.vote.clone(),
                    });
                }
            }
        }
        Ok(())
    }

    pub fn update_drep_expirations(
        &mut self,
        current_epoch: u64,
        expired_epoch_param: u32,
    ) -> Result<()> {
        let expired_offset = expired_epoch_param as u64;

        // If historical storage isn’t enabled, nothing to do.
        let Some(historical_dreps) = self.historical_dreps.as_mut() else {
            return Ok(());
        };

        for (_cred, drep_record) in historical_dreps.iter_mut() {
            if let Some(info) = drep_record.info.as_mut() {
                if let (Some(active_epoch), false) = (info.active_epoch, info.expired) {
                    if active_epoch + expired_offset <= current_epoch {
                        info.expired = true;
                    }
                }
            }
        }

        Ok(())
    }

    pub fn active_drep_list(&self) -> Vec<(DRepCredential, Lovelace)> {
        let mut distribution = Vec::new();
        for (drep, drep_info) in self.dreps.iter() {
            distribution.push((drep.clone(), drep_info.deposit));
        }
        distribution
    }

    fn process_one_cert(&mut self, tx_cert: &TxCertificateWithPos, epoch: u64) -> Result<bool> {
        match &tx_cert.cert {
            TxCertificate::DRepRegistration(reg) => {
                let new = match self.dreps.get_mut(&reg.credential) {
                    Some(drep) => {
                        if reg.deposit != 0 {
                            return Err(anyhow!(
                                "DRep registration {:?}: replacement requires deposit = 0, got {}",
                                reg.credential,
                                reg.deposit
                            ));
                        }
                        drep.anchor = reg.anchor.clone();
                        false
                    }
                    None => {
                        self.dreps.insert(
                            reg.credential.clone(),
                            DRepRecord::new(reg.deposit, reg.anchor.clone()),
                        );
                        true
                    }
                };

                if self.historical_dreps.is_some() {
                    if let Err(err) = self.update_historical(&reg.credential, true, |entry| {
                        if let Some(info) = entry.info.as_mut() {
                            info.deposit = reg.deposit;
                            info.expired = false;
                            info.retired = false;
                            info.active_epoch = Some(epoch);
                            info.last_active_epoch = epoch;
                        }
                        if let Some(updates) = entry.updates.as_mut() {
                            updates.push(DRepUpdateEvent {
                                tx_identifier: tx_cert.tx_identifier,
                                cert_index: tx_cert.cert_index,
                                action: DRepActionUpdate::Registered,
                            });
                        }
                        if let Some(anchor) = &reg.anchor {
                            if let Some(inner) = entry.metadata.as_mut() {
                                *inner = Some(anchor.clone());
                            }
                        }
                    }) {
                        return Err(anyhow!("Failed to update DRep on registration: {err}"));
                    }
                }

                Ok(new)
            }

            TxCertificate::DRepDeregistration(reg) => {
                // Update live state
                if self.dreps.remove(&reg.credential).is_none() {
                    return Err(anyhow!(
                        "DRep deregistration {:?}: credential not found",
                        reg.credential
                    ));
                }

                // Update history if enabled
                if self.historical_dreps.is_some() {
                    if let Err(err) = self.update_historical(&reg.credential, false, |entry| {
                        if let Some(info) = entry.info.as_mut() {
                            info.deposit = 0;
                            info.expired = false;
                            info.retired = true;
                            info.active_epoch = None;
                            info.last_active_epoch = epoch;
                        }
                        if let Some(updates) = entry.updates.as_mut() {
                            updates.push(DRepUpdateEvent {
                                tx_identifier: tx_cert.tx_identifier,
                                cert_index: tx_cert.cert_index,
                                action: DRepActionUpdate::Deregistered,
                            });
                        }
                    }) {
                        return Err(anyhow!("Failed to update DRep on deregistration: {err}"));
                    }
                }

                Ok(true)
            }

            TxCertificate::DRepUpdate(reg) => {
                // Update live state
                let drep = self.dreps.get_mut(&reg.credential).ok_or_else(|| {
                    anyhow!("DRep update {:?}: credential not found", reg.credential)
                })?;
                drep.anchor = reg.anchor.clone();

                // Update history if enabled
                if let Err(err) = self.update_historical(&reg.credential, false, |entry| {
                    if let Some(info) = entry.info.as_mut() {
                        info.expired = false;
                        info.retired = false;
                        info.last_active_epoch = epoch;
                    }
                    if let Some(updates) = entry.updates.as_mut() {
                        updates.push(DRepUpdateEvent {
                            tx_identifier: tx_cert.tx_identifier,
                            cert_index: tx_cert.cert_index,
                            action: DRepActionUpdate::Updated,
                        });
                    }
                    if let Some(anchor) = &reg.anchor {
                        if let Some(inner) = entry.metadata.as_mut() {
                            *inner = Some(anchor.clone());
                        }
                    }
                }) {
                    error!("Historical update failed: {err}");
                }

                Ok(false)
            }

            _ => Ok(false),
        }
    }

    fn update_historical<F>(
        &mut self,
        credential: &DRepCredential,
        create_if_missing: bool,
        f: F,
    ) -> Result<()>
    where
        F: FnOnce(&mut HistoricalDRepState),
    {
        let Some(hist) = self.historical_dreps.as_mut() else {
            return Ok(());
        };

        if create_if_missing {
            let cfg = self.config;
            let entry = hist
                .entry(credential.clone())
                .or_insert_with(|| HistoricalDRepState::from_config(&cfg));
            f(entry);
        } else if let Some(entry) = hist.get_mut(credential) {
            f(entry);
        } else {
            error!("Tried to update unknown DRep credential: {:?}", credential);
        }

        Ok(())
    }

    async fn update_delegators(
        &mut self,
        context: &Arc<Context<Message>>,
        delegators: &[(&StakeAddress, &DRepChoice)],
    ) -> Result<()> {
        let mut stake_key_to_input = HashMap::with_capacity(delegators.len());
        let mut stake_addresses = Vec::with_capacity(delegators.len());

        for &(sc, drep) in delegators {
            stake_addresses.push(sc.clone());
            stake_key_to_input.insert(sc.get_credential().get_hash(), (sc, drep));
        }

        let msg = Arc::new(Message::StateQuery(StateQuery::Accounts(
            AccountsStateQuery::GetAccountsDrepDelegationsMap { stake_addresses },
        )));

        let accounts_query_topic = get_query_topic(context.clone(), DEFAULT_ACCOUNTS_QUERY_TOPIC);
        let response = context.message_bus.request(&accounts_query_topic, msg).await?;
        let message = Arc::try_unwrap(response).unwrap_or_else(|arc| (*arc).clone());

        // TODO: Ensure AccountsStateQueryResponse is for the correct block
        let result_map = match message {
            Message::StateQueryResponse(StateQueryResponse::Accounts(
                AccountsStateQueryResponse::AccountsDrepDelegationsMap(map),
            )) => map,
            _ => {
                return Err(anyhow!("Unexpected accounts-state response"));
            }
        };

        for (stake_key, old_drep_opt) in result_map {
            let &(delegator, new_drep_choice) = match stake_key_to_input.get(&stake_key) {
                Some(pair) => pair,
                None => continue,
            };

            let new_drep_cred = match drep_choice_to_credential(new_drep_choice) {
                Some(c) => c,
                None => continue,
            };

            if let Some(old_drep) = old_drep_opt {
                if let Some(old_drep_cred) = drep_choice_to_credential(&old_drep) {
                    if old_drep_cred != new_drep_cred {
                        self.update_historical(&old_drep_cred, false, |entry| {
                            if let Some(delegators) = entry.delegators.as_mut() {
                                delegators.retain(|s| {
                                    s.get_credential().get_hash()
                                        != delegator.get_credential().get_hash()
                                });
                            }
                        })?;
                    }
                }
            }

            // Add delegator to new DRep
            match self.update_historical(&new_drep_cred, true, |entry| {
                if let Some(delegators) = entry.delegators.as_mut() {
                    if !delegators.contains(delegator) {
                        delegators.push(delegator.clone());
                    }
                }
            }) {
                Ok(_) => {}
                Err(err) => return Err(anyhow!("Failed to update new delegator: {err}")),
            }
        }

        Ok(())
    }

    fn extract_delegation_fields(cert: &TxCertificate) -> Option<(&StakeAddress, &DRepChoice)> {
        match cert {
            TxCertificate::VoteDelegation(d) => Some((&d.stake_address, &d.drep)),
            TxCertificate::StakeAndVoteDelegation(d) => Some((&d.stake_address, &d.drep)),
            TxCertificate::StakeRegistrationAndVoteDelegation(d) => {
                Some((&d.stake_address, &d.drep))
            }
            TxCertificate::StakeRegistrationAndStakeAndVoteDelegation(d) => {
                Some((&d.stake_address, &d.drep))
            }
            _ => None,
        }
    }
}

fn drep_choice_to_credential(choice: &DRepChoice) -> Option<DRepCredential> {
    match choice {
        DRepChoice::Key(k) => Some(DRepCredential::AddrKeyHash(k.clone())),
        DRepChoice::Script(k) => Some(DRepCredential::ScriptHash(k.clone())),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use crate::state::{DRepRecord, DRepStorageConfig, State};
    use acropolis_common::{
        Anchor, Credential, DRepDeregistration, DRepRegistration, DRepUpdate, TxCertificate,
        TxCertificateWithPos, TxIdentifier,
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
        let tx_cert = TxCertificateWithPos {
            cert: TxCertificate::DRepRegistration(DRepRegistration {
                credential: tx_cred.clone(),
                deposit: 500000000,
                anchor: None,
            }),
            tx_identifier: TxIdentifier::default(),
            cert_index: 0,
        };
        let mut state = State::new(DRepStorageConfig::default());
        assert!(state.process_one_cert(&tx_cert, 1).unwrap());
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
        let tx_cert = TxCertificateWithPos {
            cert: TxCertificate::DRepRegistration(DRepRegistration {
                credential: tx_cred.clone(),
                deposit: 500000000,
                anchor: None,
            }),
            tx_identifier: TxIdentifier::default(),
            cert_index: 0,
        };

        let mut state = State::new(DRepStorageConfig::default());
        assert!(state.process_one_cert(&tx_cert, 1).unwrap());

        let bad_tx_cert = TxCertificateWithPos {
            cert: TxCertificate::DRepRegistration(DRepRegistration {
                credential: tx_cred.clone(),
                deposit: 600000000,
                anchor: None,
            }),
            tx_identifier: TxIdentifier::default(),
            cert_index: 1,
        };
        assert!(state.process_one_cert(&bad_tx_cert, 1).is_err());

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
        let tx_cert = TxCertificateWithPos {
            cert: TxCertificate::DRepRegistration(DRepRegistration {
                credential: tx_cred.clone(),
                deposit: 500000000,
                anchor: None,
            }),
            tx_identifier: TxIdentifier::default(),
            cert_index: 1,
        };
        let mut state = State::new(DRepStorageConfig::default());
        assert!(state.process_one_cert(&tx_cert, 1).unwrap());

        let anchor = Anchor {
            url: "https://poop.bike".into(),
            data_hash: vec![0x13, 0x37],
        };
        let update_anchor_tx_cert = TxCertificateWithPos {
            cert: TxCertificate::DRepUpdate(DRepUpdate {
                credential: tx_cred.clone(),
                anchor: Some(anchor.clone()),
            }),
            tx_identifier: TxIdentifier::default(),
            cert_index: 1,
        };

        assert!(!state.process_one_cert(&update_anchor_tx_cert, 1).unwrap());

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
        let tx_cert = TxCertificateWithPos {
            cert: TxCertificate::DRepRegistration(DRepRegistration {
                credential: tx_cred.clone(),
                deposit: 500000000,
                anchor: None,
            }),
            tx_identifier: TxIdentifier::default(),
            cert_index: 1,
        };
        let mut state = State::new(DRepStorageConfig::default());
        assert!(state.process_one_cert(&tx_cert, 1).unwrap());

        let anchor = Anchor {
            url: "https://poop.bike".into(),
            data_hash: vec![0x13, 0x37],
        };
        let update_anchor_tx_cert = TxCertificateWithPos {
            cert: TxCertificate::DRepUpdate(DRepUpdate {
                credential: Credential::AddrKeyHash(CRED_2.to_vec()),
                anchor: Some(anchor.clone()),
            }),
            tx_identifier: TxIdentifier::default(),
            cert_index: 1,
        };
        assert!(state.process_one_cert(&update_anchor_tx_cert, 1).is_err());

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
        let tx_cert = TxCertificateWithPos {
            cert: TxCertificate::DRepRegistration(DRepRegistration {
                credential: tx_cred.clone(),
                deposit: 500000000,
                anchor: None,
            }),
            tx_identifier: TxIdentifier::default(),
            cert_index: 1,
        };
        let mut state = State::new(DRepStorageConfig::default());
        assert!(state.process_one_cert(&tx_cert, 1).unwrap());

        let unregister_tx_cert = TxCertificateWithPos {
            cert: TxCertificate::DRepDeregistration(DRepDeregistration {
                credential: tx_cred.clone(),
                refund: 500000000,
            }),
            tx_identifier: TxIdentifier::default(),
            cert_index: 1,
        };
        assert!(state.process_one_cert(&unregister_tx_cert, 1).unwrap());
        assert_eq!(state.get_count(), 0);
        assert!(state.get_drep(&tx_cred).is_none());
    }

    #[test]
    fn test_drep_do_not_deregister_nonexistent_cert() {
        let tx_cred = Credential::AddrKeyHash(CRED_1.to_vec());
        let tx_cert = TxCertificateWithPos {
            cert: TxCertificate::DRepRegistration(DRepRegistration {
                credential: tx_cred.clone(),
                deposit: 500000000,
                anchor: None,
            }),
            tx_identifier: TxIdentifier::default(),
            cert_index: 1,
        };
        let mut state = State::new(DRepStorageConfig::default());
        assert!(state.process_one_cert(&tx_cert, 1).unwrap());

        let unregister_tx_cert = TxCertificateWithPos {
            cert: TxCertificate::DRepDeregistration(DRepDeregistration {
                credential: Credential::AddrKeyHash(CRED_2.to_vec()),
                refund: 500000000,
            }),
            tx_identifier: TxIdentifier::default(),
            cert_index: 1,
        };
        assert!(state.process_one_cert(&unregister_tx_cert, 1).is_err());
        assert_eq!(state.get_count(), 1);
        assert_eq!(state.get_drep(&tx_cred).unwrap().deposit, 500000000);
    }
}
