use std::collections::HashSet;

use acropolis_common::PoolId;
use imbl::HashMap;

pub fn latest_issue_no_praos(
    ocert_counters: &HashMap<PoolId, u64>,
    active_spos: &HashSet<PoolId>,
    pool_id: &PoolId,
) -> Option<u64> {
    ocert_counters.get(pool_id).copied().or(if active_spos.contains(pool_id) {
        Some(0)
    } else {
        None
    })
}
