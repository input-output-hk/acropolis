use crate::{
    address::map_address,
    certs::map_certificate,
    map_all_governance_voting_procedures, map_alonzo_update, map_babbage_update,
    map_governance_proposals_procedure, map_mint_burn, map_redeemer,
    utxo::{map_datum, map_reference_script, map_value},
    witness::{map_native_scripts, map_vkey_witnesses},
};
use acropolis_common::{validation::Phase1ValidationError, *};
use pallas_primitives::Metadatum as PallasMetadatum;
use pallas_traverse::{Era as PallasEra, MultiEraInput, MultiEraSigners, MultiEraTx};

pub fn map_transaction_inputs(inputs: &[MultiEraInput]) -> Vec<UTxOIdentifier> {
    inputs
        .iter()
        .map(|input| {
            let oref = input.output_ref();
            UTxOIdentifier::new(TxHash::from(**oref.hash()), oref.index() as u16)
        })
        .collect()
}

pub fn map_required_signatories(required_signers: &MultiEraSigners) -> Vec<KeyHash> {
    match required_signers {
        MultiEraSigners::AlonzoCompatible(signers) => {
            signers.iter().map(|signer| KeyHash::from(**signer)).collect()
        }
        _ => Vec::new(),
    }
}

/// Parse transaction consumes and produces,
/// and return the parsed consumes, produces and errors
/// NOTE:
/// This function returns consumes sorted lexicographically by UTxO identifier
pub fn map_transaction_consumes_produces(
    tx: &MultiEraTx,
) -> (Vec<UTxOIdentifier>, Vec<TxOutput>, Vec<String>) {
    let parsed_consumes = map_transaction_inputs(&tx.inputs_sorted_set());
    let mut parsed_produces = Vec::new();
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

    (parsed_consumes, parsed_produces, errors)
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

/// Map a Pallas Transaction
/// NOTE:
/// This function sorts
/// - consumes sorted lexicographically by UTxO identifier
/// - withdrawals by RewardAccount (bytes)
/// - mint/burn sorted by policy id
pub fn map_transaction(
    tx: &MultiEraTx,
    raw_tx: &[u8],
    tx_identifier: TxIdentifier,
    network_id: NetworkId,
    era: Era,
) -> Transaction {
    let (consumes, produces, input_output_errors) = map_transaction_consumes_produces(tx);

    let fee = tx.fee().unwrap_or(0);
    let is_valid = tx.is_valid();

    let mut certs = Vec::new();
    let mut withdrawals = Vec::new();
    let mut mint_burn_deltas = Vec::new();
    let mut alonzo_babbage_update_proposal = None;
    let mut voting_procedures = None;
    let mut proposal_procedures = None;
    let mut errors = input_output_errors;

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

    let required_signers = map_required_signatories(&tx.required_signers());

    for policy_group in tx.mints_sorted_set().iter() {
        if let Some((policy_id, deltas)) = map_mint_burn(policy_group) {
            mint_burn_deltas.push((policy_id, deltas));
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

    if era == Era::Conway
        && let Some(conway) = tx.as_conway()
    {
        if let Some(ref pallas_vp) = conway.transaction_body.voting_procedures {
            match map_all_governance_voting_procedures(pallas_vp) {
                Ok(vp) => voting_procedures = Some(vp),
                Err(e) => errors.push(format!("Cannot decode governance voting procedures: {e}")),
            }
        }

        if let Some(ref pallas_pp) = conway.transaction_body.proposal_procedures {
            let mut procedures = Vec::new();
            let mut proc_id = GovActionId {
                transaction_id: TxHash::from(*tx.hash()),
                action_index: 0,
            };
            for (action_index, proposal_procedure) in pallas_pp.iter().enumerate() {
                match proc_id.set_action_index(action_index).and_then(|proc_id| {
                    map_governance_proposals_procedure(proc_id, proposal_procedure)
                }) {
                    Ok(pp) => procedures.push(pp),
                    Err(e) => errors.push(format!(
                        "Cannot decode governance proposal procedure {} idx {}: {e}",
                        proc_id, action_index
                    )),
                }
            }

            if !procedures.is_empty() {
                proposal_procedures = Some(procedures);
            }
        }
    }

    let (vkey_witnesses, vkey_witness_errors) = map_vkey_witnesses(tx.vkey_witnesses());
    errors.extend(vkey_witness_errors);
    let native_scripts = map_native_scripts(tx.native_scripts());

    let mut redeemers = Vec::new();
    for redeemer in tx.redeemers() {
        match map_redeemer(&redeemer) {
            Ok(r) => redeemers.push(r),
            Err(e) => errors.push(e.to_string()),
        }
    }

    Transaction {
        id: tx_identifier,
        consumes,
        produces,
        fee,
        is_valid,
        certs,
        withdrawals,
        required_signers,
        mint_burn_deltas,
        proposal_update: alonzo_babbage_update_proposal,
        voting_procedures,
        proposal_procedures,
        vkey_witnesses,
        native_scripts,
        redeemers,
        error: if errors.is_empty() {
            None
        } else {
            Some(Phase1ValidationError::MalformedTransaction { errors })
        },
    }
}
