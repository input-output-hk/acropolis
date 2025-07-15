//! Acropolis AccountsState: State storage
use acropolis_common::{
    messages::{
        DRepStateMessage, EpochActivityMessage, PotDeltasMessage, ProtocolParamsMessage,
        SPOStateMessage, StakeAddressDeltasMessage, TxCertificatesMessage, WithdrawalsMessage,
    },
    rational_number::RationalNumber,
    DelegatedStake,
    DRepChoice, DRepCredential, InstantaneousRewardSource, InstantaneousRewardTarget, KeyHash,
    Lovelace, MoveInstantaneousReward, PoolRegistration, Pot, ProtocolParams, RewardAccount,
    StakeAddress, StakeCredential, TxCertificate,
};
use crate::rewards::StakeSnapshot;
use anyhow::{bail, anyhow, Result};
use dashmap::DashMap;
use imbl::OrdMap;
use std::collections::{HashMap, BTreeMap};
use rayon::prelude::*;
use serde_with::{hex::Hex, serde_as};
use std::sync::{atomic::AtomicU64, Arc, Mutex};
use tracing::{debug, error, info, warn};
use bigdecimal::{BigDecimal, ToPrimitive, Zero, One};
use std::cmp::min;

const DEFAULT_KEY_DEPOSIT: u64 = 2_000_000;
const DEFAULT_POOL_DEPOSIT: u64 = 500_000_000;

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

#[derive(Default, Debug, PartialEq, Eq)]
pub struct DRepDelegationDistribution {
    pub abstain: Lovelace,
    pub no_confidence: Lovelace,
    pub dreps: Vec<(DRepCredential, Lovelace)>,
}

/// Global 'pot' account state
#[derive(Debug, Default, Clone, serde::Serialize)]
pub struct Pots {
    /// Unallocated reserves
    reserves: Lovelace,

    /// Treasury
    treasury: Lovelace,

    /// Deposits
    deposits: Lovelace,
}

/// Overall state - stored per block
#[derive(Debug, Default, Clone)]
pub struct State {
    /// Epoch this state is for
    epoch: u64,

    /// Map of active SPOs by operator ID
    spos: OrdMap<KeyHash, PoolRegistration>,

    /// Map of staking address values
    /// Wrapped in an Arc so it doesn't get cloned in full by StateHistory
    stake_addresses: Arc<Mutex<HashMap<KeyHash, StakeAddressState>>>,

    /// Snapshots of stake taken at (epoch-2) and (epoch-1)
    /// Arcs because we don't want them copied by StateHistory
    previous_snapshot: Arc<StakeSnapshot>,
    last_snapshot: Arc<StakeSnapshot>,

    /// Global account pots
    pots: Pots,

    /// All registered DReps
    dreps: Vec<(DRepCredential, Lovelace)>,

    /// Protocol parameters that apply during this epoch
    protocol_parameters: Option<ProtocolParams>,
}

impl State {
    /// Get the stake address state for a give stake key
    pub fn get_stake_state(&self, stake_key: &KeyHash) -> Option<StakeAddressState> {
        self.stake_addresses.lock().unwrap().get(stake_key).cloned()
    }

    /// Get the current pot balances
    pub fn get_pots(&self) -> Pots {
        self.pots.clone()
    }

    /// Log statistics
    fn log_stats(&self) {
        info!(num_stake_addresses = self.stake_addresses.lock().unwrap().keys().len(),);
    }

    /// Background tick
    pub async fn tick(&self) -> Result<()> {
        self.log_stats();
        Ok(())
    }

    /// Calculate rewards given
    ///   total_fees: Total fees taken in this epoch
    ///   spo_block_counts: Count of blocks minted by VRF key
    // Follows the general scheme in https://docs.cardano.org/about-cardano/learn/pledging-rewards
    pub fn calculate_rewards(&mut self, epoch: u64, total_fees: u64,
                             spo_block_counts: HashMap<KeyHash, usize>) -> Result<()> {

        // Get Shelley parameters, silently return if too early in the chain so no
        // rewards to calculate
        let shelley_params = match &self.protocol_parameters {
            Some(ProtocolParams { shelley: Some(sp), .. }) => sp,
            _ => return Ok(())
        };

        // For each pool, calculate the total stake, including its own and
        // from other stake addresses
        let spdd = self.generate_spdd();

        // Calculate total supply (total in circulation + treasury) or
        // equivalently max-supply - reserves - this is the denominator
        // for sigma, z0, s
        // TODO - do we calculate this before or after reducing reserves?
        let total_supply = BigDecimal::from(shelley_params.max_lovelace_supply - self.pots.reserves);

        // Handle monetary expansion - movement from reserves to rewards and treasury
        let monetary_expansion_factor = &shelley_params.protocol_params.monetary_expansion; // Rho
        let monetary_expansion = (BigDecimal::from(self.pots.reserves)
                                  * BigDecimal::from(monetary_expansion_factor.numer())
                                  / BigDecimal::from(monetary_expansion_factor.denom()))
            .with_scale(0);
        self.pots.reserves -= monetary_expansion.to_u64()
            .ok_or(anyhow!("Can't calculate integral monetary expansion"))?;

        // Top-slice some for treasury
        let treasury_cut = &shelley_params.protocol_params.treasury_cut;  // Tau
        let treasury_increase = (&monetary_expansion
                                 * BigDecimal::from(treasury_cut.numer())
                                 / BigDecimal::from(treasury_cut.denom()))
            .with_scale(0);
        self.pots.treasury += treasury_increase.to_u64()
            .ok_or(anyhow!("Can't calculate integral treasury cut"))?;

        // Calculate the total rewards available (R) - fees + monetary expansion left over
        // after treasury cut
        let total_rewards = BigDecimal::from(total_fees) + monetary_expansion.clone()
            - treasury_increase.clone();

        // Total blocks
        let total_blocks: usize = spo_block_counts.values().sum();
        if total_blocks == 0 {
            bail!("No blocks produced");
        }

        info!(epoch, %monetary_expansion, %treasury_increase, %total_rewards, %total_supply,
              total_blocks, "Reward calculations");

        // Relative pool saturation size (z0)
        let k = BigDecimal::from(&shelley_params.protocol_params.stake_pool_target_num);
        if k.is_zero() {
            bail!("k is zero!");
        }
        let relative_pool_saturation_size = k.inverse();

        // Pledge influence factor (a0)
        let a0 = &shelley_params.protocol_params.pool_pledge_influence;
        let pledge_influence_factor = BigDecimal::from(a0.numer()) / BigDecimal::from(a0.denom());

        // Map of SPO reward account to amount earned this epoch
        // Note: Accumulated then spent to avoid borrow horrors on self
        let mut spo_earnings: HashMap<RewardAccount, Lovelace> = HashMap::new();

        // Map of SPO operator ID to total stake and rewards to split to delegators (not
        // including the SPO itself, which has already been taken)
        let mut spo_stake_and_rewards: HashMap<KeyHash, (Lovelace, Lovelace)> = HashMap::new();

        // Calculate for every registered SPO (even those who didn't participate in this epoch)
        for spo in self.spos.values() {

            // Look up SPO in block counts, by VRF key
            let block_count = spo_block_counts.get(&spo.vrf_key_hash).unwrap_or(&0);

            // Actual blocks produced as proportion of epoch (Beta)
            let relative_blocks = BigDecimal::from(*block_count as u64)
                / BigDecimal::from(total_blocks as u64);

            // and in SPDD to get active stake (sigma)
            let pool_stake_u64 = spdd.get(&spo.operator).map(|ds| ds.active).unwrap_or(0);
            if pool_stake_u64 == 0 {
                error!("No pool stake in SPO {}", hex::encode(&spo.operator));
                continue;
            }
            let pool_stake = BigDecimal::from(pool_stake_u64);

            // TODO!  We need to look at owners and find the actual pledge, not just
            // the declared amount
            let mut pool_pledge = BigDecimal::from(&spo.pledge);

            // TODO! Given this we need to make sure we don't make the calculation below
            // go negative if they haven't even got enough total stake to make their pledge
            // Can't happen if we actually count owners' stake, of course
            if pool_stake < pool_pledge {
                error!("SPO {} has stake {} less than pledge {} - fenced pledge",
                       hex::encode(&spo.operator), pool_stake, pool_pledge);
                pool_pledge = pool_stake.clone();  // Fence for safety for now
            }

            // Relative stake as fraction of total supply (sigma), and capped with 1/k (sigma')
            let relative_pool_stake = &pool_stake / &total_supply;
            let capped_relative_pool_stake = min(&relative_pool_stake,
                                                 &relative_pool_saturation_size);

            // Stake pledged by operator (s) and capped with 1/k (s')
            let relative_pool_pledge = pool_pledge / &total_supply;
            let capped_relative_pool_pledge = min(&relative_pool_pledge,
                                                  &relative_pool_saturation_size);

            // Get the optimum reward for this pool
            let optimum_rewards = (
                (&total_rewards / (BigDecimal::one() + &pledge_influence_factor))
                *
                (
                    capped_relative_pool_stake + (
                        capped_relative_pool_pledge * &pledge_influence_factor * (
                            capped_relative_pool_stake - (
                                capped_relative_pool_pledge * (
                                    (&relative_pool_saturation_size - capped_relative_pool_stake)
                                        / &relative_pool_saturation_size)
                            )
                        )
                    ) / &relative_pool_saturation_size
                )
            ).with_scale(0);

            // If decentralisation_param >= 0.8 => performance = 1
            // Shelley Delegation Spec 3.8.3
            let decentralisation = &shelley_params.protocol_params.decentralisation_param;
            let pool_performance = if decentralisation >= &RationalNumber::new(8,10) {
                BigDecimal::one()
            } else {
                relative_blocks.clone() / relative_pool_stake.clone()
            };

            // Get actual pool rewards
            let pool_rewards = (&optimum_rewards * &pool_performance).with_scale(0);

            debug!(%block_count, %pool_stake, %relative_pool_stake, %relative_blocks,
                  %pool_performance, %optimum_rewards, %pool_rewards,
                   "Pool {}", hex::encode(spo.operator.clone()));

            // Subtract fixed costs
            let fixed_cost = BigDecimal::from(spo.cost);
            if pool_rewards <= fixed_cost {
                // No margin or pledge reward if under cost - all goes to SPO
                spo_earnings.insert(spo.reward_account.clone(),
                                    pool_rewards.to_u64().unwrap_or_else(|| {
                                        error!("Non-integral pool rewards {} for SPO {}",
                                               pool_rewards, hex::encode(&spo.operator));
                                        0
                                    }));
            } else {
                // Enough left over for some margin split
                let margin = ((&pool_rewards - &fixed_cost)
                              * BigDecimal::from(spo.margin.numerator)  // TODO use RationalNumber
                              / BigDecimal::from(spo.margin.denominator))
                    .with_scale(0);
                let costs = fixed_cost + margin;
                let remainder = pool_rewards - &costs;

                // TODO: Double check this against ledger spec p.61

                // Calculate the SPOs reward from their own pledge, too
                let pledge_reward = (&remainder * BigDecimal::from(spo.pledge) / pool_stake)
                    .with_scale(0);
                let spo_benefit = (costs + &pledge_reward)
                    .to_u64()
                    .ok_or(anyhow!("Non-integral costs"))?;
                spo_earnings.insert(spo.reward_account.clone(), spo_benefit);

                // Keep remainder by SPO id
                let to_delegators = (&remainder - &pledge_reward).to_u64().unwrap_or_else(|| {
                    error!("Non-integral remainder {remainder} or pledge_reward {pledge_reward}");
                    0
                });

                if to_delegators > 0 {
                    spo_stake_and_rewards.insert(spo.operator.clone(),
                                                 (pool_stake_u64, to_delegators));
                }
            }
        }

        // Pay the SPOs from reserves
        spo_earnings.into_iter().for_each(|(reward_account, reward)| {
            self.add_to_reward(&reward_account, reward);
            self.pots.reserves -= reward;
        });

        // Capture a new snapshot
        let new_snapshot = StakeSnapshot::new(&self.stake_addresses.lock().unwrap());

        // Pay the delegators - split remainder in proportional to delegated stake,
        // * as it was 2 epochs ago *
        // TODO: Although these are calculated now, they are *paid* at the next epoch
        self.previous_snapshot.clone().spos.iter().for_each(|(spo_id, delegators)| {
            // Look up the SPO in the rewards map
            // May be absent if they didn't meet their costs
            if let Some((total_stake, rewards)) = spo_stake_and_rewards.get(spo_id) {
                for (hash, stake) in delegators {
                    let proportion = BigDecimal::from(stake) / BigDecimal::from(total_stake);

                    // and hence how much of the total reward they get
                    let reward = BigDecimal::from(rewards) * &proportion;
                    let to_pay = reward.with_scale(0).to_u64().unwrap_or(0);

                    debug!("Reward stake {stake} -> proportion {proportion} of SPO rewards {rewards} -> {to_pay} to hash {}",
                           hex::encode(&hash));

                    // Transfer from reserves to this account
                    self.add_to_reward(&hash, to_pay);
                    self.pots.reserves -= to_pay;
                }
            }
        });

        // Rotate the snapshots
        self.previous_snapshot = self.last_snapshot.clone();
        self.last_snapshot = Arc::new(new_snapshot);

        Ok(())
    }

    /// Add a reward to a reward account (by hash)
    fn add_to_reward(&mut self, account: &KeyHash, amount: Lovelace) {
        // Get old stake address state, or create one
        let mut stake_addresses = self.stake_addresses.lock().unwrap();

        // Get or create account entry, avoiding clone when existing
        let sas = match stake_addresses.get_mut(account) {
            Some(existing) => existing,
            None => {
                stake_addresses.insert(account.clone(), StakeAddressState::default());
                stake_addresses.get_mut(account).unwrap()
            }
        };

        if let Err(e) = Self::update_value_with_delta(&mut sas.rewards, amount as i64) {
            error!("Adding to reward account {}: {e}", hex::encode(account));
        }
    }

    /// Derive the Stake Pool Delegation Distribution (SPDD) - a map of total stake values
    /// (both with and without rewards) for each active SPO
    /// Key of returned map is the SPO 'operator' ID
    pub fn generate_spdd(&self) -> BTreeMap<KeyHash, DelegatedStake> {
        // Shareable Dashmap with referenced keys
        let spo_stakes = Arc::new(DashMap::<KeyHash, DelegatedStake>::new());

        // Total stake across all addresses in parallel, first collecting into a vector
        // because imbl::OrdMap doesn't work in Rayon
        let stake_addresses = self.stake_addresses.lock().unwrap();

        // Collect the SPO keys and UTXO, reward values
        let sas_data: Vec<(KeyHash, (u64, u64))> = stake_addresses
            .values()
            .filter_map(|sas| {
                sas.delegated_spo.as_ref()
                    .map(|spo| (spo.clone(), (sas.utxo_value, sas.rewards)))
            })
            .collect();

        // Parallel sum all the stakes into the spo_stake map
        sas_data
            .par_iter() // Rayon multi-threaded iterator
            .for_each_init(
                || Arc::clone(&spo_stakes),
                |map, (spo, (utxo_value, rewards))| {
                    map.entry(spo.clone()).and_modify(|v| {
                        v.active += *utxo_value;
                        v.live += *utxo_value + *rewards;
                    }).or_insert(DelegatedStake {
                        active: *utxo_value,
                        live: *utxo_value + *rewards
                    });
                },
            );

        // Collect into a plain BTreeMap, so that it is ordered on output
        spo_stakes.iter().map(|entry| (entry.key().clone(), entry.value().clone())).collect()
    }

    /// Derive the DRep Delegation Distribution (SPDD) - the total amount
    /// delegated to each DRep, including the special "abstain" and "no confidence" dreps.
    pub fn generate_drdd(&self) -> DRepDelegationDistribution {
        let abstain = AtomicU64::new(0);
        let no_confidence = AtomicU64::new(0);
        let dreps = self
            .dreps
            .iter()
            .map(|(cred, deposit)| (cred.clone(), AtomicU64::new(*deposit)))
            .collect::<BTreeMap<_, _>>();
        self.stake_addresses
            .lock()
            .unwrap()
            .values()
            .collect::<Vec<_>>()
            .par_iter()
            .for_each(|state| {
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

    /// Handle an ProtocolParamsMessage with the latest parameters at the start of a new
    /// epoch
    pub fn handle_parameters(&mut self, params_msg: &ProtocolParamsMessage) -> Result<()> {
        self.protocol_parameters = Some(params_msg.params.clone());
        info!("New parameter set: {:?}", self.protocol_parameters);
        Ok(())
    }

    /// Handle an EpochActivityMessage giving total fees and block counts by VRF key for
    /// the just-ended epoch
    pub fn handle_epoch_activity(&mut self, ea_msg: &EpochActivityMessage) -> Result<()> {
        self.epoch = ea_msg.epoch;

        // Create a HashMap of the spo count data, for quick access
        let spo_block_counts: HashMap<KeyHash, usize> =
            ea_msg.vrf_vkey_hashes.iter().map(|(k, v)| (k.clone(), *v)).collect();
        self.calculate_rewards(self.epoch, ea_msg.total_fees, spo_block_counts)?;
        Ok(())
    }

    /// Handle an SPOStateMessage with the full set of SPOs valid at the end of the last
    /// epoch
    pub fn handle_spo_state(&mut self, spo_msg: &SPOStateMessage) -> Result<()> {

        // Capture current SPOs, mapped by operator ID
        let new_spos: OrdMap<_, _> = spo_msg
            .spos
            .iter()
            .cloned()
            .map(|spo| (spo.operator.clone(), spo))
            .collect();

        // Get pool deposit amount from parameters, or default
        let deposit = self.protocol_parameters
            .as_ref()
            .and_then(|pp| pp.shelley.as_ref())
            .map(|sp| sp.protocol_params.pool_deposit)
            .unwrap_or(DEFAULT_POOL_DEPOSIT);

        // Check for how many new SPOs
        let new_count = new_spos
            .keys()
            .filter(|id| !self.spos.contains_key(*id))
            .count();

        // They've each paid their deposit, so increment that (the UTXO spend is taken
        // care of in UTXOState)
        let total_deposits = (new_count as u64) * deposit;
        self.pots.deposits += total_deposits;
        info!("{new_count} new SPOs, total new deposits {total_deposits}");

        // Check for any SPOs that have retired and need deposit refunds
        let mut refunds: Vec<KeyHash> = Vec::new();
        for (id, old_spo) in self.spos.iter() {
            if !new_spos.contains_key(id) {
                match StakeAddress::from_binary(&old_spo.reward_account) {
                    Ok(stake_address) => {
                        let keyhash = stake_address.get_hash();
                        info!("SPO {} has retired - refunding their deposit to {}",
                              hex::encode(id), hex::encode(keyhash));
                        refunds.push(keyhash.to_vec());
                    }
                    Err(e) => error!("Error repaying SPO deposit: {e}")
                }
            }
        }

        // TODO - if their reward account has been deregistered, it goes to Treasury
        // TODO - wipe any delegations to retired pools

        // Send them their deposits back
        for keyhash in refunds.iter() {
            self.add_to_reward(&keyhash, deposit);
            self.pots.deposits -= deposit;
        }

        self.spos = new_spos;
        Ok(())
    }

    /// Register a stake address, with specified deposit if known
    fn register_stake_address(&mut self, credential: &StakeCredential,
                              deposit: Option<Lovelace>) {
        let hash = credential.get_hash();

        // Stake addresses can be registered after being used in UTXOs
        let mut stake_addresses = self.stake_addresses.lock().unwrap();
        let sas = stake_addresses.entry(hash.clone()).or_default();
        if sas.registered {
            error!("Stake address hash {} registered when already registered", hex::encode(&hash));
        } else {
            sas.registered = true;

            // Account for the deposit
            let deposit = match deposit {
                Some(deposit) => deposit,
                None => {
                    // Get stake deposit amount from parameters, or default
                    self.protocol_parameters
                        .as_ref()
                        .and_then(|pp| pp.shelley.as_ref())
                        .map(|sp| sp.protocol_params.key_deposit)
                        .unwrap_or(DEFAULT_KEY_DEPOSIT)
                }
            };

            self.pots.deposits += deposit;
        }
    }

    /// Deregister a stake address, with specified refund if known
    fn deregister_stake_address(&mut self, credential: &StakeCredential,
                                refund: Option<Lovelace>) {
        let hash = credential.get_hash();

        // Check if it existed
        let mut stake_addresses = self.stake_addresses.lock().unwrap();
        if let Some(sas) = stake_addresses.get_mut(&hash) {

            if sas.registered {
                // Account for the deposit, if registered before
                let deposit = match refund {
                    Some(deposit) => deposit,
                    None => {
                        // Get stake deposit amount from parameters, or default
                        self.protocol_parameters
                            .as_ref()
                            .and_then(|pp| pp.shelley.as_ref())
                            .map(|sp| sp.protocol_params.key_deposit)
                            .unwrap_or(DEFAULT_KEY_DEPOSIT)
                    }
                };
                self.pots.deposits -= deposit;
                sas.registered = false;
            } else {
                error!("Deregistration of unregistered stake address hash {}", hex::encode(hash));
            }
        } else {
            error!("Deregistration of unknown stake address hash {}", hex::encode(hash));
        }
    }

    pub fn handle_drep_state(&mut self, drep_msg: &DRepStateMessage) {
        self.dreps = drep_msg.dreps.clone();
    }

    /// Record a stake delegation
    fn record_stake_delegation(&mut self, credential: &StakeCredential, spo: &KeyHash) {
        let hash = credential.get_hash();

        // Get old stake address state, or create one
        let mut stake_addresses = self.stake_addresses.lock().unwrap();
        if let Some(sas) = stake_addresses.get_mut(&hash) {
            if sas.registered {
                sas.delegated_spo = Some(spo.clone());
            } else {
                error!("Unregistered stake address in stake delegation: {}", hex::encode(hash));
            }
        } else {
            error!("Unknown stake address in stake delegation: {}", hex::encode(hash));
        }
    }

    /// Handle an MoveInstantaneousReward (pre-Conway only)
    pub fn handle_mir(&mut self, mir: &MoveInstantaneousReward) -> Result<()> {
        let (source, source_name, other, other_name) = match &mir.source {
            InstantaneousRewardSource::Reserves => (
                &mut self.pots.reserves,
                "reserves",
                &mut self.pots.treasury,
                "treasury",
            ),
            InstantaneousRewardSource::Treasury => (
                &mut self.pots.treasury,
                "treasury",
                &mut self.pots.reserves,
                "reserves",
            ),
        };

        match &mir.target {
            InstantaneousRewardTarget::StakeCredentials(deltas) => {
                // Transfer to (in theory also from) stake addresses from (to) a pot
                for (credential, value) in deltas.iter() {
                    let hash = credential.get_hash();

                    // Get old stake address state, or create one
                    let mut stake_addresses = self.stake_addresses.lock().unwrap();
                    let sas = stake_addresses.entry(hash.clone()).or_default();

                    // Add to this one
                    if let Err(e) = Self::update_value_with_delta(&mut sas.rewards, *value) {
                        error!("MIR to stake hash {}: {e}", hex::encode(hash));
                    }

                    // Update the source
                    if let Err(e) = Self::update_value_with_delta(source, -*value) {
                        error!("MIR from {source_name}: {e}");
                    }
                }
            }

            InstantaneousRewardTarget::OtherAccountingPot(value) => {
                // Transfer between pots
                if let Err(e) = Self::update_value_with_delta(source, -(*value as i64)) {
                    error!("MIR from {source_name}: {e}");
                }
                if let Err(e) = Self::update_value_with_delta(other, *value as i64) {
                    error!("MIR to {other_name}: {e}");
                }
            }
        }

        Ok(())
    }

    /// Update an unsigned value with a signed delta, with fences
    pub fn update_value_with_delta(value: &mut u64, delta: i64) -> Result<()> {
        if delta >= 0 {
            *value = (*value).saturating_add(delta as u64);
        } else {
            let abs = (-delta) as u64;
            if abs > *value {
                bail!("Value underflow - was {}, delta {}", *value, delta);
            } else {
                *value -= abs;
            }
        }

        Ok(())
    }

    /// record a drep delegation
    fn record_drep_delegation(&mut self, credential: &StakeCredential, drep: &DRepChoice) {
        let hash = credential.get_hash();
        let mut stake_addresses = self.stake_addresses.lock().unwrap();
        if let Some(sas) = stake_addresses.get_mut(&hash) {
            if sas.registered {
                sas.delegated_drep = Some(drep.clone());
            } else {
                error!("Unregistered stake address in DRep delegation: {}", hex::encode(hash));
            }
        } else {
            error!("Unknown stake address in stake delegation: {}", hex::encode(hash));
        }
    }

    /// Handle TxCertificates
    pub fn handle_tx_certificates(&mut self, tx_certs_msg: &TxCertificatesMessage) -> Result<()> {
        // Handle certificates
        for tx_cert in tx_certs_msg.certificates.iter() {
            match tx_cert {
                TxCertificate::StakeRegistration(sc_with_pos) => {
                    self.register_stake_address(&sc_with_pos.stake_credential, None);
                }

                TxCertificate::StakeDeregistration(sc) => {
                    self.deregister_stake_address(&sc, None);
                }

                TxCertificate::MoveInstantaneousReward(mir) => {
                    self.handle_mir(&mir).unwrap_or_else(|e| error!("MIR failed: {e:#}"));
                }

                TxCertificate::Registration(reg) => {
                    self.register_stake_address(&reg.credential, Some(reg.deposit));
                }

                TxCertificate::Deregistration(dreg) => {
                    self.deregister_stake_address(&dreg.credential, Some(dreg.refund));
                }

                TxCertificate::StakeDelegation(delegation) => {
                    self.record_stake_delegation(&delegation.credential, &delegation.operator);
                }

                TxCertificate::VoteDelegation(delegation) => {
                    self.record_drep_delegation(&delegation.credential, &delegation.drep);
                }

                TxCertificate::StakeAndVoteDelegation(delegation) => {
                    self.record_stake_delegation(&delegation.credential, &delegation.operator);
                    self.record_drep_delegation(&delegation.credential, &delegation.drep);
                }

                TxCertificate::StakeRegistrationAndDelegation(delegation) => {
                    self.register_stake_address(&delegation.credential, Some(delegation.deposit));
                    self.record_stake_delegation(&delegation.credential, &delegation.operator);
                }

                TxCertificate::StakeRegistrationAndVoteDelegation(delegation) => {
                    self.register_stake_address(&delegation.credential, Some(delegation.deposit));
                    self.record_drep_delegation(&delegation.credential, &delegation.drep);
                }

                TxCertificate::StakeRegistrationAndStakeAndVoteDelegation(delegation) => {
                    self.register_stake_address(&delegation.credential, Some(delegation.deposit));
                    self.record_stake_delegation(&delegation.credential, &delegation.operator);
                    self.record_drep_delegation(&delegation.credential, &delegation.drep);
                }

                _ => (),
            }
        }

        Ok(())
    }

    /// Handle withdrawals
    pub fn handle_withdrawals(&mut self, withdrawals_msg: &WithdrawalsMessage) -> Result<()> {
        for withdrawal in withdrawals_msg.withdrawals.iter() {
            let hash = withdrawal.address.get_hash();

            // Get old stake address state - which must exist
            let mut stake_addresses = self.stake_addresses.lock().unwrap();
            if let Some(sas) = stake_addresses.get(hash) {

                // Zero withdrawals are expected, as a way to validate stake addresses (per Pi)
                if withdrawal.value != 0 {
                    let mut sas = sas.clone();
                    if let Err(e) = Self::update_value_with_delta(&mut sas.rewards,
                                                                  -(withdrawal.value as i64)) {
                        error!("Withdrawing from stake address {} hash {}: {e}",
                               withdrawal.address.to_string().unwrap_or("???".to_string()),
                               hex::encode(hash));
                        continue;
                    } else {
                        // Update the stake address
                        stake_addresses.insert(hash.to_vec(), sas);
                    }
                }
            } else {
                error!("Unknown stake address in withdrawal: {}",
                       withdrawal.address.to_string().unwrap_or("???".to_string()));
            }
        }

        Ok(())
    }

    /// Handle pots
    pub fn handle_pot_deltas(&mut self, pot_deltas_msg: &PotDeltasMessage) -> Result<()> {
        for pot_delta in pot_deltas_msg.deltas.iter() {
            let pot = match pot_delta.pot {
                Pot::Reserves => &mut self.pots.reserves,
                Pot::Treasury => &mut self.pots.treasury,
                Pot::Deposits => &mut self.pots.deposits,
            };

            if let Err(e) = Self::update_value_with_delta(pot, pot_delta.delta) {
                error!("Applying pot delta {pot_delta:?}: {e}");
            } else {
                info!("Pot delta for {:?} {} => {}", pot_delta.pot, pot_delta.delta, *pot);
            }
        }

        Ok(())
    }

    /// Handle stake deltas
    pub fn handle_stake_deltas(&mut self, deltas_msg: &StakeAddressDeltasMessage) -> Result<()> {
        // Handle deltas
        for delta in deltas_msg.deltas.iter() {
            // Fold both stake key and script hashes into one - assuming the chance of
            // collision is negligible
            let hash = delta.address.get_hash();

            // Stake addresses don't need to be registered if they aren't used for
            // stake or drep delegation, but we need to track them in case they are later
            let mut stake_addresses = self.stake_addresses.lock().unwrap();
            let sas = stake_addresses.entry(hash.to_vec()).or_default();

            if let Err(e) = Self::update_value_with_delta(&mut sas.utxo_value, delta.delta) {
                error!("Applying delta to stake hash {}: {e}", hex::encode(hash));
            }
        }

        Ok(())
    }
}

// -- Tests --
#[cfg(test)]
mod tests {
    use super::*;
    use acropolis_common::{
        rational_number::RationalNumber, AddressNetwork, Anchor, Committee, Constitution,
        ConwayParams, Credential, DRepVotingThresholds, PoolVotingThresholds, Pot, PotDelta,
        ProtocolParams, Ratio, Registration, StakeAddress, StakeAddressDelta, StakeAddressPayload,
        StakeAndVoteDelegation, StakeRegistrationAndStakeAndVoteDelegation,
        StakeRegistrationAndVoteDelegation, VoteDelegation, Withdrawal,
    };

    const STAKE_KEY_HASH: [u8; 3] = [0x99, 0x0f, 0x00];
    const DREP_HASH: [u8; 4] = [0xca, 0xfe, 0xd0, 0x0d];

    fn create_address(hash: &[u8]) -> StakeAddress {
        StakeAddress {
            network: AddressNetwork::Main,
            payload: StakeAddressPayload::StakeKeyHash(hash.to_vec()),
        }
    }

    #[test]
    fn stake_addresses_initialise_to_first_delta_and_increment_subsequently() {
        let mut state = State::default();

        // Register first
        state.register_stake_address(&StakeCredential::AddrKeyHash(STAKE_KEY_HASH.to_vec()), None);

        {
            let stake_addresses = state.stake_addresses.lock().unwrap();
            assert_eq!(stake_addresses.len(), 1);
        }

        // Pass in deltas
        let msg = StakeAddressDeltasMessage {
            deltas: vec![StakeAddressDelta {
                address: create_address(&STAKE_KEY_HASH),
                delta: 42,
            }],
        };

        state.handle_stake_deltas(&msg).unwrap();

        {
            let stake_addresses = state.stake_addresses.lock().unwrap();
            assert_eq!(
                stake_addresses.get(&STAKE_KEY_HASH.to_vec()).unwrap().utxo_value,
                42
            );
        }

        state.handle_stake_deltas(&msg).unwrap();

        {
            let stake_addresses = state.stake_addresses.lock().unwrap();
            assert_eq!(
                stake_addresses.get(&STAKE_KEY_HASH.to_vec()).unwrap().utxo_value,
                84
            );
        }
    }

    #[test]
    fn spdd_is_empty_at_start() {
        let state = State::default();
        let spdd = state.generate_spdd();
        assert!(spdd.is_empty());
    }

    // TODO! Misnomer - pledge is not specifically included in spdd because it is handled
    // by the owner's own staking.  What does need to be tested is the difference between
    // 'active' and 'live' by adding rewards
    #[test]
    fn spdd_from_delegation_with_utxo_values_and_pledge() {
        let mut state = State::default();

        let spo1: KeyHash = vec![0x01];
        let spo2: KeyHash = vec![0x02];

        // Create the SPOs
        state.handle_spo_state(&SPOStateMessage {
            epoch: 1,
            spos: vec![
                PoolRegistration {
                    operator: spo1.clone(),
                    vrf_key_hash: spo1.clone(),
                    pledge: 26,
                    cost: 0,
                    margin: Ratio { numerator: 1, denominator: 20 },
                    reward_account: Vec::new(),
                    pool_owners: Vec::new(),
                    relays: Vec::new(),
                    pool_metadata: None
                },
                PoolRegistration {
                    operator: spo2.clone(),
                    vrf_key_hash: spo2.clone(),
                    pledge: 47,
                    cost: 10,
                    margin: Ratio { numerator: 1, denominator: 10 },
                    reward_account: Vec::new(),
                    pool_owners: Vec::new(),
                    relays: Vec::new(),
                    pool_metadata: None
                },
            ],
        }).unwrap();

        // Delegate
        let addr1: KeyHash = vec![0x11];
        let cred1 = Credential::AddrKeyHash(addr1.clone());
        state.register_stake_address(&cred1, None);
        state.record_stake_delegation(&cred1, &spo1);

        let addr2: KeyHash = vec![0x12];
        let cred2 = Credential::AddrKeyHash(addr2.clone());
        state.register_stake_address(&cred2, None);
        state.record_stake_delegation(&cred2, &spo2);

        // Put some value in
        let msg1 = StakeAddressDeltasMessage {
            deltas: vec![StakeAddressDelta {
                address: create_address(&addr1),
                delta: 42,
            }],
        };

        state.handle_stake_deltas(&msg1).unwrap();

        let msg2 = StakeAddressDeltasMessage {
            deltas: vec![StakeAddressDelta {
                address: create_address(&addr2),
                delta: 21,
            }],
        };

        state.handle_stake_deltas(&msg2).unwrap();

        // Get the SPDD
        let spdd = state.generate_spdd();
        assert_eq!(spdd.len(), 2);

        let stake1 = spdd.get(&spo1).unwrap();
        assert_eq!(stake1.active, 42);
        let stake2 = spdd.get(&spo2).unwrap();
        assert_eq!(stake2.active, 21);
    }

    #[test]
    fn pots_are_zero_at_start() {
        let state = State::default();
        assert_eq!(state.pots.reserves, 0);
        assert_eq!(state.pots.treasury, 0);
        assert_eq!(state.pots.deposits, 0);
    }

    #[test]
    fn pot_delta_updates_pots() {
        let mut state = State::default();

        // Send in a MIR reserves->42->treasury
        let mir = PotDeltasMessage {
            deltas: vec![
                PotDelta {
                    pot: Pot::Reserves,
                    delta: 43,
                },
                PotDelta {
                    pot: Pot::Reserves,
                    delta: -1,
                },
                PotDelta {
                    pot: Pot::Treasury,
                    delta: 99,
                },
                PotDelta {
                    pot: Pot::Deposits,
                    delta: 77,
                },
            ],
        };

        state.handle_pot_deltas(&mir).unwrap();
        assert_eq!(state.pots.reserves, 42);
        assert_eq!(state.pots.treasury, 99);
        assert_eq!(state.pots.deposits, 77);
    }

    #[test]
    fn mir_transfers_between_pots() {
        let mut state = State::default();

        // Bootstrap with some in reserves
        state.pots.reserves = 100;

        // Send in a MIR reserves->42->treasury
        let mir = MoveInstantaneousReward {
            source: InstantaneousRewardSource::Reserves,
            target: InstantaneousRewardTarget::OtherAccountingPot(42),
        };

        state.handle_mir(&mir).unwrap();
        assert_eq!(state.pots.reserves, 58);
        assert_eq!(state.pots.treasury, 42);
        assert_eq!(state.pots.deposits, 0);

        // Send some of it back
        let mir = MoveInstantaneousReward {
            source: InstantaneousRewardSource::Treasury,
            target: InstantaneousRewardTarget::OtherAccountingPot(10),
        };

        state.handle_mir(&mir).unwrap();
        assert_eq!(state.pots.reserves, 68);
        assert_eq!(state.pots.treasury, 32);
        assert_eq!(state.pots.deposits, 0);
    }

    #[test]
    fn mir_transfers_to_stake_addresses() {
        let mut state = State::default();

        // Bootstrap with some in reserves
        state.pots.reserves = 100;

        // Set up one stake address
        state.register_stake_address(&StakeCredential::AddrKeyHash(STAKE_KEY_HASH.to_vec()), None);
        let msg = StakeAddressDeltasMessage {
            deltas: vec![StakeAddressDelta {
                address: create_address(&STAKE_KEY_HASH),
                delta: 99,
            }],
        };

        state.handle_stake_deltas(&msg).unwrap();

        {
            let stake_addresses = state.stake_addresses.lock().unwrap();
            assert_eq!(stake_addresses.len(), 1);
            let sas = stake_addresses.get(&STAKE_KEY_HASH.to_vec()).unwrap();
            assert_eq!(sas.utxo_value, 99);
            assert_eq!(sas.rewards, 0);
        }

        // Send in a MIR reserves->{47,-5}->stake
        let mir = MoveInstantaneousReward {
            source: InstantaneousRewardSource::Reserves,
            target: InstantaneousRewardTarget::StakeCredentials(vec![
                (Credential::AddrKeyHash(STAKE_KEY_HASH.to_vec()), 47),
                (Credential::AddrKeyHash(STAKE_KEY_HASH.to_vec()), -5),
            ]),
        };

        state.handle_mir(&mir).unwrap();
        assert_eq!(state.pots.reserves, 58);
        assert_eq!(state.pots.treasury, 0);
        assert_eq!(state.pots.deposits, 2_000_000);  // Paid deposit

        let stake_addresses = state.stake_addresses.lock().unwrap();
        let sas = stake_addresses.get(&STAKE_KEY_HASH.to_vec()).unwrap();
        assert_eq!(sas.utxo_value, 99);
        assert_eq!(sas.rewards, 42);
    }

    #[test]
    fn withdrawal_transfers_from_stake_addresses() {
        let mut state = State::default();

        // Bootstrap with some in reserves
        state.pots.reserves = 100;

        // Set up one stake address
        state.register_stake_address(&StakeCredential::AddrKeyHash(STAKE_KEY_HASH.to_vec()), None);
        let msg = StakeAddressDeltasMessage {
            deltas: vec![StakeAddressDelta {
                address: create_address(&STAKE_KEY_HASH),
                delta: 99,
            }],
        };

        state.handle_stake_deltas(&msg).unwrap();

        {
            let stake_addresses = state.stake_addresses.lock().unwrap();
            assert_eq!(stake_addresses.len(), 1);

            let sas = stake_addresses.get(&STAKE_KEY_HASH.to_vec()).unwrap();
            assert_eq!(sas.utxo_value, 99);
            assert_eq!(sas.rewards, 0);
        }

        // Send in a MIR reserves->42->stake
        let mir = MoveInstantaneousReward {
            source: InstantaneousRewardSource::Reserves,
            target: InstantaneousRewardTarget::StakeCredentials(vec![(
                Credential::AddrKeyHash(STAKE_KEY_HASH.to_vec()),
                42,
            )]),
        };

        state.handle_mir(&mir).unwrap();
        assert_eq!(state.pots.reserves, 58);

        {
            let stake_addresses = state.stake_addresses.lock().unwrap();
            let sas = stake_addresses.get(&STAKE_KEY_HASH.to_vec()).unwrap();
            assert_eq!(sas.rewards, 42);
        }

        // Withdraw most of it
        let withdrawals = WithdrawalsMessage {
            withdrawals: vec![Withdrawal {
                address: create_address(&STAKE_KEY_HASH),
                value: 39,
            }],
        };

        state.handle_withdrawals(&withdrawals).unwrap();

        let stake_addresses = state.stake_addresses.lock().unwrap();
        let sas = stake_addresses.get(&STAKE_KEY_HASH.to_vec()).unwrap();
        assert_eq!(sas.rewards, 3);
    }

    #[test]
    fn drdd_is_default_from_start() {
        let state = State::default();
        let drdd = state.generate_drdd();
        assert_eq!(drdd, DRepDelegationDistribution::default());
    }

    #[test]
    fn drdd_includes_initial_deposit() {
        let mut state = State::default();

        let drep_addr_cred = DRepCredential::AddrKeyHash(DREP_HASH.to_vec());
        state.handle_drep_state(&DRepStateMessage {
            epoch: 1337,
            dreps: vec![(drep_addr_cred.clone(), 1_000_000)],
        });

        let drdd = state.generate_drdd();
        assert_eq!(
            drdd,
            DRepDelegationDistribution {
                abstain: 0,
                no_confidence: 0,
                dreps: vec![(drep_addr_cred, 1_000_000)],
            }
        );
    }

    #[test]
    fn drdd_respects_different_delegations() -> Result<()> {
        let mut state = State::default();

        let drep_addr_cred = DRepCredential::AddrKeyHash(DREP_HASH.to_vec());
        let drep_script_cred = DRepCredential::ScriptHash(DREP_HASH.to_vec());
        state.handle_drep_state(&DRepStateMessage {
            epoch: 1337,
            dreps: vec![
                (drep_addr_cred.clone(), 1_000_000),
                (drep_script_cred.clone(), 2_000_000),
            ],
        });

        let spo1 = vec![0x01];
        let spo2 = vec![0x02];
        let spo3 = vec![0x03];
        let spo4 = vec![0x04];

        let certificates = vec![
            // register the first two SPOs separately from their delegation
            TxCertificate::Registration(Registration {
                credential: Credential::AddrKeyHash(spo1.clone()),
                deposit: 1,
            }),
            TxCertificate::Registration(Registration {
                credential: Credential::AddrKeyHash(spo2.clone()),
                deposit: 1,
            }),
            TxCertificate::VoteDelegation(VoteDelegation {
                credential: Credential::AddrKeyHash(spo1.clone()),
                drep: DRepChoice::Key(DREP_HASH.to_vec()),
            }),
            TxCertificate::StakeAndVoteDelegation(StakeAndVoteDelegation {
                credential: Credential::AddrKeyHash(spo2.clone()),
                operator: spo1.clone(),
                drep: DRepChoice::Script(DREP_HASH.to_vec()),
            }),
            TxCertificate::StakeRegistrationAndVoteDelegation(StakeRegistrationAndVoteDelegation {
                credential: Credential::AddrKeyHash(spo3.clone()),
                drep: DRepChoice::Abstain,
                deposit: 1,
            }),
            TxCertificate::StakeRegistrationAndStakeAndVoteDelegation(
                StakeRegistrationAndStakeAndVoteDelegation {
                    credential: Credential::AddrKeyHash(spo4.clone()),
                    operator: spo1.clone(),
                    drep: DRepChoice::NoConfidence,
                    deposit: 1,
                },
            ),
        ];

        state.handle_tx_certificates(&TxCertificatesMessage { certificates })?;

        let deltas = vec![
            StakeAddressDelta {
                address: create_address(&spo1),
                delta: 100,
            },
            StakeAddressDelta {
                address: create_address(&spo2),
                delta: 1_000,
            },
            StakeAddressDelta {
                address: create_address(&spo3),
                delta: 10_000,
            },
            StakeAddressDelta {
                address: create_address(&spo4),
                delta: 100_000,
            },
        ];
        state.handle_stake_deltas(&StakeAddressDeltasMessage { deltas })?;

        let drdd = state.generate_drdd();
        assert_eq!(
            drdd,
            DRepDelegationDistribution {
                abstain: 10_000,
                no_confidence: 100_000,
                dreps: vec![(drep_addr_cred, 1_000_100), (drep_script_cred, 2_001_000),],
            }
        );

        Ok(())
    }

    #[test]
    fn protocol_params_are_captured_from_message() {
        // Fake Conway parameters (a lot of work to test an assignment!)
        let params = ProtocolParams {
            conway: Some(ConwayParams {
                pool_voting_thresholds: PoolVotingThresholds {
                    motion_no_confidence: RationalNumber::ONE,
                    committee_normal: RationalNumber::ZERO,
                    committee_no_confidence: RationalNumber::ZERO,
                    hard_fork_initiation: RationalNumber::ONE,
                    security_voting_threshold: RationalNumber::ZERO,
                },
                d_rep_voting_thresholds: DRepVotingThresholds {
                    motion_no_confidence: RationalNumber::ONE,
                    committee_normal: RationalNumber::ZERO,
                    committee_no_confidence: RationalNumber::ZERO,
                    update_constitution: RationalNumber::ONE,
                    hard_fork_initiation: RationalNumber::ZERO,
                    pp_network_group: RationalNumber::ZERO,
                    pp_economic_group: RationalNumber::ZERO,
                    pp_technical_group: RationalNumber::ZERO,
                    pp_governance_group: RationalNumber::ZERO,
                    treasury_withdrawal: RationalNumber::ONE,
                },
                committee_min_size: 42,
                committee_max_term_length: 3,
                gov_action_lifetime: 99,
                gov_action_deposit: 500_000_000,
                d_rep_deposit: 100_000_000,
                d_rep_activity: 27,
                min_fee_ref_script_cost_per_byte: RationalNumber::new(1, 42),
                plutus_v3_cost_model: Vec::new(),
                constitution: Constitution {
                    anchor: Anchor {
                        url: "constitution.cardano.org".to_string(),
                        data_hash: vec![0x99],
                    },
                    guardrail_script: None,
                },
                committee: Committee {
                    members: HashMap::new(),
                    threshold: RationalNumber::new(5, 32),
                },
            }),

            ..ProtocolParams::default()
        };

        let msg = ProtocolParamsMessage {
            params: params.clone(),
        };
        let mut state = State::default();

        state.handle_parameters(&msg).unwrap();

        assert_eq!(
            state.protocol_parameters.unwrap().conway.unwrap().pool_voting_thresholds,
            params.conway.unwrap().pool_voting_thresholds
        );
    }
}
