//! Acropolis Governance State: State storage

use crate::{
    alonzo_babbage_voting::AlonzoBabbageVoting, conway_voting::ConwayVoting,
    VotingRegistrationState,
};
use acropolis_common::validation::ValidationOutcomes;
use acropolis_common::{
    caryatid::RollbackAwarePublisher,
    messages::{
        CardanoMessage, DRepStakeDistributionMessage, GovernanceOutcomesMessage,
        GovernanceProceduresMessage, Message, ProtocolParamsMessage, SPOStakeDistributionMessage,
    },
    protocol_params::ProtocolVersion,
    validation::{GovernanceValidationError, ValidationError},
    BlockInfo, DRepCredential, DelegatedStake, Era, GovActionId, Lovelace, PoolId,
    ProposalProcedure, TxHash, Voter, VotingProcedure,
};
use anyhow::{anyhow, bail, Result};
use caryatid_sdk::Context;
use hex::ToHex;
use std::{collections::HashMap, sync::Arc};
use tracing::info;

pub struct State {
    publisher: RollbackAwarePublisher<Message>,

    pub drep_stake_messages_count: usize,

    current_era: Era,
    drep_stake: HashMap<DRepCredential, Lovelace>,
    drep_no_confidence: u64,
    drep_abstain: u64,
    spo_stake: HashMap<PoolId, DelegatedStake>,

    alonzo_babbage_voting: AlonzoBabbageVoting,
    conway_voting: ConwayVoting,
}

impl State {
    pub fn new(
        context: Arc<Context<Message>>,
        enact_state_topic: String,
        verification_output_file: Option<String>,
    ) -> Self {
        Self {
            publisher: RollbackAwarePublisher::new(context, enact_state_topic),

            drep_stake_messages_count: 0,

            current_era: Era::default(),

            alonzo_babbage_voting: AlonzoBabbageVoting::default(),
            conway_voting: ConwayVoting::new(verification_output_file),

            drep_stake: HashMap::new(),
            drep_no_confidence: 0,
            drep_abstain: 0,
            spo_stake: HashMap::new(),
        }
    }

    /// Update current fields to new epoch values. The function should be called
    /// after all block processing is done.
    pub fn advance_epoch(&mut self, epoch_blk: &BlockInfo) -> Result<()> {
        if !epoch_blk.new_epoch {
            bail!("Block {epoch_blk:?} must start a new epoch");
        }
        self.current_era = epoch_blk.era; // If era is the same -- no problem
        self.alonzo_babbage_voting.advance_epoch(epoch_blk);
        Ok(())
    }

    pub async fn handle_protocol_parameters(
        &mut self,
        message: &ProtocolParamsMessage,
    ) -> Result<()> {
        if let Some(ps) = &message.params.shelley {
            self.alonzo_babbage_voting.update_parameters(ps.epoch_length, ps.update_quorum);

            if message.params.conway.is_some() {
                let bootstrap = ps.protocol_params.protocol_version.is_chang()?;
                self.conway_voting.update_parameters(&message.params.conway, bootstrap)
            }
        } else if message.params.conway.is_some() {
            bail!("Impossible parameters combination: Shelley is missing, but Conway is present.");
        }

        Ok(())
    }

    pub async fn handle_drep_stake(
        &mut self,
        drep_message: &DRepStakeDistributionMessage,
    ) -> Result<()> {
        self.drep_stake_messages_count += 1;
        self.drep_stake = HashMap::from_iter(drep_message.drdd.dreps.iter().cloned());
        self.drep_no_confidence = drep_message.drdd.no_confidence;
        self.drep_abstain = drep_message.drdd.abstain;

        Ok(())
    }

    pub async fn handle_spo_stake(
        &mut self,
        spo_message: &SPOStakeDistributionMessage,
    ) -> Result<()> {
        self.spo_stake = HashMap::from_iter(spo_message.spos.iter().cloned());

        Ok(())
    }

    /// Implementation of governance message processing handle
    pub async fn handle_governance(
        &mut self,
        block: &BlockInfo,
        governance_message: &GovernanceProceduresMessage,
    ) -> Result<ValidationOutcomes> {
        if block.era < Era::Conway {
            // Alonzo-Babbage governance
            if !(governance_message.proposal_procedures.is_empty()
                && governance_message.voting_procedures.is_empty())
            {
                let mut outcomes = ValidationOutcomes::new();
                outcomes.push(ValidationError::BadGovernance(
                    GovernanceValidationError::WrongProtocolForGovernance(ProtocolVersion::chang()),
                ));
                return Ok(outcomes);
            }

            if !governance_message.alonzo_babbage_updates.is_empty() {
                self.alonzo_babbage_voting
                    .process_update_proposals(block, &governance_message.alonzo_babbage_updates)
                    .map_err(|e| anyhow!("Error handling Babbage governance_message: '{e}'"))?;
            }

            Ok(ValidationOutcomes::new())
        } else {
            // Conway governance
            let mut outcomes = ValidationOutcomes::new();
            for pproc in &governance_message.proposal_procedures {
                outcomes.merge(
                    &mut self
                        .conway_voting
                        .insert_proposal_procedure(block.epoch, pproc)
                        .map_err(|e| anyhow!("Error handling Conway governance_message: '{e}'"))?,
                );
            }

            for (trans, vproc) in &governance_message.voting_procedures {
                for (voter, voter_votes) in vproc.votes.iter() {
                    outcomes.merge(
                        &mut self
                            .conway_voting
                            .insert_voting_procedure(block.epoch, voter, trans, voter_votes)
                            .map_err(|e| {
                                anyhow!(
                                    "Error handling Conway voting block {}, trans {} '{e}'",
                                    block.number,
                                    trans.encode_hex::<String>(),
                                )
                            })?,
                    );
                }
            }
            Ok(outcomes)
        }
    }

    fn recalculate_voting_state(&self) -> Result<VotingRegistrationState> {
        let drep_stake = self.drep_stake.values().sum();

        let committee_usize = self.conway_voting.get_conway_params()?.committee.members.len();
        let committee = committee_usize
            .try_into()
            .map_err(|e| anyhow!("Commitee size: conversion usize -> u64 failed, {e}"))?;

        let spo_stake = self.spo_stake.values().map(|ds| ds.active).sum();

        Ok(VotingRegistrationState::new(
            spo_stake,
            drep_stake,
            self.drep_no_confidence,
            self.drep_abstain,
            committee,
        ))
    }

    /// Loops through all actions and checks their status for the new_epoch
    /// All incoming data (parameters for the epoch, drep distribution, etc)
    /// should already be actual at this moment.
    pub fn process_new_epoch(
        &mut self,
        new_block: &BlockInfo,
    ) -> Result<GovernanceOutcomesMessage> {
        let mut output = GovernanceOutcomesMessage {
            alonzo_babbage_outcomes: self.alonzo_babbage_voting.finalize_voting(new_block)?,
            ..Default::default()
        };

        if self.current_era >= Era::Conway {
            // Last chance to print actual votes; later they'll be cleaned
            self.conway_voting.log_conway_voting_stats(new_block.epoch);
            let voting_state = self.recalculate_voting_state()?;
            let ratified = self.conway_voting.finalize_conway_voting(
                new_block,
                &voting_state,
                &self.drep_stake,
                &self.spo_stake,
            )?;
            self.conway_voting.update_action_status_with_outcomes(new_block.epoch, &ratified)?;
            let acc = ratified.iter().filter(|oc| oc.voting.accepted).count();

            info!(
                "Conway voting, epoch {} ({}): {voting_state}, total {} actions, {acc} accepted",
                new_block.epoch,
                new_block.era,
                ratified.len()
            );

            info!(
                "Conway voting: new epoch {}, outcomes: {ratified:?}",
                new_block.epoch
            );
            output.conway_outcomes = ratified;
        }

        self.conway_voting.print_outcome_to_verify(&output.conway_outcomes)?;
        Ok(output)
    }

    fn log_stats(&self) {
        info!(
            "{}, {}, drep stake msgs (size): {} ({})",
            self.alonzo_babbage_voting.get_stats(),
            self.conway_voting.get_stats(),
            self.drep_stake_messages_count,
            self.drep_stake.len(),
        );
    }

    pub async fn send(
        &mut self,
        block: &BlockInfo,
        message: GovernanceOutcomesMessage,
    ) -> Result<()> {
        let packed_message = Arc::new(Message::Cardano((
            block.clone(),
            CardanoMessage::GovernanceOutcomes(message),
        )));
        self.publisher.publish(packed_message).await
    }

    /// Publish a rollback message, if we have anything to roll back
    pub async fn publish_rollback(&mut self, message: Arc<Message>) -> anyhow::Result<()> {
        self.publisher.publish(message).await
    }

    /// Get list of actual voting proposals
    pub fn list_proposals(&self) -> Vec<GovActionId> {
        self.conway_voting.proposals.keys().cloned().collect()
    }

    /// Get details for a specific proposal
    pub fn get_proposal(&self, id: &GovActionId) -> Option<ProposalProcedure> {
        self.conway_voting.proposals.get(id).map(|(_epoch, prop)| prop.clone())
    }

    /// Get list of votes for a specific proposal
    pub fn get_proposal_votes(
        &self,
        proposal_id: &GovActionId,
    ) -> Result<HashMap<Voter, (TxHash, VotingProcedure)>> {
        match self.conway_voting.votes.get(proposal_id) {
            Some(all_votes) => Ok(all_votes.clone()),
            None => Err(anyhow::anyhow!(
                "Governance action: {:?} not found",
                proposal_id
            )),
        }
    }

    pub async fn tick(&self) -> Result<()> {
        self.log_stats();
        Ok(())
    }

    /// Get a reference to the conway voting state
    pub fn get_conway_voting(&self) -> &ConwayVoting {
        &self.conway_voting
    }

    /// Get a mutable reference to the conway voting state
    pub fn get_conway_voting_mut(&mut self) -> &mut ConwayVoting {
        &mut self.conway_voting
    }
}
