use crate::address::map_address;
use acropolis_common::{validation::ValidationError, *};
use pallas_primitives::conway;
use pallas_traverse::{MultiEraInput, MultiEraPolicyAssets, MultiEraTx, MultiEraValue};

pub fn map_value(pallas_value: &MultiEraValue) -> Value {
    let lovelace = pallas_value.coin();
    let pallas_assets = pallas_value.assets();

    let mut assets: NativeAssets = Vec::new();

    for policy_group in pallas_assets {
        match policy_group {
            MultiEraPolicyAssets::AlonzoCompatibleOutput(policy, kvps) => {
                match policy.as_ref().try_into() {
                    Ok(policy_id) => {
                        let native_assets = kvps
                            .iter()
                            .filter_map(|(name, amt)| {
                                AssetName::new(name).map(|asset_name| NativeAsset {
                                    name: asset_name,
                                    amount: *amt,
                                })
                            })
                            .collect::<Vec<_>>();

                        assets.push((policy_id, native_assets));
                    }
                    Err(_) => {
                        tracing::error!(
                            "Invalid policy id length: expected 28 bytes, got {}",
                            policy.len()
                        );
                        continue;
                    }
                }
            }
            MultiEraPolicyAssets::ConwayOutput(policy, kvps) => match policy.as_ref().try_into() {
                Ok(policy_id) => {
                    let native_assets = kvps
                        .iter()
                        .filter_map(|(name, amt)| {
                            AssetName::new(name).map(|asset_name| NativeAsset {
                                name: asset_name,
                                amount: u64::from(*amt),
                            })
                        })
                        .collect();

                    assets.push((policy_id, native_assets));
                }
                Err(_) => {
                    tracing::error!(
                        "Invalid policy id length: expected 28 bytes, got {}",
                        policy.len()
                    );
                    continue;
                }
            },
            _ => {}
        }
    }
    Value::new(lovelace, assets)
}

pub fn map_transaction_inputs(inputs: &Vec<MultiEraInput>) -> Vec<TxOutRef> {
    let mut parsed_inputs = Vec::new();
    for input in inputs {
        // MultiEraInput
        let oref = input.output_ref();
        let tx_ref = TxOutRef::new(TxHash::from(**oref.hash()), oref.index() as u16);

        parsed_inputs.push(tx_ref);
    }

    parsed_inputs
}

pub fn map_datum(datum: &Option<conway::MintedDatumOption>) -> Option<Datum> {
    match datum {
        Some(conway::MintedDatumOption::Hash(h)) => Some(Datum::Hash(h.to_vec())),
        Some(conway::MintedDatumOption::Data(d)) => Some(Datum::Inline(d.raw_cbor().to_vec())),
        None => None,
    }
}

pub fn map_reference_script(script: &Option<conway::MintedScriptRef>) -> Option<ReferenceScript> {
    match script {
        Some(conway::PseudoScript::NativeScript(script)) => {
            Some(ReferenceScript::Native(script.raw_cbor().to_vec()))
        }
        Some(conway::PseudoScript::PlutusV1Script(script)) => {
            Some(ReferenceScript::PlutusV1(script.as_ref().to_vec()))
        }
        Some(conway::PseudoScript::PlutusV2Script(script)) => {
            Some(ReferenceScript::PlutusV2(script.as_ref().to_vec()))
        }
        Some(conway::PseudoScript::PlutusV3Script(script)) => {
            Some(ReferenceScript::PlutusV3(script.as_ref().to_vec()))
        }
        None => None,
    }
}

pub fn map_transaction_inputs_outputs(
    block_number: u32,
    tx_index: u16,
    tx: &MultiEraTx,
) -> (
    Vec<TxOutRef>,
    Vec<(TxOutRef, TxOutput)>,
    Vec<ValidationError>,
) {
    let mut parsed_inputs = Vec::new();
    let mut parsed_outputs = Vec::new();
    let mut errors = Vec::new();

    let Ok(tx_hash) = tx.hash().to_vec().try_into() else {
        errors.push(ValidationError::MalformedTransaction(
            tx_index,
            format!("Tx has incorrect hash length ({:?})", tx.hash().to_vec()),
        ));
        return (parsed_inputs, parsed_outputs, errors);
    };

    let inputs = tx.consumes();
    let outputs = tx.produces();

    for input in inputs {
        let tx_ref = TxOutRef::new(
            TxHash::from(**input.output_ref().hash()),
            input.output_ref().index() as u16,
        );
        parsed_inputs.push(tx_ref);
    }

    for (index, output) in outputs {
        let tx_out_ref = TxOutRef {
            tx_hash,
            output_index: index as u16,
        };

        let utxo_id = UTxOIdentifier::new(block_number, tx_index, tx_out_ref.output_index);

        match output.address() {
            Ok(pallas_address) => match map_address(&pallas_address) {
                Ok(address) => {
                    // Add TxOutput to utxo_deltas
                    parsed_outputs.push((
                        tx_out_ref,
                        TxOutput {
                            utxo_identifier: utxo_id,
                            address,
                            value: map_value(&output.value()),
                            datum: map_datum(&output.datum()),
                            reference_script: map_reference_script(&output.script_ref()),
                        },
                    ));
                }
                Err(e) => {
                    errors.push(ValidationError::MalformedTransaction(
                        tx_index,
                        format!("Output {index} has been ignored: {e}"),
                    ));
                }
            },
            Err(e) => errors.push(ValidationError::MalformedTransaction(
                tx_index,
                format!("Can't parse output {index} in tx: {e}"),
            )),
        }
    }

    (parsed_inputs, parsed_outputs, errors)
}
