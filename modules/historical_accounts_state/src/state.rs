use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use acropolis_common::{
    messages::{
        AddressDeltasMessage, StakeRewardDeltasMessage, TxCertificatesMessage, WithdrawalsMessage,
    },
    queries::accounts::{
        AccountWithdrawal, DelegationUpdate, RegistrationStatus, RegistrationUpdate, RewardHistory,
    },
    BlockInfo, InstantaneousRewardTarget, PoolId, ShelleyAddress, StakeAddress, TxCertificate,
    TxIdentifier,
};
use tracing::warn;

use crate::{
    immutable_historical_account_store::ImmutableHistoricalAccountStore,
    volatile_historical_accounts::VolatileHistoricalAccounts,
};

use anyhow::Result;

#[derive(Debug, Default, Clone)]
pub struct AccountEntry {
    pub reward_history: Option<Vec<RewardHistory>>,
    pub active_stake_history: Option<Vec<ActiveStakeHistory>>,
    pub delegation_history: Option<Vec<DelegationUpdate>>,
    pub registration_history: Option<Vec<RegistrationUpdate>>,
    pub withdrawal_history: Option<Vec<AccountWithdrawal>>,
    pub mir_history: Option<Vec<AccountWithdrawal>>,
    pub addresses: Option<Vec<ShelleyAddress>>,
}

#[derive(Debug, Clone, minicbor::Decode, minicbor::Encode)]
pub struct ActiveStakeHistory {
    #[n(0)]
    pub active_epoch: u32,
    #[n(1)]
    pub amount: u64,
    #[n(2)]
    pub pool: PoolId,
}

#[derive(Debug, Clone)]
pub struct HistoricalAccountsConfig {
    pub db_path: String,

    pub store_rewards_history: bool,
    pub store_active_stake_history: bool,
    pub store_delegation_history: bool,
    pub store_registration_history: bool,
    pub store_mir_history: bool,
    pub store_withdrawal_history: bool,
    pub store_addresses: bool,
}

impl HistoricalAccountsConfig {
    pub fn any_enabled(&self) -> bool {
        self.store_rewards_history
            || self.store_active_stake_history
            || self.store_delegation_history
            || self.store_registration_history
            || self.store_mir_history
            || self.store_withdrawal_history
            || self.store_addresses
    }
}

/// Overall state - stored per block
#[derive(Clone)]
pub struct State {
    pub config: HistoricalAccountsConfig,
    pub volatile: VolatileHistoricalAccounts,
    pub immutable: Arc<ImmutableHistoricalAccountStore>,
}

impl State {
    pub async fn new(config: HistoricalAccountsConfig) -> Result<Self> {
        let db_path = if Path::new(&config.db_path).is_relative() {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(&config.db_path)
        } else {
            PathBuf::from(&config.db_path)
        };

        let store = Arc::new(ImmutableHistoricalAccountStore::new(&db_path)?);

        Ok(Self {
            config,
            volatile: VolatileHistoricalAccounts::default(),
            immutable: store,
        })
    }

    pub async fn prune_volatile(&mut self) {
        let drained = self.volatile.prune_volatile();
        self.immutable.update_immutable(drained).await;
    }

    pub fn ready_to_prune(&self, block_info: &BlockInfo) -> bool {
        block_info.epoch > 0
            && Some(block_info.epoch) != self.volatile.last_persisted_epoch
            && block_info.number > self.volatile.epoch_start_block + self.volatile.security_param_k
    }

    pub fn handle_rewards(&mut self, reward_deltas: &StakeRewardDeltasMessage, epoch: u32) {
        let volatile = self.volatile.window.back_mut().expect("window should never be empty");
        for reward in reward_deltas.deltas.iter() {
            let entry = volatile.entry(reward.stake_address.clone()).or_default();
            let update = RewardHistory {
                epoch,
                amount: reward.delta,
                pool: reward.pool.clone(),
                reward_type: reward.reward_type.clone(),
            };
            entry.reward_history.get_or_insert_with(Vec::new).push(update);
        }
    }

    pub fn handle_tx_certificates(&mut self, tx_certs: &TxCertificatesMessage, epoch: u32) {
        // Handle certificates
        for tx_cert in tx_certs.certificates.iter() {
            match &tx_cert.cert {
                // Pre-Conway stake registration/deregistration certs
                TxCertificate::StakeRegistration(stake_address) => {
                    self.handle_stake_registration_change(
                        stake_address,
                        &tx_cert.tx_identifier,
                        RegistrationStatus::Registered,
                    );
                }
                TxCertificate::StakeDeregistration(stake_address) => {
                    self.handle_stake_registration_change(
                        stake_address,
                        &tx_cert.tx_identifier,
                        RegistrationStatus::Deregistered,
                    );
                }

                // Post-Conway stake registration/deregistration certs
                TxCertificate::Registration(reg) => {
                    self.handle_stake_registration_change(
                        &reg.stake_address,
                        &tx_cert.tx_identifier,
                        RegistrationStatus::Registered,
                    );
                }
                TxCertificate::Deregistration(dreg) => {
                    self.handle_stake_registration_change(
                        &dreg.stake_address,
                        &tx_cert.tx_identifier,
                        RegistrationStatus::Deregistered,
                    );
                }

                // Registration and delegation certs
                TxCertificate::StakeRegistrationAndStakeAndVoteDelegation(delegation) => {
                    self.handle_stake_registration_change(
                        &delegation.stake_address,
                        &tx_cert.tx_identifier,
                        RegistrationStatus::Registered,
                    );
                    self.handle_stake_delegation(
                        &delegation.stake_address,
                        &delegation.operator,
                        &tx_cert.tx_identifier,
                        epoch,
                    );
                }
                TxCertificate::StakeRegistrationAndDelegation(delegation) => {
                    self.handle_stake_registration_change(
                        &delegation.stake_address,
                        &tx_cert.tx_identifier,
                        RegistrationStatus::Registered,
                    );
                    self.handle_stake_delegation(
                        &delegation.stake_address,
                        &delegation.operator,
                        &tx_cert.tx_identifier,
                        epoch,
                    );
                }

                // Delegation certs
                TxCertificate::StakeDelegation(delegation) => {
                    self.handle_stake_delegation(
                        &delegation.stake_address,
                        &delegation.operator,
                        &tx_cert.tx_identifier,
                        epoch,
                    );
                }
                TxCertificate::StakeAndVoteDelegation(delegation) => {
                    self.handle_stake_delegation(
                        &delegation.stake_address,
                        &delegation.operator,
                        &tx_cert.tx_identifier,
                        epoch,
                    );
                }
                TxCertificate::StakeRegistrationAndVoteDelegation(delegation) => {
                    self.handle_stake_registration_change(
                        &delegation.stake_address,
                        &tx_cert.tx_identifier,
                        RegistrationStatus::Registered,
                    );
                }

                // MIR certs
                TxCertificate::MoveInstantaneousReward(mir) => {
                    self.handle_mir(&mir.target, &tx_cert.tx_identifier);
                }

                _ => (),
            };
        }
    }

    pub fn handle_address_deltas(&mut self, _address_deltas: &AddressDeltasMessage) -> Result<()> {
        Ok(())
    }

    pub fn handle_withdrawals(&mut self, withdrawals_msg: &WithdrawalsMessage) {
        let window = self.volatile.window.back_mut().expect("window should never be empty");

        for w in &withdrawals_msg.withdrawals {
            window
                .entry(w.address.clone())
                .or_default()
                .withdrawal_history
                .get_or_insert_with(Vec::new)
                .push(AccountWithdrawal {
                    tx_identifier: w.tx_identifier,
                    amount: w.value,
                })
        }
    }

    pub async fn get_reward_history(
        &self,
        account: &StakeAddress,
    ) -> Result<Option<Vec<RewardHistory>>> {
        let immutable = self.immutable.get_reward_history(account).await?;

        let mut volatile = Vec::new();
        self.merge_volatile_history(account, |e| e.reward_history.as_ref(), &mut volatile);

        match immutable {
            Some(mut rewards) => {
                rewards.extend(volatile);
                Ok(Some(rewards))
            }
            None if volatile.is_empty() => Ok(None),
            None => Ok(Some(volatile)),
        }
    }

    pub async fn _get_active_stake_history(
        &self,
        _account: &StakeAddress,
    ) -> Result<Vec<ActiveStakeHistory>> {
        Ok(Vec::new())
    }

    pub async fn get_registration_history(
        &self,
        account: &StakeAddress,
    ) -> Result<Option<Vec<RegistrationUpdate>>> {
        let immutable = self.immutable.get_registration_history(account).await?;

        let mut volatile = Vec::new();
        self.merge_volatile_history(account, |e| e.registration_history.as_ref(), &mut volatile);

        match immutable {
            Some(mut registrations) => {
                registrations.extend(volatile);
                Ok(Some(registrations))
            }
            None if volatile.is_empty() => Ok(None),
            None => Ok(Some(volatile)),
        }
    }

    pub async fn get_delegation_history(
        &self,
        account: &StakeAddress,
    ) -> Result<Option<Vec<DelegationUpdate>>> {
        let immutable = self.immutable.get_delegation_history(account).await?;

        let mut volatile = Vec::new();
        self.merge_volatile_history(account, |e| e.delegation_history.as_ref(), &mut volatile);

        match immutable {
            Some(mut delegations) => {
                delegations.extend(volatile);
                Ok(Some(delegations))
            }
            None if volatile.is_empty() => Ok(None),
            None => Ok(Some(volatile)),
        }
    }

    pub async fn get_mir_history(
        &self,
        account: &StakeAddress,
    ) -> Result<Option<Vec<AccountWithdrawal>>> {
        let immutable = self.immutable.get_mir_history(account).await?;

        let mut volatile = Vec::new();
        self.merge_volatile_history(account, |e| e.mir_history.as_ref(), &mut volatile);

        match immutable {
            Some(mut mirs) => {
                mirs.extend(volatile);
                Ok(Some(mirs))
            }
            None if volatile.is_empty() => Ok(None),
            None => Ok(Some(volatile)),
        }
    }

    pub async fn get_withdrawal_history(
        &self,
        account: &StakeAddress,
    ) -> Result<Option<Vec<AccountWithdrawal>>> {
        let immutable = self.immutable.get_withdrawal_history(account).await?;

        let mut volatile = Vec::new();
        self.merge_volatile_history(account, |e| e.withdrawal_history.as_ref(), &mut volatile);
        match immutable {
            Some(mut withdrawals) => {
                withdrawals.extend(volatile);
                Ok(Some(withdrawals))
            }
            None if volatile.is_empty() => Ok(None),
            None => Ok(Some(volatile)),
        }
    }

    pub async fn _get_addresses(&self, _account: StakeAddress) -> Result<Vec<ShelleyAddress>> {
        Ok(Vec::new())
    }

    fn handle_stake_registration_change(
        &mut self,
        account: &StakeAddress,
        tx_identifier: &TxIdentifier,
        status: RegistrationStatus,
    ) {
        let volatile = self.volatile.window.back_mut().expect("window should never be empty");
        let entry = volatile.entry(account.clone()).or_default();
        let update = RegistrationUpdate {
            tx_identifier: *tx_identifier,
            status,
        };
        entry.registration_history.get_or_insert_with(Vec::new).push(update);
    }

    fn handle_stake_delegation(
        &mut self,
        account: &StakeAddress,
        pool: &PoolId,
        tx_identifier: &TxIdentifier,
        epoch: u32,
    ) {
        let volatile = self.volatile.window.back_mut().expect("window should never be empty");
        let entry = volatile.entry(account.clone()).or_default();
        let update = DelegationUpdate {
            active_epoch: epoch.saturating_add(2),
            tx_identifier: *tx_identifier,
            amount: 0, // Amount is set during persistence when active stake is known
            pool: pool.clone(),
        };
        entry.delegation_history.get_or_insert_with(Vec::new).push(update);
    }

    fn handle_mir(&mut self, mir: &InstantaneousRewardTarget, tx_identifier: &TxIdentifier) {
        let volatile = self.volatile.window.back_mut().expect("window should never be empty");

        if let InstantaneousRewardTarget::StakeAddresses(payments) = mir {
            for (account, amount) in payments {
                if *amount <= 0 {
                    warn!(
                        "Ignoring invalid MIR (negative or zero) for stake credential {}",
                        hex::encode(account.get_hash())
                    );
                    continue;
                }

                let entry = volatile.entry(account.clone()).or_default();
                let update = AccountWithdrawal {
                    tx_identifier: *tx_identifier,
                    amount: *amount as u64,
                };

                entry.mir_history.get_or_insert_with(Vec::new).push(update);
            }
        }
    }

    fn merge_volatile_history<T, F>(&self, account: &StakeAddress, f: F, out: &mut Vec<T>)
    where
        F: Fn(&AccountEntry) -> Option<&Vec<T>>,
        T: Clone,
    {
        for block_map in self.volatile.window.iter() {
            if let Some(entry) = block_map.get(account) {
                if let Some(pending) = f(entry) {
                    out.extend(pending.iter().cloned());
                }
            }
        }
    }
}
