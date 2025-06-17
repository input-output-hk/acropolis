use anyhow::{anyhow, bail, Result};
use acropolis_common::{
    Anchor, BlockVersionData, ProtocolConsts, SoftForkRule, ShelleyProtocolParams,
    TxFeePolicy, Nonce, NonceVariant, ProtocolVersion,
    Committee, Constitution, DRepVotingThresholds, NetworkId, PoolVotingThresholds,
    Credential, Era, AlonzoParams, ByronParams, ConwayParams, ShelleyParams,
    rational_number::RationalNumber
};
use fraction::Fraction;
use hex::decode;
use pallas::ledger::{configs::*, primitives};
use serde::Deserialize;
use std::collections::HashMap;

const PREDEFINED_GENESIS: [(Era, &[u8]); 4] = [
    (Era::Byron, include_bytes!("../downloads/mainnet-byron-genesis.json")),
    (Era::Shelley, include_bytes!("../downloads/mainnet-shelley-genesis.json")),
    (Era::Alonzo, include_bytes!("../downloads/mainnet-alonzo-genesis.json")),
    (Era::Conway, include_bytes!("../downloads/mainnet-conway-genesis.json"))
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
    RationalNumber {
        numerator: fraction.numerator,
        denominator: fraction.denominator,
    }
}

pub fn map_f32_to_rational(value: f32) -> Result<RationalNumber> {
    if value.is_sign_negative() {
        return Err(anyhow!("Value {} must be greater than 0", value));
    }
    let fract = Fraction::from(value);
    Ok(RationalNumber {
        numerator: *fract
            .numer()
            .ok_or_else(|| anyhow!("Cannot get numerator for {}", value))?,
        denominator: *fract
            .denom()
            .ok_or_else(|| anyhow!("Cannot get denominator for {}", value))?,
    })
}

fn map_pool_thresholds(thresholds: &conway::PoolVotingThresholds) -> Result<PoolVotingThresholds> {
    Ok(PoolVotingThresholds {
        motion_no_confidence: map_f32_to_rational(thresholds.motion_no_confidence)?,
        committee_normal: map_f32_to_rational(thresholds.committee_normal)?,
        committee_no_confidence: map_f32_to_rational(thresholds.committee_no_confidence)?,
        hard_fork_initiation: map_f32_to_rational(thresholds.hard_fork_initiation)?,
        security_voting_threshold: map_f32_to_rational(thresholds.pp_security_group)?,
    })
}

fn map_drep_thresholds(thresholds: &conway::DRepVotingThresholds) -> Result<DRepVotingThresholds> {
    Ok(DRepVotingThresholds {
        motion_no_confidence: map_f32_to_rational(thresholds.motion_no_confidence)?,
        committee_normal: map_f32_to_rational(thresholds.committee_normal)?,
        committee_no_confidence: map_f32_to_rational(thresholds.committee_normal)?,
        update_constitution: map_f32_to_rational(thresholds.update_to_constitution)?,
        hard_fork_initiation: map_f32_to_rational(thresholds.hard_fork_initiation)?,
        pp_network_group: map_f32_to_rational(thresholds.pp_network_group)?,
        pp_economic_group: map_f32_to_rational(thresholds.pp_economic_group)?,
        pp_technical_group: map_f32_to_rational(thresholds.pp_technical_group)?,
        pp_governance_group: map_f32_to_rational(thresholds.pp_gov_group)?,
        treasury_withdrawal: map_f32_to_rational(thresholds.treasury_withdrawal)?,
    })
}

pub fn map_constitution(constitution: &conway::Constitution) -> Result<Constitution> {
    Ok(Constitution {
        anchor: map_anchor(&constitution.anchor)?,
        guardrail_script: Some(decode_hex_string(&constitution.script, 28)?),
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
        ),
        plutus_v3_cost_model: genesis.plutus_v3_cost_model.clone(),
        constitution: map_constitution(&genesis.constitution)?,
        committee: map_committee(&genesis.committee)?,
    })
}

#[allow(dead_code)]
fn map_alonzo(_genesis: &alonzo::GenesisFile) -> Result<AlonzoParams> {
    bail!("Alonzo not supported")
}

pub fn map_pallas_rational(r: &primitives::RationalNumber) -> RationalNumber {
    RationalNumber {
        numerator: r.numerator,
        denominator: r.denominator
    }
}

fn map_network_id(id: &str) -> Result<NetworkId> {
    match id {
        "Testnet" => Ok(NetworkId::Testnet),
        "Mainnet" => Ok(NetworkId::Mainnet),
        n => Err(anyhow!("Network id {n} is unknown"))
    }
}

fn map_shelley_nonce(e: &shelley::ExtraEntropy) -> Result<Nonce> {
    Ok(Nonce {
        tag: match &e.tag {
            shelley::NonceVariant::NeutralNonce => NonceVariant::NeutralNonce,
            shelley::NonceVariant::Nonce => NonceVariant::Nonce
        },
        hash: e.hash.as_ref().map(|h| decode_hex_string(h, 32)).transpose()?
    })
}

fn map_shelley_protocol_params(p: &shelley::ProtocolParams) -> Result<ShelleyProtocolParams> {
    Ok(ShelleyProtocolParams {
        protocol_version: ProtocolVersion {
            minor: p.protocol_version.minor,
            major: p.protocol_version.major
        },
        max_tx_size: p.max_tx_size,
        max_block_body_size: p.max_block_body_size,
        max_block_header_size: p.max_block_header_size,
        key_deposit: p.key_deposit,
        min_utxo_value: p.min_utxo_value,
        minfee_a: p.min_fee_a,
        minfee_b: p.min_fee_b,
        pool_deposit: p.pool_deposit,
        stake_pool_target_num: p.n_opt,
        min_pool_cost: p.min_pool_cost,
        pool_retire_max_epoch: p.e_max,
        extra_entropy: map_shelley_nonce(&p.extra_entropy)?,
        decentralisation_param: map_pallas_rational(&p.decentralisation_param),
        monetary_expansion: map_pallas_rational(&p.rho),
        treasury_cut: map_pallas_rational(&p.tau),
        pool_pledge_influence: map_pallas_rational(&p.a0)
    })
}

fn map_shelley(genesis: &shelley::GenesisFile) -> Result<ShelleyParams> {
    Ok(ShelleyParams {
        active_slots_coeff: genesis.active_slots_coeff.clone(),
        epoch_length: genesis.epoch_length.clone(),
        max_kes_evolutions: genesis.max_kes_evolutions.clone(),
        max_lovelace_supply: genesis.max_lovelace_supply.clone(),
        network_id: genesis.network_id.as_deref().map(map_network_id).transpose()?,
        network_magic: genesis.network_magic,
        protocol_params: map_shelley_protocol_params(&genesis.protocol_params)?,
        security_param: genesis.security_param.clone(),
        slot_length: genesis.slot_length.clone(),
        slots_per_kes_period: genesis.slots_per_kes_period.clone(),
        system_start: genesis.system_start.as_ref().map(|s| s.parse()).transpose()?,
        update_quorum: genesis.update_quorum
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
        update_vote_thd: bvd.update_vote_thd
    })
}

fn map_protocol_consts(c: &byron::ProtocolConsts) -> Result<ProtocolConsts> {
    Ok(ProtocolConsts {
        k: c.k,
        protocol_magic: c.protocol_magic,
        vss_max_ttl: c.vss_max_ttl,
        vss_min_ttl: c.vss_min_ttl
    })
}

fn map_byron(genesis: &byron::GenesisFile) -> Result<ByronParams> {
    Ok(ByronParams {
        block_version_data: map_block_version_data(&genesis.block_version_data)?,
        fts_seed: genesis.fts_seed.as_ref().map(|s| decode_hex_string(s, 42)).transpose()?,
        protocol_consts: map_protocol_consts(&genesis.protocol_consts)?,
        start_time: genesis.start_time
    })
}

fn read_pdef_genesis<'a, PallasStruct: Deserialize<'a>, OurStruct>(
    era: Era, map: impl Fn(&PallasStruct) -> Result<OurStruct>
) -> Result<OurStruct> {
    let genesis = match PREDEFINED_GENESIS.iter().find(|(e,_g)| *e == era) {
        Some(eg) => eg,
        None => bail!("Genesis for {era} not defined")
    };

    match &serde_json::from_slice(genesis.1) {
        Ok(decoded) => Ok(map(decoded)?),
        Err(e) => bail!("Cannot read JSON for {era} genesis: {e}")
    }
}

pub fn read_byron_genesis() -> Result<ByronParams> {
    read_pdef_genesis::<byron::GenesisFile, ByronParams> (Era::Byron, map_byron)
}

pub fn read_shelley_genesis() -> Result<ShelleyParams> {
    read_pdef_genesis::<shelley::GenesisFile, ShelleyParams> (Era::Shelley, map_shelley)
}

pub fn _read_alonzo_genesis() -> Result<AlonzoParams> {
    read_pdef_genesis::<alonzo::GenesisFile, AlonzoParams> (Era::Alonzo, map_alonzo)
}

pub fn read_conway_genesis() -> Result<ConwayParams> {
    read_pdef_genesis::<conway::GenesisFile, ConwayParams> (Era::Conway, map_conway)
}
