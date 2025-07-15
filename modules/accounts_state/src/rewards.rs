//! Acropolis AccountsState: rewards calculations

use bigdecimal::BigDecimal;
use std::collections::HashMap;
use acropolis_common::{Lovelace, KeyHash};
use crate::state::StakeAddressState;

/// Snapshot of stake distribution taken at a particular epoch
#[derive(Debug, Default)]
pub struct StakeSnapshot {
    /// Map of SPOs by operator ID, with list of stake addresses and delegated stake values
    pub spos: HashMap<KeyHash, Vec<(KeyHash, Lovelace)>>,
}

impl StakeSnapshot {

    /// Get a stake snapshot based the current stake addresses
    pub fn new(stake_addresses: &HashMap<KeyHash, StakeAddressState>) -> Self {
        let mut snapshot = Self::default();

        // Scan all stake addresses and post to their delegated SPO's list
        // Note this is _active_ stake, for reward calculations, and hence doesn't include rewards
        for (hash, sas) in stake_addresses {
            if sas.utxo_value > 0 {
                if let Some(spo_id) = &sas.delegated_spo {
                    // Only clone if insertion is needed
                    if let Some(delegators) = snapshot.spos.get_mut(spo_id) {
                        delegators.push((hash.clone(), sas.utxo_value));
                    } else {
                        snapshot.spos
                            .insert(spo_id.clone(), vec![(hash.clone(), sas.utxo_value)]);
                    }
                }
            }
        }

        snapshot
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

        let snapshot = StakeSnapshot::new(&stake_addresses);

        assert_eq!(snapshot.spos.len(), 2);

        let delegates1 = snapshot.spos.get(&spo1).unwrap();
        assert_eq!(delegates1.len(), 1);
        let (hash1, value1) = &delegates1[0];
        assert_eq!(*hash1, addr1);
        assert_eq!(*value1, 42);

        let delegates2 = snapshot.spos.get(&spo2).unwrap();
        assert_eq!(delegates2.len(), 1);
        let (hash2, value2) = &delegates2[0];
        assert_eq!(*hash2, addr2);
        assert_eq!(*value2, 99);
    }
}
