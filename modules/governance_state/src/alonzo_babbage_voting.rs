use acropolis_common::{
    AlonzoBabbageUpdateProposal, AlonzoBabbageVotingOutcome, BlockInfo, Era, GenesisKeyhash,
    ProtocolParamUpdate,
};
use anyhow::{bail, Result};
use std::collections::{HashMap, HashSet};

const GENESIS_KEYS_VOTES_THRESHOLD: u64 = 5;
const MAINNET_SHELLEY_SLOTS_PER_EPOCH: u64 = 432_000;

pub struct AlonzoBabbageVoting {
    /// map "enact epoch" (proposal enacts at this epoch end) to voting
    /// "voting": map voter (genesis key) => (vote epoch, vote slot, proposal)
    /// "vote epoch/slot" --- moment, when the vote was cast for the proposal
    proposals: HashMap<u64, HashMap<GenesisKeyhash, (u64, u64, Box<ProtocolParamUpdate>)>>,
    shelley_slots_per_epoch: u64,
}

impl AlonzoBabbageVoting {
    pub fn new() -> Self {
        Self {
            proposals: HashMap::new(),
            shelley_slots_per_epoch: MAINNET_SHELLEY_SLOTS_PER_EPOCH,
        }
    }

    /// Vote is counted for the new epoch if cast in previous epoch
    /// before 4/10 of its start (not too fresh).
    /// Here is it: [!++++++++++!++++------!]
    fn is_timely_vote(&self, _epoch: u64, slot: u64, new_block: &BlockInfo) -> bool {
        slot + (6 * self.shelley_slots_per_epoch / 10) < new_block.slot
    }

    pub fn update_shelley_slots_per_epoch(&mut self, shelley_slots_per_epoch: u64) {
        self.shelley_slots_per_epoch = shelley_slots_per_epoch;
    }

    pub fn process_update_proposals(
        &mut self,
        block_info: &BlockInfo,
        updates: &Vec<AlonzoBabbageUpdateProposal>,
    ) -> Result<()> {
        if updates.is_empty() {
            return Ok(());
        }

        if block_info.era < Era::Shelley {
            bail!("Cannot process Alonzo/Babbage update proposals in pre-Shelley era");
        }

        for pp in updates.iter() {
            let entry = self.proposals.entry(pp.enactment_epoch + 1).or_insert(HashMap::new());
            for (k, p) in &pp.proposals {
                // A new proposal for key k always replaces the old one
                entry.insert(k.clone(), (block_info.epoch, block_info.slot, p.clone()));
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
            .filter(|(_k, (epoch, slot, _proposal))| self.is_timely_vote(*epoch, *slot, new_blk))
            .map(|(k, (_e, _s, proposal))| (k.clone(), proposal.clone()))
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
                    .filter_map(|(k, v)| (v == parameter_update).then(|| k.clone()))
                    .collect();

                for v in &votes {
                    cast_votes.insert(v.clone());
                }

                let votes_len = votes.len() as u64;

                Some(AlonzoBabbageVotingOutcome {
                    votes_threshold: GENESIS_KEYS_VOTES_THRESHOLD,
                    voting: votes,
                    accepted: votes_len >= GENESIS_KEYS_VOTES_THRESHOLD,
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
}

#[cfg(test)]
mod tests {
    use crate::alonzo_babbage_voting::{AlonzoBabbageVoting, MAINNET_SHELLEY_SLOTS_PER_EPOCH};
    use acropolis_common::{
        rational_number::rational_number_from_f32, AlonzoBabbageUpdateProposal,
        AlonzoBabbageVotingOutcome, BlockInfo, BlockStatus, GenesisKeyhash, ProtocolParamUpdate,
    };
    use anyhow::Result;
    use serde_with::{base64::Base64, serde_as};

    #[serde_as]
    #[derive(serde::Deserialize, Debug)]
    struct ReplayerGenesisKeyhash(#[serde_as(as = "Base64")] GenesisKeyhash);

    fn run_voting() -> Result<Vec<(BlockInfo, Vec<AlonzoBabbageVotingOutcome>)>> {
        let mut voting = AlonzoBabbageVoting::new();

        let update_proposal_json = include_bytes!("./alonzo_babbage_voting.json");
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
                slot,
                number: slot,
                epoch,
                epoch_slot: epoch % MAINNET_SHELLEY_SLOTS_PER_EPOCH,
                era: era.try_into()?,
                new_epoch: new_epoch != 0,
                timestamp: 0,
                hash: Vec::new(),
            };

            for prop in proposals {
                let decoded_updates =
                    prop.1.iter().map(|(k, v)| (k.0.clone(), v.clone())).collect();

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
        f: impl Fn(&ProtocolParamUpdate) -> Option<T>,
    ) -> Result<Vec<(u64, T)>> {
        let updates = run_voting()?;
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
    // Tests
    //

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
        let dcu = extract_parameter(|p| p.decentralisation_constant)?;

        assert_eq!(DECENTRALISATION.len(), dcu.len());
        for i in 0..dcu.len() {
            let rat = rational_number_from_f32(DECENTRALISATION[i].1)?;
            assert_eq!((DECENTRALISATION[i].0, rat), *dcu.get(i).unwrap());
        }

        Ok(())
    }

    #[test]
    fn test_protocol_version() -> Result<()> {
        let dcu = extract_parameter(|p| {
            p.protocol_version.as_ref().map(|version| (version.major, version.minor))
        })?;

        assert_eq!(PROTOCOL_VERSION.to_vec(), dcu);

        Ok(())
    }
    #[test]
    fn test_desired_number_of_stake_pools() -> Result<()> {
        let dcu = extract_parameter(|p| p.desired_number_of_stake_pools)?;

        assert_eq!(STAKE_POOLS.to_vec(), dcu);

        Ok(())
    }
}
