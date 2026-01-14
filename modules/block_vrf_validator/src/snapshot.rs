use acropolis_common::{
    messages::{AccountsBootstrapMessage, SPOStakeDistributionMessage, SPOStateMessage},
    PoolId, VrfKeyHash,
};
use std::collections::HashMap;
use std::fmt::{Display, Formatter};

/// Epoch data for block vrf validation
#[derive(Debug, Default)]
pub struct Snapshot {
    /// Map of pool_id to its vrf_key_hash
    pub active_spos: HashMap<PoolId, VrfKeyHash>,

    /// active stakes keyed by pool id
    pub active_stakes: HashMap<PoolId, u64>,

    pub total_active_stakes: u64,
}

impl Display for Snapshot {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "total_active: {}, active stakes: {:?}",
            self.total_active_stakes, self.active_stakes
        )
    }
}

impl From<(&SPOStateMessage, &SPOStakeDistributionMessage)> for Snapshot {
    fn from((spo_state_msg, spdd_msg): (&SPOStateMessage, &SPOStakeDistributionMessage)) -> Self {
        let active_spos: HashMap<PoolId, VrfKeyHash> = spo_state_msg
            .spos
            .iter()
            .map(|registration| (registration.operator, registration.vrf_key_hash))
            .collect();
        let active_stakes: HashMap<PoolId, u64> =
            spdd_msg.spos.iter().map(|(pool_id, stake)| (*pool_id, stake.active)).collect();
        let total_active_stakes = active_stakes.values().sum();
        Self {
            active_spos,
            active_stakes,
            total_active_stakes,
        }
    }
}

impl From<AccountsBootstrapMessage> for Snapshot {
    fn from(bootstrap_msg: AccountsBootstrapMessage) -> Self {
        let vrf_by_pool: HashMap<PoolId, VrfKeyHash> = bootstrap_msg
            .pools
            .iter()
            .map(|reg| {
                let pool_id = reg.operator;
                let vrf = reg.vrf_key_hash;
                (pool_id, vrf)
            })
            .collect();

        let active_stakes: HashMap<PoolId, u64> = bootstrap_msg
            .bootstrap_snapshots
            .mark
            .spos
            .iter()
            .filter_map(|(pool_id, snapshot)| {
                let stake = snapshot.total_stake;
                if stake > 0 {
                    Some((*pool_id, stake))
                } else {
                    None
                }
            })
            .collect();

        let active_spos: HashMap<PoolId, VrfKeyHash> = active_stakes
            .keys()
            .filter_map(|pool_id| vrf_by_pool.get(pool_id).map(|vrf| (*pool_id, *vrf)))
            .collect();

        let total_active_stakes = active_stakes.values().copied().sum();

        Self {
            active_spos,
            active_stakes,
            total_active_stakes,
        }
    }
}
