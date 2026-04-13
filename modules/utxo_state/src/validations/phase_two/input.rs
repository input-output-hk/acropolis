use acropolis_common::{
    validation::ScriptContextError, Address, Datum, DatumHash, ScriptRef, UTXOValue,
    UTxOIdentifier, Value,
};
use uplc_turbo::{arena::Arena, data::PlutusData, machine::PlutusVersion};

use super::to_plutus_data::*;

#[derive(Debug)]
pub struct ResolvedInput {
    pub utxo_id: UTxOIdentifier,
    pub utxo_value: UTXOValue,
}

// ============================================================================
// UTxOIdentifier
// ============================================================================

impl ToPlutusData for UTxOIdentifier {
    fn to_plutus_data<'a>(
        &self,
        arena: &'a Arena,
        version: PlutusVersion,
    ) -> Result<&'a PlutusData<'a>, ScriptContextError> {
        match version {
            PlutusVersion::V1 | PlutusVersion::V2 => {
                encode_utxo_identifier_with_wrapped_tx_id(self, arena, version)
            }
            PlutusVersion::V3 => encode_utxo_identifier_without_wrapped_tx_id(self, arena, version),
        }
    }
}

/// For Plutus V1 and V2
pub fn encode_utxo_identifier_with_wrapped_tx_id<'a>(
    utxo_id: &UTxOIdentifier,
    arena: &'a Arena,
    version: PlutusVersion,
) -> Result<&'a PlutusData<'a>, ScriptContextError> {
    let tx_id = constr(
        arena,
        0,
        vec![utxo_id.tx_hash.to_plutus_data(arena, version)?],
    );
    let output_idx = utxo_id.output_index.to_plutus_data(arena, version)?;
    Ok(constr(arena, 0, vec![tx_id, output_idx]))
}

/// For Plutus V3 (no wrapping of tx_id)
pub fn encode_utxo_identifier_without_wrapped_tx_id<'a>(
    utxo_id: &UTxOIdentifier,
    arena: &'a Arena,
    version: PlutusVersion,
) -> Result<&'a PlutusData<'a>, ScriptContextError> {
    let tx_id = utxo_id.tx_hash.to_plutus_data(arena, version)?;
    let output_idx = utxo_id.output_index.to_plutus_data(arena, version)?;
    Ok(constr(arena, 0, vec![tx_id, output_idx]))
}

// ============================================================================
// DatumOption
// ============================================================================

pub fn encode_datum_option<'a>(
    datum: &Option<Datum>,
    arena: &'a Arena,
    version: PlutusVersion,
) -> Result<&'a PlutusData<'a>, ScriptContextError> {
    match version {
        PlutusVersion::V1 => match datum {
            Some(Datum::Hash(hash)) => {
                let h = hash.to_plutus_data(arena, version)?;
                Ok(constr(arena, 0, vec![h]))
            }
            _ => Ok(constr(arena, 1, vec![])),
        },
        PlutusVersion::V2 | PlutusVersion::V3 => match datum {
            None => Ok(constr(arena, 0, vec![])), // NoOutputDatum
            Some(Datum::Hash(hash)) => {
                let h = hash.to_plutus_data(arena, version)?;
                Ok(constr(arena, 1, vec![h]))
            }
            Some(Datum::Inline(cbor_bytes)) => {
                let data = from_cbor(arena, cbor_bytes)?;
                Ok(constr(arena, 2, vec![data]))
            }
        },
    }
}

// ============================================================================
// Script reference (V2/V3 only)
// ============================================================================

pub fn encode_script_ref<'a>(
    script_ref: &Option<ScriptRef>,
    arena: &'a Arena,
    version: PlutusVersion,
) -> Result<&'a PlutusData<'a>, ScriptContextError> {
    match version {
        PlutusVersion::V1 => Err(ScriptContextError::UnsupportedReferenceScript),
        PlutusVersion::V2 | PlutusVersion::V3 => match script_ref {
            None => Ok(constr(arena, 1, vec![])),
            Some(sr) => {
                let h = sr.script_hash.to_plutus_data(arena, version)?;
                Ok(constr(arena, 0, vec![h]))
            }
        },
    }
}

// ============================================================================
// TxOut
// ============================================================================

pub fn encode_tx_out<'a>(
    address: &Address,
    value: &Value,
    datum: &Option<Datum>,
    script_ref: &Option<ScriptRef>,
    arena: &'a Arena,
    version: PlutusVersion,
) -> Result<&'a PlutusData<'a>, ScriptContextError> {
    let addr = address.to_plutus_data(arena, version)?;
    let val = value.to_plutus_data(arena, version)?;
    let dat = encode_datum_option(datum, arena, version)?;

    match version {
        PlutusVersion::V1 => Ok(constr(arena, 0, vec![addr, val, dat])),
        PlutusVersion::V2 | PlutusVersion::V3 => {
            let sr = encode_script_ref(script_ref, arena, version)?;
            Ok(constr(arena, 0, vec![addr, val, dat, sr]))
        }
    }
}

// ============================================================================
// TxInInfo (resolved input)
// ============================================================================

pub fn encode_tx_in_info<'a>(
    utxo_id: &UTxOIdentifier,
    utxo_value: &UTXOValue,
    arena: &'a Arena,
    version: PlutusVersion,
) -> Result<&'a PlutusData<'a>, ScriptContextError> {
    let out_ref = utxo_id.to_plutus_data(arena, version)?;
    let tx_out = encode_tx_out(
        &utxo_value.address,
        &utxo_value.value,
        &utxo_value.datum,
        &utxo_value.script_ref,
        arena,
        version,
    )?;
    Ok(constr(arena, 0, vec![out_ref, tx_out]))
}

// ============================================================================
// Datum witness map
// ============================================================================

/// Encode datums as PlutusData.
///
/// V1: `List [Constr(0, [hash, datum])...]` — `[(DatumHash, Datum)]` uses
///     standard list-of-tuples `ToData`, producing a List of Constr pairs.
/// V2/V3: `Map [(hash, datum)...]` — `Map DatumHash Datum` uses the Map
///     `ToData` instance, producing a PlutusData Map node.
pub fn encode_datums<'a>(
    datums: &[(DatumHash, Vec<u8>)],
    arena: &'a Arena,
    version: PlutusVersion,
) -> Result<&'a PlutusData<'a>, ScriptContextError> {
    match version {
        PlutusVersion::V1 => {
            let tuples: Vec<_> = datums
                .iter()
                .map(|(hash, cbor_bytes)| {
                    let k = hash.to_plutus_data(arena, version)?;
                    let v = from_cbor(arena, cbor_bytes)?;
                    Ok(constr(arena, 0, vec![k, v]))
                })
                .collect::<Result<_, ScriptContextError>>()?;
            Ok(list(arena, tuples))
        }
        PlutusVersion::V2 | PlutusVersion::V3 => {
            let pairs: Vec<_> = datums
                .iter()
                .map(|(hash, cbor_bytes)| {
                    let k = hash.to_plutus_data(arena, version)?;
                    let v = from_cbor(arena, cbor_bytes)?;
                    Ok((k, v))
                })
                .collect::<Result<_, ScriptContextError>>()?;
            Ok(map(arena, pairs))
        }
    }
}
