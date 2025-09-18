use std::{
    borrow::Borrow,
    collections::{
        hash_map::{Entry, Iter, Values},
        BTreeMap, HashMap,
    },
    hash::Hash,
    sync::atomic::AtomicU64,
};

use crate::{
    math::update_value_with_delta, messages::DRepDelegationDistribution, DRepChoice,
    DRepCredential, DelegatedStake, KeyHash, Lovelace, StakeAddressDelta, StakeCredential,
    Withdrawal,
};
use anyhow::Result;
use dashmap::DashMap;
use rayon::prelude::*;
use serde_with::{hex::Hex, serde_as};
use tracing::{error, warn};

/// State of an individual stake address
#[serde_as]
#[derive(Debug, Default, Clone, serde::Serialize)]
pub struct StakeAddressState {
    /// Is it registered (or only used in addresses)?
    pub registered: bool,

    /// Total value in UTXO addresses
    pub utxo_value: u64,

    /// Value in reward account
    pub rewards: u64,

    /// SPO ID they are delegated to ("operator" ID)
    #[serde_as(as = "Option<Hex>")]
    pub delegated_spo: Option<KeyHash>,

    /// DRep they are delegated to
    pub delegated_drep: Option<DRepChoice>,
}

#[derive(Default, Debug)]
pub struct StakeAddressMap {
    inner: HashMap<KeyHash, StakeAddressState>,
}

impl StakeAddressMap {
    #[inline]
    pub fn new() -> Self {
        Self {
            inner: HashMap::new(),
        }
    }

    #[inline]
    pub fn get<K>(&self, stake_key: &K) -> Option<StakeAddressState>
    where
        KeyHash: Borrow<K>,
        K: Hash + Eq + ?Sized,
    {
        self.inner.get(stake_key).cloned()
    }

    #[inline]
    pub fn get_mut<K>(&mut self, stake_key: &K) -> Option<&mut StakeAddressState>
    where
        KeyHash: Borrow<K>,
        K: Hash + Eq + ?Sized,
    {
        self.inner.get_mut(stake_key)
    }

    #[inline]
    pub fn insert(
        &mut self,
        stake_key: KeyHash,
        stake_address_state: StakeAddressState,
    ) -> Option<StakeAddressState> {
        self.inner.insert(stake_key, stake_address_state)
    }

    #[inline]
    pub fn remove(&mut self, stake_key: &KeyHash) -> Option<StakeAddressState> {
        self.inner.remove(stake_key)
    }

    #[inline]
    pub fn entry(&mut self, stake_key: KeyHash) -> Entry<KeyHash, StakeAddressState> {
        self.inner.entry(stake_key)
    }

    #[inline]
    pub fn values(&self) -> Values<KeyHash, StakeAddressState> {
        self.inner.values()
    }

    #[inline]
    pub fn iter(&self) -> Iter<KeyHash, StakeAddressState> {
        self.inner.iter()
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    #[inline]
    pub fn is_registered(&self, stake_key: &KeyHash) -> bool {
        self.get(stake_key).map(|sas| sas.registered).unwrap_or(false)
    }

    /// Get Pool's Live Stake (same order as spos)
    pub fn get_pools_live_stakes(&self, spos: &Vec<KeyHash>) -> Vec<u64> {
        let mut live_stakes_map = HashMap::<KeyHash, u64>::new();

        // Collect the SPO keys and UTXO
        let sas_data: Vec<(KeyHash, u64)> = self
            .inner
            .values()
            .filter_map(|sas| sas.delegated_spo.as_ref().map(|spo| (spo.clone(), sas.utxo_value)))
            .collect();

        sas_data.iter().for_each(|(spo, utxo_value)| {
            live_stakes_map
                .entry(spo.clone())
                .and_modify(|v| *v += utxo_value)
                .or_insert(*utxo_value);
        });

        spos.iter()
            .map(|pool_operator| live_stakes_map.get(pool_operator).map(|v| *v).unwrap_or(0))
            .collect()
    }

    /// Get Pool Delegators with live_stakes
    pub fn get_pool_delegators(&self, pool_operator: &KeyHash) -> Vec<(KeyHash, u64)> {
        // Find stake addresses delegated to pool_operator
        let delegators: Vec<(KeyHash, u64)> = self
            .inner
            .iter()
            .filter_map(|(stake_key, sas)| match sas.delegated_spo.as_ref() {
                Some(delegated_spo) => {
                    if delegated_spo.eq(pool_operator) {
                        Some((stake_key.clone(), sas.utxo_value + sas.rewards))
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
    pub fn get_drep_delegators(&self, drep: &DRepChoice) -> Vec<(KeyHash, u64)> {
        // Find stake addresses delegated to drep
        let delegators: Vec<(KeyHash, u64)> = self
            .inner
            .iter()
            .filter_map(|(stake_key, sas)| match sas.delegated_drep.as_ref() {
                Some(delegated_drep) => {
                    if delegated_drep.eq(drep) {
                        Some((stake_key.clone(), sas.utxo_value))
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
        stake_keys: &[Vec<u8>],
    ) -> Option<HashMap<Vec<u8>, u64>> {
        let mut map = HashMap::new();

        for key in stake_keys {
            let account = self.get(key)?;
            let utxo_value = account.utxo_value;
            map.insert(key.clone(), utxo_value);
        }

        Some(map)
    }

    /// Map stake_keys to their total balances (utxo + rewards)
    /// Return None if any of the stake_keys are not found
    pub fn get_accounts_balances_map(
        &self,
        stake_keys: &[Vec<u8>],
    ) -> Option<HashMap<Vec<u8>, u64>> {
        let mut map = HashMap::new();

        for key in stake_keys {
            let account = self.get(key)?;
            let balance = account.utxo_value + account.rewards;
            map.insert(key.clone(), balance);
        }

        Some(map)
    }

    /// Map stake_keys to their delegated DRep
    /// Return None if any of the stake_keys are not found
    pub fn get_drep_delegations_map(
        &self,
        stake_keys: &[Vec<u8>],
    ) -> Option<HashMap<Vec<u8>, Option<DRepChoice>>> {
        let mut map = HashMap::new();

        for stake_key in stake_keys {
            let account = self.get(stake_key)?;
            let maybe_drep = account.delegated_drep.clone();
            map.insert(stake_key.clone(), maybe_drep);
        }

        Some(map)
    }

    /// Sum stake_keys utxo_values
    /// Return None if any of the stake_keys are not found
    pub fn get_accounts_utxo_values_sum(&self, stake_keys: &[Vec<u8>]) -> Option<u64> {
        let mut total = 0;
        for key in stake_keys {
            let account = self.get(key)?;
            total += account.utxo_value;
        }
        Some(total)
    }

    /// Sum stake_keys balances (utxo + rewards)
    /// Return None if any of stake_keys are not found
    pub fn get_account_balances_sum(&self, stake_keys: &[Vec<u8>]) -> Option<u64> {
        let mut total = 0;
        for key in stake_keys {
            let account = self.get(key)?;
            total += account.utxo_value + account.rewards;
        }
        Some(total)
    }

    /// Derive the Stake Pool Delegation Distribution (SPDD) - a map of total stake values
    /// (both with and without rewards) for each active SPO
    /// And Stake Pool Reward State (rewards and delegators_count for each pool)
    /// Key of returned map is the SPO 'operator' ID
    pub fn generate_spdd(&self) -> BTreeMap<KeyHash, DelegatedStake> {
        // Shareable Dashmap with referenced keys
        let spo_stakes = DashMap::<KeyHash, DelegatedStake>::new();

        // Total stake across all addresses in parallel, first collecting into a vector
        // because imbl::OrdMap doesn't work in Rayon
        // Collect the SPO keys and UTXO, reward values
        let sas_data: Vec<(KeyHash, (u64, u64))> = self
            .inner
            .values()
            .filter_map(|sas| {
                sas.delegated_spo.as_ref().map(|spo| (spo.clone(), (sas.utxo_value, sas.rewards)))
            })
            .collect();

        // Parallel sum all the stakes into the spo_stake map
        sas_data
            .par_iter() // Rayon multi-threaded iterator
            .for_each(|(spo, (utxo_value, rewards))| {
                spo_stakes
                    .entry(spo.clone())
                    .and_modify(|v| {
                        v.active += *utxo_value;
                        v.active_delegators_count += 1;
                        v.live += *utxo_value + *rewards;
                    })
                    .or_insert(DelegatedStake {
                        active: *utxo_value,
                        active_delegators_count: 1,
                        live: *utxo_value + *rewards,
                    });
            });

        // Collect into a plain BTreeMap, so that it is ordered on output
        spo_stakes.iter().map(|entry| (entry.key().clone(), entry.value().clone())).collect()
    }

    /// Derive the DRep Delegation Distribution (DRDD) - the total amount
    /// delegated to each DRep, including the special "abstain" and "no confidence" dreps.
    pub fn generate_drdd(
        &self,
        dreps: &Vec<(DRepCredential, Lovelace)>,
    ) -> DRepDelegationDistribution {
        let abstain = AtomicU64::new(0);
        let no_confidence = AtomicU64::new(0);
        let dreps = dreps
            .iter()
            .map(|(cred, deposit)| (cred.clone(), AtomicU64::new(*deposit)))
            .collect::<BTreeMap<_, _>>();
        self.inner.values().collect::<Vec<_>>().par_iter().for_each(|state| {
            let Some(drep) = state.delegated_drep.clone() else {
                return;
            };
            let total = match drep {
                DRepChoice::Key(hash) => {
                    let cred = DRepCredential::AddrKeyHash(hash);
                    let Some(total) = dreps.get(&cred) else {
                        warn!("Delegated to unregistered DRep address {cred:?}");
                        return;
                    };
                    total
                }
                DRepChoice::Script(hash) => {
                    let cred = DRepCredential::ScriptHash(hash);
                    let Some(total) = dreps.get(&cred) else {
                        warn!("Delegated to unregistered DRep script {cred:?}");
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
            .map(|(k, v)| (k, v.load(std::sync::atomic::Ordering::Relaxed)))
            .collect();
        DRepDelegationDistribution {
            abstain,
            no_confidence,
            dreps,
        }
    }

    /// Register a stake address, with specified deposit if known
    /// Return True if registered, False if already registered
    pub fn register_stake_address(&mut self, credential: &StakeCredential) -> bool {
        let hash = credential.get_hash();

        // Stake addresses can be registered after being used in UTXOs
        let sas = self.entry(hash.clone()).or_default();
        if sas.registered {
            error!(
                "Stake address hash {} registered when already registered",
                hex::encode(&hash)
            );
            false
        } else {
            sas.registered = true;

            true
        }
    }

    /// Deregister a stake address, with specified refund if known
    /// Return True if deregistered, False if unregistered or unknown stake key hash
    pub fn deregister_stake_address(&mut self, credential: &StakeCredential) -> bool {
        let hash = credential.get_hash();

        // Check if it existed
        if let Some(sas) = self.get_mut(&hash) {
            if sas.registered {
                sas.registered = false;
                true
            } else {
                error!(
                    "Deregistration of unregistered stake address hash {}",
                    hex::encode(hash)
                );
                false
            }
        } else {
            error!(
                "Deregistration of unknown stake address hash {}",
                hex::encode(hash)
            );
            false
        }
    }

    /// Record a stake delegation
    pub fn record_stake_delegation(&mut self, credential: &StakeCredential, spo: &KeyHash) -> bool {
        let hash = credential.get_hash();

        if let Some(sas) = self.get_mut(&hash) {
            if sas.registered {
                sas.delegated_spo = Some(spo.clone());
                true
            } else {
                error!(
                    "Unregistered stake address in stake delegation: {}",
                    hex::encode(hash)
                );
                false
            }
        } else {
            error!(
                "Unknown stake address in stake delegation: {}",
                hex::encode(hash)
            );
            false
        }
    }

    /// Record a drep delegation
    pub fn record_drep_delegation(
        &mut self,
        credential: &StakeCredential,
        drep: &DRepChoice,
    ) -> bool {
        let hash = credential.get_hash();

        if let Some(sas) = self.get_mut(&hash) {
            if sas.registered {
                sas.delegated_drep = Some(drep.clone());
                true
            } else {
                error!(
                    "Unregistered stake address in DRep delegation: {}",
                    hex::encode(hash)
                );
                false
            }
        } else {
            error!(
                "Unknown stake address in drep delegation: {}",
                hex::encode(hash)
            );
            false
        }
    }

    /// Add a reward to a reward account (by stake key hash)
    pub fn add_to_reward(&mut self, account: &KeyHash, amount: Lovelace) {
        // Get or create account entry, avoiding clone when existing
        let sas = match self.get_mut(account) {
            Some(existing) => existing,
            None => {
                self.insert(account.clone(), StakeAddressState::default());
                self.get_mut(account).unwrap()
            }
        };

        if let Err(e) = update_value_with_delta(&mut sas.rewards, amount as i64) {
            error!("Adding to reward account {}: {e}", hex::encode(account));
        }
    }

    /// Stake Delta
    pub fn process_stake_delta(&mut self, stake_delta: &StakeAddressDelta) {
        // Fold both stake key and script hashes into one - assuming the chance of
        // collision is negligible
        let hash = stake_delta.address.get_hash();

        // Stake addresses don't need to be registered if they aren't used for
        // stake or drep delegation, but we need to track them in case they are later
        let sas = self.entry(hash.to_vec()).or_default();
        if let Err(e) = update_value_with_delta(&mut sas.utxo_value, stake_delta.delta) {
            error!("Applying delta to stake hash {}: {e}", hex::encode(hash));
        }
    }

    /// Withdraw
    pub fn process_withdrawal(&mut self, withdrawal: &Withdrawal) {
        let hash = withdrawal.address.get_hash();

        if let Some(sas) = self.get(hash) {
            // Zero withdrawals are expected, as a way to validate stake addresses (per Pi)
            if withdrawal.value != 0 {
                let mut sas = sas.clone();
                if let Err(e) =
                    update_value_with_delta(&mut sas.rewards, -(withdrawal.value as i64))
                {
                    error!(
                        "Withdrawing from stake address {} hash {}: {e}",
                        withdrawal.address.to_string().unwrap_or("???".to_string()),
                        hex::encode(hash)
                    );
                } else {
                    // Update the stake address
                    self.insert(hash.to_vec(), sas);
                }
            }
        } else {
            error!(
                "Unknown stake address in withdrawal: {}",
                withdrawal.address.to_string().unwrap_or("???".to_string())
            );
        }
    }

    /// Update reward with delta
    pub fn update_reward(&mut self, account: &KeyHash, delta: i64) -> Result<()> {
        let sas = self.entry(account.clone()).or_default();
        update_value_with_delta(&mut sas.rewards, delta)
    }

    /// Update utxo value with delta
    pub fn update_utxo_value(&mut self, account: &KeyHash, delta: i64) -> Result<()> {
        let sas = self.entry(account.clone()).or_default();
        update_value_with_delta(&mut sas.utxo_value, delta)
    }
}

#[cfg(test)]
mod tests {
    use crate::{AddressNetwork, StakeAddress, StakeAddressPayload};

    use super::*;

    const STAKE_KEY_HASH: [u8; 3] = [0x99, 0x0f, 0x00];
    const SPO_HASH: [u8; 4] = [0x01, 0x02, 0x03, 0x04];
    const DREP_HASH: [u8; 4] = [0xca, 0xfe, 0xd0, 0x0d];

    fn create_address(hash: &[u8]) -> StakeAddress {
        StakeAddress {
            network: AddressNetwork::Main,
            payload: StakeAddressPayload::StakeKeyHash(hash.to_vec()),
        }
    }

    #[test]
    fn test_stake_delta() {
        let mut stake_addresses = StakeAddressMap::new();

        // Register first
        stake_addresses
            .register_stake_address(&StakeCredential::AddrKeyHash(STAKE_KEY_HASH.to_vec()));
        assert_eq!(stake_addresses.len(), 1);

        // Pass in deltas
        let delta = StakeAddressDelta {
            address: create_address(&STAKE_KEY_HASH),
            delta: 42,
        };
        stake_addresses.process_stake_delta(&delta);
        assert_eq!(
            stake_addresses.get(&STAKE_KEY_HASH.to_vec()).unwrap().utxo_value,
            42
        );
        stake_addresses.process_stake_delta(&delta);
        assert_eq!(
            stake_addresses.get(&STAKE_KEY_HASH.to_vec()).unwrap().utxo_value,
            84
        );
    }

    #[test]
    fn test_stake_delta_and_reward() {
        let mut stake_addresses = StakeAddressMap::new();

        // Register first
        stake_addresses
            .register_stake_address(&StakeCredential::AddrKeyHash(STAKE_KEY_HASH.to_vec()));
        assert_eq!(stake_addresses.len(), 1);

        // Pass in deltas
        let delta = StakeAddressDelta {
            address: create_address(&STAKE_KEY_HASH),
            delta: 42,
        };
        stake_addresses.process_stake_delta(&delta);
        assert_eq!(
            stake_addresses.get(&STAKE_KEY_HASH.to_vec()).unwrap().utxo_value,
            42
        );

        // Reward
        stake_addresses.add_to_reward(&STAKE_KEY_HASH.to_vec(), 12);
        assert_eq!(
            stake_addresses.get(&STAKE_KEY_HASH.to_vec()).unwrap().rewards,
            12
        );
    }

    #[test]
    fn test_withdrawal() {
        let mut stake_addresses = StakeAddressMap::new();

        // Register first
        stake_addresses
            .register_stake_address(&StakeCredential::AddrKeyHash(STAKE_KEY_HASH.to_vec()));
        assert_eq!(stake_addresses.len(), 1);

        // Reward
        stake_addresses.add_to_reward(&STAKE_KEY_HASH.to_vec(), 12);
        assert_eq!(
            stake_addresses.get(&STAKE_KEY_HASH.to_vec()).unwrap().rewards,
            12
        );

        // Withdraw more than reward
        let withdrawal = Withdrawal {
            address: create_address(&STAKE_KEY_HASH),
            value: 24,
        };
        stake_addresses.process_withdrawal(&withdrawal);
        assert_eq!(
            stake_addresses.get(&STAKE_KEY_HASH.to_vec()).unwrap().rewards,
            12
        );

        // Withdraw less than reward
        let withdrawal = Withdrawal {
            address: create_address(&STAKE_KEY_HASH),
            value: 2,
        };
        stake_addresses.process_withdrawal(&withdrawal);
        assert_eq!(
            stake_addresses.get(&STAKE_KEY_HASH.to_vec()).unwrap().rewards,
            10
        );
    }

    #[test]
    fn test_certs() {
        let mut stake_addresses = StakeAddressMap::new();
        let stake_credential = StakeCredential::AddrKeyHash(STAKE_KEY_HASH.to_vec());

        // Register first
        stake_addresses.register_stake_address(&stake_credential);
        assert_eq!(stake_addresses.len(), 1);

        // Stake delegation
        stake_addresses.record_stake_delegation(&stake_credential, &SPO_HASH.to_vec());
        assert_eq!(
            stake_addresses.get(&STAKE_KEY_HASH.to_vec()).unwrap().delegated_spo,
            Some(SPO_HASH.to_vec())
        );

        // Drep delegation
        let drep_choice = DRepChoice::Key(DREP_HASH.to_vec());
        stake_addresses.record_drep_delegation(&stake_credential, &drep_choice);
        assert_eq!(
            stake_addresses.get(&STAKE_KEY_HASH.to_vec()).unwrap().delegated_drep,
            Some(drep_choice)
        );

        // Deregister
        stake_addresses.deregister_stake_address(&stake_credential);
        assert_eq!(
            stake_addresses.get(&STAKE_KEY_HASH.to_vec()).unwrap().registered,
            false
        );
    }
}
