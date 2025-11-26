use std::collections::HashMap;

use acropolis_common::{
    messages::{SPOStakeDistributionMessage, SPOStateMessage},
    PoolId, VrfKeyHash,
};

/// Epoch data for block vrf validation
#[derive(Debug, Default)]
pub struct Snapshot {
    /// Map of pool_id to its vrf_key_hash
    pub active_spos: HashMap<PoolId, VrfKeyHash>,

    /// active stakes keyed by pool id
    pub active_stakes: HashMap<PoolId, u64>,

    pub total_active_stakes: u64,
}

impl From<(&SPOStateMessage, &SPOStakeDistributionMessage)> for Snapshot {
    fn from((spo_state_msg, spdd_msg): (&SPOStateMessage, &SPOStakeDistributionMessage)) -> Self {
        let active_spos: HashMap<PoolId, VrfKeyHash> = spo_state_msg
            .spos
            .iter()
            .map(|registration| (registration.operator, registration.vrf_key_hash))
            .collect();
        let active_stakes: HashMap<PoolId, u64> =
            spdd_msg.spos.iter().map(|(pool_id, stake)| (*pool_id, stake.live)).collect();
        let total_active_stakes = spdd_msg.spos.iter().map(|(_, stake)| stake.active).sum();
        Self {
            active_spos,
            active_stakes,
            total_active_stakes,
        }
    }
}
