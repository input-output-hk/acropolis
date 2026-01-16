use std::collections::HashSet;
use tracing::error;

use acropolis_common::{
    protocol_params::ProtocolParams, AlonzoBabbageUpdateProposal, KeyHash, NativeAsset,
    NativeAssetsDelta, ScriptHash, TxCertificateWithPos, Value, Withdrawal,
};

/// Get VKey Witnesses needed for transaction
/// Get Scripts needed for transaction
/// Reference: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/shelley/impl/src/Cardano/Ledger/Shelley/UTxO.hs#L274
/// https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/shelley/impl/src/Cardano/Ledger/Shelley/UTxO.hs#L226
/// https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/shelley/impl/src/Cardano/Ledger/Shelley/UTxO.hs#L103
///
/// VKey Witnesses needed
/// 1. UTxO authors: keys that own the UTxO being spent
/// 2. Certificate authors: keys authorizing certificates
/// 3. Pool owners: owners that must sign pool registration
/// 4. Withdrawal authors: keys authorizing reward withdrawals
/// 5. Governance authors: keys authorizing governance actions (e.g. protocol update)
///
/// Script Witnesses needed
/// 1. Input scripts: scripts locking UTxO being spent
/// 2. Withdrawal scripts: scripts controlling reward accounts
/// 3. Certificate scripts: scripts in certificate credentials.
///
/// NOTE:
/// This doesn't count `inputs`
/// which will be considered in the utxos_state
pub fn get_vkey_script_needed(
    certs: &[TxCertificateWithPos],
    withdrawals: &[Withdrawal],
    proposal_update: &Option<AlonzoBabbageUpdateProposal>,
    protocol_params: &ProtocolParams,
    vkey_hashes: &mut HashSet<KeyHash>,
    script_hashes: &mut HashSet<ScriptHash>,
) {
    let genesis_delegs =
        protocol_params.shelley.as_ref().map(|shelley_params| &shelley_params.gen_delegs);
    // for each certificate, get the required vkey and script hashes
    for cert_with_pos in certs.iter() {
        cert_with_pos.cert.get_cert_authors(vkey_hashes, script_hashes);
    }

    // for each withdrawal, get the required vkey and script hashes
    for withdrawal in withdrawals.iter() {
        withdrawal.get_withdrawal_authors(vkey_hashes, script_hashes);
    }

    // for each governance action, get the required vkey hashes
    if let Some(proposal_update) = proposal_update.as_ref() {
        if let Some(genesis_delegs) = genesis_delegs {
            proposal_update.get_governance_authors(vkey_hashes, genesis_delegs);
        } else {
            error!("Genesis delegates not found in protocol parameters");
        }
    }
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
