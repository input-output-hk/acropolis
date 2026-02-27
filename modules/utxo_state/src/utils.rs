use std::collections::{HashMap, HashSet};
use tracing::error;

use acropolis_common::{
    get_scripts_needed_from_certificates, get_scripts_needed_from_inputs,
    get_scripts_needed_from_mint_burn, get_scripts_needed_from_proposal,
    get_scripts_needed_from_voting, get_scripts_needed_from_withdrawals,
    protocol_params::ShelleyParams, KeyHash, RedeemerPointer, ScriptHash, ScriptLang,
    ShelleyAddressPaymentPart, TxUTxODeltas, UTXOValue, UTxOIdentifier,
};

/// Get VKey Hashes needed for transaction
/// VKey Witnesses needed
/// 1. UTxO authors: keys that own the UTxO being spent.
/// 2. Certificate authors: keys authorizing certificates
/// 3. Pool owners: owners that must sign pool registration
/// 4. Withdrawal authors: keys authorizing reward withdrawals
/// 5. Required Signers: keys that are required to sign the tx (added from Alonzo era)
/// 6. Governance authors: keys authorizing governance actions (e.g. protocol update)
///    **NOTE:** This is removed from Conway era.
/// 7. Vote authors: Keys for Commmittee, DRep, Stake Pool (added from Conway era)
pub fn get_vkeys_needed(
    tx_deltas: &TxUTxODeltas,
    utxos: &HashMap<UTxOIdentifier, UTXOValue>,
    shelley_params: Option<&ShelleyParams>,
) -> HashSet<KeyHash> {
    let TxUTxODeltas {
        consumes: sorted_inputs,
        certs,
        withdrawals,
        required_signers,
        proposal_update,
        voting_procedures,
        ..
    } = tx_deltas;
    let mut vkey_hashes = HashSet::new();

    let genesis_delegs = shelley_params.map(|shelley_params| &shelley_params.gen_delegs);

    // for each input, get the required vkey hashes
    for input in sorted_inputs.iter() {
        if let Some(utxo) = utxos.get(input) {
            if let Some(ShelleyAddressPaymentPart::PaymentKeyHash(payment_key_hash)) =
                utxo.address.get_payment_part()
            {
                vkey_hashes.insert(payment_key_hash);
            }
        }
    }

    // for each certificate, get the required vkey hashes
    if let Some(certs) = certs.as_ref() {
        for cert_with_pos in certs.iter() {
            vkey_hashes.extend(cert_with_pos.cert.get_vkey_cert_authors());
        }
    }

    // for each withdrawal, get the required vkey hashes
    if let Some(withdrawals) = withdrawals.as_ref() {
        for withdrawal in withdrawals.iter() {
            if let Some(vkey_hash) = withdrawal.get_withdrawal_vkey_author() {
                vkey_hashes.insert(vkey_hash);
            }
        }
    }

    // extend with required signers
    if let Some(required_signers) = required_signers.as_ref() {
        vkey_hashes.extend(required_signers.iter().cloned());
    }

    // for each governance action, get the required vkey hashes
    if let Some(proposal_update) = proposal_update.as_ref() {
        if let Some(genesis_delegs) = genesis_delegs {
            vkey_hashes.extend(proposal_update.get_governance_vkey_authors(genesis_delegs));
        } else {
            error!("Genesis delegates not found in protocol parameters");
        }
    }

    // for each voters, get required vkey hashes
    if let Some(voting_procedures) = voting_procedures.as_ref() {
        for (voter, _) in voting_procedures.votes.iter() {
            if let Some(vkey_hash) = voter.get_voter_key_hash() {
                vkey_hashes.insert(vkey_hash);
            }
        }
    }

    vkey_hashes
}

/// Get Scripts needed for transaction
/// 1. Input scripts: scripts locking UTxO being spent
/// 2. Certificate scripts: scripts in certificate credentials.
/// 3. Withdrawal scripts: scripts controlling reward accounts
/// 4. Mint/Burn scripts: scripts which mint/burn non-Ada assets (added from Mary era)
/// 5. Voting scripts: scripts authorizing voting actions (added from Conway era)
/// 6. Proposing scripts: scripts authorizing proposing actions (added from Conway era)
pub fn get_scripts_needed(
    tx_deltas: &TxUTxODeltas,
    utxos: &HashMap<UTxOIdentifier, UTXOValue>,
) -> HashMap<RedeemerPointer, ScriptHash> {
    let TxUTxODeltas {
        consumes: sorted_inputs,
        certs,
        withdrawals: sorted_withdrawals,
        mint_burn_deltas: sorted_mint_burn,
        voting_procedures,
        proposal_procedures,
        ..
    } = tx_deltas;
    let mut scripts_needed = HashMap::new();

    // for each input, get the required scripts
    scripts_needed.extend(get_scripts_needed_from_inputs(sorted_inputs, utxos));

    // for each certificate, get the required scripts
    if let Some(certs) = certs.as_ref() {
        scripts_needed.extend(get_scripts_needed_from_certificates(certs));
    }

    // for each withdrawal, get the required scripts
    if let Some(sorted_withdrawals) = sorted_withdrawals.as_ref() {
        scripts_needed.extend(get_scripts_needed_from_withdrawals(sorted_withdrawals));
    }

    // for each mint/burn, get the required scripts
    if let Some(sorted_mint_burn) = sorted_mint_burn.as_ref() {
        scripts_needed.extend(get_scripts_needed_from_mint_burn(sorted_mint_burn));
    }

    // for each voting procedure, get the required scripts
    if let Some(voting_procedures) = voting_procedures.as_ref() {
        scripts_needed.extend(get_scripts_needed_from_voting(voting_procedures));
    }

    // for each proposal procedure, get the required scripts
    if let Some(proposal_procedures) = proposal_procedures.as_ref() {
        scripts_needed.extend(get_scripts_needed_from_proposal(proposal_procedures));
    }

    scripts_needed
}

/// Get Scripts provided by transaction
/// Provided = Scripts Witnesses + Reference Scripts
pub fn get_scripts_provided(
    tx_deltas: &TxUTxODeltas,
    utxos: &HashMap<UTxOIdentifier, UTXOValue>,
) -> HashMap<ScriptHash, ScriptLang> {
    let mut scripts_provided = HashMap::new();

    // Check scripts witnesses
    if let Some(script_witnesses) = tx_deltas.script_witnesses.as_ref() {
        scripts_provided.extend(
            script_witnesses
                .iter()
                .map(|(script_hash, script_lang)| (*script_hash, script_lang.clone())),
        );
    }

    // for each input, get the script hash (for reference scripts)
    for input in tx_deltas.consumes.iter() {
        if let Some(utxo) = utxos.get(input) {
            if let Some(script_ref) = utxo.script_ref.as_ref() {
                scripts_provided.insert(script_ref.script_hash, script_ref.script_lang.clone());
            }
        }
    }

    // for each reference input, get the script hash
    for input in tx_deltas.reference_inputs.iter() {
        if let Some(utxo) = utxos.get(input) {
            if let Some(script_ref) = utxo.script_ref.as_ref() {
                scripts_provided.insert(script_ref.script_hash, script_ref.script_lang.clone());
            }
        }
    }
    scripts_provided
}
