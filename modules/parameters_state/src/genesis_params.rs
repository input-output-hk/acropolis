use crate::alonzo_genesis;
use acropolis_common::{
    protocol_params::{AlonzoParams, BabbageParams, ByronParams, ConwayParams, ShelleyParams},
    rational_number::{rational_number_from_f32, RationalNumber},
    Anchor, BlockVersionData, Committee, Constitution, CostModel, Credential, DRepVotingThresholds,
    Era, HeavyDelegate, MagicNumber, PoolId, PoolVotingThresholds, ProtocolConsts, SoftForkRule,
    TxFeePolicy,
};
use anyhow::{anyhow, bail, Result};
use base64::prelude::*;
use hex::decode;
use pallas::ledger::configs::*;
use serde::Deserialize;
use std::collections::HashMap;

const PREDEFINED_GENESIS: [(&str, Era, &[u8]); 12] = [
    (
        "sanchonet",
        Era::Byron,
        include_bytes!("../downloads/sanchonet-byron-genesis.json"),
    ),
    (
        "sanchonet",
        Era::Shelley,
        include_bytes!("../downloads/sanchonet-shelley-genesis.json"),
    ),
    (
        "sanchonet",
        Era::Alonzo,
        include_bytes!("../downloads/sanchonet-alonzo-genesis.json"),
    ),
    (
        "sanchonet",
        Era::Conway,
        include_bytes!("../downloads/sanchonet-conway-genesis.json"),
    ),
    (
        "preview",
        Era::Byron,
        include_bytes!("../downloads/preview-byron-genesis.json"),
    ),
    (
        "preview",
        Era::Shelley,
        include_bytes!("../downloads/preview-shelley-genesis.json"),
    ),
    (
        "preview",
        Era::Alonzo,
        include_bytes!("../downloads/preview-alonzo-genesis.json"),
    ),
    (
        "preview",
        Era::Conway,
        include_bytes!("../downloads/preview-conway-genesis.json"),
    ),
    (
        "mainnet",
        Era::Byron,
        include_bytes!("../downloads/mainnet-byron-genesis.json"),
    ),
    (
        "mainnet",
        Era::Shelley,
        include_bytes!("../downloads/mainnet-shelley-genesis.json"),
    ),
    (
        "mainnet",
        Era::Alonzo,
        include_bytes!("../downloads/mainnet-alonzo-genesis.json"),
    ),
    (
        "mainnet",
        Era::Conway,
        include_bytes!("../downloads/mainnet-conway-genesis.json"),
    ),
];

fn decode_hex_string(s: &str, len: usize) -> Result<Vec<u8>> {
    let key_hash = decode(s.to_owned().into_bytes())?;
    if key_hash.len() == len {
        Ok(key_hash)
    } else {
        Err(anyhow!(
            "Incorrect hex length: {} instead of {}",
            key_hash.len(),
            len
        ))
    }
}

fn map_anchor(anchor: &conway::Anchor) -> Result<Anchor> {
    Ok(Anchor {
        url: anchor.url.clone(),
        data_hash: decode_hex_string(&anchor.data_hash, 32)?,
    })
}

pub fn map_fraction(fraction: &conway::Fraction) -> RationalNumber {
    RationalNumber::new(fraction.numerator, fraction.denominator)
}

fn map_pool_thresholds(thresholds: &conway::PoolVotingThresholds) -> Result<PoolVotingThresholds> {
    Ok(PoolVotingThresholds {
        motion_no_confidence: rational_number_from_f32(thresholds.motion_no_confidence)?,
        committee_normal: rational_number_from_f32(thresholds.committee_normal)?,
        committee_no_confidence: rational_number_from_f32(thresholds.committee_no_confidence)?,
        hard_fork_initiation: rational_number_from_f32(thresholds.hard_fork_initiation)?,
        security_voting_threshold: rational_number_from_f32(thresholds.pp_security_group)?,
    })
}

fn map_drep_thresholds(thresholds: &conway::DRepVotingThresholds) -> Result<DRepVotingThresholds> {
    Ok(DRepVotingThresholds {
        motion_no_confidence: rational_number_from_f32(thresholds.motion_no_confidence)?,
        committee_normal: rational_number_from_f32(thresholds.committee_normal)?,
        committee_no_confidence: rational_number_from_f32(thresholds.committee_normal)?,
        update_constitution: rational_number_from_f32(thresholds.update_to_constitution)?,
        hard_fork_initiation: rational_number_from_f32(thresholds.hard_fork_initiation)?,
        pp_network_group: rational_number_from_f32(thresholds.pp_network_group)?,
        pp_economic_group: rational_number_from_f32(thresholds.pp_economic_group)?,
        pp_technical_group: rational_number_from_f32(thresholds.pp_technical_group)?,
        pp_governance_group: rational_number_from_f32(thresholds.pp_gov_group)?,
        treasury_withdrawal: rational_number_from_f32(thresholds.treasury_withdrawal)?,
    })
}

pub fn map_constitution(constitution: &conway::Constitution) -> Result<Constitution> {
    Ok(Constitution {
        anchor: map_anchor(&constitution.anchor)?,
        guardrail_script: Some(decode_hex_string(&constitution.script, 28)?.try_into().unwrap()),
    })
}

pub fn map_committee(committee: &conway::Committee) -> Result<Committee> {
    let mut members = HashMap::new();

    for (member, expiry_epoch) in committee.members.iter() {
        members.insert(Credential::from_json_string(member)?, *expiry_epoch);
    }

    Ok(Committee {
        members,
        threshold: map_fraction(&committee.threshold),
    })
}

fn map_conway(genesis: &conway::GenesisFile) -> Result<ConwayParams> {
    Ok(ConwayParams {
        pool_voting_thresholds: map_pool_thresholds(&genesis.pool_voting_thresholds)?,
        d_rep_voting_thresholds: map_drep_thresholds(&genesis.d_rep_voting_thresholds)?,
        committee_min_size: genesis.committee_min_size,
        committee_max_term_length: genesis.committee_max_term_length,
        gov_action_lifetime: genesis.gov_action_lifetime,
        gov_action_deposit: genesis.gov_action_deposit,
        d_rep_deposit: genesis.d_rep_deposit,
        d_rep_activity: genesis.d_rep_activity,
        min_fee_ref_script_cost_per_byte: RationalNumber::from(
            genesis.min_fee_ref_script_cost_per_byte,
            1,
        ),
        plutus_v3_cost_model: CostModel::new(genesis.plutus_v3_cost_model.clone()),
        constitution: map_constitution(&genesis.constitution)?,
        committee: map_committee(&genesis.committee)?,
    })
}

fn map_block_version_data(bvd: &byron::BlockVersionData) -> Result<BlockVersionData> {
    Ok(BlockVersionData {
        script_version: bvd.script_version,
        heavy_del_thd: bvd.heavy_del_thd,
        max_block_size: bvd.max_block_size,
        max_header_size: bvd.max_header_size,
        max_proposal_size: bvd.max_proposal_size,
        max_tx_size: bvd.max_tx_size,
        mpc_thd: bvd.mpc_thd,
        slot_duration: bvd.slot_duration,
        softfork_rule: SoftForkRule {
            init_thd: bvd.softfork_rule.init_thd,
            min_thd: bvd.softfork_rule.min_thd,
            thd_decrement: bvd.softfork_rule.thd_decrement,
        },
        tx_fee_policy: TxFeePolicy {
            multiplier: bvd.tx_fee_policy.multiplier,
            summand: bvd.tx_fee_policy.summand,
        },
        unlock_stake_epoch: bvd.unlock_stake_epoch,
        update_implicit: bvd.update_implicit,
        update_proposal_thd: bvd.update_proposal_thd,
        update_vote_thd: bvd.update_vote_thd,
    })
}

fn map_protocol_consts(c: &byron::ProtocolConsts) -> Result<ProtocolConsts> {
    Ok(ProtocolConsts {
        k: c.k,
        protocol_magic: MagicNumber::new(c.protocol_magic),
        vss_max_ttl: c.vss_max_ttl,
        vss_min_ttl: c.vss_min_ttl,
    })
}

fn map_byron(genesis: &byron::GenesisFile) -> Result<ByronParams> {
    let heavy_delegation = genesis
        .heavy_delegation
        .iter()
        .map(|(k, v)| {
            let k = PoolId::try_from(decode(k)?)?;
            let v = HeavyDelegate {
                cert: decode(v.cert.clone())?,
                delegate_pk: BASE64_STANDARD.decode(v.delegate_pk.clone())?,
                issuer_pk: BASE64_STANDARD.decode(v.issuer_pk.clone())?,
            };
            Ok::<(PoolId, HeavyDelegate), anyhow::Error>((k, v))
        })
        .collect::<Result<_, _>>()?;
    Ok(ByronParams {
        block_version_data: map_block_version_data(&genesis.block_version_data)?,
        fts_seed: genesis.fts_seed.as_ref().map(|s| decode_hex_string(s, 42)).transpose()?,
        protocol_consts: map_protocol_consts(&genesis.protocol_consts)?,
        start_time: genesis.start_time,
        heavy_delegation,
    })
}

fn read_pdef_genesis<'a, PallasStruct: Deserialize<'a>, OurStruct>(
    network: &str,
    era: Era,
    map: impl Fn(&PallasStruct) -> Result<OurStruct>,
) -> Result<OurStruct> {
    let (_net, _era, genesis) =
        match PREDEFINED_GENESIS.iter().find(|(n, e, _g)| *n == network && *e == era) {
            Some(eg) => eg,
            None => bail!("Genesis for {era} not defined"),
        };

    match &serde_json::from_slice(genesis) {
        Ok(decoded) => map(decoded),
        Err(e) => bail!("Cannot read JSON for {network} {era} genesis: {e}"),
    }
}

pub fn read_byron_genesis(network: &str) -> Result<ByronParams> {
    read_pdef_genesis::<byron::GenesisFile, ByronParams>(network, Era::Byron, map_byron)
}

pub fn read_shelley_genesis(network: &str) -> Result<ShelleyParams> {
    read_pdef_genesis::<ShelleyParams, ShelleyParams>(network, Era::Shelley, |x| Ok(x.clone()))
}

pub fn read_alonzo_genesis(network: &str) -> Result<AlonzoParams> {
    read_pdef_genesis::<alonzo_genesis::Genesis, AlonzoParams>(
        network,
        Era::Alonzo,
        alonzo_genesis::map_alonzo,
    )
}

pub fn apply_babbage_transition(alonzo_params_opt: Option<&AlonzoParams>) -> Result<BabbageParams> {
    match alonzo_params_opt {
        Some(alonzo_params) => Ok(BabbageParams {
            coins_per_utxo_byte: alonzo_params.lovelace_per_utxo_word / 8,
            plutus_v2_cost_model: None,
        }),
        None => bail!("Alonzo params must be set before babbage transition"),
    }
}

pub fn read_conway_genesis(network: &str) -> Result<ConwayParams> {
    read_pdef_genesis::<conway::GenesisFile, ConwayParams>(network, Era::Conway, map_conway)
}

#[cfg(test)]
mod test {
    use crate::genesis_params::{self, PREDEFINED_GENESIS};
    use acropolis_common::{protocol_params::ShelleyParams, rational_number::RationalNumber, Era};
    use anyhow::Result;
    use blake2::{digest::consts::U32, Blake2b, Digest};
    use std::collections::HashSet;

    fn get_networks() -> HashSet<&'static str> {
        HashSet::<&str>::from_iter(genesis_params::PREDEFINED_GENESIS.iter().map(|p| p.0))
    }

    #[test]
    fn test_read_genesis() -> Result<()> {
        for net in get_networks().iter() {
            println!("{:?}", genesis_params::read_byron_genesis(net)?);
            println!("{:?}", genesis_params::read_shelley_genesis(net)?);
            println!("{:?}", genesis_params::read_alonzo_genesis(net)?);
            println!("{:?}", genesis_params::read_conway_genesis(net)?);
        }
        Ok(())
    }

    #[test]
    fn test_shelley_genesis_hash() -> Result<()> {
        let (_net, _era, genesis) = PREDEFINED_GENESIS
            .iter()
            .find(|(n, e, _g)| *n == "mainnet" && *e == Era::Shelley)
            .unwrap();

        // blake2b-256
        let mut hasher = Blake2b::<U32>::new();
        hasher.update([&genesis[..]].concat());
        let hash: [u8; 32] = hasher.finalize().into();
        println!("{:?}", hex::encode(hash));
        Ok(())
    }

    #[test]
    fn test_read_write_shelley() -> Result<()> {
        for net in get_networks().iter() {
            let shelley = genesis_params::read_shelley_genesis(net)?;
            let shelley_str = serde_json::to_string(&shelley).unwrap();
            let shelley_back = serde_json::from_str::<ShelleyParams>(&shelley_str).unwrap();
            println!("Encoded: {shelley:?}\n\nStr: {shelley_str}\n\nBack: {shelley_back:?}\n");
            assert_eq!(shelley, shelley_back);
        }
        Ok(())
    }

    /// Checking that value for monetary expansion is correctly parsed.
    /// Pallas loses precision here, and does not parse this value properly.
    #[test]
    fn test_shelley_monetary_expansion_value() -> Result<()> {
        for net in get_networks().iter() {
            let shelley_params = genesis_params::read_shelley_genesis(net)?.protocol_params;
            assert_eq!(
                shelley_params.monetary_expansion,
                RationalNumber::new(3, 1000)
            );
        }
        Ok(())
    }

    #[test]
    fn test_pool_pledge_influence() -> Result<()> {
        for net in get_networks().iter() {
            let shelley_params = genesis_params::read_shelley_genesis(net)?.protocol_params;
            assert_eq!(
                shelley_params.pool_pledge_influence,
                RationalNumber::new(3, 10)
            );
        }
        Ok(())
    }
}
