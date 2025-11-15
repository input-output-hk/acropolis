use std::collections::HashSet;

use acropolis_common::{GenesisDelegates, PoolId};
use imbl::HashMap;

/// This function is used to get the latest issue number for a given pool id.
/// First check ocert_counters
/// Check if the pool is in active_spos (registered or not)
/// And if the pool is a genesis delegate
/// Reference
/// https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/libs/cardano-protocol-tpraos/src/Cardano/Protocol/TPraos/OCert.hs#L66
pub fn latest_issue_no_tpraos(
    ocert_counters: &HashMap<PoolId, u64>,
    active_spos: &HashSet<PoolId>,
    genesis_delegs: &GenesisDelegates,
    pool_id: &PoolId,
) -> Option<u64> {
    ocert_counters.get(pool_id).copied().or(if active_spos.contains(pool_id) {
        Some(0)
    } else {
        genesis_delegs.as_ref().values().any(|v| v.delegate.eq(pool_id.as_ref())).then_some(0)
    })
}
