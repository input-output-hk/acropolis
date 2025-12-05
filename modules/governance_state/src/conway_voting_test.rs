#[cfg(test)]
mod tests {
    use crate::conway_voting::ConwayVoting;
    use crate::voting_state::VotingRegistrationState;
    use acropolis_common::{
        protocol_params::ProtocolParams, rational_number::RationalNumber,
        ConstitutionalCommitteeKeyHash, ConstitutionalCommitteeScriptHash, Credential,
        DRepCredential, DRepScriptHash, DelegatedStake, DrepKeyHash, GovActionId, KeyHash,
        Lovelace, PoolId, ProposalProcedure, SingleVoterVotes, TxHash, Vote, VoteCount, VoteResult,
        Voter, VotingProcedure,
    };
    use anyhow::{anyhow, bail, Result};

    use serde_with::{base64::Base64, serde_as};
    use std::{
        collections::{BTreeMap, HashMap},
        ops::Bound::Included,
        str::FromStr,
    };

    use tracing_subscriber::prelude::*;
    use tracing_subscriber::{filter, fmt, EnvFilter, Registry};

    struct ConwayVotingTestRecord {
        action_id: GovActionId,
        proposal_procedure: ProposalProcedure,
        start_epoch: u64,
        ratification_epoch: Option<u64>,
        expiration_epoch: Option<u64>,
        votes_count: VoteResult<VoteCount>,
        votes_threshold: VoteResult<RationalNumber>,
    }

    #[serde_as]
    #[derive(Clone, Debug, serde::Deserialize)]
    struct PoolRecord(
        #[serde_as(as = "Base64")] PoolId, // key
        u64,                               // active stake
        u64,                               // live stake
    );

    #[serde_as]
    #[derive(Clone, Debug, serde::Deserialize)]
    struct DRepRecord(
        u8,                                 // id of DRep credential type (0=addr, 1=script)
        #[serde_as(as = "Base64")] KeyHash, // DRep credential value
        Lovelace,                           // value
    );

    #[serde_as]
    #[derive(Clone, Debug, serde::Deserialize)]
    struct VotingRecord(
        u8, // id of DRep credential type (0=addr, 1=script), or SPO (2=pool)
        #[serde_as(as = "Base64")] KeyHash, // Key
    );

    impl VotingRecord {
        pub fn to_voter(&self) -> Result<Voter> {
            let key = self.1;
            match &self.0 {
                0 => Ok(Voter::DRepKey(DrepKeyHash::from(key))),
                1 => Ok(Voter::DRepScript(DRepScriptHash::from(key))),
                2 => Ok(Voter::StakePoolKey(PoolId::from(key))),
                3 => Ok(Voter::ConstitutionalCommitteeKey(
                    ConstitutionalCommitteeKeyHash::from(key),
                )),
                4 => Ok(Voter::ConstitutionalCommitteeScript(
                    ConstitutionalCommitteeScriptHash::from(key),
                )),
                _ => bail!("Unknown voter key type {}", self.0),
            }
        }
    }

    fn parse_drep_credential(cred_type: u8, key: KeyHash) -> Result<DRepCredential> {
        match cred_type {
            0 => Ok(Credential::AddrKeyHash(key)),
            1 => Ok(Credential::ScriptHash(key)),
            x => bail!("Unsupported DRep credential type: {x}"),
        }
    }

    fn parse_u64_opt(s: &str) -> Result<Option<u64>> {
        if s.is_empty() {
            Ok(None)
        } else {
            let wp = s
                .strip_prefix("Some(")
                .ok_or_else(|| anyhow!("Does not have 'Some(' prefix {}", s))?;
            let num = wp.strip_suffix(")").ok_or_else(|| anyhow!("Must have ')' suffix {}", s))?;
            num.parse().map_err(|e| anyhow!("Cannot parse value {num}, error {e}")).map(Some)
        }
    }

    fn read_pools(pools_json: &[u8]) -> Result<HashMap<u64, HashMap<PoolId, DelegatedStake>>> {
        let pools = serde_json::from_slice::<Vec<(u64, Vec<PoolRecord>)>>(pools_json)?;
        let res = HashMap::from_iter(pools.iter().map(|(epoch, distr)| {
            (
                *epoch,
                HashMap::from_iter(distr.iter().map(|PoolRecord(id, active, live)| {
                    (
                        *id,
                        DelegatedStake {
                            active: *active,
                            active_delegators_count: 0,
                            live: *live,
                        },
                    )
                })),
            )
        }));
        Ok(res)
    }

    fn read_dreps(dreps_json: &[u8]) -> Result<HashMap<u64, HashMap<DRepCredential, Lovelace>>> {
        let dreps = serde_json::from_slice::<Vec<(u64, Vec<DRepRecord>)>>(dreps_json)?;

        let converted = dreps
            .iter()
            .map(|(epoch, distr)| {
                Ok((
                    *epoch,
                    HashMap::from_iter(
                        distr
                            .iter()
                            .map(|DRepRecord(dt, key, lvl)| {
                                Ok((parse_drep_credential(*dt, *key)?, *lvl))
                            })
                            .collect::<Result<Vec<(DRepCredential, Lovelace)>>>()?,
                    ),
                ))
            })
            .collect::<Result<Vec<(u64, HashMap<DRepCredential, Lovelace>)>>>()?;

        let res = HashMap::from_iter(converted);

        Ok(res)
    }

    type VoterList = Vec<(Voter, VotingProcedure)>;

    fn map_voter_list(votes: &[VotingRecord], v: Vote) -> Result<VoterList> {
        votes
            .iter()
            .map(|x| {
                Ok((
                    x.to_voter()?,
                    VotingProcedure {
                        vote: v.clone(),
                        anchor: None,
                        vote_index: 0,
                    },
                ))
            })
            .collect()
    }

    /// Reads list of votes: for each epoch, for each gov-action, three lists of voters:
    /// ([yes voters], [no voters], [abstain voters])
    fn read_voting_state(voting_json: &[u8]) -> Result<HashMap<(u64, GovActionId), VoterList>> {
        let voting =
            serde_json::from_slice::<Vec<(u64, String, Vec<Vec<VotingRecord>>)>>(voting_json)?;

        let mut voting_hash = HashMap::new();
        for (epoch, action_id, votes) in voting.iter() {
            let action_id = GovActionId::from_bech32(action_id)?;

            let mut vote_procs = Vec::new();
            if !votes.is_empty() {
                vote_procs = map_voter_list(votes.first().unwrap(), Vote::Yes)?;
                vote_procs.append(&mut map_voter_list(votes.get(1).unwrap(), Vote::No)?);
                vote_procs.append(&mut map_voter_list(votes.get(2).unwrap(), Vote::Abstain)?);
            }

            let prev = voting_hash.insert((*epoch, action_id.clone()), vote_procs);

            if let Some(prev) = prev {
                bail!("Epoch {epoch}, action {action_id} is already present: {prev:?}");
            }
        }

        Ok(voting_hash)
    }

    fn read_voting_test_records(records: &[u8]) -> Result<Vec<ConwayVotingTestRecord>> {
        let mut test_records = Vec::new();
        let mut reader = csv::ReaderBuilder::new().delimiter(b',').from_reader(records);
        for line in reader.records() {
            // gov_action10lty9xka3unprtvdfrqvcjgsz33sjwhv9p06afqzar8au782trtsq7dhd95,
            // 7fd6429add8f2611ad8d48c0cc49101463093aec285faea402e8cfde78ea58d7,0,,100000000000,
            // e17a094354239fd5e7b24665158ff7ee2afdfabcc947ba3b64742ffa48,521,,Information,
            // "Information",,,,,Some(521),
            // c0/3/2:d137995525188443/298036308191794/97888570682988:
            // s504645304083669/961815354510517/93516758517300,c0:d1:s1

            let records = line?.iter().map(|x| x.to_owned()).collect::<Vec<String>>();
            if records.is_empty() {
                continue;
            } else if records.len() != 17 {
                bail!("Wrong number of elements in csv line: {:?}", records)
            }

            let action_id = records.first().unwrap();
            let start_epoch = records.get(6).unwrap();
            let proposal = records.get(9).unwrap();
            let ratification_epoch = records.get(11).unwrap();
            let expiration_epoch = records.get(14).unwrap();
            let count = records.get(15).unwrap();
            let threshold = records.get(16).unwrap();

            test_records.push(ConwayVotingTestRecord {
                action_id: GovActionId::from_bech32(action_id)?,
                proposal_procedure: serde_json::from_slice::<ProposalProcedure>(
                    proposal.as_bytes(),
                )?,
                start_epoch: u64::from_str(start_epoch)?,
                ratification_epoch: parse_u64_opt(ratification_epoch)?,
                expiration_epoch: parse_u64_opt(expiration_epoch)?,
                votes_count: VoteResult::from_str(count)?,
                votes_threshold: VoteResult::from_str(threshold)?,
            });
        }
        Ok(test_records)
    }

    fn read_protocol_params(configs_json: &[u8]) -> Result<BTreeMap<u64, ProtocolParams>> {
        let configs = serde_json::from_slice::<Vec<(u64, ProtocolParams)>>(configs_json)?;
        Ok(BTreeMap::from_iter(configs))
    }

    #[test]
    #[ignore]
    fn test_voting_mainnet_up_573() -> Result<()> {
        let fmt_layer = fmt::layer()
            .with_filter(
                EnvFilter::from_default_env().add_directive(filter::LevelFilter::INFO.into()),
            )
            .with_filter(filter::filter_fn(|meta| meta.is_event()));
        //tracing_subscriber::fmt::init();
        Registry::default().with(fmt_layer).init();

        let epoch_info = serde_json::from_slice::<Vec<(u64, u64, u64, u64, u64)>>(include_bytes!(
            "../data/epoch_pool_stats.json"
        ))?;

        let epoch_info = HashMap::<u64, VotingRegistrationState>::from_iter(epoch_info.iter().map(
            |(epoch, spos, dreps, nconf, abs)| {
                (
                    *epoch,
                    VotingRegistrationState::new(*spos, *dreps, *nconf, *abs, 7),
                )
            },
        ));

        println!("Reading pool_state");
        let pools = read_pools(include_bytes!("../data/pool_state.json"))?;

        println!("Reading drep_state");
        let dreps = read_dreps(include_bytes!("../data/drep_state.json"))?;

        println!("Reading voting_state");
        let votes = read_voting_state(include_bytes!("../data/voting_state.json"))?;

        println!("Reading conway_verification");
        let voting_test =
            read_voting_test_records(include_bytes!("../data/conway_verification.csv"))?;

        println!("Reading configs");
        let protocol_params = read_protocol_params(include_bytes!("../data/param_state.json"))?;

        for record in voting_test.iter() {
            println!(
                "Testing epoch {}, action {}",
                record.start_epoch, record.action_id
            );

            let (_cfg_epoch, cfg) =
                protocol_params.range((Included(0), Included(record.start_epoch))).last().unwrap();
            let shelley = cfg.shelley.as_ref().unwrap();
            let conway = cfg.conway.as_ref().unwrap();
            let bootstrap = shelley.protocol_params.protocol_version.major <= 9;

            let mut conway_voting = ConwayVoting::new(None);
            conway_voting.update_parameters(&cfg.conway, bootstrap);
            conway_voting
                .insert_proposal_procedure(record.start_epoch, &record.proposal_procedure)?;

            for epoch in record.start_epoch.. {
                println!("Testing action {} in epoch {epoch}", record.action_id);
                let voting_state = epoch_info.get(&epoch).unwrap();

                if let Some(votes) = votes.get(&(epoch + 1, record.action_id.clone())) {
                    for (voter, voteproc) in votes {
                        let procs =
                            HashMap::from_iter([(record.action_id.clone(), voteproc.clone())]);
                        conway_voting
                            .insert_voting_procedure(
                                epoch,
                                voter,
                                &TxHash::default(),
                                &SingleVoterVotes {
                                    voting_procedures: procs,
                                },
                            )?
                            .as_result()?
                    }
                }

                let current_drep = dreps.get(&epoch).unwrap();
                let current_pool = pools.get(&epoch).unwrap();
                println!(
                    "Processing proposal, expired = {}",
                    conway_voting.is_expired(epoch, &record.action_id)?
                );

                let outcome = conway_voting.process_one_proposal(
                    epoch,
                    voting_state,
                    &record.action_id,
                    current_drep,
                    current_pool,
                )?;

                let Some(outcome) = outcome else {
                    assert!(epoch < record.start_epoch + conway.gov_action_lifetime as u64 + 1);
                    continue; // We don't have exact votes yet
                };

                assert_eq!(outcome.accepted, record.ratification_epoch.is_some());
                assert_eq!(outcome.accepted, record.expiration_epoch.is_none());
                if outcome.accepted {
                    assert_eq!(Some(epoch + 2), record.ratification_epoch)
                } else {
                    assert_eq!(Some(epoch), record.expiration_epoch)
                }

                assert_eq!(
                    outcome.votes_threshold.committee,
                    record.votes_threshold.committee
                );
                assert_eq!(outcome.votes_threshold.drep, record.votes_threshold.drep);
                assert_eq!(outcome.votes_threshold.pool, record.votes_threshold.pool);

                assert_eq!(outcome.votes_cast.committee, record.votes_count.committee);
                //TODO: proper votes counting
                //assert_eq!(outcome.votes_cast.drep, record.votes_count.drep);
                //assert_eq!(outcome.votes_cast.pool, record.votes_count.pool);

                break;
            }
        }

        Ok(())
    }
}
