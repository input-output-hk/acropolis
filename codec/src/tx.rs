use crate::{
    address::map_address,
    certs::map_certificate,
    parameter::{map_alonzo_update, map_babbage_update},
    utxo::{map_datum, map_reference_script, map_value},
    witness::{map_native_scripts, map_vkey_witnesses},
};
use acropolis_common::{validation::Phase1ValidationError, *};
use pallas_primitives::Metadatum as PallasMetadatum;
use pallas_traverse::{Era as PallasEra, MultiEraInput, MultiEraTx};

pub fn map_transaction_inputs(inputs: &[MultiEraInput]) -> Vec<UTxOIdentifier> {
    inputs
        .iter()
        .map(|input| {
            let oref = input.output_ref();
            UTxOIdentifier::new(TxHash::from(**oref.hash()), oref.index() as u16)
        })
        .collect()
}

/// Parse transaction consumes and produces, and return the parsed consumes, produces, total output lovelace, and errors
pub fn map_transaction_consumes_produces(
    tx: &MultiEraTx,
) -> (Vec<UTxOIdentifier>, Vec<TxOutput>, u128, Vec<String>) {
    let parsed_consumes = map_transaction_inputs(&tx.consumes());
    let mut parsed_produces = Vec::new();
    let mut total_output = 0;
    let mut errors = Vec::new();

    let tx_hash = TxHash::from(*tx.hash());

    for (index, output) in tx.produces() {
        let utxo = UTxOIdentifier::new(tx_hash, index as u16);
        match output.address() {
            Ok(pallas_address) => match map_address(&pallas_address) {
                Ok(address) => {
                    // Add TxOutput to utxo_deltas
                    parsed_produces.push(TxOutput {
                        utxo_identifier: utxo,
                        address,
                        value: map_value(&output.value()),
                        datum: map_datum(&output.datum()),
                        reference_script: map_reference_script(&output.script_ref()),
                    });
                    total_output += output.value().coin() as u128;
                }
                Err(e) => {
                    errors.push(format!("Output {index} has been ignored: {e}"));
                }
            },
            Err(e) => {
                errors.push(format!("Output {index} has been ignored: {e}"));
            }
        }
    }

    (parsed_consumes, parsed_produces, total_output, errors)
}

pub fn map_metadata(metadata: &PallasMetadatum) -> Metadata {
    match metadata {
        PallasMetadatum::Int(pallas_primitives::Int(i)) => Metadata::Int(MetadataInt(*i)),
        PallasMetadatum::Bytes(b) => Metadata::Bytes(b.to_vec()),
        PallasMetadatum::Text(s) => Metadata::Text(s.clone()),
        PallasMetadatum::Array(a) => Metadata::Array(a.iter().map(map_metadata).collect()),
        PallasMetadatum::Map(m) => {
            Metadata::Map(m.iter().map(|(k, v)| (map_metadata(k), map_metadata(v))).collect())
        }
    }
}

/// Map a Pallas Transaction to extract
pub fn map_transaction(
    tx: &MultiEraTx,
    raw_tx: &[u8],
    tx_identifier: TxIdentifier,
    network_id: NetworkId,
    era: Era,
) -> Transaction {
    let (consumes, produces, total_output, input_output_errors) =
        map_transaction_consumes_produces(tx);

    let mut errors = input_output_errors;
    let mut certs = Vec::new();
    let mut withdrawals = Vec::new();
    let mut alonzo_babbage_update_proposal = None;

    for (cert_index, cert) in tx.certs().iter().enumerate() {
        match map_certificate(cert, tx_identifier, cert_index, network_id.clone()) {
            Ok(c) => certs.push(c),
            Err(e) => errors.push(format!("Certificate {cert_index} has been ignored: {e}")),
        }
    }

    for (key, value) in tx.withdrawals_sorted_set() {
        match StakeAddress::from_binary(key) {
            Ok(stake_address) => {
                withdrawals.push(Withdrawal {
                    address: stake_address,
                    value,
                    tx_identifier,
                });
            }
            Err(e) => errors.push(format!(
                "Withdrawal {} has been ignored: {e}",
                hex::encode(key)
            )),
        }
    }

    match era {
        Era::Shelley | Era::Allegra | Era::Mary | Era::Alonzo => {
            if let Ok(alonzo) = MultiEraTx::decode_for_era(PallasEra::Alonzo, raw_tx)
                && let Some(update) = alonzo.update()
                && let Some(alonzo_update) = update.as_alonzo()
            {
                match map_alonzo_update(alonzo_update) {
                    Ok(p) => {
                        alonzo_babbage_update_proposal = Some(p);
                    }
                    Err(e) => errors.push(format!("Cannot decode alonzo update: {e}")),
                }
            }
        }
        Era::Babbage => {
            if let Ok(babbage) = MultiEraTx::decode_for_era(PallasEra::Babbage, raw_tx)
                && let Some(update) = babbage.update()
                && let Some(babbage_update) = update.as_babbage()
            {
                match map_babbage_update(babbage_update) {
                    Ok(p) => {
                        alonzo_babbage_update_proposal = Some(p);
                    }
                    Err(e) => errors.push(format!("Cannot decode babbage update: {e}")),
                }
            }
        }
        _ => {}
    }

    let (vkey_witnesses, vkey_witness_errors) = map_vkey_witnesses(tx.vkey_witnesses());
    let native_scripts = map_native_scripts(tx.native_scripts());

    errors.extend(vkey_witness_errors);

    Transaction {
        consumes,
        produces,
        total_output,
        certs,
        withdrawals,
        proposal_update: alonzo_babbage_update_proposal,
        vkey_witnesses,
        native_scripts,
        error: if errors.is_empty() {
            None
        } else {
            Some(Phase1ValidationError::MalformedTransaction { errors })
        },
    }
}
