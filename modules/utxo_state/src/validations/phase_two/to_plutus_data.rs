use acropolis_common::{
    hash::Hash, protocol_params::ProtocolVersion, validation::ScriptContextError, Credential,
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

impl_to_plutus_data_for_uint!(u8, u16, u32, u64, usize);

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
