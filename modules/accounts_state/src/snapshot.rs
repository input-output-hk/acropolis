//! Acropolis AccountsState: snapshot for rewards calculations

use crate::state::{Pots, RegistrationChange};
use acropolis_common::{
    snapshot::BootstrapSnapshot, stake_addresses::StakeAddressMap, Lovelace, PoolId,
    PoolRegistration, Ratio, StakeAddress,
};
use imbl::OrdMap;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, info};

/// SPO data for stake snapshot
#[derive(Debug, Default)]
pub struct SnapshotSPO {
    /// List of delegator stake addresses and amounts
    pub delegators: Vec<(StakeAddress, Lovelace)>,

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
    pub reward_account: StakeAddress,

    /// Is the reward account from two epochs ago registered at the time of this snapshot?
    pub two_previous_reward_account_is_registered: bool,

    /// Pool owners
    pub pool_owners: Vec<StakeAddress>,
}

/// Snapshot of stake distribution taken at the end of an particular epoch
#[derive(Debug, Default)]
pub struct Snapshot {
    /// Epoch it's for (the one that has just ended)
    pub epoch: u64,

    /// Map of SPOs by operator ID
    pub spos: HashMap<PoolId, SnapshotSPO>,

    /// Persistent pot values
    pub pots: Pots,

    /// Total SPO (non-OBFT) blocks produced
    pub blocks: usize,

    /// Ordered registration changes
    pub registration_changes: Vec<RegistrationChange>,
}

impl Snapshot {
    /// Get a stake snapshot based on the current stake addresses
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        epoch: u64,
        stake_addresses: &StakeAddressMap,
        spos: &OrdMap<PoolId, PoolRegistration>,
        spo_block_counts: &HashMap<PoolId, usize>,
        pots: &Pots,
        blocks: usize,
        registration_changes: Vec<RegistrationChange>,
        two_previous_snapshot: Arc<Snapshot>,
    ) -> Self {
        let mut snapshot = Self {
            epoch,
            pots: pots.clone(),
            blocks,
            registration_changes,
            ..Self::default()
        };

        // Add all SPOs - some may only have stake, some may only produce blocks (their
        // stake has been removed), we need both in rewards
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

        // Calculate the total rewards just for logging & comparison
        let total_rewards: u64 = stake_addresses.values().map(|sas| sas.rewards).sum();

        // Log to be comparable with DBSync ada_pots table
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

    /// Convert from a pre-processed BootstrapSnapshot
    ///
    /// The BootstrapSnapshot is built by the publisher from raw CBOR data,
    /// this just converts it to the internal Snapshot format.
    pub fn from_bootstrap(bs: BootstrapSnapshot, pots: &Pots) -> Self {
        let mut snapshot = Self {
            epoch: bs.epoch,
            pots: pots.clone(),
            blocks: 0, // Not available from bootstrap data
            registration_changes: Vec::new(),
            spos: HashMap::new(),
        };

        // Convert each BootstrapSnapshotSPO to SnapshotSPO
        for (pool_id, bs_spo) in bs.spos {
            snapshot.spos.insert(
                pool_id,
                SnapshotSPO {
                    delegators: bs_spo.delegators,
                    total_stake: bs_spo.total_stake,
                    pledge: bs_spo.pledge,
                    fixed_cost: bs_spo.cost,
                    margin: bs_spo.margin,
                    blocks_produced: 0, // Not available from bootstrap
                    pool_owners: bs_spo.pool_owners,
                    reward_account: bs_spo.reward_account,
                    two_previous_reward_account_is_registered: true, // Assume registered during bootstrap
                },
            );
        }

        snapshot
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use acropolis_common::stake_addresses::StakeAddressState;
    use acropolis_common::NetworkId::Mainnet;
    use acropolis_common::{PoolId, StakeAddress, StakeCredential};

    fn create_test_stake_address(id: u8) -> StakeAddress {
        let mut hash = [0u8; 28];
        hash[0] = id;
        StakeAddress {
            network: Mainnet,
            credential: StakeCredential::AddrKeyHash(hash.into()),
        }
    }

    fn create_test_spo_hash(id: u8) -> PoolId {
        let mut hash = [0u8; 28];
        hash[0] = id;
        hash.into()
    }

    #[test]
    fn get_stake_snapshot_counts_stake_and_ignores_unregistered_undelegated_and_zero_values() {
        let spo1 = create_test_spo_hash(0x01);
        let spo2 = create_test_spo_hash(0x02);

        let addr1 = create_test_stake_address(0x11);
        let addr2 = create_test_stake_address(0x12);
        let addr3 = create_test_stake_address(0x13);
        let addr4 = create_test_stake_address(0x14);
        let addr5 = create_test_stake_address(0x15);

        let mut stake_addresses: StakeAddressMap = StakeAddressMap::new();
        stake_addresses.insert(
            addr1.clone(),
            StakeAddressState {
                utxo_value: 42,
                registered: true,
                delegated_spo: Some(spo1),
                ..StakeAddressState::default()
            },
        );
        stake_addresses.insert(
            addr2.clone(),
            StakeAddressState {
                utxo_value: 99,
                registered: true,
                delegated_spo: Some(spo2),
                ..StakeAddressState::default()
            },
        );
        stake_addresses.insert(
            addr3.clone(),
            StakeAddressState {
                utxo_value: 0,
                registered: true,
                delegated_spo: Some(spo1),
                ..StakeAddressState::default()
            },
        );
        stake_addresses.insert(
            addr4.clone(),
            StakeAddressState {
                utxo_value: 1000000,
                registered: true,
                delegated_spo: None,
                ..StakeAddressState::default()
            },
        );
        stake_addresses.insert(
            addr5.clone(),
            StakeAddressState {
                utxo_value: 2000000,
                registered: false,
                delegated_spo: None,
                ..StakeAddressState::default()
            },
        );

        let mut spos: OrdMap<PoolId, PoolRegistration> = OrdMap::new();
        spos.insert(spo1, PoolRegistration::default());
        spos.insert(spo2, PoolRegistration::default());
        let spo_block_counts: HashMap<PoolId, usize> = HashMap::new();
        let snapshot = Snapshot::new(
            42,
            &stake_addresses,
            &spos,
            &spo_block_counts,
            &Pots::default(),
            0,
            Vec::new(),
            Arc::new(Snapshot::default()),
        );

        assert_eq!(snapshot.spos.len(), 2);

        let spod1 = snapshot.spos.get(&spo1).unwrap();
        assert_eq!(spod1.delegators.len(), 1);
        let (stake_address1, value1) = &spod1.delegators[0];
        assert_eq!(*stake_address1, addr1);
        assert_eq!(*value1, 42);
        assert_eq!(spod1.total_stake, 42);

        let spod2 = snapshot.spos.get(&spo2).unwrap();
        assert_eq!(spod2.delegators.len(), 1);
        let (stake_address2, value2) = &spod2.delegators[0];
        assert_eq!(*stake_address2, addr2);
        assert_eq!(*value2, 99);
        assert_eq!(spod2.total_stake, 99);
    }

    #[test]
    fn get_stake_delegated_to_spo_by_addresses_when_some_match_is_correct() {
        let mut snapshot = Snapshot::default();
        let spo1 = create_test_spo_hash(0x01);

        let addr1 = create_test_stake_address(0x11);
        let addr2 = create_test_stake_address(0x12);
        let addr3 = create_test_stake_address(0x13);
        let addr4 = create_test_stake_address(0x14);

        snapshot.spos.insert(
            spo1,
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

        // Extract key hashes from stake addresses for the API call
        let addresses = vec![addr2, addr3, addr4];
        let result = snapshot.get_stake_delegated_to_spo_by_addresses(&spo1, &addresses);
        assert_eq!(result, 500);
    }

    #[test]
    fn get_stake_delegated_to_spo_by_addresses_with_no_match_is_0() {
        let mut snapshot = Snapshot::default();
        let spo1 = create_test_spo_hash(0x01);

        let addr1 = create_test_stake_address(0x11);
        let addr_x = create_test_stake_address(0x99);

        snapshot.spos.insert(
            spo1,
            SnapshotSPO {
                delegators: vec![(addr1.clone(), 100)],
                total_stake: 100,
                ..SnapshotSPO::default()
            },
        );

        // Extract key hash from stake address for the API call
        let addresses = vec![addr_x];
        let result = snapshot.get_stake_delegated_to_spo_by_addresses(&spo1, &addresses);
        assert_eq!(result, 0);
    }

    #[test]
    fn get_stake_delegated_to_spo_by_addresses_with_unknown_spo_is_0() {
        let snapshot = Snapshot::default();
        let spo_unknown = create_test_spo_hash(0xFF);
        let result = snapshot.get_stake_delegated_to_spo_by_addresses(&spo_unknown, &[]);
        assert_eq!(result, 0);
    }

    #[test]
    fn get_stake_delegated_to_spo_by_addresses_with_empty_addresses_is_0() {
        let mut snapshot = Snapshot::default();
        let spo1 = create_test_spo_hash(0x01);
        let addr1 = create_test_stake_address(0x11);

        snapshot.spos.insert(
            spo1,
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
