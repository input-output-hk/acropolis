use crate::{
    genesis_values::GenesisValues,
    rational_number::{ChameleonFraction, RationalNumber},
    BlockVersionData, Committee, Constitution, CostModel, DRepVotingThresholds, Era, ExUnitPrices,
    ExUnits, NetworkId, PoolVotingThresholds, ProtocolConsts,
};
use anyhow::Result;
use blake2::{digest::consts::U32, Blake2b, Digest};
use chrono::{DateTime, Utc};
use serde_with::serde_as;

#[derive(Debug, Default, PartialEq, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProtocolParams {
    pub byron: Option<ByronParams>,
    pub alonzo: Option<AlonzoParams>,
    pub shelley: Option<ShelleyParams>,
    pub babbage: Option<BabbageParams>,
    pub conway: Option<ConwayParams>,
}

//
// Byron protocol parameters
//

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ByronParams {
    pub block_version_data: BlockVersionData,
    pub fts_seed: Option<Vec<u8>>,
    pub protocol_consts: ProtocolConsts,
    pub start_time: u64,
}

//
// Alonzo protocol parameters
//

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AlonzoParams {
    pub lovelace_per_utxo_word: u64, // Deprecated after transition to Babbage
    pub execution_prices: ExUnitPrices,
    pub max_tx_ex_units: ExUnits,
    pub max_block_ex_units: ExUnits,
    pub max_value_size: u32,
    pub collateral_percentage: u32,
    pub max_collateral_inputs: u32,
    pub plutus_v1_cost_model: Option<CostModel>,
}

//
// Shelley protocol parameters
//

#[serde_as]
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShelleyProtocolParams {
    pub protocol_version: ProtocolVersion,
    pub max_tx_size: u32,
    pub max_block_body_size: u32,
    pub max_block_header_size: u32,
    pub key_deposit: u64,
    #[serde(rename = "minUTxOValue")]
    pub min_utxo_value: u64,

    #[serde(rename = "minFeeA")]
    pub minfee_a: u32,

    #[serde(rename = "minFeeB")]
    pub minfee_b: u32,
    pub pool_deposit: u64,

    /// AKA desired_number_of_stake_pools, optimal_pool_count, n_opt, technical parameter k
    /// Important: *not to be mixed* with security parameter k, which is not here
    #[serde(rename = "nOpt")]
    pub stake_pool_target_num: u32,
    pub min_pool_cost: u64,

    /// AKA eMax, e_max
    #[serde(rename = "eMax")]
    pub pool_retire_max_epoch: u64,
    pub extra_entropy: Nonce,
    #[serde_as(as = "ChameleonFraction")]
    pub decentralisation_param: RationalNumber,

    /// AKA Rho, expansion_rate
    #[serde(rename = "rho")]
    #[serde_as(as = "ChameleonFraction")]
    pub monetary_expansion: RationalNumber,

    /// AKA Tau, treasury_growth_rate
    #[serde(rename = "tau")]
    #[serde_as(as = "ChameleonFraction")]
    pub treasury_cut: RationalNumber,

    /// AKA a0
    #[serde(rename = "a0")]
    #[serde_as(as = "ChameleonFraction")]
    pub pool_pledge_influence: RationalNumber,
}

#[serde_as]
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShelleyParams {
    #[serde_as(as = "ChameleonFraction")]
    pub active_slots_coeff: RationalNumber,
    pub epoch_length: u32,
    pub max_kes_evolutions: u32,
    pub max_lovelace_supply: u64,
    pub network_id: NetworkId,
    pub network_magic: u32,
    pub protocol_params: ShelleyProtocolParams,

    /// Ouroboros security parameter k: the Shardagnostic security paramaters,
    /// aka @k@. This is the maximum number of blocks the node would ever be
    /// prepared to roll back by. Clients of the node following the chain should
    /// be prepared to handle the node switching forks up to this long.
    /// (source: GenesisParameters.hs)
    pub security_param: u32,

    pub slot_length: u32,
    pub slots_per_kes_period: u32,
    pub system_start: DateTime<Utc>,
    pub update_quorum: u32,
}

#[serde_as]
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PraosParams {
    pub security_param: u32,
    #[serde_as(as = "ChameleonFraction")]
    pub active_slots_coeff: RationalNumber,
    pub epoch_length: u32,
    pub max_kes_evolutions: u32,
    pub max_lovelace_supply: u64,
    pub network_id: NetworkId,
    pub slot_length: u32,
    pub slots_per_kes_period: u32,

    /// Relative slot from which data of the previous epoch can be considered stable.
    /// This value is used for all TPraos eras AND Babbage Era from Praos
    pub stability_window: u64,

    /// Number of slots at the end of each epoch which do NOT contribute randomness to the candidate
    /// nonce of the following epoch.
    /// This value is used for all Praos eras except Babbage
    pub randomness_stabilization_window: u64,
}

impl From<&ShelleyParams> for PraosParams {
    fn from(params: &ShelleyParams) -> Self {
        let active_slots_coeff = params.active_slots_coeff;
        let security_param = params.security_param;
        let stability_window =
            (security_param as u64) * active_slots_coeff.denom() / active_slots_coeff.numer() * 3;
        let randomness_stabilization_window =
            (security_param as u64) * active_slots_coeff.denom() / active_slots_coeff.numer() * 4;

        Self {
            security_param: security_param,
            active_slots_coeff: active_slots_coeff,
            epoch_length: params.epoch_length,
            max_kes_evolutions: params.max_kes_evolutions,
            max_lovelace_supply: params.max_lovelace_supply,
            network_id: params.network_id.clone(),
            slot_length: params.slot_length,
            slots_per_kes_period: params.slots_per_kes_period,

            stability_window: stability_window,
            randomness_stabilization_window: randomness_stabilization_window,
        }
    }
}

//
// Babbage protocol parameters
//

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BabbageParams {
    pub coins_per_utxo_byte: u64,
    pub plutus_v2_cost_model: Option<CostModel>,
}

//
// Conway protocol parameters
//

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConwayParams {
    pub pool_voting_thresholds: PoolVotingThresholds,
    pub d_rep_voting_thresholds: DRepVotingThresholds,
    pub committee_min_size: u64,
    pub committee_max_term_length: u32,
    pub gov_action_lifetime: u32,
    pub gov_action_deposit: u64,
    pub d_rep_deposit: u64,
    pub d_rep_activity: u32,
    pub min_fee_ref_script_cost_per_byte: RationalNumber,
    pub plutus_v3_cost_model: CostModel,
    pub constitution: Constitution,
    pub committee: Committee,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolVersion {
    pub minor: u64,
    pub major: u64,
}

#[derive(
    Default, Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "PascalCase")]
pub enum NonceVariant {
    #[default]
    NeutralNonce,
    Nonce,
}

pub type NonceHash = [u8; 32];

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Nonce {
    pub tag: NonceVariant,
    pub hash: Option<NonceHash>,
}

impl Default for Nonce {
    fn default() -> Self {
        Self {
            tag: NonceVariant::NeutralNonce,
            hash: None,
        }
    }
}

impl From<NonceHash> for Nonce {
    fn from(hash: NonceHash) -> Self {
        Self {
            tag: NonceVariant::Nonce,
            hash: Some(hash),
        }
    }
}

#[derive(
    Default, Debug, PartialEq, Eq, PartialOrd, Ord, Clone, serde::Serialize, serde::Deserialize,
)]
pub struct Nonces {
    pub epoch: u64,
    pub active: Nonce,
    pub evolving: Nonce,
    pub candidate: Nonce,
    // Nonce constructed from the hash of the Last Applied Block
    pub lab: Nonce,
    // Nonce corresponding to the LAB nonce of the last block of the previous epoch
    pub prev_lab: Nonce,
}

impl Nonces {
    pub fn shelley_genesis_nonces(genesis: &GenesisValues) -> Nonces {
        Nonces {
            epoch: genesis.shelley_epoch,
            active: genesis.shelley_genesis_hash.into(),
            evolving: genesis.shelley_genesis_hash.into(),
            candidate: genesis.shelley_genesis_hash.into(),
            lab: Nonce::default(),
            prev_lab: Nonce::default(),
        }
    }

    pub fn from_candidate(candidate: &Nonce, prev_lab: &Nonce) -> Result<Nonce> {
        let Some(candidate_hash) = candidate.hash.as_ref() else {
            return Err(anyhow::anyhow!("Candidate hash is not set"));
        };

        // if prev_lab is Neutral then just return candidate
        // this is for second shelley epoch boundary (from 208 to 209 in mainnet)
        match prev_lab.tag {
            NonceVariant::NeutralNonce => {
                return Ok(candidate.clone());
            }
            NonceVariant::Nonce => {
                let Some(prev_lab_hash) = prev_lab.hash.as_ref() else {
                    return Err(anyhow::anyhow!("Prev lab hash is not set"));
                };
                let mut hasher = Blake2b::<U32>::new();
                hasher.update(&[&candidate_hash.clone()[..], &prev_lab_hash.clone()[..]].concat());
                let hash: NonceHash = hasher.finalize().into();
                Ok(Nonce::from(hash))
            }
        }
    }

    /// Evolve the current nonce by combining it with the current rolling nonce and the
    /// range-extended tagged leader VRF output.
    ///
    /// Specifically, we combine it with `η` (a.k.a eta), which is a blake2b-256 hash of the
    /// tagged leader VRF output after a range extension. The range extension is, yet another
    /// blake2b-256 hash.
    pub fn evolve(current: &Nonce, nonce_vrf_output: &Vec<u8>) -> Result<Nonce> {
        // first hash nonce_vrf_output
        let mut hasher = Blake2b::<U32>::new();
        hasher.update(nonce_vrf_output.as_slice());
        let nonce_vrf_output_hash: [u8; 32] = hasher.finalize().into();

        match current.hash.as_ref() {
            Some(nonce) => {
                let mut hasher = Blake2b::<U32>::new();
                hasher.update(&[&nonce.clone()[..], &nonce_vrf_output_hash[..]].concat());
                let hash: NonceHash = hasher.finalize().into();
                Ok(Nonce::from(hash))
            }
            _ => Err(anyhow::anyhow!("Current nonce is not set")),
        }
    }

    pub fn randomness_stability_window(
        era: Era,
        slot: u64,
        genesis: &GenesisValues,
        params: &PraosParams,
    ) -> bool {
        let (epoch, _) = genesis.slot_to_epoch(slot);
        let next_epoch_first_slot = genesis.epoch_to_first_slot(epoch + 1);

        // For Praos in Babbage (just as in all TPraos eras) we use the
        // smaller (3k/f vs 4k/f slots) stability window here for
        // backwards-compatibility. See erratum 17.3 in the Shelley ledger
        // specs for context
        let window = match era {
            Era::Conway => params.randomness_stabilization_window,
            _ => params.stability_window,
        };

        slot + window < next_epoch_first_slot
    }
}
