use acropolis_common::{Credential, Withdrawal};
use amaru_uplc::machine::PlutusVersion;

/// Plutus Data encoding uses transformed Credential;
/// Which sorts PubKey first, then Script Hash;
/// Only Votes and Withdrawal from Plutus V3, they use the original ledger ordering.
pub fn cmp_credential_as_plutus_data(a: &Credential, b: &Credential) -> std::cmp::Ordering {
    match (a, b) {
        (Credential::AddrKeyHash(_), Credential::ScriptHash(_)) => std::cmp::Ordering::Less,
        (Credential::ScriptHash(_), Credential::AddrKeyHash(_)) => std::cmp::Ordering::Greater,
        (a, b) => a.cmp(b),
    }
}

// ============================================================================
// Withdrawals
// ============================================================================

/// V3 uses the original ledger ordering for Credential
/// Reference: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/conway/impl/src/Cardano/Ledger/Conway/TxInfo.hs#L545
pub fn cmp_withdrawal(
    a: &Withdrawal,
    b: &Withdrawal,
    plutus_version: PlutusVersion,
) -> std::cmp::Ordering {
    match plutus_version {
        PlutusVersion::V1 | PlutusVersion::V2 => {
            cmp_credential_as_plutus_data(&a.address.credential, &b.address.credential)
        }
        _ => a.address.credential.cmp(&b.address.credential),
    }
}
