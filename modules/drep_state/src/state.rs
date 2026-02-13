//! Acropolis DRepState: State storage

use acropolis_common::{
    messages::{DRepBootstrapMessage, Message, StateQuery, StateQueryResponse},
    protocol_params::ProtocolParams,
    queries::{
        accounts::{AccountsStateQuery, AccountsStateQueryResponse, DEFAULT_ACCOUNTS_QUERY_TOPIC},
        get_query_topic,
        governance::{DRepActionUpdate, DRepUpdateEvent, VoteRecord},
    },
    validation::ValidationOutcomes,
    Anchor, DRepChoice, DRepCredential, DRepRecord, GovActionId, Lovelace, ProposalProcedure,
    StakeAddress, TxCertificate, TxCertificateWithPos, TxHash, Voter, VotingProcedures,
};
use anyhow::{anyhow, bail, Result};
use caryatid_sdk::Context;
use std::{collections::HashMap, sync::Arc};
use tracing::info;

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

    /// Per-DRep expiry epoch.
    pub drep_expiry: HashMap<DRepCredential, u64>,

    /// Dormant epoch counter.
    pub num_dormant_epochs: u64,

    /// Government proposals currently alive (i.e. not expired) with their expiry epoch.
    pub proposals_expires_after: HashMap<GovActionId, u64>,

    pub historical_dreps: Option<HashMap<DRepCredential, HistoricalDRepState>>,

    /// Conway protocol parameter: DRep activity period (epochs).
    pub conway_d_rep_activity: Option<u32>,

    /// Conway protocol parameter: governance action lifetime (epochs).
    pub conway_gov_action_lifetime: Option<u32>,

    /// Derived from Shelley protocol parameter: for bootstrap phase detection (protocol version).
    /// See update_drep_expiry_versioned() for further information
    pub is_pv9: Option<bool>,
}

impl State {
    pub fn new(config: DRepStorageConfig) -> Self {
        Self {
            config,
            dreps: HashMap::new(),
            drep_expiry: HashMap::new(),
            num_dormant_epochs: 0,
            proposals_expires_after: HashMap::new(),
            historical_dreps: if config.any_enabled() {
                Some(HashMap::new())
            } else {
                None
            },
            conway_d_rep_activity: None,
            conway_gov_action_lifetime: None,
            is_pv9: None,
        }
    }

    /// Applies accumulated dormant-epoch adjustments to all stored DRep expiries.
    /// The dormancy counter is reset to zero after application.
    pub fn apply_dormant_expiry(&mut self, current_epoch: u64) {
        if self.num_dormant_epochs == 0 {
            return;
        }

        for expiry in self.drep_expiry.values_mut() {
            let actual_expiry = *expiry + self.num_dormant_epochs;
            if actual_expiry >= current_epoch {
                *expiry = actual_expiry;
            }
        }

        self.num_dormant_epochs = 0;
    }

    /// Records proposals observed in a block with their expiry epoch.
    pub fn record_proposals(&mut self, proposals: &[ProposalProcedure], current_epoch: u64) {
        let Some(gov_action_lifetime) = self.conway_gov_action_lifetime else {
            return;
        };
        let expires_after = current_epoch + (gov_action_lifetime as u64);
        for proposal in proposals {
            self.proposals_expires_after
                .entry(proposal.gov_action_id.clone())
                .or_insert(expires_after);
        }
    }

    /// At epoch boundary, if there are no proposals active (i.e. no proposal with
    /// `expires_after >= current_epoch`), increment `num_dormant_epochs`.
    pub fn update_num_dormant_epochs(&mut self, current_epoch: u64) {
        // Drop expired proposals first.
        self.proposals_expires_after.retain(|_, expires_after| *expires_after >= current_epoch);

        if self.proposals_expires_after.is_empty() {
            self.num_dormant_epochs += 1;
        }
    }

    /// Update protocol parameters from a ProtocolParams.
    pub fn update_protocol_params(&mut self, params: &ProtocolParams) -> Result<()> {
        if let (Some(shelley), Some(conway)) = (&params.shelley, &params.conway) {
            self.conway_d_rep_activity = Some(conway.d_rep_activity);
            self.conway_gov_action_lifetime = Some(conway.gov_action_lifetime);

            // 'Chang' is PV9.
            self.is_pv9 = Some(shelley.protocol_params.protocol_version.is_chang()?);
        } else if params.conway.is_some() {
            bail!("Invalid protocol parameters: Conway parameters require Shelley parameters.");
        }

        Ok(())
    }

    /// Compute DRep expiry for registration (versioned behavior).
    /// During Conway bootstrap phase (protocol version 9.x), dormant epochs are not subtracted.
    /// After version 10+, dormant epochs are subtracted.
    /// Reference: https://github.com/IntersectMBO/cardano-ledger/blob/master/eras/conway/impl/src/Cardano/Ledger/Conway/Rules/GovCert.hs#L290
    fn update_drep_expiry_versioned(
        &mut self,
        drep_cred: &DRepCredential,
        epoch: u64,
        drep_activity: u32,
    ) -> Result<()> {
        let expiry = match self.is_pv9 {
            // Bootstrap phase: expiry = currentEpoch + drepActivity
            Some(true) => epoch + (drep_activity as u64),
            // Post-bootstrap: expiry = (currentEpoch + drepActivity) − numDormantEpochs
            Some(false) => (epoch + (drep_activity as u64)) - self.num_dormant_epochs,
            None => bail!("Bootstrap state unknown when updating DRep expiry"),
        };

        self.drep_expiry.insert(drep_cred.clone(), expiry);

        Ok(())
    }

    /// Compute DRep expiry for updates/votes (always subtracts dormant epochs).
    fn update_drep_expiry(&mut self, drep_cred: &DRepCredential, epoch: u64, drep_activity: u32) {
        // drepExpiry = (currentEpoch+drepActivity) − numDormantEpochs
        let expiry = (epoch + (drep_activity as u64)) - self.num_dormant_epochs;
        self.drep_expiry.insert(drep_cred.clone(), expiry);
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
        drep_activity: Option<u32>,
    ) -> Result<ValidationOutcomes> {
        let mut vld = ValidationOutcomes::new();
        let mut batched_delegators = Vec::new();
        let store_delegators = self.config.store_delegators;

        for tx_cert in tx_certs {
            if store_delegators {
                if let Some((delegator, drep)) = Self::extract_delegation_fields(&tx_cert.cert) {
                    batched_delegators.push((delegator, drep));
                    continue;
                }
            }

            if let Err(e) = self.process_one_cert(tx_cert, epoch, &mut vld, drep_activity) {
                vld.push_anyhow(anyhow!("Error processing tx_cert: {e}"));
            }
        }

        // Batched delegations to reduce redundant queries to accounts_state
        if store_delegators && !batched_delegators.is_empty() {
            if let Err(e) = self.update_delegators(&context, &batched_delegators, &mut vld).await {
                vld.push_anyhow(anyhow!("Error processing batched delegators: {e}"));
            }
        }

        Ok(vld)
    }

    pub fn process_votes(
        &mut self,
        total_voting_procedures: &[(TxHash, VotingProcedures)],
        epoch: u64,
        drep_activity: Option<u32>,
    ) -> Result<ValidationOutcomes> {
        let vld = ValidationOutcomes::new();
        let cfg = self.config;

        // Nothing to process for pre-Conway blocks (empty voting procedures)
        if total_voting_procedures.is_empty() {
            return Ok(vld);
        }

        let drep_activity = drep_activity.ok_or_else(|| {
            anyhow!("Missing Conway parameter d_rep_activity (required to compute drepExpiry)")
        })?;

        // Update `drep_expiry` for DReps that vote. Update historical only if enabled.
        for (tx_hash, voting_procedures) in total_voting_procedures {
            for (voter, single_votes) in &voting_procedures.votes {
                let drep_cred = match voter {
                    Voter::DRepKey(k) => DRepCredential::AddrKeyHash(k.into_inner()),
                    Voter::DRepScript(s) => DRepCredential::ScriptHash(s.into_inner()),
                    _ => continue,
                };

                self.update_drep_expiry(&drep_cred, epoch, drep_activity);

                if let Some(ref mut hist_map) = self.historical_dreps {
                    let entry = hist_map
                        .entry(drep_cred)
                        .or_insert_with(|| HistoricalDRepState::from_config(&cfg));

                    // Voting is activity: reset inactivity fields if we track them.
                    if let Some(info) = entry.info.as_mut() {
                        info.expired = false;
                        info.active_epoch = Some(epoch);
                        info.last_active_epoch = epoch;
                    }

                    if let Some(votes) = entry.votes.as_mut() {
                        for vp in single_votes.voting_procedures.values() {
                            votes.push(VoteRecord {
                                tx_hash: *tx_hash,
                                vote_index: vp.vote_index,
                                vote: vp.vote.clone(),
                            });
                        }
                    }
                }
            }
        }
        Ok(vld)
    }

    pub fn update_drep_expirations(&mut self, current_epoch: u64) -> Result<()> {
        // If historical storage isn’t enabled, nothing to do.
        let Some(historical_dreps) = self.historical_dreps.as_mut() else {
            return Ok(());
        };

        for (cred, drep_record) in historical_dreps.iter_mut() {
            if let Some(info) = drep_record.info.as_mut() {
                // Historical "expired" reflects the current derived status from drep_expiry.
                // If we do not have an expiry tracked, treat as expired.
                info.expired = self
                    .drep_expiry
                    .get(cred)
                    .copied()
                    .map(|expiry| expiry < current_epoch)
                    .unwrap_or(true);
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

    pub fn inactive_drep_list(&self, current_epoch: u64) -> Vec<DRepCredential> {
        self.dreps
            .keys()
            .filter(|cred| self.drep_expiry.get(*cred).is_none_or(|expiry| *expiry < current_epoch))
            .cloned()
            .collect()
    }

    fn process_one_cert(
        &mut self,
        tx_cert: &TxCertificateWithPos,
        epoch: u64,
        vld: &mut ValidationOutcomes,
        drep_activity: Option<u32>,
    ) -> Result<bool> {
        match &tx_cert.cert {
            TxCertificate::DRepRegistration(reg) => {
                let drep_activity = drep_activity.ok_or_else(|| {
                    anyhow!(
                        "Missing Conway parameter d_rep_activity (required to compute drepExpiry)"
                    )
                })?;
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

                // Registration initializes expiry (versioned: bootstrap phase doesn't subtract dormant epochs).
                self.update_drep_expiry_versioned(&reg.credential, epoch, drep_activity)?;

                if self.historical_dreps.is_some() {
                    if let Err(err) = self.update_historical(&reg.credential, true, vld, |entry| {
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

                self.drep_expiry.remove(&reg.credential);

                // Update history if enabled
                if self.historical_dreps.is_some() {
                    if let Err(err) = self.update_historical(&reg.credential, false, vld, |entry| {
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
                let drep_activity = drep_activity.ok_or_else(|| {
                    anyhow!(
                        "Missing Conway parameter d_rep_activity (required to compute drepExpiry)"
                    )
                })?;
                // Update live state
                let drep = self.dreps.get_mut(&reg.credential).ok_or_else(|| {
                    anyhow!("DRep update {:?}: credential not found", reg.credential)
                })?;
                drep.anchor = reg.anchor.clone();

                // DRep update counts as activity: update expiry.
                self.update_drep_expiry(&reg.credential, epoch, drep_activity);

                // Update history if enabled
                if let Err(err) = self.update_historical(&reg.credential, false, vld, |entry| {
                    if let Some(info) = entry.info.as_mut() {
                        info.expired = false;
                        info.retired = false;
                        info.active_epoch = Some(epoch);
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
                    vld.push_anyhow(anyhow!("Historical update failed: {err}"));
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
        vld: &mut ValidationOutcomes,
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
            vld.push_anyhow(anyhow!(
                "Tried to update unknown DRep credential: {:?}",
                credential
            ));
        }

        Ok(())
    }

    async fn update_delegators(
        &mut self,
        context: &Arc<Context<Message>>,
        delegators: &[(&StakeAddress, &DRepChoice)],
        vld: &mut ValidationOutcomes,
    ) -> Result<()> {
        let mut stake_address_to_drep = HashMap::with_capacity(delegators.len());
        let mut stake_addresses = Vec::with_capacity(delegators.len());

        for &(stake_address, drep) in delegators {
            stake_addresses.push(stake_address.clone());
            stake_address_to_drep.insert(stake_address, drep);
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
                bail!("Unexpected accounts-state response")
            }
        };

        for (stake_address, old_drep_opt) in result_map {
            let new_drep_choice = match stake_address_to_drep.get(&stake_address) {
                Some(&drep) => drep,
                None => continue,
            };

            let new_drep_cred = match drep_choice_to_credential(new_drep_choice) {
                Some(c) => c,
                None => continue,
            };

            if let Some(old_drep) = old_drep_opt {
                if let Some(old_drep_cred) = drep_choice_to_credential(&old_drep) {
                    if old_drep_cred != new_drep_cred {
                        self.update_historical(&old_drep_cred, false, vld, |entry| {
                            if let Some(delegators) = entry.delegators.as_mut() {
                                delegators.retain(|s| s.get_hash() != stake_address.get_hash());
                            }
                        })?;
                    }
                }
            }

            // Add delegator to new DRep
            match self.update_historical(&new_drep_cred, true, vld, |entry| {
                if let Some(delegators) = entry.delegators.as_mut() {
                    if !delegators.contains(&stake_address) {
                        delegators.push(stake_address.clone());
                    }
                }
            }) {
                Ok(_) => {}
                Err(err) => bail!("Failed to update new delegator: {err}"),
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

impl State {
    /// Initialize state from snapshot data
    pub fn bootstrap(&mut self, drep_msg: &DRepBootstrapMessage) {
        for (cred, record) in &drep_msg.dreps {
            self.dreps.insert(cred.clone(), record.clone());
            // Snapshot does not include activity, assume active at snapshot epoch.
            self.drep_expiry.insert(cred.clone(), drep_msg.epoch);
            // update historical state if enabled
            if let Some(hist_map) = self.historical_dreps.as_mut() {
                let cfg = self.config;
                let entry = hist_map
                    .entry(cred.clone())
                    .or_insert_with(|| HistoricalDRepState::from_config(&cfg));
                if let Some(info) = entry.info.as_mut() {
                    info.deposit = record.deposit;
                    info.expired = false;
                    info.retired = false;
                    info.active_epoch = Some(drep_msg.epoch);
                    info.last_active_epoch = drep_msg.epoch; // assumed from snapshot
                }
            }
        }
    }
}

fn drep_choice_to_credential(choice: &DRepChoice) -> Option<DRepCredential> {
    match choice {
        DRepChoice::Key(k) => Some(DRepCredential::AddrKeyHash(*k)),
        DRepChoice::Script(k) => Some(DRepCredential::ScriptHash(*k)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use crate::state::{DRepRecord, DRepStorageConfig, State};
    use acropolis_common::{
        validation::ValidationOutcomes, Anchor, Credential, DRepDeregistration, DRepKeyHash,
        DRepRegistration, DRepUpdate, GovActionId, GovernanceAction, NetworkId, ProposalProcedure,
        SingleVoterVotes, StakeAddress, TxCertificate, TxCertificateWithPos, TxHash, TxIdentifier,
        Vote, Voter, VotingProcedure, VotingProcedures,
    };
    use std::collections::HashMap;

    const CRED_1: [u8; 28] = [
        123, 222, 247, 170, 243, 201, 37, 233, 124, 164, 45, 54, 241, 25, 176, 70, 154, 18, 204,
        164, 161, 126, 207, 239, 198, 144, 3, 80,
    ];
    const CRED_2: [u8; 28] = [
        124, 223, 248, 171, 244, 202, 38, 234, 125, 165, 46, 55, 242, 26, 177, 71, 155, 19, 205,
        165, 162, 127, 208, 240, 199, 145, 4, 81,
    ];

    fn set_params(state: &mut State) {
        state.conway_d_rep_activity = Some(20);
        state.conway_gov_action_lifetime = Some(0);
        state.is_pv9 = Some(true);
    }

    #[test]
    fn test_drep_process_one_certificate() {
        let mut vld = ValidationOutcomes::default();
        let tx_cred = Credential::AddrKeyHash(CRED_1.into());
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
        set_params(&mut state);
        assert!(state.process_one_cert(&tx_cert, 1, &mut vld, Some(20)).unwrap());
        assert_eq!(state.get_count(), 1);
        let tx_cert_record = DRepRecord {
            deposit: 500000000,
            anchor: None,
        };
        assert_eq!(
            state.get_drep(&tx_cred).unwrap().deposit,
            tx_cert_record.deposit
        );
        vld.as_result().unwrap();
    }

    #[test]
    fn test_drep_do_not_replace_existing_certificate() {
        let mut vld = ValidationOutcomes::new();
        let tx_cred = Credential::AddrKeyHash(CRED_1.into());
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
        set_params(&mut state);
        assert!(state.process_one_cert(&tx_cert, 1, &mut vld, Some(20)).unwrap());

        let bad_tx_cert = TxCertificateWithPos {
            cert: TxCertificate::DRepRegistration(DRepRegistration {
                credential: tx_cred.clone(),
                deposit: 600000000,
                anchor: None,
            }),
            tx_identifier: TxIdentifier::default(),
            cert_index: 1,
        };
        assert!(state.process_one_cert(&bad_tx_cert, 1, &mut vld, Some(20)).is_err());

        assert_eq!(state.get_count(), 1);
        let tx_cert_record = DRepRecord {
            deposit: 500000000,
            anchor: None,
        };
        assert_eq!(
            state.get_drep(&tx_cred).unwrap().deposit,
            tx_cert_record.deposit
        );
        vld.as_result().unwrap();
    }

    #[test]
    fn test_drep_update_certificate() {
        let mut vld = ValidationOutcomes::new();
        let tx_cred = Credential::AddrKeyHash(CRED_1.into());
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
        set_params(&mut state);
        assert!(state.process_one_cert(&tx_cert, 1, &mut vld, Some(20)).unwrap());

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

        assert!(!state.process_one_cert(&update_anchor_tx_cert, 1, &mut vld, Some(20)).unwrap());

        assert_eq!(state.get_count(), 1);
        let tx_cert_record = DRepRecord {
            deposit: 500000000,
            anchor: Some(anchor),
        };
        assert_eq!(
            state.get_drep(&tx_cred).unwrap().anchor,
            tx_cert_record.anchor
        );
        vld.as_result().unwrap();
    }

    #[test]
    fn test_drep_do_not_update_nonexistent_certificate() {
        let mut vld = ValidationOutcomes::new();
        let tx_cred = Credential::AddrKeyHash(CRED_1.into());
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
        set_params(&mut state);
        assert!(state.process_one_cert(&tx_cert, 1, &mut vld, Some(20)).unwrap());

        let anchor = Anchor {
            url: "https://poop.bike".into(),
            data_hash: vec![0x13, 0x37],
        };
        let update_anchor_tx_cert = TxCertificateWithPos {
            cert: TxCertificate::DRepUpdate(DRepUpdate {
                credential: Credential::AddrKeyHash(CRED_2.into()),
                anchor: Some(anchor.clone()),
            }),
            tx_identifier: TxIdentifier::default(),
            cert_index: 1,
        };
        assert!(state.process_one_cert(&update_anchor_tx_cert, 1, &mut vld, Some(20)).is_err());

        assert_eq!(state.get_count(), 1);
        let tx_cert_record = DRepRecord {
            deposit: 500000000,
            anchor: Some(anchor),
        };
        assert_eq!(
            state.get_drep(&tx_cred).unwrap().deposit,
            tx_cert_record.deposit
        );
        vld.as_result().unwrap();
    }

    #[test]
    fn test_drep_deregister() {
        let mut vld = ValidationOutcomes::new();
        let tx_cred = Credential::AddrKeyHash(CRED_1.into());
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
        set_params(&mut state);
        assert!(state.process_one_cert(&tx_cert, 1, &mut vld, Some(20)).unwrap());

        let unregister_tx_cert = TxCertificateWithPos {
            cert: TxCertificate::DRepDeregistration(DRepDeregistration {
                credential: tx_cred.clone(),
                refund: 500000000,
            }),
            tx_identifier: TxIdentifier::default(),
            cert_index: 1,
        };
        assert!(state.process_one_cert(&unregister_tx_cert, 1, &mut vld, Some(20)).unwrap());
        assert_eq!(state.get_count(), 0);
        assert!(state.get_drep(&tx_cred).is_none());
        vld.as_result().unwrap();
    }

    #[test]
    fn test_drep_inactivity() {
        let mut vld = ValidationOutcomes::new();
        let tx_cred = Credential::AddrKeyHash(CRED_1.into());

        // Enable historical for checking on expired/active_epoch fields.
        let config = DRepStorageConfig {
            store_info: true,
            ..Default::default()
        };
        let mut state = State::new(config);
        set_params(&mut state);

        // Register at epoch 10
        let register_cert = TxCertificateWithPos {
            cert: TxCertificate::DRepRegistration(DRepRegistration {
                credential: tx_cred.clone(),
                deposit: 500000000,
                anchor: None,
            }),
            tx_identifier: TxIdentifier::default(),
            cert_index: 0,
        };
        assert!(state.process_one_cert(&register_cert, 10, &mut vld, Some(20)).unwrap());
        assert_eq!(
            state.drep_expiry.get(&tx_cred).copied(),
            Some(30),
            "registration should set drep_expiry using drep_activity"
        );

        // Expire at epoch 31 (expiry=30 is still active at epoch 30)
        state.update_drep_expirations(31).unwrap();
        let historical = state.historical_dreps.as_ref().unwrap();
        let drep_info = historical.get(&tx_cred).unwrap().info.as_ref().unwrap();
        assert!(drep_info.expired, "DRep should be expired at epoch 31");

        // Update at epoch 31 => should reset activity and clear expired
        let update_cert = TxCertificateWithPos {
            cert: TxCertificate::DRepUpdate(DRepUpdate {
                credential: tx_cred.clone(),
                anchor: None,
            }),
            tx_identifier: TxIdentifier::default(),
            cert_index: 0,
        };
        state.process_one_cert(&update_cert, 31, &mut vld, Some(20)).unwrap();
        assert_eq!(
            state.drep_expiry.get(&tx_cred).copied(),
            Some(51),
            "DRepUpdate should reset drep_expiry using drep_activity"
        );

        let historical = state.historical_dreps.as_ref().unwrap();
        let drep_info = historical.get(&tx_cred).unwrap().info.as_ref().unwrap();
        assert!(
            !drep_info.expired,
            "DRep should not be expired after update"
        );
        assert_eq!(
            drep_info.active_epoch,
            Some(31),
            "active_epoch should be reset to 31 after DRepUpdate"
        );

        // Next epoch boundary shouldn't re-expire immediately
        state.update_drep_expirations(32).unwrap();
        let inactive = state.inactive_drep_list(32);
        assert!(
            !inactive.contains(&tx_cred),
            "DRep should not be considered inactive at epoch 32 after updating at epoch 31"
        );
        let historical = state.historical_dreps.as_ref().unwrap();
        let drep_info = historical.get(&tx_cred).unwrap().info.as_ref().unwrap();
        assert!(
            !drep_info.expired,
            "Historical DRep should not be re-expired at epoch 32 after updating at epoch 31"
        );
    }

    #[test]
    fn test_drep_do_not_deregister_nonexistent_cert() {
        let mut vld = ValidationOutcomes::new();
        let tx_cred = Credential::AddrKeyHash(CRED_1.into());
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
        set_params(&mut state);
        assert!(state.process_one_cert(&tx_cert, 1, &mut vld, Some(20)).unwrap());

        let unregister_tx_cert = TxCertificateWithPos {
            cert: TxCertificate::DRepDeregistration(DRepDeregistration {
                credential: Credential::AddrKeyHash(CRED_2.into()),
                refund: 500000000,
            }),
            tx_identifier: TxIdentifier::default(),
            cert_index: 1,
        };
        assert!(state.process_one_cert(&unregister_tx_cert, 1, &mut vld, Some(20)).is_err());
        assert_eq!(state.get_count(), 1);
        assert_eq!(state.get_drep(&tx_cred).unwrap().deposit, 500000000);
        vld.as_result().unwrap();
    }

    #[test]
    fn test_process_votes_refreshes_drep_expiry() {
        let mut vld = ValidationOutcomes::new();
        let drep_key = CRED_1.into();
        let drep_cred = Credential::AddrKeyHash(drep_key);

        let mut state = State::new(DRepStorageConfig::default());
        set_params(&mut state);

        // Register at epoch 10 with d_rep_activity = 20 should expiry = 30 (no dormancy).
        let register_cert = TxCertificateWithPos {
            cert: TxCertificate::DRepRegistration(DRepRegistration {
                credential: drep_cred.clone(),
                deposit: 500000000,
                anchor: None,
            }),
            tx_identifier: TxIdentifier::default(),
            cert_index: 0,
        };
        assert!(state.process_one_cert(&register_cert, 10, &mut vld, Some(20)).unwrap());
        assert_eq!(state.drep_expiry.get(&drep_cred).copied(), Some(30));

        // Simulate dormancy accumulation. Votes should compute expiry with subtraction.
        state.num_dormant_epochs = 2;

        let gov_action_id = GovActionId {
            transaction_id: TxHash::default(),
            action_index: 0,
        };

        let mut single = SingleVoterVotes::default();
        single.voting_procedures.insert(
            gov_action_id,
            VotingProcedure {
                vote: Vote::Yes,
                anchor: None,
                vote_index: 0,
            },
        );

        let mut votes = VotingProcedures {
            votes: HashMap::new(),
        };
        votes.votes.insert(Voter::DRepKey(DRepKeyHash::from(drep_key)), single);

        // At epoch 15: (15 + 20) - 2 = 33
        state.process_votes(&[(TxHash::default(), votes)], 15, Some(20)).unwrap();
        assert_eq!(state.drep_expiry.get(&drep_cred).copied(), Some(33));
    }

    #[test]
    fn test_update_num_dormant_epochs_tracks_active_proposals() {
        let mut state = State::new(DRepStorageConfig::default());

        let reward_account =
            StakeAddress::new(Credential::AddrKeyHash(CRED_2.into()), NetworkId::Mainnet);
        let proposal = ProposalProcedure {
            deposit: 0,
            reward_account,
            gov_action_id: GovActionId {
                transaction_id: TxHash::default(),
                action_index: 0,
            },
            gov_action: GovernanceAction::Information,
            anchor: Anchor {
                url: "https://poop.bike".into(),
                data_hash: vec![],
            },
        };

        // gov_action_lifetime = 2, expires_after = 12 for a proposal recorded at epoch 10.
        state.conway_gov_action_lifetime = Some(2);
        state.record_proposals(&[proposal], 10);

        // At epoch 11 the proposal is still active, counter unchanged.
        state.update_num_dormant_epochs(11);
        assert_eq!(state.num_dormant_epochs, 0);

        // At epoch 13 the proposal has expired, counter increments.
        state.update_num_dormant_epochs(13);
        assert_eq!(state.num_dormant_epochs, 1);
    }

    #[test]
    fn test_apply_dormant_expiry_updates_and_resets_counter() {
        let mut state = State::new(DRepStorageConfig::default());
        let drep_cred = Credential::AddrKeyHash(CRED_1.into());

        state.drep_expiry.insert(drep_cred.clone(), 30);
        state.num_dormant_epochs = 2;
        state.apply_dormant_expiry(20);
        assert_eq!(state.drep_expiry.get(&drep_cred).copied(), Some(32));
        assert_eq!(state.num_dormant_epochs, 0);

        // Already-expired DReps should not be "revived" by the bump.
        state.drep_expiry.insert(drep_cred.clone(), 10);
        state.num_dormant_epochs = 2;
        state.apply_dormant_expiry(15);
        assert_eq!(state.drep_expiry.get(&drep_cred).copied(), Some(10));
        assert_eq!(state.num_dormant_epochs, 0);
    }

    #[test]
    fn test_registration_expiry_bootstrap_phase() {
        // During bootstrap phase (protocol version 9.x), registration should NOT subtract
        // dormant epochs from expiry.
        let mut vld = ValidationOutcomes::new();
        let tx_cred = Credential::AddrKeyHash(CRED_1.into());

        // is_bootstrap = Some(true) means we're in bootstrap phase
        let mut state = State::new(DRepStorageConfig::default());
        set_params(&mut state);
        state.num_dormant_epochs = 5; // Accumulated dormant epochs

        let register_cert = TxCertificateWithPos {
            cert: TxCertificate::DRepRegistration(DRepRegistration {
                credential: tx_cred.clone(),
                deposit: 500000000,
                anchor: None,
            }),
            tx_identifier: TxIdentifier::default(),
            cert_index: 0,
        };

        // Register at epoch 10 with drep_activity=20
        // Bootstrap: expiry = 10 + 20 = 30 (dormant epochs NOT subtracted)
        state.process_one_cert(&register_cert, 10, &mut vld, Some(20)).unwrap();
        assert_eq!(
            state.drep_expiry.get(&tx_cred).copied(),
            Some(30),
            "Bootstrap registration should NOT subtract dormant epochs"
        );
    }

    #[test]
    fn test_registration_expiry_post_bootstrap() {
        // After bootstrap phase (protocol version 10+), registration should subtract
        // dormant epochs from expiry.
        let mut vld = ValidationOutcomes::new();
        let tx_cred = Credential::AddrKeyHash(CRED_1.into());

        // is_bootstrap = Some(false) means we're post-bootstrap
        let mut state = State::new(DRepStorageConfig::default());
        set_params(&mut state);
        state.is_pv9 = Some(false);
        state.num_dormant_epochs = 5; // Accumulated dormant epochs

        let register_cert = TxCertificateWithPos {
            cert: TxCertificate::DRepRegistration(DRepRegistration {
                credential: tx_cred.clone(),
                deposit: 500000000,
                anchor: None,
            }),
            tx_identifier: TxIdentifier::default(),
            cert_index: 0,
        };

        // Register at epoch 10 with drep_activity=20
        // Post-bootstrap: expiry = (10 + 20) - 5 = 25 (dormant epochs subtracted)
        state.process_one_cert(&register_cert, 10, &mut vld, Some(20)).unwrap();
        assert_eq!(
            state.drep_expiry.get(&tx_cred).copied(),
            Some(25),
            "Post-bootstrap registration should subtract dormant epochs"
        );
    }

    #[test]
    fn test_votes_always_subtract_dormant_epochs() {
        // Votes should always subtract dormant epochs, regardless of bootstrap phase.
        let drep_key = CRED_1.into();
        let drep_cred = Credential::AddrKeyHash(drep_key);

        // Test with bootstrap=true (votes should still subtract dormant epochs)
        let mut state = State::new(DRepStorageConfig::default());
        state.dreps.insert(drep_cred.clone(), DRepRecord::new(500000000, None));
        state.drep_expiry.insert(drep_cred.clone(), 30);
        state.num_dormant_epochs = 3;

        let gov_action_id = GovActionId {
            transaction_id: TxHash::default(),
            action_index: 0,
        };

        let mut single = SingleVoterVotes::default();
        single.voting_procedures.insert(
            gov_action_id,
            VotingProcedure {
                vote: Vote::Yes,
                anchor: None,
                vote_index: 0,
            },
        );

        let mut votes = VotingProcedures {
            votes: HashMap::new(),
        };
        votes.votes.insert(Voter::DRepKey(DRepKeyHash::from(drep_key)), single);

        // Vote at epoch 15 with drep_activity=20, num_dormant=3
        // expiry = (15 + 20) - 3 = 32 (always subtracts dormant epochs)
        state.process_votes(&[(TxHash::default(), votes)], 15, Some(20)).unwrap();
        assert_eq!(
            state.drep_expiry.get(&drep_cred).copied(),
            Some(32),
            "Votes should always subtract dormant epochs, even during bootstrap"
        );
    }
}
