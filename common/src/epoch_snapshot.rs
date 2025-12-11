use crate::stake_addresses::StakeAddressMap;
use crate::{
    Lovelace, NetworkId, PoolId, PoolRegistration, Pots, Ratio, RegistrationChange, StakeAddress,
    StakeCredential,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::Display;

/// SPO data captured in a stake snapshot
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SnapshotSPO {
    /// List of delegator stake addresses and their stake amounts
    pub delegators: Vec<(StakeAddress, Lovelace)>,

    /// Total stake delegated to this pool
    pub total_stake: Lovelace,

    /// Pool pledge amount
    pub pledge: Lovelace,

    /// Pool fixed cost
    pub fixed_cost: Lovelace,

    /// Pool margin (fee percentage)
    pub margin: Ratio,

    /// Number of blocks produced by this pool in this epoch
    pub blocks_produced: usize,

    /// Pool reward account
    pub reward_account: StakeAddress,

    /// Pool owners
    pub pool_owners: Vec<StakeAddress>,

    /// Is the reward account from two epochs ago registered at the time of this snapshot?
    /// Used for rewards calculation edge cases. Defaults to false.
    #[serde(default = "default_false")]
    pub two_previous_reward_account_is_registered: bool,
}

fn default_false() -> bool {
    false
}

/// Captures the state of an epoch at a moment in time (typically at epoch end):
/// stake distribution, blocks produced, pots, and registration changes.
/// Used for rewards calculations. The mark/set/go pattern refers to the timing
/// of when these snapshots are taken, not what they contain.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EpochSnapshot {
    /// Epoch this snapshot is for (the one that has just ended)
    pub epoch: u64,

    /// Map of SPOs by operator ID with their delegation data
    pub spos: HashMap<PoolId, SnapshotSPO>,

    /// Total SPO (non-OBFT) blocks produced in this epoch
    pub blocks: usize,

    /// Pot balances at the time of this snapshot
    pub pots: Pots,

    /// Ordered registration changes that occurred during this epoch
    #[serde(default)]
    pub registration_changes: Vec<RegistrationChange>,
}

impl EpochSnapshot {
    /// Create a new snapshot from the current stake address state (used at epoch boundary)
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        epoch: u64,
        stake_addresses: &StakeAddressMap,
        spos: &imbl::OrdMap<PoolId, PoolRegistration>,
        spo_block_counts: &HashMap<PoolId, usize>,
        pots: &Pots,
        blocks: usize,
        registration_changes: Vec<RegistrationChange>,
        two_previous_snapshot: std::sync::Arc<EpochSnapshot>,
    ) -> Self {
        use tracing::{debug, info};

        let mut snapshot = EpochSnapshot {
            epoch,
            pots: pots.clone(),
            blocks,
            registration_changes,
            ..EpochSnapshot::default()
        };

        // Add all SPOs - some may only have stake, some may only produce blocks (their
        // stake has been removed); we need both in rewards
        for (spo_id, spo) in spos {
            // See how many blocks produced
            let blocks_produced = spo_block_counts.get(spo_id).copied().unwrap_or(0);

            // Check if the reward account from two epochs ago is still registered
            let two_previous_reward_account_is_registered =
                match two_previous_snapshot.spos.get(spo_id) {
                    Some(old_spo) => stake_addresses
                        .get(&old_spo.reward_account)
                        .map(|sas| sas.registered)
                        .unwrap_or(false),
                    None => false,
                };
            debug!(
                epoch,
                previous_epoch = two_previous_snapshot.epoch,
                "Two previous reward account for SPO {} registered: {}",
                spo_id,
                two_previous_reward_account_is_registered
            );

            // Add the new one
            snapshot.spos.insert(
                *spo_id,
                SnapshotSPO {
                    delegators: vec![],
                    total_stake: 0,
                    pledge: spo.pledge,
                    fixed_cost: spo.cost,
                    margin: spo.margin.clone(),
                    blocks_produced,
                    pool_owners: spo.pool_owners.clone(),
                    reward_account: spo.reward_account.clone(),
                    two_previous_reward_account_is_registered,
                },
            );
        }

        // Scan all stake addresses and post to their delegated SPO's list
        // Note this is 'active stake', for reward calculations, and does include rewards
        let mut total_stake: Lovelace = 0;
        for (stake_address, sas) in stake_addresses.iter() {
            let active_stake = sas.utxo_value + sas.rewards;

            if sas.registered && active_stake > 0 {
                if let Some(spo_id) = &sas.delegated_spo {
                    if let Some(snap_spo) = snapshot.spos.get_mut(spo_id) {
                        snap_spo.delegators.push((stake_address.clone(), active_stake));
                        snap_spo.total_stake += active_stake;
                    } else {
                        // SPO has retired - this stake is simply ignored
                        debug!(
                            epoch,
                            "SPO {} for stake address {} retired?  Ignored", spo_id, stake_address
                        );
                        continue;
                    }
                }
                total_stake += active_stake;
            }
        }

        // Calculate the total rewards just for logging and comparison
        let total_rewards: u64 = stake_addresses.values().map(|sas| sas.rewards).sum();

        // Log to be comparable with the DBSync ada_pots table
        info!(
            epoch,
            treasury = pots.treasury,
            reserves = pots.reserves,
            rewards = total_rewards,
            deposits = pots.deposits,
            total_stake,
            spos = snapshot.spos.len(),
            blocks,
            "Snapshot"
        );

        snapshot
    }

    /// Create a new snapshot from raw CBOR-parsed data (used during bootstrap parsing)
    /// Takes ownership of the maps to avoid cloning large data structures.
    pub fn from_raw(
        epoch: u64,
        stake_map: HashMap<StakeCredential, i64>,
        delegation_map: HashMap<StakeCredential, PoolId>,
        pool_params_map: HashMap<PoolId, PoolRegistration>,
        block_counts: &HashMap<PoolId, usize>,
        pots: Pots,
        network: NetworkId,
    ) -> Self {
        // First pass: group delegations by pool (O(n) instead of O(n*m))
        let mut delegations_by_pool: HashMap<PoolId, Vec<(StakeAddress, Lovelace)>> =
            HashMap::new();
        let mut stake_by_pool: HashMap<PoolId, Lovelace> = HashMap::new();

        for (credential, pool_id) in delegation_map {
            if let Some(&stake) = stake_map.get(&credential) {
                let stake_lovelace = stake.max(0) as Lovelace;
                if stake_lovelace > 0 {
                    let stake_address = StakeAddress {
                        network: network.clone(),
                        credential,
                    };
                    delegations_by_pool
                        .entry(pool_id)
                        .or_default()
                        .push((stake_address, stake_lovelace));
                    *stake_by_pool.entry(pool_id).or_default() += stake_lovelace;
                }
            }
        }

        // Second pass: build SPO entries
        let mut spos = HashMap::new();
        for (pool_id, pool_reg) in pool_params_map {
            let delegators = delegations_by_pool.remove(&pool_id).unwrap_or_default();
            let total_stake = stake_by_pool.get(&pool_id).copied().unwrap_or(0);
            let blocks_produced = block_counts.get(&pool_id).copied().unwrap_or(0);

            spos.insert(
                pool_id,
                SnapshotSPO {
                    delegators,
                    total_stake,
                    pledge: pool_reg.pledge,
                    fixed_cost: pool_reg.cost,
                    margin: pool_reg.margin,
                    blocks_produced,
                    pool_owners: pool_reg.pool_owners,
                    reward_account: pool_reg.reward_account,
                    two_previous_reward_account_is_registered: true,
                },
            );
        }

        EpochSnapshot {
            epoch,
            spos,
            blocks: 0,
            pots,
            registration_changes: Vec::new(),
        }
    }

    /// Get the total stake held by a vector of stake addresses for a particular SPO (by ID)
    pub fn get_stake_delegated_to_spo_by_addresses(
        &self,
        spo: &PoolId,
        addresses: &[StakeAddress],
    ) -> Lovelace {
        let Some(snapshot_spo) = self.spos.get(spo) else {
            return 0;
        };

        let address_set: std::collections::HashSet<_> = addresses.iter().collect();
        snapshot_spo
            .delegators
            .iter()
            .filter_map(|(address, amount)| {
                if address_set.contains(&address) {
                    Some(*amount)
                } else {
                    None
                }
            })
            .sum()
    }
}

/// Container for the three snapshots used in rewards calculation (mark, set, go)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SnapshotsContainer {
    /// Mark snapshot (epoch - 2)
    pub mark: EpochSnapshot,

    /// Set snapshot (epoch - 1)
    pub set: EpochSnapshot,

    /// Go snapshot (current epoch)
    pub go: EpochSnapshot,
}

impl Display for SnapshotsContainer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Mark: {}, Set: {}, Go: {}", self.mark, self.set, self.go)
    }
}

impl Display for EpochSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "EpochSnapshot {{ epoch: {}, blocks: {}, pots: {:?} }}", self.epoch, self.blocks, self.pots)
    }
}
