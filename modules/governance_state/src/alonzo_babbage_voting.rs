use acropolis_common::{
    AlonzoBabbageUpdateProposal, AlonzoBabbageVotingOutcome, BlockInfo, Era, GenesisKeyhash,
    ProtocolParamUpdate,
};
use anyhow::{bail, Result};
use std::collections::{HashMap, HashSet};
use acropolis_common::validation::ValidationStatus;

// (vote epoch, vote slot, proposal)
type VoteData = (u64, u64, Box<ProtocolParamUpdate>);

#[derive(Default)]
pub struct AlonzoBabbageVoting {
    /// map "enact epoch" (proposal enacts at this epoch end) to voting
    /// "voting": map voter (genesis key) => votedata
    /// "vote epoch/slot" --- moment, when the vote was cast for the proposal
    proposals: HashMap<u64, HashMap<GenesisKeyhash, VoteData>>,
    slots_per_epoch: u32,
    update_quorum: u32,
}

impl AlonzoBabbageVoting {
    /// Vote is counted for the new epoch if cast in previous epoch
    /// before 4/10 of its start (not too fresh).
    /// Here is it: [!++++++++++!++++------!]
    fn is_timely_vote(&self, slot: u64, new_block: &BlockInfo) -> bool {
        slot + (6 * self.slots_per_epoch as u64 / 10) < new_block.slot
    }

    pub fn update_parameters(&mut self, slots_per_epoch: u32, update_quorum: u32) {
        self.slots_per_epoch = slots_per_epoch;
        self.update_quorum = update_quorum;
    }

    pub fn process_update_proposals(
        &mut self,
        block_info: &BlockInfo,
        updates: &[AlonzoBabbageUpdateProposal],
    ) -> Result<ValidationStatus> {
        if updates.is_empty() {
            return Ok(ValidationStatus::Go);
        }

        if block_info.era < Era::Shelley {
            bail!("Processing Alonzo/Babbage update proposals in pre-Shelley era");
        }

        if self.slots_per_epoch == u32::default() || self.update_quorum == u32::default() {
            bail!("Processing Alonzo/Babbage update proposals with unknown protocol parameters");
        }

        for pp in updates.iter() {
            let entry = self.proposals.entry(pp.enactment_epoch + 1).or_default();
            for (k, p) in &pp.proposals {
                // A new proposal for key k always replaces the old one
                entry.insert(*k, (block_info.epoch, block_info.slot, p.clone()));
            }
        }

        Ok(())
    }

    pub fn finalize_voting(
        &mut self,
        new_blk: &BlockInfo,
    ) -> Result<Vec<AlonzoBabbageVotingOutcome>> {
        let proposals_for_new_epoch = match self.proposals.get(&new_blk.epoch) {
            Some(proposal) => proposal,
            None => return Ok(Vec::new()),
        };

        let proposals = proposals_for_new_epoch
            .iter()
            .filter(|(_k, (_epoch, slot, _proposal))| self.is_timely_vote(*slot, new_blk))
            .map(|(k, (_e, _s, proposal))| (*k, proposal.clone()))
            .collect::<Vec<_>>();

        let mut cast_votes = HashSet::new();
        let outcomes: Vec<_> = proposals
            .iter()
            .filter_map(|(k, parameter_update)| {
                if cast_votes.contains(k) {
                    return None;
                }

                let votes: Vec<_> = proposals
                    .iter()
                    .filter(|&(_, v)| v == parameter_update)
                    .map(|(k, _)| *k)
                    .collect();

                for v in &votes {
                    // TODO Check keys (whether they are genesis keys)
                    cast_votes.insert(*v);
                }

                let votes_len = votes.len() as u32;

                Some(AlonzoBabbageVotingOutcome {
                    votes_threshold: self.update_quorum,
                    voting: votes,
                    accepted: votes_len >= self.update_quorum,
                    parameter_update: parameter_update.clone(),
                })
            })
            .collect();
        Ok(outcomes)
    }

    /// Advance pointers, clear all outdated proposals
    pub fn advance_epoch(&mut self, epoch_blk: &BlockInfo) {
        self.proposals.retain(|enact_epoch, _| *enact_epoch >= epoch_blk.epoch);
    }

    pub fn get_stats(&self) -> String {
        format!("alonzo proposal epochs: {:?}", self.proposals.keys())
    }
}

#[cfg(test)]
mod tests {
    use crate::alonzo_babbage_voting::AlonzoBabbageVoting;
    use acropolis_common::{
        rational_number::rational_number_from_f32, AlonzoBabbageUpdateProposal,
        AlonzoBabbageVotingOutcome, BlockHash, BlockInfo, BlockIntent, BlockStatus, GenesisKeyhash,
        ProtocolParamUpdate,
    };
    use anyhow::Result;
    use serde_with::{base64::Base64, serde_as};

    #[serde_as]
    #[derive(serde::Deserialize, Debug)]
    struct ReplayerGenesisKeyhash(#[serde_as(as = "Base64")] GenesisKeyhash);

    /// Returns list of voting results: list of pairs (BlockInfo, outcomes), where
    /// BlockInfo is the first block of the epoch, where the new parameters must
    /// be actual (parameter update should happen right before this block is processed).
    fn run_voting(
        update_quorum: u32,
        slots_per_epoch: u32,
        update_proposal_json: &[u8],
    ) -> Result<Vec<(BlockInfo, Vec<AlonzoBabbageVotingOutcome>)>> {
        let mut voting = AlonzoBabbageVoting::default();
        voting.update_parameters(update_quorum, slots_per_epoch);

        let update_proposal_msgs = serde_json::from_slice::<
            Vec<(
                u64,
                u64,
                u8,
                u8,
                Vec<(u64, Vec<(ReplayerGenesisKeyhash, Box<ProtocolParamUpdate>)>)>,
            )>,
        >(update_proposal_json)?;

        let mut voting_outcomes: Vec<(BlockInfo, Vec<AlonzoBabbageVotingOutcome>)> = Vec::new();
        for (slot, epoch, era, new_epoch, proposals) in update_proposal_msgs {
            let mut proposal = Vec::new();
            let blk = BlockInfo {
                status: BlockStatus::Immutable,
                intent: BlockIntent::Apply,
                slot,
                number: slot,
                epoch,
                epoch_slot: 0,
                era: era.try_into()?,
                new_epoch: new_epoch != 0,
                timestamp: 0,
                hash: BlockHash::default(),
            };

            for prop in proposals {
                let decoded_updates = prop.1.iter().map(|(k, v)| (k.0, v.clone())).collect();

                let update_prop = AlonzoBabbageUpdateProposal {
                    proposals: decoded_updates,
                    enactment_epoch: prop.0,
                };
                proposal.push(update_prop)
            }

            voting.process_update_proposals(&blk, &proposal)?;

            if blk.new_epoch {
                let outcome = voting.finalize_voting(&blk)?;
                voting.advance_epoch(&blk);
                if !outcome.is_empty() {
                    voting_outcomes.push((blk, outcome));
                }
            }
        }

        Ok(voting_outcomes)
    }

    fn extract_parameter<T: Clone>(
        update_quorum: u32,
        slots_per_epoch: u32,
        update_proposals_json: &[u8],
        f: impl Fn(&ProtocolParamUpdate) -> Option<T>,
    ) -> Result<Vec<(u64, T)>> {
        let updates = run_voting(slots_per_epoch, update_quorum, update_proposals_json)?;
        let mut dcu = Vec::new();

        for (blk, upd) in updates {
            assert!(blk.new_epoch);
            dcu.append(
                &mut upd
                    .iter()
                    .filter(|x| x.accepted)
                    .filter_map(|x| f(&x.parameter_update).map(|t| (blk.epoch, t.clone())))
                    .collect::<Vec<(u64, T)>>(),
            );
        }

        Ok(dcu)
    }

    //
    // Mainnet Tests
    //

    const MAINNET_PROPOSALS_JSON: &[u8] = include_bytes!("../data/alonzo_babbage_voting.json");

    fn extract_mainnet_parameter<T: Clone>(
        f: impl Fn(&ProtocolParamUpdate) -> Option<T>,
    ) -> Result<Vec<(u64, T)>> {
        extract_parameter(5, 432_000, MAINNET_PROPOSALS_JSON, f)
    }

    const DECENTRALISATION: [(u64, f32); 39] = [
        (211, 0.9),
        (212, 0.8),
        (213, 0.78),
        (214, 0.76),
        (215, 0.74),
        (216, 0.72),
        (217, 0.7),
        (218, 0.68),
        (219, 0.66),
        (220, 0.64),
        (221, 0.62),
        (222, 0.6),
        (223, 0.58),
        (224, 0.56),
        (225, 0.54),
        (226, 0.52),
        (227, 0.5),
        (228, 0.48),
        (229, 0.46),
        (230, 0.44),
        (231, 0.42),
        (232, 0.4),
        (233, 0.38),
        (234, 0.32),
        (242, 0.3),
        (243, 0.28),
        (244, 0.26),
        (245, 0.24),
        (246, 0.22),
        (247, 0.2),
        (248, 0.18),
        (249, 0.16),
        (250, 0.14),
        (251, 0.12),
        (252, 0.1),
        (253, 0.08),
        (254, 0.06),
        (256, 0.02),
        (257, 0.0),
    ];

    const PROTOCOL_VERSION: [(u64, (u64, u64)); 7] = [
        (236, (3, 0)),
        (251, (4, 0)),
        (290, (5, 0)),
        (298, (6, 0)),
        (365, (7, 0)),
        (394, (8, 0)),
        (507, (9, 0)), // No Conway era versions
    ];

    const STAKE_POOLS: [(u64, u64); 1] = [(234, 500)];

    #[test]
    fn test_decentralisation_updates() -> Result<()> {
        let dcu = extract_mainnet_parameter(|p| p.decentralisation_constant)?;

        assert_eq!(DECENTRALISATION.len(), dcu.len());
        for (decent, param) in DECENTRALISATION.iter().zip(dcu) {
            let rat = rational_number_from_f32(decent.1)?;
            assert_eq!((decent.0, rat), param);
        }

        Ok(())
    }

    #[test]
    fn test_protocol_version() -> Result<()> {
        let dcu = extract_mainnet_parameter(|p| {
            p.protocol_version.as_ref().map(|version| (version.major, version.minor))
        })?;

        assert_eq!(PROTOCOL_VERSION.to_vec(), dcu);

        Ok(())
    }

    #[test]
    fn test_desired_number_of_stake_pools() -> Result<()> {
        let dcu = extract_mainnet_parameter(|p| p.desired_number_of_stake_pools)?;

        assert_eq!(STAKE_POOLS.to_vec(), dcu);

        Ok(())
    }

    //
    // SanchoNet Tests
    //

    const SANCHONET_PROPOSALS_JSON: &[u8] = include_bytes!("../data/ab_sancho_voting.json");

    fn extract_sanchonet_parameter<T: Clone>(
        f: impl Fn(&ProtocolParamUpdate) -> Option<T>,
    ) -> Result<Vec<(u64, T)>> {
        extract_parameter(3, 86_400, SANCHONET_PROPOSALS_JSON, f)
    }

    const SANCHONET_PROTOCOL_VERSION: [(u64, (u64, u64)); 3] =
        [(2, (7, 0)), (3, (8, 0)), (492, (9, 0))];

    #[test]
    fn test_sanchonet_protocol_version() -> Result<()> {
        let dcu = extract_sanchonet_parameter(|p| {
            p.protocol_version.as_ref().map(|version| (version.major, version.minor))
        })?;

        assert_eq!(SANCHONET_PROTOCOL_VERSION.to_vec(), dcu);

        Ok(())
    }
}
