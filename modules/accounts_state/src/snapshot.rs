//! Acropolis AccountsState: snapshot for rewards calculations

use std::collections::HashMap;
use acropolis_common::{Lovelace, KeyHash, PoolRegistration, Ratio, RewardAccount};
use crate::state::{StakeAddressState, Pots};
use tracing::{info, error};
use imbl::OrdMap;

/// SPO data for stake snapshot
#[derive(Debug, Default)]
pub struct SnapshotSPO {
    /// List of delegator stake addresses and amounts
    pub delegators: Vec<(KeyHash, Lovelace)>,

    /// Total stake delegated
    pub total_stake: Lovelace,

    /// Pledge
    pub pledge: Lovelace,

    /// Fixed cost
    pub fixed_cost: Lovelace,

    /// Margin
    pub margin: Ratio,

    /// Blocks produced
    pub blocks_produced: usize,

    /// Reward account
    pub reward_account: RewardAccount,

    /// Pool owners
    pub pool_owners: Vec<KeyHash>,
}

/// Snapshot of stake distribution taken at the *start* of a particular epoch
#[derive(Debug, Default)]
pub struct Snapshot {
    /// Epoch it's for (the new one that has just started)
    pub _epoch: u64,

    /// Map of SPOs by operator ID
    pub spos: HashMap<KeyHash, SnapshotSPO>,

    /// Persistent pot values
    pub pots: Pots,

    /// Fees
    pub fees: Lovelace,
}

impl Snapshot {

    /// Get a stake snapshot based the current stake addresses
    pub fn new(epoch: u64, stake_addresses: &HashMap<KeyHash, StakeAddressState>,
               spos: &OrdMap<KeyHash, PoolRegistration>,
               spo_block_counts: &HashMap<KeyHash, usize>,
               pots: &Pots,
               fees: Lovelace) -> Self {
        let mut snapshot = Self {
            _epoch: epoch,
            pots: pots.clone(),
            fees,
            ..Self::default()
        };

        // Scan all stake addresses and post to their delegated SPO's list
        // Note this is _active_ stake, for reward calculations, and hence doesn't include rewards
        let mut total_stake: Lovelace = 0;
        for (hash, sas) in stake_addresses {
            if sas.utxo_value > 0 {
                if let Some(spo_id) = &sas.delegated_spo {
                    // Only clone if insertion is needed
                    if let Some(snap_spo) = snapshot.spos.get_mut(spo_id) {
                        snap_spo.delegators.push((hash.clone(), sas.utxo_value));
                        snap_spo.total_stake += sas.utxo_value;
                    } else {
                        // Find in the SPO list
                        let Some(spo) = spos.get(spo_id) else {
                            error!("Referenced SPO {} not found", hex::encode(spo_id));
                            continue;
                        };

                        // See how many blocks produced
                        let blocks_produced = spo_block_counts.get(spo_id).copied().unwrap_or(0);
                        snapshot.spos.insert(spo_id.clone(), SnapshotSPO {
                            delegators: vec![(hash.clone(), sas.utxo_value)],
                            total_stake: sas.utxo_value,
                            pledge: spo.pledge,
                            fixed_cost: spo.cost,
                            margin: spo.margin.clone(),
                            blocks_produced,
                            pool_owners: spo.pool_owners.clone(),
                            reward_account: spo.reward_account.clone(),
                        });
                    }
                }
                total_stake += sas.utxo_value;
            }
        }

        info!(epoch, reserves=pots.reserves, treasury=pots.treasury, deposits=pots.deposits,
              total_stake, spos=snapshot.spos.len(), "Snapshot");

        snapshot
    }

    /// Get the total stake held by a vector of stake addresses for a particular SPO (by ID)
    pub fn get_stake_delegated_to_spo_by_addresses(&self, spo: &KeyHash,
                                                   addresses: &[KeyHash]) -> Lovelace {
        let Some(snapshot_spo) = self.spos.get(spo) else {
            return 0;
        };

        let addr_set: std::collections::HashSet<_> = addresses.iter().collect();
        snapshot_spo
            .delegators
            .iter()
            .filter_map(|(addr, amount)| {
                if addr_set.contains(addr) {
                    Some(*amount)
                } else {
                    None
                }
            })
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_stake_snapshot_counts_stake_and_ignores_undelegated_and_zero_values() {
        let spo1: KeyHash = vec![0x01];
        let spo2: KeyHash = vec![0x02];

        let addr1: KeyHash = vec![0x11];
        let addr2: KeyHash = vec![0x12];
        let addr3: KeyHash = vec![0x13];
        let addr4: KeyHash = vec![0x14];

        let mut stake_addresses: HashMap<KeyHash, StakeAddressState> = HashMap::new();
        stake_addresses.insert(addr1.clone(), StakeAddressState {
            utxo_value: 42,
            delegated_spo: Some(spo1.clone()),
            .. StakeAddressState::default()
        });
        stake_addresses.insert(addr2.clone(), StakeAddressState {
            utxo_value: 99,
            delegated_spo: Some(spo2.clone()),
            .. StakeAddressState::default()
        });
        stake_addresses.insert(addr3.clone(), StakeAddressState {
            utxo_value: 0,
            delegated_spo: Some(spo1.clone()),
            .. StakeAddressState::default()
        });
        stake_addresses.insert(addr4.clone(), StakeAddressState {
            utxo_value: 1000000,
            delegated_spo: None,
            .. StakeAddressState::default()
        });

        let mut spos: OrdMap<KeyHash, PoolRegistration> = OrdMap::new();
        spos.insert(spo1.clone(), PoolRegistration::default());
        spos.insert(spo2.clone(), PoolRegistration::default());
        let spo_block_counts: HashMap<KeyHash, usize> = HashMap::new();
        let snapshot = Snapshot::new(42, &stake_addresses, &spos, &spo_block_counts,
                                     &Pots::default(), 0);

        assert_eq!(snapshot.spos.len(), 2);

        let spod1 = snapshot.spos.get(&spo1).unwrap();
        assert_eq!(spod1.delegators.len(), 1);
        let (hash1, value1) = &spod1.delegators[0];
        assert_eq!(*hash1, addr1);
        assert_eq!(*value1, 42);
        assert_eq!(spod1.total_stake, 42);

        let spod2 = snapshot.spos.get(&spo2).unwrap();
        assert_eq!(spod2.delegators.len(), 1);
        let (hash2, value2) = &spod2.delegators[0];
        assert_eq!(*hash2, addr2);
        assert_eq!(*value2, 99);
        assert_eq!(spod2.total_stake, 99);
    }

    #[test]
    fn get_stake_delegated_to_spo_by_addresses_when_some_match_is_correct() {
        let mut snapshot = Snapshot::default();
        let spo1: KeyHash = vec![0x01];

        let addr1: KeyHash = vec![0x11];
        let addr2: KeyHash = vec![0x12];
        let addr3: KeyHash = vec![0x13];
        let addr4: KeyHash = vec![0x14];

        snapshot.spos.insert(
            spo1.clone(),
            SnapshotSPO {
                delegators: vec![
                    (addr1.clone(), 100),
                    (addr2.clone(), 200),
                    (addr3.clone(), 300),
                ],
                total_stake: 600,
                ..SnapshotSPO::default()
            },
        );

        let addresses = vec![addr2, addr3, addr4];
        let result = snapshot.get_stake_delegated_to_spo_by_addresses(&spo1, &addresses);
        assert_eq!(result, 500);
    }

    #[test]
    fn get_stake_delegated_to_spo_by_addresses_with_no_match_is_0() {
        let mut snapshot = Snapshot::default();
        let spo1: KeyHash = vec![0x01];

        let addr1: KeyHash = vec![0x11];
        let addr_x: KeyHash = vec![0x99];

        snapshot.spos.insert(
            spo1.clone(),
            SnapshotSPO {
                delegators: vec![(addr1.clone(), 100)],
                total_stake: 100,
                ..SnapshotSPO::default()
            },
        );

        let addresses = vec![addr_x];
        let result = snapshot.get_stake_delegated_to_spo_by_addresses(&spo1, &addresses);
        assert_eq!(result, 0);
    }

    #[test]
    fn get_stake_delegated_to_spo_by_addresses_with_unknown_spo_is_0() {
        let snapshot = Snapshot::default();
        let spo_unknown: KeyHash = vec![0xFF];
        let result = snapshot.get_stake_delegated_to_spo_by_addresses(&spo_unknown, &[]);
        assert_eq!(result, 0);
    }

    #[test]
    fn get_stake_delegated_to_spo_by_addresses_with_empty_addresses_is_0() {
        let mut snapshot = Snapshot::default();
        let spo1: KeyHash = vec![0x01];
        let addr1: KeyHash = vec![0x11];

        snapshot.spos.insert(
            spo1.clone(),
            SnapshotSPO {
                delegators: vec![(addr1.clone(), 100)],
                total_stake: 100,
                ..SnapshotSPO::default()
            },
        );

        let result = snapshot.get_stake_delegated_to_spo_by_addresses(&spo1, &[]);
        assert_eq!(result, 0);
    }
}
