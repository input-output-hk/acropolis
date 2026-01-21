use std::collections::HashSet;
use tracing::error;

use acropolis_common::{
    get_scripts_needed_from_certificates, get_scripts_needed_from_mint_burn,
    get_scripts_needed_from_proposal, get_scripts_needed_from_voting,
    get_scripts_needed_from_withdrawals, protocol_params::ProtocolParams,
    AlonzoBabbageUpdateProposal, KeyHash, NativeAsset, NativeAssetsDelta, ProposalProcedure,
    RedeemerPointer, ScriptHash, TxCertificateWithPos, Value, VotingProcedures, Withdrawal,
};

/// Get VKey Hashes needed for transaction
/// VKey Witnesses needed
/// 1. UTxO authors: keys that own the UTxO being spent.
///    **NOTE:** This is not included in this function.
///    This will be handled in other place where utxos are stored
/// 2. Certificate authors: keys authorizing certificates
/// 3. Pool owners: owners that must sign pool registration
/// 4. Withdrawal authors: keys authorizing reward withdrawals
/// 5. Required Signers: keys that are required to sign the tx (added from Alonzo era)
/// 6. Governance authors: keys authorizing governance actions (e.g. protocol update)
///    **NOTE:** This is removed from Conway era.
/// 7. Vote authors: Keys for Commmittee, DRep, Stake Pool (added from Conway era)
pub fn get_vkey_needed(
    certs: &[TxCertificateWithPos],
    withdrawals: &[Withdrawal],
    required_signers: &[KeyHash],
    proposal_update: &Option<AlonzoBabbageUpdateProposal>,
    voting_procedures: &Option<VotingProcedures>,
    protocol_params: &ProtocolParams,
) -> HashSet<KeyHash> {
    let mut vkey_hashes = HashSet::new();

    let genesis_delegs =
        protocol_params.shelley.as_ref().map(|shelley_params| &shelley_params.gen_delegs);

    // NOTE:
    // Inputs authors will be handled by utxo_state

    // for each certificate, get the required vkey hashes
    for cert_with_pos in certs.iter() {
        vkey_hashes.extend(cert_with_pos.cert.get_vkey_cert_authors());
    }

    // for each withdrawal, get the required vkey hashes
    for withdrawal in withdrawals.iter() {
        if let Some(vkey_hash) = withdrawal.get_withdrawal_vkey_author() {
            vkey_hashes.insert(vkey_hash);
        }
    }

    // extend with required signers
    vkey_hashes.extend(required_signers.iter().cloned());

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
///    **NOTE:** This is not included in this function.
///    This will be handled in other place where utxos are stored
/// 2. Certificate scripts: scripts in certificate credentials.
/// 3. Withdrawal scripts: scripts controlling reward accounts
/// 4. Mint/Burn scripts: scripts which mint/burn non-Ada assets (added from Mary era)
/// 5. Voting scripts: scripts authorizing voting actions (added from Conway era)
/// 6. Proposing scripts: scripts authorizing proposing actions (added from Conway era)
pub fn get_script_needed(
    certs: &[TxCertificateWithPos],
    sorted_withdrawals: &[Withdrawal],
    sorted_mint_burn: &NativeAssetsDelta,
    voting_procedures: &Option<VotingProcedures>,
    proposal_procedures: &Option<Vec<ProposalProcedure>>,
) -> Vec<(RedeemerPointer, ScriptHash)> {
    let mut scripts_needed = Vec::new();

    // NOTE:
    // inputs scripts will be handled by utxo_state

    // for each certificate, get the required scripts
    scripts_needed.extend(get_scripts_needed_from_certificates(certs));

    // for each withdrawal, get the required scripts
    scripts_needed.extend(get_scripts_needed_from_withdrawals(sorted_withdrawals));

    // for each mint/burn, get the required scripts
    scripts_needed.extend(get_scripts_needed_from_mint_burn(sorted_mint_burn));

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

pub fn get_value_minted_burnt_from_deltas(deltas: &NativeAssetsDelta) -> (Value, Value) {
    let mut value_minted = Value::default();
    let mut value_burnt = Value::default();

    for (policy_id, asset_deltas) in deltas.iter() {
        for asset_delta in asset_deltas.iter() {
            if asset_delta.amount > 0 {
                value_minted += &Value::new(
                    0,
                    vec![(
                        *policy_id,
                        vec![NativeAsset {
                            name: asset_delta.name,
                            amount: asset_delta.amount.unsigned_abs(),
                        }],
                    )],
                );
            } else if asset_delta.amount < 0 {
                value_burnt += &Value::new(
                    0,
                    vec![(
                        *policy_id,
                        vec![NativeAsset {
                            name: asset_delta.name,
                            amount: asset_delta.amount.unsigned_abs(),
                        }],
                    )],
                );
            }
        }
    }

    (value_minted, value_burnt)
}
