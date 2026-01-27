use crate::{
    math::update_value_with_delta, messages::DRepDelegationDistribution, DRepChoice,
    DRepCredential, DelegatedStake, Lovelace, PoolId, PoolLiveStakeInfo, StakeAddress,
    StakeAddressDelta, Withdrawal,
};
use anyhow::{anyhow, bail, Result};
use dashmap::DashMap;
use imbl::{OrdMap, OrdSet};
use rayon::prelude::*;
use serde_with::{hex::Hex, serde_as};
use std::{
    collections::{
        hash_map::{Entry, Iter, Values},
        BTreeMap, HashMap,
    },
    sync::atomic::AtomicU64,
};
use tracing::error;

/// State of an individual stake address
#[serde_as]
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct StakeAddressState {
    /// Is it registered (or only used in addresses)?
    pub registered: bool,

    /// Total value in UTXO addresses
    pub utxo_value: u64,

    /// Value in a reward account
    pub rewards: u64,

    /// SPO ID they are delegated to ("operator" ID)
    #[serde_as(as = "Option<Hex>")]
    pub delegated_spo: Option<PoolId>,

    /// DRep they are delegated to
    pub delegated_drep: Option<DRepChoice>,
}

// A self-contained stake address state for exporting across module boundaries
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AccountState {
    pub stake_address: StakeAddress,
    pub address_state: StakeAddressState,
}

#[derive(Default, Debug)]
pub struct StakeAddressMap {
    inner: HashMap<StakeAddress, StakeAddressState>,
}

impl StakeAddressMap {
    #[inline]
    pub fn new() -> Self {
        Self {
            inner: HashMap::new(),
        }
    }

    #[inline]
    pub fn get(&self, stake_address: &StakeAddress) -> Option<StakeAddressState> {
        self.inner.get(stake_address).cloned()
    }

    #[inline]
    pub fn get_mut(&mut self, stake_address: &StakeAddress) -> Option<&mut StakeAddressState> {
        self.inner.get_mut(stake_address)
    }

    #[inline]
    pub fn insert(
        &mut self,
        stake_address: StakeAddress,
        stake_address_state: StakeAddressState,
    ) -> Option<StakeAddressState> {
        self.inner.insert(stake_address, stake_address_state)
    }

    #[inline]
    pub fn remove(&mut self, stake_address: &StakeAddress) -> Option<StakeAddressState> {
        self.inner.remove(stake_address)
    }

    #[inline]
    pub fn entry(
        &mut self,
        stake_address: StakeAddress,
    ) -> Entry<'_, StakeAddress, StakeAddressState> {
        self.inner.entry(stake_address)
    }

    #[inline]
    pub fn values(&self) -> Values<'_, StakeAddress, StakeAddressState> {
        self.inner.values()
    }

    #[inline]
    pub fn iter(&self) -> Iter<'_, StakeAddress, StakeAddressState> {
        self.inner.iter()
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    #[inline]
    pub fn is_registered(&self, stake_address: &StakeAddress) -> bool {
        self.get(stake_address).map(|sas| sas.registered).unwrap_or(false)
    }

    /// Get Pool's Live Stake Info
    pub fn get_pool_live_stake_info(&self, spo: &PoolId) -> PoolLiveStakeInfo {
        let total_live_stakes = AtomicU64::new(0);
        let live_stake = AtomicU64::new(0);
        let live_delegators = AtomicU64::new(0);

        // Par Iter stake addresses values
        self.inner.par_iter().for_each(|(_, sas)| {
            total_live_stakes.fetch_add(sas.utxo_value, std::sync::atomic::Ordering::Relaxed);
            if sas.delegated_spo.as_ref().map(|d_spo| d_spo.eq(spo)).unwrap_or(false) {
                live_stake.fetch_add(
                    sas.utxo_value + sas.rewards,
                    std::sync::atomic::Ordering::Relaxed,
                );
                live_delegators.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        });

        let total_live_stakes = total_live_stakes.load(std::sync::atomic::Ordering::Relaxed);
        let live_stake = live_stake.load(std::sync::atomic::Ordering::Relaxed);
        let live_delegators = live_delegators.load(std::sync::atomic::Ordering::Relaxed);
        PoolLiveStakeInfo {
            live_stake,
            live_delegators,
            total_live_stakes,
        }
    }

    /// Get Pool's Live Stake (same order as spos)
    pub fn get_pools_live_stakes(&self, spos: &[PoolId]) -> Vec<u64> {
        let mut live_stakes_map = HashMap::<PoolId, u64>::new();

        // Collect the SPO keys and UTXO
        let sas_data: Vec<(PoolId, u64)> = self
            .inner
            .values()
            .filter_map(|sas| sas.delegated_spo.as_ref().map(|spo| (*spo, sas.utxo_value)))
            .collect();

        sas_data.iter().for_each(|(spo, utxo_value)| {
            live_stakes_map.entry(*spo).and_modify(|v| *v += utxo_value).or_insert(*utxo_value);
        });

        spos.iter()
            .map(|pool_operator| live_stakes_map.get(pool_operator).copied().unwrap_or(0))
            .collect()
    }

    /// Get Pool Delegators with live_stakes
    pub fn get_pool_delegators(&self, pool_operator: &PoolId) -> Vec<(StakeAddress, u64)> {
        // Find stake addresses delegated to pool_operator
        let delegators: Vec<(StakeAddress, u64)> = self
            .inner
            .iter()
            .filter_map(|(stake_address, sas)| match sas.delegated_spo.as_ref() {
                Some(delegated_spo) => {
                    if delegated_spo.eq(pool_operator) {
                        Some((stake_address.clone(), sas.utxo_value + sas.rewards))
                    } else {
                        None
                    }
                }
                _ => None,
            })
            .collect();

        delegators
    }

    /// Get DRep Delegators with live_stakes
    pub fn get_drep_delegators(&self, drep: &DRepChoice) -> Vec<(StakeAddress, u64)> {
        // Find stake addresses delegated to drep
        let delegators: Vec<(StakeAddress, u64)> = self
            .inner
            .iter()
            .filter_map(|(stake_address, sas)| match sas.delegated_drep.as_ref() {
                Some(delegated_drep) => {
                    if delegated_drep.eq(drep) {
                        Some((stake_address.clone(), sas.utxo_value))
                    } else {
                        None
                    }
                }
                _ => None,
            })
            .collect();

        delegators
    }

    /// Map stake_keys to their utxo_value
    /// Return None if any of the stake_keys are not found
    pub fn get_accounts_utxo_values_map(
        &self,
        stake_addresses: &[StakeAddress],
    ) -> Option<HashMap<StakeAddress, u64>> {
        let mut map = HashMap::new();

        for stake_address in stake_addresses {
            let account = self.get(stake_address)?;
            let utxo_value = account.utxo_value;
            map.insert(stake_address.clone(), utxo_value);
        }

        Some(map)
    }

    /// Map stake_addresses to their total balances (utxo + rewards)
    /// Return None if any of the stake_addresses are not found
    pub fn get_accounts_balances_map(
        &self,
        stake_addresses: &[StakeAddress],
    ) -> Option<HashMap<StakeAddress, u64>> {
        let mut map = HashMap::new();

        for stake_address in stake_addresses {
            let account = self.get(stake_address)?;
            let balance = account.utxo_value + account.rewards;
            map.insert(stake_address.clone(), balance);
        }

        Some(map)
    }

    /// Map stake_addresses to their delegated DRep
    /// Return None if any of the stake_addresses are not found
    pub fn get_drep_delegations_map(
        &self,
        stake_addresses: &[StakeAddress],
    ) -> Option<HashMap<StakeAddress, Option<DRepChoice>>> {
        let mut map = HashMap::new();

        for stake_address in stake_addresses {
            let account = self.get(stake_address)?;
            let maybe_drep = account.delegated_drep.clone();
            map.insert(stake_address.clone(), maybe_drep);
        }

        Some(map)
    }

    /// Sum stake_addresss utxo_values
    /// Return None if any of the stake_addresss are not found
    pub fn get_accounts_utxo_values_sum(&self, stake_addresses: &[StakeAddress]) -> Option<u64> {
        let mut total = 0;
        for address in stake_addresses {
            let account = self.get(address)?;
            total += account.utxo_value;
        }
        Some(total)
    }

    /// Sum stake_addresses balances (utxo + rewards)
    /// Return None if any of stake_addresses are not found
    pub fn get_account_balances_sum(&self, stake_addresses: &[StakeAddress]) -> Option<u64> {
        let mut total = 0;
        for stake_address in stake_addresses {
            let account = self.get(stake_address)?;
            total += account.utxo_value + account.rewards;
        }
        Some(total)
    }

    /// Derive the Stake Pool Delegation Distribution (SPDD) - a map of total stake values
    /// (both with and without rewards) for each active SPO
    /// And Stake Pool Reward State (rewards and delegators_count for each pool)
    /// <KeyHash -> DelegatedStake>;Key of returned map is the SPO 'operator' ID
    pub fn generate_spdd(&self) -> BTreeMap<PoolId, DelegatedStake> {
        // Shareable Dashmap with referenced keys
        let spo_stakes = DashMap::<PoolId, DelegatedStake>::new();

        // Total stake across all addresses in parallel, first collecting into a vector
        // because imbl::OrdMap doesn't work in Rayon
        // Collect the SPO keys and UTXO, reward values
        let sas_data: Vec<(PoolId, (u64, u64))> = self
            .inner
            .values()
            .filter_map(|sas| {
                sas.delegated_spo.as_ref().map(|spo| (*spo, (sas.utxo_value, sas.rewards)))
            })
            .collect();

        // Parallel sum all the stakes into the spo_stake map
        sas_data
            .par_iter() // Rayon multi-threaded iterator
            .for_each(|(spo, (utxo_value, rewards))| {
                let total_stake = *utxo_value + *rewards;
                spo_stakes
                    .entry(*spo)
                    .and_modify(|v| {
                        v.active += total_stake;
                        v.active_delegators_count += 1;
                    })
                    .or_insert(DelegatedStake {
                        active: total_stake,
                        active_delegators_count: 1,
                    });
            });

        // Collect into a plain BTreeMap, so that it is ordered on output
        spo_stakes.iter().map(|entry| (*entry.key(), *entry.value())).collect()
    }

    /// Dump current Stake Pool Delegation Distribution State
    /// <PoolId -> (Stake Key, Active Stakes Amount)>
    pub fn dump_spdd_state(&self) -> HashMap<PoolId, Vec<(StakeAddress, u64)>> {
        let entries: Vec<_> = self
            .inner
            .par_iter()
            .filter_map(|(key, sas)| {
                sas.delegated_spo.as_ref().map(|spo| (*spo, (key.clone(), sas.utxo_value)))
            })
            .collect();

        let mut result: HashMap<PoolId, Vec<(StakeAddress, u64)>> = HashMap::new();
        for (spo, entry) in entries {
            result.entry(spo).or_default().push((entry.0, entry.1));
        }
        result
    }

    /// Derive the DRep Delegation Distribution (DRDD) - the total amount
    /// delegated to each DRep, including the special "abstain" and "no confidence" dreps.
    pub fn generate_drdd(
        &self,
        dreps: &OrdMap<DRepCredential, Lovelace>,
    ) -> DRepDelegationDistribution {
        let abstain = AtomicU64::new(0);
        let no_confidence = AtomicU64::new(0);
        let dreps = dreps
            .iter()
            .map(|(cred, _)| (cred.clone(), AtomicU64::new(0)))
            .collect::<BTreeMap<_, _>>();
        self.inner.values().collect::<Vec<_>>().par_iter().for_each(|state| {
            let Some(drep) = state.delegated_drep.clone() else {
                return;
            };
            let total = match drep {
                DRepChoice::Key(hash) => {
                    let cred = DRepCredential::AddrKeyHash(hash);
                    let Some(total) = dreps.get(&cred) else {
                        return;
                    };
                    total
                }
                DRepChoice::Script(hash) => {
                    let cred = DRepCredential::ScriptHash(hash);
                    let Some(total) = dreps.get(&cred) else {
                        return;
                    };
                    total
                }
                DRepChoice::Abstain => &abstain,
                DRepChoice::NoConfidence => &no_confidence,
            };
            let stake = state.utxo_value + state.rewards;
            total.fetch_add(stake, std::sync::atomic::Ordering::Relaxed);
        });
        let abstain = abstain.load(std::sync::atomic::Ordering::Relaxed);
        let no_confidence = no_confidence.load(std::sync::atomic::Ordering::Relaxed);
        let dreps = dreps
            .into_iter()
            .filter_map(|(k, v)| {
                let total = v.load(std::sync::atomic::Ordering::Relaxed);
                if total > 0 {
                    Some((k, total))
                } else {
                    None
                }
            })
            .collect();
        DRepDelegationDistribution {
            abstain,
            no_confidence,
            dreps,
        }
    }

    /// Register a stake address
    /// Return True if registered, False if already registered
    pub fn register_stake_address(&mut self, stake_address: &StakeAddress) -> bool {
        // Stake addresses can be registered after being used in UTXOs
        let sas = self.entry(stake_address.clone()).or_default();
        if sas.registered {
            error!(
                "Stake address {} registered when already registered",
                stake_address
            );
            false
        } else {
            sas.registered = true;
            true
        }
    }

    /// Deregister a stake address
    /// Return True if deregistered, False if unregistered or unknown stake address
    pub fn deregister_stake_address(&mut self, stake_address: &StakeAddress) -> bool {
        // Check if it existed
        if let Some(sas) = self.get_mut(stake_address) {
            if sas.registered {
                sas.registered = false;
                sas.delegated_spo = None;
                sas.delegated_drep = None;
                true
            } else {
                error!(
                    "Deregistration of unregistered stake address {}",
                    stake_address
                );
                false
            }
        } else {
            error!("Deregistration of unknown stake address {}", stake_address);
            false
        }
    }

    /// Record a stake delegation
    pub fn record_stake_delegation(&mut self, stake_address: &StakeAddress, spo: &PoolId) -> bool {
        if let Some(sas) = self.get_mut(stake_address) {
            if sas.registered {
                sas.delegated_spo = Some(*spo);
                true
            } else {
                error!(
                    "Unregistered stake address in stake delegation: {}",
                    stake_address
                );
                false
            }
        } else {
            error!(
                "Unknown stake address in stake delegation: {}",
                stake_address
            );
            false
        }
    }

    /// Remove all delegations to a given SPO
    pub fn remove_all_delegations_to(&mut self, spo: &PoolId) {
        for sas in self.inner.values_mut() {
            if sas.delegated_spo.as_ref() == Some(spo) {
                sas.delegated_spo = None;
            }
        }
    }

    /// Record a drep delegation
    pub fn record_drep_delegation(
        &mut self,
        stake_address: &StakeAddress,
        drep: &DRepChoice,
    ) -> Result<Option<DRepChoice>> {
        let sas = self
            .get_mut(stake_address)
            .filter(|sas| sas.registered)
            .ok_or_else(|| anyhow!("Invalid or unregistered stake address: {stake_address}"))?;

        Ok(sas.delegated_drep.replace(drep.clone()))
    }

    /// Remove delegators from a DRep
    /// This is only used during PV9 to handle the DRep deregistration bug
    /// which requires that we remove the current delegation from all accounts
    /// that have ever delegated to a DRep upon its deregistration, even if the
    /// account is no longer delegating to the DRep.
    pub fn remove_delegators_from_drep(&mut self, delegators: OrdSet<StakeAddress>) {
        for stake_address in delegators {
            if let Some(sas) = self.get_mut(&stake_address) {
                sas.delegated_drep.take();
            }
        }
    }

    /// Add a reward to a reward account (by stake address)
    pub fn add_to_reward(&mut self, stake_address: &StakeAddress, amount: Lovelace) {
        // Get or create account entry, avoiding clone when existing
        let sas = match self.get_mut(stake_address) {
            Some(existing) => existing,
            None => {
                self.insert(stake_address.clone(), StakeAddressState::default());
                self.get_mut(stake_address).unwrap()
            }
        };

        if let Err(e) = update_value_with_delta(&mut sas.rewards, amount as i64) {
            error!("Adding to reward account {}: {e}", stake_address);
        }
    }

    /// Stake Delta
    pub fn process_stake_delta(&mut self, stake_delta: &StakeAddressDelta) -> Result<()> {
        // Use the full stake address directly - no need to extract hash!
        let stake_address = &stake_delta.stake_address;

        // Stake addresses don't need to be registered if they aren't used for
        // stake or drep delegation, but we need to track them in case they are later
        let sas = self.entry(stake_address.clone()).or_default();
        if let Err(e) = update_value_with_delta(&mut sas.utxo_value, stake_delta.delta) {
            bail!("Applying delta to stake address {}: {e}", stake_address);
        }

        Ok(())
    }

    /// Withdraw
    pub fn process_withdrawal(&mut self, withdrawal: &Withdrawal) -> Result<()> {
        let stake_address = &withdrawal.address;

        if let Some(sas) = self.get(stake_address) {
            // Zero withdrawals are expected, as a way to validate stake addresses (per Pi)
            if withdrawal.value != 0 {
                let mut sas = sas.clone();
                if let Err(e) =
                    update_value_with_delta(&mut sas.rewards, -(withdrawal.value as i64))
                {
                    bail!("Withdrawing from stake address {}: {e}", stake_address);
                } else {
                    // Update the stake address
                    self.insert(stake_address.clone(), sas);
                }
            }
        } else {
            bail!("Unknown stake address in withdrawal: {}", stake_address);
        }
        Ok(())
    }

    /// Update reward with delta
    pub fn update_reward(&mut self, stake_address: &StakeAddress, delta: i64) -> Result<()> {
        let sas = self.entry(stake_address.clone()).or_default();
        update_value_with_delta(&mut sas.rewards, delta)
    }

    pub fn pay_reward(&mut self, stake_address: &StakeAddress, delta: u64) -> Result<()> {
        let sas = self.entry(stake_address.clone()).or_default();
        sas.rewards =
            sas.rewards.checked_add(delta).ok_or_else(|| anyhow::anyhow!("reward overflow"))?;
        Ok(())
    }

    /// Update utxo value with delta
    pub fn update_utxo_value(&mut self, stake_address: &StakeAddress, delta: i64) -> Result<()> {
        let sas = self.entry(stake_address.clone()).or_default();
        update_value_with_delta(&mut sas.utxo_value, delta)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::Hash;
    use crate::{KeyHash, NetworkId, StakeAddress, StakeCredential};

    const STAKE_KEY_HASH: KeyHash = KeyHash::new([0x99; 28]);
    const STAKE_KEY_HASH_2: KeyHash = KeyHash::new([0xaa; 28]);
    const STAKE_KEY_HASH_3: KeyHash = KeyHash::new([0xbb; 28]);

    const SPO_HASH: PoolId = PoolId::new(Hash::new([0xbb_u8; 28]));
    const SPO_HASH_2: PoolId = PoolId::new(Hash::new([0x02_u8; 28]));
    const DREP_HASH: KeyHash = KeyHash::new([0xca; 28]);

    fn create_stake_address(hash: KeyHash) -> StakeAddress {
        StakeAddress::new(StakeCredential::AddrKeyHash(hash), NetworkId::Mainnet)
    }

    mod registration_tests {
        use super::*;

        #[test]
        fn test_register_success() {
            let mut stake_addresses = StakeAddressMap::new();
            let stake_address = create_stake_address(STAKE_KEY_HASH);

            assert!(stake_addresses.register_stake_address(&stake_address));
            assert_eq!(stake_addresses.len(), 1);
            assert!(stake_addresses.get(&stake_address).unwrap().registered);
        }

        #[test]
        fn test_double_registration_fails() {
            let mut stake_addresses = StakeAddressMap::new();
            let stake_address = create_stake_address(STAKE_KEY_HASH);

            assert!(stake_addresses.register_stake_address(&stake_address));
            assert!(!stake_addresses.register_stake_address(&stake_address));
            assert_eq!(stake_addresses.len(), 1);
        }

        #[test]
        fn test_deregister_success() {
            let mut stake_addresses = StakeAddressMap::new();
            let stake_address = create_stake_address(STAKE_KEY_HASH);

            stake_addresses.register_stake_address(&stake_address);
            assert!(stake_addresses.deregister_stake_address(&stake_address));
            assert!(!stake_addresses.get(&stake_address).unwrap().registered);
        }

        #[test]
        fn test_deregister_unregistered_fails() {
            let mut stake_addresses = StakeAddressMap::new();
            let stake_address = create_stake_address(STAKE_KEY_HASH);

            // Create an entry but don't register
            stake_addresses
                .process_stake_delta(&StakeAddressDelta {
                    stake_address: stake_address.clone(),
                    addresses: Vec::new(),
                    tx_count: 1,
                    delta: 100,
                })
                .unwrap();

            assert!(!stake_addresses.deregister_stake_address(&stake_address));
        }

        #[test]
        fn test_deregister_unknown_fails() {
            let mut stake_addresses = StakeAddressMap::new();
            let stake_address = create_stake_address(STAKE_KEY_HASH);

            assert!(!stake_addresses.deregister_stake_address(&stake_address));
        }

        #[test]
        fn test_stake_address_lifecycle() {
            let mut stake_addresses = StakeAddressMap::new();
            let stake_address = create_stake_address(STAKE_KEY_HASH);

            // Register
            assert!(stake_addresses.register_stake_address(&stake_address));

            // Delegate
            stake_addresses.record_stake_delegation(&stake_address, &SPO_HASH);
            let drep_choice = DRepChoice::Key(DREP_HASH);
            stake_addresses.record_drep_delegation(&stake_address, &drep_choice).unwrap();

            // Deregister
            assert!(stake_addresses.deregister_stake_address(&stake_address));
            assert!(!stake_addresses.get(&stake_address).unwrap().registered);
        }
    }

    mod delegation_tests {
        use super::*;

        #[test]
        fn test_spo_delegation_success() {
            let mut stake_addresses = StakeAddressMap::new();
            let stake_address = create_stake_address(STAKE_KEY_HASH);

            stake_addresses.register_stake_address(&stake_address);
            assert!(stake_addresses.record_stake_delegation(&stake_address, &SPO_HASH));
            assert_eq!(
                stake_addresses.get(&stake_address).unwrap().delegated_spo,
                Some(SPO_HASH)
            );
        }

        #[test]
        fn test_spo_delegation_and_retirement() {
            let mut stake_addresses = StakeAddressMap::new();
            let stake_address = create_stake_address(STAKE_KEY_HASH);

            stake_addresses.register_stake_address(&stake_address);
            assert!(stake_addresses.record_stake_delegation(&stake_address, &SPO_HASH));
            assert_eq!(
                stake_addresses.get(&stake_address).unwrap().delegated_spo,
                Some(SPO_HASH)
            );

            // Retire the SPO
            stake_addresses.remove_all_delegations_to(&SPO_HASH);
            assert_eq!(
                stake_addresses.get(&stake_address).unwrap().delegated_spo,
                None
            );
        }

        #[test]
        fn test_drep_delegation_success() {
            let mut stake_addresses = StakeAddressMap::new();
            let stake_address = create_stake_address(STAKE_KEY_HASH);

            stake_addresses.register_stake_address(&stake_address);
            let drep_choice = DRepChoice::Key(DREP_HASH);
            let prev =
                stake_addresses.record_drep_delegation(&stake_address, &drep_choice).unwrap();
            assert_eq!(prev, None);
            assert_eq!(
                stake_addresses.get(&stake_address).unwrap().delegated_drep,
                Some(drep_choice)
            );
        }

        #[test]
        fn test_delegation_requires_registration() {
            let mut stake_addresses = StakeAddressMap::new();
            let stake_address = create_stake_address(STAKE_KEY_HASH);

            // Test unknown address
            assert!(!stake_addresses.record_stake_delegation(&stake_address, &SPO_HASH));
            assert!(stake_addresses
                .record_drep_delegation(&stake_address, &DRepChoice::Key(DREP_HASH))
                .is_err());

            // Create an unregistered entry with UTXO value
            stake_addresses
                .process_stake_delta(&StakeAddressDelta {
                    stake_address: stake_address.clone(),
                    addresses: Vec::new(),
                    tx_count: 1,
                    delta: 100,
                })
                .unwrap();

            // Delegation should still fail for unregistered address
            assert!(!stake_addresses.record_stake_delegation(&stake_address, &SPO_HASH));
            assert!(stake_addresses
                .record_drep_delegation(&stake_address, &DRepChoice::Key(DREP_HASH))
                .is_err());
        }

        #[test]
        fn test_re_delegation() {
            let mut stake_addresses = StakeAddressMap::new();
            let stake_address = create_stake_address(STAKE_KEY_HASH);

            stake_addresses.register_stake_address(&stake_address);

            // First SPO delegation
            stake_addresses.record_stake_delegation(&stake_address, &SPO_HASH);
            assert_eq!(
                stake_addresses.get(&stake_address).unwrap().delegated_spo,
                Some(SPO_HASH)
            );

            // Re-delegate to different pool
            stake_addresses.record_stake_delegation(&stake_address, &SPO_HASH_2);
            assert_eq!(
                stake_addresses.get(&stake_address).unwrap().delegated_spo,
                Some(SPO_HASH_2)
            );

            // First DRep delegation
            stake_addresses.record_drep_delegation(&stake_address, &DRepChoice::Abstain).unwrap();
            assert_eq!(
                stake_addresses.get(&stake_address).unwrap().delegated_drep,
                Some(DRepChoice::Abstain)
            );

            // DRep re-delegation
            stake_addresses
                .record_drep_delegation(&stake_address, &DRepChoice::NoConfidence)
                .unwrap();
            assert_eq!(
                stake_addresses.get(&stake_address).unwrap().delegated_drep,
                Some(DRepChoice::NoConfidence)
            );
        }
    }

    mod stake_delta_tests {
        use super::*;

        #[test]
        fn test_positive_delta_accumulates() {
            let mut stake_addresses = StakeAddressMap::new();
            let stake_address = create_stake_address(STAKE_KEY_HASH);

            stake_addresses.register_stake_address(&stake_address);

            let delta = StakeAddressDelta {
                stake_address: stake_address.clone(),
                addresses: Vec::new(),
                tx_count: 1,
                delta: 42,
            };
            stake_addresses.process_stake_delta(&delta).unwrap();
            assert_eq!(stake_addresses.get(&stake_address).unwrap().utxo_value, 42);

            stake_addresses.process_stake_delta(&delta).unwrap();
            assert_eq!(stake_addresses.get(&stake_address).unwrap().utxo_value, 84);
        }

        #[test]
        fn test_negative_delta_reduces() {
            let mut stake_addresses = StakeAddressMap::new();
            let stake_address = create_stake_address(STAKE_KEY_HASH);

            stake_addresses.register_stake_address(&stake_address);

            stake_addresses
                .process_stake_delta(&StakeAddressDelta {
                    stake_address: stake_address.clone(),
                    addresses: Vec::new(),
                    tx_count: 1,
                    delta: 100,
                })
                .unwrap();

            stake_addresses
                .process_stake_delta(&StakeAddressDelta {
                    stake_address: stake_address.clone(),
                    addresses: Vec::new(),
                    tx_count: 1,
                    delta: -30,
                })
                .unwrap();

            assert_eq!(stake_addresses.get(&stake_address).unwrap().utxo_value, 70);
        }

        #[test]
        fn test_negative_delta_underflow_prevented() {
            let mut stake_addresses = StakeAddressMap::new();
            let stake_address = create_stake_address(STAKE_KEY_HASH);

            stake_addresses.register_stake_address(&stake_address);

            stake_addresses
                .process_stake_delta(&StakeAddressDelta {
                    stake_address: stake_address.clone(),
                    addresses: Vec::new(),
                    tx_count: 1,
                    delta: 50,
                })
                .unwrap();

            // Try to subtract more than available -- should result in error
            assert!(stake_addresses
                .process_stake_delta(&StakeAddressDelta {
                    stake_address: stake_address.clone(),
                    addresses: Vec::new(),
                    tx_count: 1,
                    delta: -100,
                })
                .is_err());

            // Value should remain unchanged after error
            assert_eq!(stake_addresses.get(&stake_address).unwrap().utxo_value, 50);
        }
    }

    mod reward_tests {
        use super::*;

        #[test]
        fn test_utxo_and_rewards_tracked_independently() {
            let mut stake_addresses = StakeAddressMap::new();
            let stake_address = create_stake_address(STAKE_KEY_HASH);

            stake_addresses.register_stake_address(&stake_address);

            stake_addresses
                .process_stake_delta(&StakeAddressDelta {
                    stake_address: stake_address.clone(),
                    addresses: Vec::new(),
                    tx_count: 1,
                    delta: 42,
                })
                .unwrap();
            assert_eq!(stake_addresses.get(&stake_address).unwrap().utxo_value, 42);

            stake_addresses.add_to_reward(&stake_address, 12);
            assert_eq!(stake_addresses.get(&stake_address).unwrap().rewards, 12);
            assert_eq!(stake_addresses.get(&stake_address).unwrap().utxo_value, 42);
        }

        #[test]
        fn test_add_to_reward() {
            let mut stake_addresses = StakeAddressMap::new();
            let stake_address = create_stake_address(STAKE_KEY_HASH);

            stake_addresses.register_stake_address(&stake_address);
            stake_addresses.add_to_reward(&stake_address, 100);
            assert_eq!(stake_addresses.get(&stake_address).unwrap().rewards, 100);

            stake_addresses.add_to_reward(&stake_address, 50);
            assert_eq!(stake_addresses.get(&stake_address).unwrap().rewards, 150);
        }

        #[test]
        fn test_update_reward_positive_delta() {
            let mut stake_addresses = StakeAddressMap::new();
            let stake_address = create_stake_address(STAKE_KEY_HASH);

            assert!(stake_addresses.update_reward(&stake_address, 100).is_ok());
            assert_eq!(stake_addresses.get(&stake_address).unwrap().rewards, 100);
        }

        #[test]
        fn test_update_reward_negative_delta() {
            let mut stake_addresses = StakeAddressMap::new();
            let stake_address = create_stake_address(STAKE_KEY_HASH);

            stake_addresses.update_reward(&stake_address, 100).unwrap();
            assert!(stake_addresses.update_reward(&stake_address, -50).is_ok());
            assert_eq!(stake_addresses.get(&stake_address).unwrap().rewards, 50);
        }

        #[test]
        fn test_update_reward_underflow() {
            let mut stake_addresses = StakeAddressMap::new();
            let stake_address = create_stake_address(STAKE_KEY_HASH);

            stake_addresses.update_reward(&stake_address, 50).unwrap();

            let result = stake_addresses.update_reward(&stake_address, -100);
            assert!(result.is_err());
            assert_eq!(stake_addresses.get(&stake_address).unwrap().rewards, 50);
        }

        #[test]
        fn test_update_reward_creates_entry_if_missing() {
            let mut stake_addresses = StakeAddressMap::new();
            let stake_address = create_stake_address(STAKE_KEY_HASH);

            assert!(stake_addresses.update_reward(&stake_address, 100).is_ok());
            assert_eq!(stake_addresses.len(), 1);
            assert_eq!(stake_addresses.get(&stake_address).unwrap().rewards, 100);
        }
    }

    mod withdrawal_tests {
        use crate::TxIdentifier;

        use super::*;

        #[test]
        fn test_withdrawal_success() -> Result<()> {
            let mut stake_addresses = StakeAddressMap::new();
            let stake_address = create_stake_address(STAKE_KEY_HASH);

            stake_addresses.register_stake_address(&stake_address);
            stake_addresses.add_to_reward(&stake_address, 100);

            let withdrawal = Withdrawal {
                address: stake_address.clone(),
                value: 40,
                tx_identifier: TxIdentifier::default(),
            };
            stake_addresses.process_withdrawal(&withdrawal)?;

            assert_eq!(stake_addresses.get(&stake_address).unwrap().rewards, 60);
            Ok(())
        }

        #[test]
        fn test_withdrawal_prevents_underflow() -> Result<()> {
            let mut stake_addresses = StakeAddressMap::new();
            let stake_address = create_stake_address(STAKE_KEY_HASH);

            stake_addresses.register_stake_address(&stake_address);
            stake_addresses.add_to_reward(&stake_address, 12);

            // Withdraw more than reward (should be prevented)
            let withdrawal = Withdrawal {
                address: stake_address.clone(),
                value: 24,
                tx_identifier: TxIdentifier::default(),
            };
            assert!(stake_addresses.process_withdrawal(&withdrawal).is_err());
            assert_eq!(stake_addresses.get(&stake_address).unwrap().rewards, 12);

            // Withdraw less than reward (should succeed)
            let withdrawal = Withdrawal {
                address: stake_address.clone(),
                value: 2,
                tx_identifier: TxIdentifier::default(),
            };
            stake_addresses.process_withdrawal(&withdrawal)?;
            assert_eq!(stake_addresses.get(&stake_address).unwrap().rewards, 10);
            Ok(())
        }

        #[test]
        fn test_zero_withdrawal() -> Result<()> {
            let mut stake_addresses = StakeAddressMap::new();
            let stake_address = create_stake_address(STAKE_KEY_HASH);

            stake_addresses.register_stake_address(&stake_address);
            stake_addresses.add_to_reward(&stake_address, 100);

            let withdrawal = Withdrawal {
                address: stake_address.clone(),
                value: 0,
                tx_identifier: TxIdentifier::default(),
            };

            stake_addresses.process_withdrawal(&withdrawal)?;
            assert_eq!(stake_addresses.get(&stake_address).unwrap().rewards, 100);
            Ok(())
        }

        #[test]
        fn test_withdrawal_unknown_address() -> Result<()> {
            let mut stake_addresses = StakeAddressMap::new();
            let stake_address = create_stake_address(STAKE_KEY_HASH);

            let withdrawal = Withdrawal {
                address: stake_address.clone(),
                value: 10,
                tx_identifier: TxIdentifier::default(),
            };

            // Should return error, but not panic
            assert!(stake_addresses.process_withdrawal(&withdrawal).is_err());
            Ok(())
        }
    }

    mod update_tests {
        use super::*;

        #[test]
        fn test_update_utxo_value_positive_delta() {
            let mut stake_addresses = StakeAddressMap::new();
            let stake_address = create_stake_address(STAKE_KEY_HASH);

            assert!(stake_addresses.update_utxo_value(&stake_address, 500).is_ok());
            assert_eq!(stake_addresses.get(&stake_address).unwrap().utxo_value, 500);
        }

        #[test]
        fn test_update_utxo_value_negative_delta() {
            let mut stake_addresses = StakeAddressMap::new();
            let stake_address = create_stake_address(STAKE_KEY_HASH);

            stake_addresses.update_utxo_value(&stake_address, 500).unwrap();
            assert!(stake_addresses.update_utxo_value(&stake_address, -200).is_ok());
            assert_eq!(stake_addresses.get(&stake_address).unwrap().utxo_value, 300);
        }

        #[test]
        fn test_update_utxo_value_underflow() {
            let mut stake_addresses = StakeAddressMap::new();
            let stake_address = create_stake_address(STAKE_KEY_HASH);

            stake_addresses.update_utxo_value(&stake_address, 100).unwrap();

            let result = stake_addresses.update_utxo_value(&stake_address, -200);
            assert!(result.is_err());
            assert_eq!(stake_addresses.get(&stake_address).unwrap().utxo_value, 100);
        }

        #[test]
        fn test_update_utxo_value_creates_entry_if_missing() {
            let mut stake_addresses = StakeAddressMap::new();
            let stake_address = create_stake_address(STAKE_KEY_HASH);

            assert!(stake_addresses.update_utxo_value(&stake_address, 500).is_ok());
            assert_eq!(stake_addresses.len(), 1);
            assert_eq!(stake_addresses.get(&stake_address).unwrap().utxo_value, 500);
        }
    }

    mod distribution_tests {
        use super::*;

        #[test]
        fn test_generate_spdd_single_pool() {
            let mut stake_addresses = StakeAddressMap::new();

            let addr1 = create_stake_address(STAKE_KEY_HASH);
            let addr2 = create_stake_address(STAKE_KEY_HASH_2);

            stake_addresses.register_stake_address(&addr1);
            stake_addresses.register_stake_address(&addr2);
            stake_addresses.record_stake_delegation(&addr1, &SPO_HASH);
            stake_addresses.record_stake_delegation(&addr2, &SPO_HASH);

            stake_addresses
                .process_stake_delta(&StakeAddressDelta {
                    stake_address: addr1.clone(),
                    addresses: Vec::new(),
                    tx_count: 1,
                    delta: 1000,
                })
                .unwrap();
            stake_addresses.add_to_reward(&addr1, 50);

            stake_addresses
                .process_stake_delta(&StakeAddressDelta {
                    stake_address: addr2.clone(),
                    addresses: Vec::new(),
                    tx_count: 1,
                    delta: 2000,
                })
                .unwrap();
            stake_addresses.add_to_reward(&addr2, 100);

            let spdd = stake_addresses.generate_spdd();

            let pool_stake = spdd.get(&SPO_HASH).unwrap();
            assert_eq!(pool_stake.active, 3150);
            assert_eq!(pool_stake.active_delegators_count, 2);
        }

        #[test]
        fn test_generate_spdd_multiple_pools() {
            let mut stake_addresses = StakeAddressMap::new();

            let addr1 = create_stake_address(STAKE_KEY_HASH);
            let addr2 = create_stake_address(STAKE_KEY_HASH_2);

            stake_addresses.register_stake_address(&addr1);
            stake_addresses.register_stake_address(&addr2);
            stake_addresses.record_stake_delegation(&addr1, &SPO_HASH);
            stake_addresses.record_stake_delegation(&addr2, &SPO_HASH_2);

            stake_addresses
                .process_stake_delta(&StakeAddressDelta {
                    stake_address: addr1.clone(),
                    addresses: Vec::new(),
                    tx_count: 1,
                    delta: 1000,
                })
                .unwrap();
            stake_addresses
                .process_stake_delta(&StakeAddressDelta {
                    stake_address: addr2.clone(),
                    addresses: Vec::new(),
                    tx_count: 1,
                    delta: 2000,
                })
                .unwrap();

            let spdd = stake_addresses.generate_spdd();

            assert_eq!(spdd.len(), 2);
            assert_eq!(spdd.get(&SPO_HASH).unwrap().active, 1000);
            assert_eq!(spdd.get(&SPO_HASH_2).unwrap().active, 2000);
        }

        #[test]
        fn test_generate_spdd_no_delegations() {
            let mut stake_addresses = StakeAddressMap::new();
            let addr1 = create_stake_address(STAKE_KEY_HASH);

            stake_addresses.register_stake_address(&addr1);
            stake_addresses
                .process_stake_delta(&StakeAddressDelta {
                    stake_address: addr1.clone(),
                    addresses: Vec::new(),
                    tx_count: 1,
                    delta: 1000,
                })
                .unwrap();

            let spdd = stake_addresses.generate_spdd();
            assert!(spdd.is_empty());
        }

        #[test]
        fn test_generate_drdd_with_special_choices() {
            let mut stake_addresses = StakeAddressMap::new();

            let addr1 = create_stake_address(STAKE_KEY_HASH);
            let addr2 = create_stake_address(STAKE_KEY_HASH_2);
            let addr3 = create_stake_address(STAKE_KEY_HASH_3);

            stake_addresses.register_stake_address(&addr1);
            stake_addresses.register_stake_address(&addr2);
            stake_addresses.register_stake_address(&addr3);

            stake_addresses.record_drep_delegation(&addr1, &DRepChoice::Abstain).unwrap();
            stake_addresses.record_drep_delegation(&addr2, &DRepChoice::NoConfidence).unwrap();
            stake_addresses.record_drep_delegation(&addr3, &DRepChoice::Key(DREP_HASH)).unwrap();

            stake_addresses
                .process_stake_delta(&StakeAddressDelta {
                    stake_address: addr1.clone(),
                    addresses: Vec::new(),
                    tx_count: 1,
                    delta: 1000,
                })
                .unwrap();
            stake_addresses.add_to_reward(&addr1, 50);

            stake_addresses
                .process_stake_delta(&StakeAddressDelta {
                    stake_address: addr2.clone(),
                    addresses: Vec::new(),
                    tx_count: 1,
                    delta: 2000,
                })
                .unwrap();
            stake_addresses.add_to_reward(&addr2, 100);

            stake_addresses
                .process_stake_delta(&StakeAddressDelta {
                    stake_address: addr3.clone(),
                    addresses: Vec::new(),
                    tx_count: 1,
                    delta: 3000,
                })
                .unwrap();
            stake_addresses.add_to_reward(&addr3, 150);

            let dreps = OrdMap::from_iter([(DRepCredential::AddrKeyHash(DREP_HASH), 500_u64)]);
            let drdd = stake_addresses.generate_drdd(&dreps);

            assert_eq!(drdd.abstain, 1050); // 1000 + 50
            assert_eq!(drdd.no_confidence, 2100); // 2000 + 100

            let drep_cred = DRepCredential::AddrKeyHash(DREP_HASH);
            let drep_stake = drdd
                .dreps
                .iter()
                .find(|(cred, _)| cred == &drep_cred)
                .map(|(_, stake)| *stake)
                .unwrap();

            assert_eq!(drep_stake, 3650); // 3000 + 150 + 500 deposit
        }
    }

    mod pool_query_tests {
        use super::*;

        #[test]
        fn test_get_pool_live_stake_info() {
            let mut stake_addresses = StakeAddressMap::new();

            let addr1 = create_stake_address(STAKE_KEY_HASH);
            let addr2 = create_stake_address(STAKE_KEY_HASH_2);

            stake_addresses.register_stake_address(&addr1);
            stake_addresses.register_stake_address(&addr2);
            stake_addresses.record_stake_delegation(&addr1, &SPO_HASH);
            stake_addresses.record_stake_delegation(&addr2, &SPO_HASH_2);

            stake_addresses
                .process_stake_delta(&StakeAddressDelta {
                    stake_address: addr1.clone(),
                    addresses: Vec::new(),
                    tx_count: 1,
                    delta: 1000,
                })
                .unwrap();
            stake_addresses.add_to_reward(&addr1, 50);

            stake_addresses
                .process_stake_delta(&StakeAddressDelta {
                    stake_address: addr2.clone(),
                    addresses: Vec::new(),
                    tx_count: 1,
                    delta: 2000,
                })
                .unwrap();
            stake_addresses.add_to_reward(&addr2, 100);

            let info = stake_addresses.get_pool_live_stake_info(&SPO_HASH);

            assert_eq!(info.live_stake, 1050); // utxo + rewards for pool 1
            assert_eq!(info.live_delegators, 1);
            assert_eq!(info.total_live_stakes, 3000); // total utxo across all addresses
        }

        #[test]
        fn test_get_pools_live_stakes() {
            let mut stake_addresses = StakeAddressMap::new();

            let addr1 = create_stake_address(STAKE_KEY_HASH);
            let addr2 = create_stake_address(STAKE_KEY_HASH_2);

            stake_addresses.register_stake_address(&addr1);
            stake_addresses.register_stake_address(&addr2);
            stake_addresses.record_stake_delegation(&addr1, &SPO_HASH);
            stake_addresses.record_stake_delegation(&addr2, &SPO_HASH_2);

            stake_addresses
                .process_stake_delta(&StakeAddressDelta {
                    stake_address: addr1.clone(),
                    addresses: Vec::new(),
                    tx_count: 1,
                    delta: 1000,
                })
                .unwrap();
            stake_addresses
                .process_stake_delta(&StakeAddressDelta {
                    stake_address: addr2.clone(),
                    addresses: Vec::new(),
                    tx_count: 1,
                    delta: 2000,
                })
                .unwrap();

            let pools = vec![SPO_HASH, SPO_HASH_2];
            let stakes = stake_addresses.get_pools_live_stakes(&pools);

            assert_eq!(stakes, vec![1000, 2000]);
        }

        #[test]
        fn test_get_pool_delegators() {
            let mut stake_addresses = StakeAddressMap::new();

            let addr1 = create_stake_address(STAKE_KEY_HASH);
            let addr2 = create_stake_address(STAKE_KEY_HASH_2);

            stake_addresses.register_stake_address(&addr1);
            stake_addresses.register_stake_address(&addr2);
            stake_addresses.record_stake_delegation(&addr1, &SPO_HASH);
            stake_addresses.record_stake_delegation(&addr2, &SPO_HASH);

            stake_addresses
                .process_stake_delta(&StakeAddressDelta {
                    stake_address: addr1.clone(),
                    addresses: Vec::new(),
                    tx_count: 1,
                    delta: 1000,
                })
                .unwrap();
            stake_addresses.add_to_reward(&addr1, 50);

            stake_addresses
                .process_stake_delta(&StakeAddressDelta {
                    stake_address: addr2.clone(),
                    addresses: Vec::new(),
                    tx_count: 1,
                    delta: 2000,
                })
                .unwrap();

            let delegators = stake_addresses.get_pool_delegators(&SPO_HASH);

            assert_eq!(delegators.len(), 2);
            assert!(delegators.iter().any(|(_, stake)| *stake == 1050));
            assert!(delegators.iter().any(|(_, stake)| *stake == 2000));
        }
    }

    mod account_utxo_query_tests {
        use super::*;

        #[test]
        fn test_get_accounts_utxo_values_map_success() {
            let mut stake_addresses = StakeAddressMap::new();

            let addr1 = create_stake_address(STAKE_KEY_HASH);
            let addr2 = create_stake_address(STAKE_KEY_HASH_2);

            stake_addresses.register_stake_address(&addr1);
            stake_addresses.register_stake_address(&addr2);

            stake_addresses
                .process_stake_delta(&StakeAddressDelta {
                    stake_address: addr1.clone(),
                    addresses: Vec::new(),
                    tx_count: 1,
                    delta: 1000,
                })
                .unwrap();
            stake_addresses.add_to_reward(&addr1, 500);

            stake_addresses
                .process_stake_delta(&StakeAddressDelta {
                    stake_address: addr2.clone(),
                    addresses: Vec::new(),
                    tx_count: 1,
                    delta: 2000,
                })
                .unwrap();

            let keys = vec![addr1.clone(), addr2.clone()];
            let map = stake_addresses.get_accounts_utxo_values_map(&keys).unwrap();

            assert_eq!(map.len(), 2);
            assert_eq!(map.get(&addr1).copied().unwrap(), 1000);
            assert_eq!(map.get(&addr2).copied().unwrap(), 2000);
        }

        #[test]
        fn test_get_accounts_utxo_values_map_missing_account() {
            let mut stake_addresses = StakeAddressMap::new();
            let addr1 = create_stake_address(STAKE_KEY_HASH);
            let addr2 = create_stake_address(STAKE_KEY_HASH_2);

            stake_addresses.register_stake_address(&addr1);
            stake_addresses
                .process_stake_delta(&StakeAddressDelta {
                    stake_address: addr1.clone(),
                    addresses: Vec::new(),
                    tx_count: 1,
                    delta: 1000,
                })
                .unwrap();

            let keys = vec![addr1, addr2];

            assert!(stake_addresses.get_accounts_utxo_values_map(&keys).is_none());
        }

        #[test]
        fn test_get_accounts_utxo_values_map_empty_list() {
            let stake_addresses = StakeAddressMap::new();

            let keys: Vec<StakeAddress> = vec![];
            let map = stake_addresses.get_accounts_utxo_values_map(&keys).unwrap();

            assert!(map.is_empty());
        }

        #[test]
        fn test_get_accounts_utxo_values_sum_success() {
            let mut stake_addresses = StakeAddressMap::new();

            let addr1 = create_stake_address(STAKE_KEY_HASH);
            let addr2 = create_stake_address(STAKE_KEY_HASH_2);

            stake_addresses.register_stake_address(&addr1);
            stake_addresses.register_stake_address(&addr2);

            stake_addresses
                .process_stake_delta(&StakeAddressDelta {
                    stake_address: addr1.clone(),
                    addresses: Vec::new(),
                    tx_count: 1,
                    delta: 1000,
                })
                .unwrap();
            stake_addresses.add_to_reward(&addr1, 500);

            stake_addresses
                .process_stake_delta(&StakeAddressDelta {
                    stake_address: addr2.clone(),
                    addresses: Vec::new(),
                    tx_count: 1,
                    delta: 2000,
                })
                .unwrap();

            let keys = vec![addr1, addr2];
            let sum = stake_addresses.get_accounts_utxo_values_sum(&keys).unwrap();

            assert_eq!(sum, 3000);
        }

        #[test]
        fn test_get_accounts_utxo_values_sum_missing_account() {
            let mut stake_addresses = StakeAddressMap::new();
            let addr1 = create_stake_address(STAKE_KEY_HASH);
            let addr2 = create_stake_address(STAKE_KEY_HASH_2);

            stake_addresses.register_stake_address(&addr1);
            stake_addresses
                .process_stake_delta(&StakeAddressDelta {
                    stake_address: addr1.clone(),
                    addresses: Vec::new(),
                    tx_count: 1,
                    delta: 1000,
                })
                .unwrap();

            let keys = vec![addr1, addr2];

            assert!(stake_addresses.get_accounts_utxo_values_sum(&keys).is_none());
        }

        #[test]
        fn test_get_accounts_utxo_values_sum_empty_list() {
            let stake_addresses = StakeAddressMap::new();

            let keys: Vec<StakeAddress> = vec![];
            let sum = stake_addresses.get_accounts_utxo_values_sum(&keys).unwrap();

            assert_eq!(sum, 0);
        }
    }

    mod account_balance_query_tests {
        use super::*;

        #[test]
        fn test_get_accounts_balances_map_includes_rewards() {
            let mut stake_addresses = StakeAddressMap::new();

            let addr1 = create_stake_address(STAKE_KEY_HASH);
            let addr2 = create_stake_address(STAKE_KEY_HASH_2);

            stake_addresses.register_stake_address(&addr1);
            stake_addresses.register_stake_address(&addr2);

            stake_addresses
                .process_stake_delta(&StakeAddressDelta {
                    stake_address: addr1.clone(),
                    addresses: Vec::new(),
                    tx_count: 1,
                    delta: 1000,
                })
                .unwrap();
            stake_addresses.add_to_reward(&addr1, 100);

            stake_addresses
                .process_stake_delta(&StakeAddressDelta {
                    stake_address: addr2.clone(),
                    addresses: Vec::new(),
                    tx_count: 1,
                    delta: 2000,
                })
                .unwrap();

            let addresses = vec![addr1.clone(), addr2.clone()];
            let map = stake_addresses.get_accounts_balances_map(&addresses).unwrap();

            assert_eq!(map.len(), 2);
            assert_eq!(map.get(&addr1).copied().unwrap(), 1100);
            assert_eq!(map.get(&addr2).copied().unwrap(), 2000);
        }

        #[test]
        fn test_get_accounts_balances_map_missing_account() {
            let mut stake_addresses = StakeAddressMap::new();
            let addr1 = create_stake_address(STAKE_KEY_HASH);
            let addr2 = create_stake_address(STAKE_KEY_HASH_2);

            stake_addresses.register_stake_address(&addr1);
            stake_addresses
                .process_stake_delta(&StakeAddressDelta {
                    stake_address: addr1.clone(),
                    addresses: Vec::new(),
                    tx_count: 1,
                    delta: 1000,
                })
                .unwrap();

            let addresses = vec![addr1, addr2];

            assert!(stake_addresses.get_accounts_balances_map(&addresses).is_none());
        }

        #[test]
        fn test_get_accounts_balances_map_empty_list() {
            let stake_addresses = StakeAddressMap::new();

            let addresses: Vec<StakeAddress> = vec![];
            let map = stake_addresses.get_accounts_balances_map(&addresses).unwrap();

            assert!(map.is_empty());
        }

        #[test]
        fn test_get_account_balances_sum_includes_rewards() {
            let mut stake_addresses = StakeAddressMap::new();

            let addr1 = create_stake_address(STAKE_KEY_HASH);
            let addr2 = create_stake_address(STAKE_KEY_HASH_2);

            stake_addresses.register_stake_address(&addr1);
            stake_addresses.register_stake_address(&addr2);

            stake_addresses
                .process_stake_delta(&StakeAddressDelta {
                    stake_address: addr1.clone(),
                    addresses: Vec::new(),
                    tx_count: 1,
                    delta: 1000,
                })
                .unwrap();
            stake_addresses.add_to_reward(&addr1, 100);

            stake_addresses
                .process_stake_delta(&StakeAddressDelta {
                    stake_address: addr2.clone(),
                    addresses: Vec::new(),
                    tx_count: 1,
                    delta: 2000,
                })
                .unwrap();

            let addresses = vec![addr1, addr2];
            let sum = stake_addresses.get_account_balances_sum(&addresses).unwrap();

            assert_eq!(sum, 3100);
        }

        #[test]
        fn test_get_account_balances_sum_missing_account() {
            let mut stake_addresses = StakeAddressMap::new();
            let addr1 = create_stake_address(STAKE_KEY_HASH);
            let addr2 = create_stake_address(STAKE_KEY_HASH_2);

            stake_addresses.register_stake_address(&addr1);
            stake_addresses
                .process_stake_delta(&StakeAddressDelta {
                    stake_address: addr1.clone(),
                    addresses: Vec::new(),
                    tx_count: 1,
                    delta: 1000,
                })
                .unwrap();

            let addresses = vec![addr1, addr2];

            assert!(stake_addresses.get_account_balances_sum(&addresses).is_none());
        }

        #[test]
        fn test_get_account_balances_sum_empty_list() {
            let stake_addresses = StakeAddressMap::new();

            let addresses = vec![];
            let sum = stake_addresses.get_account_balances_sum(&addresses).unwrap();

            assert_eq!(sum, 0);
        }
    }

    mod drep_query_tests {
        use super::*;

        #[test]
        fn test_get_drep_delegations_map_various_choices() {
            let mut stake_addresses = StakeAddressMap::new();

            let addr1 = create_stake_address(STAKE_KEY_HASH);
            let addr2 = create_stake_address(STAKE_KEY_HASH_2);
            let addr3 = create_stake_address(STAKE_KEY_HASH_3);

            stake_addresses.register_stake_address(&addr1);
            stake_addresses.register_stake_address(&addr2);
            stake_addresses.register_stake_address(&addr3);

            stake_addresses.record_drep_delegation(&addr1, &DRepChoice::Abstain).unwrap();
            stake_addresses.record_drep_delegation(&addr2, &DRepChoice::Key(DREP_HASH)).unwrap();

            let addresses = vec![addr1.clone(), addr2.clone(), addr3.clone()];
            let map = stake_addresses.get_drep_delegations_map(&addresses).unwrap();

            assert_eq!(map.len(), 3);
            assert_eq!(map.get(&addr1).unwrap(), &Some(DRepChoice::Abstain));
            assert_eq!(map.get(&addr2).unwrap(), &Some(DRepChoice::Key(DREP_HASH)));
            assert_eq!(map.get(&addr3).unwrap(), &None);
        }

        #[test]
        fn test_get_drep_delegations_map_missing_account() {
            let mut stake_addresses = StakeAddressMap::new();
            let addr1 = create_stake_address(STAKE_KEY_HASH);
            let addr2 = create_stake_address(STAKE_KEY_HASH_2);

            stake_addresses.register_stake_address(&addr1);
            stake_addresses.record_drep_delegation(&addr1, &DRepChoice::NoConfidence).unwrap();

            let addresses = vec![addr1, addr2];

            assert!(stake_addresses.get_drep_delegations_map(&addresses).is_none());
        }

        #[test]
        fn test_get_drep_delegations_map_empty_list() {
            let stake_addresses = StakeAddressMap::new();

            let addresses: Vec<StakeAddress> = vec![];
            let map = stake_addresses.get_drep_delegations_map(&addresses).unwrap();

            assert!(map.is_empty());
        }

        #[test]
        fn test_get_drep_delegators() {
            let mut stake_addresses = StakeAddressMap::new();

            let addr1 = create_stake_address(STAKE_KEY_HASH);
            let addr2 = create_stake_address(STAKE_KEY_HASH_2);
            let addr3 = create_stake_address(STAKE_KEY_HASH_3);

            stake_addresses.register_stake_address(&addr1);
            stake_addresses.register_stake_address(&addr2);
            stake_addresses.register_stake_address(&addr3);

            let drep_choice = DRepChoice::Key(DREP_HASH);
            stake_addresses.record_drep_delegation(&addr1, &drep_choice).unwrap();
            stake_addresses.record_drep_delegation(&addr2, &drep_choice).unwrap();
            stake_addresses.record_drep_delegation(&addr3, &DRepChoice::Abstain).unwrap();

            stake_addresses
                .process_stake_delta(&StakeAddressDelta {
                    stake_address: addr1.clone(),
                    addresses: Vec::new(),
                    tx_count: 1,
                    delta: 1000,
                })
                .unwrap();
            stake_addresses
                .process_stake_delta(&StakeAddressDelta {
                    stake_address: addr2.clone(),
                    addresses: Vec::new(),
                    tx_count: 1,
                    delta: 2000,
                })
                .unwrap();
            stake_addresses
                .process_stake_delta(&StakeAddressDelta {
                    stake_address: addr3.clone(),
                    addresses: Vec::new(),
                    tx_count: 1,
                    delta: 3000,
                })
                .unwrap();

            let delegators = stake_addresses.get_drep_delegators(&drep_choice);

            assert_eq!(delegators.len(), 2);
            assert!(delegators.iter().any(|(_, stake)| *stake == 1000));
            assert!(delegators.iter().any(|(_, stake)| *stake == 2000));
        }
    }
}
