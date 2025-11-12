use acropolis_common::{GenesisDelegates, PoolId};
use imbl::HashMap;

pub fn latest_issue_no_tpraos(
    ocert_counter: &HashMap<PoolId, u64>,
    active_spos: &[PoolId],
    genesis_delegs: &GenesisDelegates,
    pool_id: &PoolId,
) -> Option<u64> {
    ocert_counter.get(pool_id).copied().or(if active_spos.contains(pool_id) {
        Some(0)
    } else {
        genesis_delegs.as_ref().values().any(|v| v.delegate.eq(pool_id.as_ref())).then_some(0)
    })
}
