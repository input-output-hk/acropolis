use acropolis_common::{
    hash::Hash, protocol_params::ProtocolVersion, rational_number::RationalNumber,
    validation::ScriptContextError, CostModel, CostModels, Credential, DRepVotingThresholds,
    ExUnitPrices, ExUnits, PoolVotingThresholds, ProtocolParamUpdate,
};
use uplc_turbo::{arena::Arena, data::PlutusData, machine::PlutusVersion};

/// Trait for converting Acropolis domain types into arena-allocated PlutusData.
///
/// The `version` parameter controls version-specific encoding differences
/// between PlutusV1, PlutusV2, and PlutusV3.
pub trait ToPlutusData {
    fn to_plutus_data<'a>(
        &self,
        arena: &'a Arena,
        version: PlutusVersion,
    ) -> Result<&'a PlutusData<'a>, ScriptContextError>;
}

// ============================================================================
// Arena allocation helpers
//
// These allocate Vecs in the arena's bump storage. The Vec header lives in
// the arena; the heap buffer leaks when the arena resets. This is acceptable
// for short-lived per-transaction arenas.
// ============================================================================

pub fn alloc_fields<'a>(
    arena: &'a Arena,
    items: Vec<&'a PlutusData<'a>>,
) -> &'a [&'a PlutusData<'a>] {
    if items.is_empty() {
        return &[];
    }
    arena.alloc(items).as_slice()
}

pub fn alloc_pairs<'a>(
    arena: &'a Arena,
    pairs: Vec<(&'a PlutusData<'a>, &'a PlutusData<'a>)>,
) -> &'a [(&'a PlutusData<'a>, &'a PlutusData<'a>)] {
    if pairs.is_empty() {
        return &[];
    }
    arena.alloc(pairs).as_slice()
}

pub fn alloc_bytes<'a>(arena: &'a Arena, data: &[u8]) -> &'a [u8] {
    if data.is_empty() {
        return &[];
    }
    arena.alloc(data.to_vec()).as_slice()
}

// ============================================================================
// PlutusData construction helpers
// ============================================================================

pub fn constr<'a>(
    arena: &'a Arena,
    tag: u64,
    fields: Vec<&'a PlutusData<'a>>,
) -> &'a PlutusData<'a> {
    PlutusData::constr(arena, tag, alloc_fields(arena, fields))
}

pub fn list<'a>(arena: &'a Arena, items: Vec<&'a PlutusData<'a>>) -> &'a PlutusData<'a> {
    PlutusData::list(arena, alloc_fields(arena, items))
}

pub fn map<'a>(
    arena: &'a Arena,
    pairs: Vec<(&'a PlutusData<'a>, &'a PlutusData<'a>)>,
) -> &'a PlutusData<'a> {
    PlutusData::map(arena, alloc_pairs(arena, pairs))
}

pub fn integer<'a>(arena: &'a Arena, value: i128) -> &'a PlutusData<'a> {
    PlutusData::integer_from(arena, value)
}

pub fn bytes<'a>(arena: &'a Arena, data: &[u8]) -> &'a PlutusData<'a> {
    PlutusData::byte_string(arena, alloc_bytes(arena, data))
}

/// Parse CBOR-encoded PlutusData into arena-allocated form.
pub fn from_cbor<'a>(
    arena: &'a Arena,
    cbor: &[u8],
) -> Result<&'a PlutusData<'a>, ScriptContextError> {
    PlutusData::from_cbor(arena, cbor)
        .map_err(|e| ScriptContextError::CborDecodeFailed(e.to_string()))
}

// ============================================================================
// Primitive implementations
// ============================================================================

macro_rules! impl_to_plutus_data_for_uint {
    ($($t:ty),*) => {
        $(
            impl ToPlutusData for $t {
                fn to_plutus_data<'a>(
                    &self,
                    arena: &'a Arena,
                    _version: PlutusVersion,
                ) -> Result<&'a PlutusData<'a>, ScriptContextError> {
                    Ok(integer(arena, *self as i128))
                }
            }
        )*
    };
}

impl_to_plutus_data_for_uint!(u8, u16, u32, u64, usize, i64);

impl ToPlutusData for bool {
    fn to_plutus_data<'a>(
        &self,
        arena: &'a Arena,
        _version: PlutusVersion,
    ) -> Result<&'a PlutusData<'a>, ScriptContextError> {
        Ok(constr(arena, if *self { 1 } else { 0 }, vec![]))
    }
}

impl<const N: usize> ToPlutusData for Hash<N> {
    fn to_plutus_data<'a>(
        &self,
        arena: &'a Arena,
        _version: PlutusVersion,
    ) -> Result<&'a PlutusData<'a>, ScriptContextError> {
        Ok(bytes(arena, self.as_ref()))
    }
}

impl ToPlutusData for RationalNumber {
    fn to_plutus_data<'a>(
        &self,
        arena: &'a Arena,
        version: PlutusVersion,
    ) -> Result<&'a PlutusData<'a>, ScriptContextError> {
        Ok(list(
            arena,
            vec![
                self.0.numer().to_plutus_data(arena, version)?,
                self.0.denom().to_plutus_data(arena, version)?,
            ],
        ))
    }
}

impl ToPlutusData for ExUnits {
    fn to_plutus_data<'a>(
        &self,
        arena: &'a Arena,
        version: PlutusVersion,
    ) -> Result<&'a PlutusData<'a>, ScriptContextError> {
        Ok(list(
            arena,
            vec![
                self.mem.to_plutus_data(arena, version)?,
                self.steps.to_plutus_data(arena, version)?,
            ],
        ))
    }
}

impl ToPlutusData for ExUnitPrices {
    fn to_plutus_data<'a>(
        &self,
        arena: &'a Arena,
        version: PlutusVersion,
    ) -> Result<&'a PlutusData<'a>, ScriptContextError> {
        Ok(list(
            arena,
            vec![
                self.mem_price.to_plutus_data(arena, version)?,
                self.step_price.to_plutus_data(arena, version)?,
            ],
        ))
    }
}

impl ToPlutusData for Credential {
    fn to_plutus_data<'a>(
        &self,
        arena: &'a Arena,
        version: PlutusVersion,
    ) -> Result<&'a PlutusData<'a>, ScriptContextError> {
        match self {
            Credential::AddrKeyHash(hash) => {
                let h = hash.to_plutus_data(arena, version)?;
                Ok(constr(arena, 0, vec![h]))
            }
            Credential::ScriptHash(hash) => {
                let h = hash.to_plutus_data(arena, version)?;
                Ok(constr(arena, 1, vec![h]))
            }
        }
    }
}

impl<A: ToPlutusData> ToPlutusData for Option<A> {
    fn to_plutus_data<'a>(
        &self,
        arena: &'a Arena,
        version: PlutusVersion,
    ) -> Result<&'a PlutusData<'a>, ScriptContextError> {
        match self {
            None => Ok(constr(arena, 1, vec![])),
            Some(data) => {
                let inner = data.to_plutus_data(arena, version)?;
                Ok(constr(arena, 0, vec![inner]))
            }
        }
    }
}

impl ToPlutusData for ProtocolVersion {
    fn to_plutus_data<'a>(
        &self,
        arena: &'a Arena,
        version: PlutusVersion,
    ) -> Result<&'a PlutusData<'a>, ScriptContextError> {
        let major = self.major.to_plutus_data(arena, version)?;
        let minor = self.minor.to_plutus_data(arena, version)?;
        Ok(constr(arena, 0, vec![major, minor]))
    }
}

impl ToPlutusData for PoolVotingThresholds {
    fn to_plutus_data<'a>(
        &self,
        arena: &'a Arena,
        version: PlutusVersion,
    ) -> Result<&'a PlutusData<'a>, ScriptContextError> {
        Ok(list(
            arena,
            vec![
                self.motion_no_confidence.to_plutus_data(arena, version)?,
                self.committee_normal.to_plutus_data(arena, version)?,
                self.committee_no_confidence.to_plutus_data(arena, version)?,
                self.hard_fork_initiation.to_plutus_data(arena, version)?,
                self.security_voting_threshold.to_plutus_data(arena, version)?,
            ],
        ))
    }
}

impl ToPlutusData for DRepVotingThresholds {
    fn to_plutus_data<'a>(
        &self,
        arena: &'a Arena,
        version: PlutusVersion,
    ) -> Result<&'a PlutusData<'a>, ScriptContextError> {
        Ok(list(
            arena,
            vec![
                self.motion_no_confidence.to_plutus_data(arena, version)?,
                self.committee_normal.to_plutus_data(arena, version)?,
                self.committee_no_confidence.to_plutus_data(arena, version)?,
                self.update_constitution.to_plutus_data(arena, version)?,
                self.hard_fork_initiation.to_plutus_data(arena, version)?,
                self.pp_network_group.to_plutus_data(arena, version)?,
                self.pp_economic_group.to_plutus_data(arena, version)?,
                self.pp_technical_group.to_plutus_data(arena, version)?,
                self.pp_governance_group.to_plutus_data(arena, version)?,
                self.treasury_withdrawal.to_plutus_data(arena, version)?,
            ],
        ))
    }
}

impl ToPlutusData for CostModel {
    fn to_plutus_data<'a>(
        &self,
        arena: &'a Arena,
        version: PlutusVersion,
    ) -> Result<&'a PlutusData<'a>, ScriptContextError> {
        Ok(list(
            arena,
            self.as_vec()
                .iter()
                .map(|c| c.to_plutus_data(arena, version))
                .collect::<Result<Vec<_>, _>>()?,
        ))
    }
}

impl ToPlutusData for CostModels {
    fn to_plutus_data<'a>(
        &self,
        arena: &'a Arena,
        version: PlutusVersion,
    ) -> Result<&'a PlutusData<'a>, ScriptContextError> {
        let mut pairs = Vec::new();
        if let Some(v1) = self.plutus_v1.as_ref() {
            pairs.push((
                0_usize.to_plutus_data(arena, version)?,
                v1.to_plutus_data(arena, version)?,
            ));
        }
        if let Some(v2) = self.plutus_v2.as_ref() {
            pairs.push((
                1_usize.to_plutus_data(arena, version)?,
                v2.to_plutus_data(arena, version)?,
            ));
        }
        if let Some(v3) = self.plutus_v3.as_ref() {
            pairs.push((
                2_usize.to_plutus_data(arena, version)?,
                v3.to_plutus_data(arena, version)?,
            ));
        }

        Ok(map(arena, pairs))
    }
}

impl ToPlutusData for ProtocolParamUpdate {
    fn to_plutus_data<'a>(
        &self,
        arena: &'a Arena,
        version: PlutusVersion,
    ) -> Result<&'a PlutusData<'a>, ScriptContextError> {
        let mut pparams = Vec::with_capacity(30);

        let mut push = |ix: usize, p: &'a PlutusData| -> Result<(), ScriptContextError> {
            let ix_pd = ix.to_plutus_data(arena, version)?;
            pparams.push((ix_pd, p));

            Ok(())
        };

        if let Some(p) = self.minfee_a {
            push(0, p.to_plutus_data(arena, version)?)?;
        }

        if let Some(p) = self.minfee_b {
            push(1, p.to_plutus_data(arena, version)?)?;
        }

        if let Some(p) = self.max_block_body_size {
            push(2, p.to_plutus_data(arena, version)?)?;
        }

        if let Some(p) = self.max_transaction_size {
            push(3, p.to_plutus_data(arena, version)?)?;
        }

        if let Some(p) = self.max_block_header_size {
            push(4, p.to_plutus_data(arena, version)?)?;
        }

        if let Some(p) = self.key_deposit {
            push(5, p.to_plutus_data(arena, version)?)?;
        }

        if let Some(p) = self.pool_deposit {
            push(6, p.to_plutus_data(arena, version)?)?;
        }

        if let Some(p) = self.maximum_epoch {
            push(7, p.to_plutus_data(arena, version)?)?;
        }

        if let Some(p) = self.desired_number_of_stake_pools {
            push(8, p.to_plutus_data(arena, version)?)?;
        }

        if let Some(ref p) = self.pool_pledge_influence {
            push(9, p.to_plutus_data(arena, version)?)?;
        }

        if let Some(ref p) = self.expansion_rate {
            push(10, p.to_plutus_data(arena, version)?)?;
        }

        if let Some(ref p) = self.treasury_growth_rate {
            push(11, p.to_plutus_data(arena, version)?)?;
        }

        if let Some(p) = self.min_pool_cost {
            push(16, p.to_plutus_data(arena, version)?)?;
        }

        if let Some(p) = self.coins_per_utxo_byte {
            push(17, p.to_plutus_data(arena, version)?)?;
        }

        if let Some(p) = self.cost_models_for_script_languages.as_ref() {
            push(18, p.to_plutus_data(arena, version)?)?;
        }

        if let Some(ref p) = self.execution_costs {
            push(19, p.to_plutus_data(arena, version)?)?;
        }

        if let Some(p) = self.max_tx_ex_units {
            push(20, p.to_plutus_data(arena, version)?)?;
        }

        if let Some(p) = self.max_block_ex_units {
            push(21, p.to_plutus_data(arena, version)?)?;
        }

        if let Some(p) = self.max_value_size {
            push(22, p.to_plutus_data(arena, version)?)?;
        }

        if let Some(p) = self.collateral_percentage {
            push(23, p.to_plutus_data(arena, version)?)?;
        }

        if let Some(p) = self.max_collateral_inputs {
            push(24, p.to_plutus_data(arena, version)?)?;
        }

        if let Some(ref p) = self.pool_voting_thresholds {
            push(25, p.to_plutus_data(arena, version)?)?;
        }

        if let Some(ref p) = self.drep_voting_thresholds {
            push(26, p.to_plutus_data(arena, version)?)?;
        }

        if let Some(p) = self.min_committee_size {
            push(27, p.to_plutus_data(arena, version)?)?;
        }

        if let Some(p) = self.committee_term_limit {
            push(28, p.to_plutus_data(arena, version)?)?;
        }

        if let Some(p) = self.governance_action_validity_period {
            push(29, p.to_plutus_data(arena, version)?)?;
        }

        if let Some(p) = self.governance_action_deposit {
            push(30, p.to_plutus_data(arena, version)?)?;
        }

        if let Some(p) = self.drep_deposit {
            push(31, p.to_plutus_data(arena, version)?)?;
        }

        if let Some(p) = self.drep_inactivity_period {
            push(32, p.to_plutus_data(arena, version)?)?;
        }

        if let Some(ref p) = self.minfee_refscript_cost_per_byte {
            push(33, p.to_plutus_data(arena, version)?)?;
        }

        Ok(map(arena, pparams))
    }
}
