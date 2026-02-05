use crate::{
    genesis_values::GenesisValues,
    rational_number::{ChameleonFraction, RationalNumber},
    BlockHash, BlockVersionData, Committee, Constitution, CostModel, DRepVotingThresholds, Era,
    ExUnitPrices, ExUnits, GenesisDelegates, HeavyDelegate, NetworkId, PoolId,
    PoolVotingThresholds, ProtocolConsts,
};
use anyhow::{bail, Result};
use blake2::{digest::consts::U32, Blake2b, Digest};
use chrono::{DateTime, Utc};
use serde_with::serde_as;
use std::fmt::Formatter;
use std::ops::Deref;
use std::{collections::HashMap, fmt::Display};

#[derive(Debug, Default, PartialEq, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProtocolParams {
    pub byron: Option<ByronParams>,
    pub alonzo: Option<AlonzoParams>,
    pub shelley: Option<ShelleyParams>,
    pub babbage: Option<BabbageParams>,
    pub conway: Option<ConwayParams>,
}

impl ProtocolParams {
    /// Calculate Transaction's Mininum required fee for shelley Era
    /// Reference: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/shelley/impl/src/Cardano/Ledger/Shelley/Tx.hs#L254
    pub fn shelley_min_fee(&self, tx_bytes: u32) -> Result<u64> {
        self.shelley
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Shelley params are not set"))
            .map(|shelley_params| shelley_params.min_fee(tx_bytes))
    }
}

//
// Byron protocol parameters
//

#[serde_as]
#[derive(Debug, Default, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ByronParams {
    pub block_version_data: BlockVersionData,
    pub fts_seed: Option<Vec<u8>>,
    pub protocol_consts: ProtocolConsts,
    pub start_time: u64,

    #[serde_as(as = "Vec<(_, _)>")]
    pub heavy_delegation: HashMap<PoolId, HeavyDelegate>,
}

//
// Alonzo protocol parameters
//

#[derive(Debug, Default, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
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
#[derive(Debug, Default, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
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
#[derive(Debug, Default, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
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

    pub gen_delegs: GenesisDelegates,
}

impl ShelleyParams {
    pub fn min_fee(&self, tx_bytes: u32) -> u64 {
        (tx_bytes as u64 * self.protocol_params.minfee_a as u64)
            + (self.protocol_params.minfee_b as u64)
    }
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
    pub extra_entropy: Nonce,

    /// Relative slot from which data of the previous epoch can be considered stable.
    /// This value is used for all TPraos eras AND Babbage Era from Praos
    pub stability_window: u64,

    /// Number of slots at the end of each epoch which do NOT contribute randomness to the candidate
    /// nonce of the following epoch.
    /// This value is used for all Praos eras except Babbage
    pub randomness_stabilization_window: u64,
}

impl PraosParams {
    pub fn mainnet() -> Self {
        PraosParams {
            security_param: 2160,
            active_slots_coeff: RationalNumber::new(1, 20),
            epoch_length: 432000,
            max_kes_evolutions: 62,
            max_lovelace_supply: 45_000_000_000_000_000,
            network_id: NetworkId::Mainnet,
            slot_length: 1,
            slots_per_kes_period: 129600,
            extra_entropy: Nonce::default(),
            stability_window: 129600,
            randomness_stabilization_window: 172800,
        }
    }
}

impl From<&ShelleyParams> for PraosParams {
    fn from(params: &ShelleyParams) -> Self {
        let active_slots_coeff = &params.active_slots_coeff;
        let security_param = params.security_param;
        let stability_window =
            (security_param as u64) * active_slots_coeff.denom() / active_slots_coeff.numer() * 3;
        let randomness_stabilization_window =
            (security_param as u64) * active_slots_coeff.denom() / active_slots_coeff.numer() * 4;

        Self {
            security_param,
            active_slots_coeff: active_slots_coeff.clone(),
            epoch_length: params.epoch_length,
            max_kes_evolutions: params.max_kes_evolutions,
            max_lovelace_supply: params.max_lovelace_supply,
            network_id: params.network_id.clone(),
            slot_length: params.slot_length,
            slots_per_kes_period: params.slots_per_kes_period,
            extra_entropy: params.protocol_params.extra_entropy.clone(),

            stability_window,
            randomness_stabilization_window,
        }
    }
}

//
// Babbage protocol parameters
//

#[derive(Default, Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BabbageParams {
    pub coins_per_utxo_byte: u64,
    pub plutus_v2_cost_model: Option<CostModel>,
}

//
// Conway protocol parameters
//

#[derive(Debug, Default, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
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

#[derive(
    Debug, Default, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolVersion {
    pub major: u64,
    pub minor: u64,
}

impl Display for ProtocolVersion {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

impl ProtocolVersion {
    pub fn new(major: u64, minor: u64) -> Self {
        Self { major, minor }
    }

    pub fn chang() -> Self {
        Self { major: 9, minor: 0 }
    }

    pub fn is_chang(&self) -> Result<bool> {
        if self.major == 9 {
            if self.minor != 0 {
                bail!("Chang version 9.xx with nonzero xx is not supported")
            }
            return Ok(true);
        }
        Ok(false)
    }
}

#[derive(
    Default,
    Debug,
    Clone,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
    minicbor::Encode,
    minicbor::Decode,
)]
#[serde(rename_all = "PascalCase")]
pub enum NonceVariant {
    #[n(0)]
    #[default]
    NeutralNonce,
    #[n(1)]
    Nonce,
}

pub type NonceHash = [u8; 32];

#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
    minicbor::Encode,
    minicbor::Decode,
)]
#[serde(rename_all = "camelCase")]
pub struct Nonce {
    #[n(0)]
    pub tag: NonceVariant,
    #[n(1)]
    pub hash: Option<NonceHash>,
}

impl std::ops::Mul<&Nonce> for &Nonce {
    type Output = Nonce;

    fn mul(self, other: &Nonce) -> Nonce {
        if let Some(self_hash) = self.hash.as_ref() {
            if let Some(other_hash) = other.hash.as_ref() {
                let mut hasher = Blake2b::<U32>::new();
                let mut data = Vec::new();
                data.extend_from_slice(self_hash);
                data.extend_from_slice(other_hash);
                hasher.update(data);
                let hash: NonceHash = hasher.finalize().into();
                Nonce::from(hash)
            } else {
                self.clone()
            }
        } else {
            other.clone()
        }
    }
}

impl std::ops::Mul<Nonce> for &Nonce {
    type Output = Nonce;

    fn mul(self, other: Nonce) -> Nonce {
        self * &other
    }
}

impl std::ops::Mul<&Nonce> for Nonce {
    type Output = Nonce;

    fn mul(self, other: &Nonce) -> Nonce {
        &self * other
    }
}

impl std::ops::Mul<Nonce> for Nonce {
    type Output = Nonce;

    fn mul(self, other: Nonce) -> Nonce {
        &self * &other
    }
}

impl Display for Nonce {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.hash {
            Some(hash) => write!(f, "{}", hex::encode(hash)),
            None => write!(f, "NeutralNonce"),
        }
    }
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

impl Nonce {
    pub fn from_number(n: u64) -> Self {
        let mut hasher = Blake2b::<U32>::new();
        hasher.update(n.to_be_bytes());
        let hash: NonceHash = hasher.finalize().into();
        Self::from(hash)
    }

    pub fn neutral() -> Self {
        Self {
            tag: NonceVariant::NeutralNonce,
            hash: None,
        }
    }

    /// Seed constant for eta (randomness/entropy) computation
    /// Used when generating the epoch nonce
    pub fn seed_eta() -> Self {
        Self::from_number(0)
    }

    /// Seed constant for leader (L) computation
    /// Used when determining if a stake pool is the slot leader
    pub fn seed_l() -> Self {
        Self::from_number(1)
    }
}

impl From<BlockHash> for Nonce {
    fn from(hash: BlockHash) -> Self {
        Self {
            tag: NonceVariant::Nonce,
            hash: Some(*hash.deref()),
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

    pub fn from_candidate(
        candidate: &Nonce,
        prev_lab: &Nonce,
        extra_entropy: &Nonce,
    ) -> Result<Nonce> {
        let Some(_) = candidate.hash.as_ref() else {
            return Err(anyhow::anyhow!("Candidate hash is not set"));
        };
        Ok(candidate * prev_lab * extra_entropy)
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
                hasher.update([&(*nonce)[..], &nonce_vrf_output_hash[..]].concat());
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
        // backwards-compatibility.
        let window = match era {
            Era::Conway => params.randomness_stabilization_window,
            _ => params.stability_window,
        };

        slot + window < next_epoch_first_slot
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocol_version_order() {
        let v9_0 = ProtocolVersion::new(9, 0);
        let v9_1 = ProtocolVersion::new(9, 1);
        let v9_10 = ProtocolVersion::new(9, 10);
        let v10_0 = ProtocolVersion::new(10, 0);
        let v10_9 = ProtocolVersion::new(10, 9);
        let v10_10 = ProtocolVersion::new(10, 10);
        let v10_11 = ProtocolVersion::new(10, 11);

        assert!(v10_9 > v9_10);

        let from = vec![v9_0, v9_1, v9_10, v10_0, v10_9, v10_10, v10_11];
        let mut upd = from.clone();
        upd.sort();

        assert_eq!(from, upd);
    }

    #[test]
    fn test_protocol_version_parsing() -> Result<()> {
        let v9_0 = serde_json::from_slice::<ProtocolVersion>(b"{\"minor\": 0, \"major\": 9}")?;
        let v9_0a = serde_json::from_slice::<ProtocolVersion>(b"{\"major\": 9, \"minor\": 0}")?;
        let v0_9 = serde_json::from_slice::<ProtocolVersion>(b"{\"minor\": 9, \"major\": 0}")?;
        let v0_9a = serde_json::from_slice::<ProtocolVersion>(b"{\"major\": 0, \"minor\": 9}")?;

        assert_eq!(v9_0, v9_0a);
        assert_eq!(v0_9, v0_9a);
        assert_eq!(v9_0, ProtocolVersion::new(9, 0));
        assert_eq!(v9_0.major, 9);
        assert_eq!(v0_9, ProtocolVersion::new(0, 9));

        Ok(())
    }

    #[test]
    fn test_nonce_mul() {
        let nonce1 = Nonce::from(
            NonceHash::try_from(
                hex::decode("d1340a9c1491f0face38d41fd5c82953d0eb48320d65e952414a0c5ebaf87587")
                    .unwrap()
                    .as_slice(),
            )
            .unwrap(),
        );
        let nonce2 = Nonce::from(
            NonceHash::try_from(
                hex::decode("ee91d679b0a6ce3015b894c575c799e971efac35c7a8cbdc2b3f579005e69abd")
                    .unwrap()
                    .as_slice(),
            )
            .unwrap(),
        );
        let nonce3 = Nonce::from(
            NonceHash::try_from(
                hex::decode("d982e06fd33e7440b43cefad529b7ecafbaa255e38178ad4189a37e4ce9bf1fa")
                    .unwrap()
                    .as_slice(),
            )
            .unwrap(),
        );
        let result = nonce1 * nonce2 * nonce3;
        assert_eq!(
            result,
            Nonce::from(
                NonceHash::try_from(
                    hex::decode("0022cfa563a5328c4fb5c8017121329e964c26ade5d167b1bd9b2ec967772b60")
                        .unwrap()
                        .as_slice()
                )
                .unwrap()
            )
        );
    }
}
