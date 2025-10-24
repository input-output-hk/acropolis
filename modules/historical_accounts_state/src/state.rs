use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use acropolis_common::{
    messages::{
        AddressDeltasMessage, StakeRewardDeltasMessage, TxCertificatesMessage, WithdrawalsMessage,
    },
    queries::accounts::{AccountWithdrawal, DelegationUpdate, RegistrationUpdate},
    BlockInfo, InstantaneousRewardTarget, PoolId, ShelleyAddress, StakeAddress, StakeCredential,
    TxCertificate, TxIdentifier,
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
pub struct RewardHistory {
    #[n(0)]
    pub epoch: u32,
    #[n(1)]
    pub amount: u64,
    #[n(2)]
    pub pool: PoolId,
    #[n(3)]
    pub is_owner: bool,
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

    pub fn handle_rewards(&mut self, _reward_deltas: &StakeRewardDeltasMessage) -> Result<()> {
        Ok(())
    }

    pub fn handle_tx_certificates(
        &mut self,
        tx_certs: &TxCertificatesMessage,
        epoch: u32,
    ) -> Result<()> {
        // Handle certificates
        for tx_cert in tx_certs.certificates.iter() {
            match tx_cert {
                // Pre-Conway stake registration/deregistration certs
                TxCertificate::StakeRegistration(sc) => {
                    self.handle_stake_registration_change(
                        &sc.stake_address,
                        &sc.tx_identifier,
                        false,
                    );
                }
                TxCertificate::StakeDeregistration(sc) => {
                    self.handle_stake_registration_change(
                        &sc.stake_address,
                        &sc.tx_identifier,
                        true,
                    );
                }

                // Post-Conway stake registration/deregistration certs
                TxCertificate::Registration(reg) => {
                    self.handle_stake_registration_change(
                        &reg.cert.stake_address,
                        &reg.tx_identifier,
                        false,
                    );
                }
                TxCertificate::Deregistration(dreg) => {
                    self.handle_stake_registration_change(
                        &dreg.cert.stake_address,
                        &dreg.tx_identifier,
                        true,
                    );
                }

                // Registration and delegation certs
                TxCertificate::StakeRegistrationAndStakeAndVoteDelegation(delegation) => {
                    self.handle_stake_registration_change(
                        &delegation.cert.stake_address,
                        &delegation.tx_identifier,
                        false,
                    );
                    self.handle_stake_delegation(
                        &delegation.cert.stake_address,
                        &delegation.cert.operator,
                        &delegation.tx_identifier,
                        epoch,
                    );
                }
                TxCertificate::StakeRegistrationAndDelegation(delegation) => {
                    self.handle_stake_registration_change(
                        &delegation.cert.stake_address,
                        &delegation.tx_identifier,
                        false,
                    );
                    self.handle_stake_delegation(
                        &delegation.cert.stake_address,
                        &delegation.cert.operator,
                        &delegation.tx_identifier,
                        epoch,
                    );
                }

                // Delegation certs
                TxCertificate::StakeDelegation(delegation) => {
                    self.handle_stake_delegation(
                        &delegation.cert.stake_address,
                        &delegation.cert.operator,
                        &delegation.tx_identifier,
                        epoch,
                    );
                }
                TxCertificate::StakeAndVoteDelegation(delegation) => {
                    self.handle_stake_delegation(
                        &delegation.cert.stake_address,
                        &delegation.cert.operator,
                        &delegation.tx_identifier,
                        epoch,
                    );
                }
                TxCertificate::StakeRegistrationAndVoteDelegation(delegation) => {
                    self.handle_stake_registration_change(
                        &delegation.cert.stake_address,
                        &delegation.tx_identifier,
                        false,
                    );
                }

                // MIR certs
                TxCertificate::MoveInstantaneousReward(mir) => {
                    self.handle_mir(&mir.cert.target, &mir.tx_identifier);
                }

                _ => (),
            };
        }
        Ok(())
    }

    pub fn handle_address_deltas(&mut self, _address_deltas: &AddressDeltasMessage) -> Result<()> {
        Ok(())
    }

    pub fn handle_withdrawals(&mut self, withdrawals_msg: &WithdrawalsMessage) -> Result<()> {
        for withdrawal in &withdrawals_msg.withdrawals {
            let volatile = self.volatile.window.back_mut().expect("window should never be empty");
            let entry = volatile.entry(withdrawal.withdrawal.address.clone()).or_default();
            let withdrawal_entry = AccountWithdrawal {
                tx_identifier: withdrawal.tx_identifier,
                amount: withdrawal.withdrawal.value,
            };
            entry.withdrawal_history.get_or_insert(Vec::new()).push(withdrawal_entry)
        }
        Ok(())
    }

    pub async fn _get_reward_history(
        &self,
        _account: &StakeCredential,
    ) -> Result<Vec<RewardHistory>> {
        Ok(Vec::new())
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
    ) -> Result<Vec<RegistrationUpdate>> {
        let mut registrations =
            self.immutable.get_registration_history(&account).await?.unwrap_or_default();

        self.merge_volatile_history(
            &account,
            |e| e.registration_history.as_ref(),
            &mut registrations,
        );

        Ok(registrations)
    }

    pub async fn get_delegation_history(
        &self,
        account: &StakeAddress,
    ) -> Result<Vec<DelegationUpdate>> {
        let mut delegations =
            self.immutable.get_delegation_history(&account).await?.unwrap_or_default();

        self.merge_volatile_history(
            &account,
            |e| e.delegation_history.as_ref(),
            &mut delegations,
        );

        Ok(delegations)
    }

    pub async fn get_mir_history(&self, account: &StakeAddress) -> Result<Vec<AccountWithdrawal>> {
        let mut mirs = self.immutable.get_mir_history(&account).await?.unwrap_or_default();

        self.merge_volatile_history(&account, |e| e.mir_history.as_ref(), &mut mirs);

        Ok(mirs)
    }

    pub async fn _get_withdrawal_history(
        &self,
        _account: &StakeAddress,
    ) -> Result<Vec<AccountWithdrawal>> {
        Ok(Vec::new())
    }

    pub async fn _get_addresses(&self, _account: StakeAddress) -> Result<Vec<ShelleyAddress>> {
        Ok(Vec::new())
    }

    fn handle_stake_registration_change(
        &mut self,
        account: &StakeAddress,
        tx_identifier: &TxIdentifier,
        deregistered: bool,
    ) {
        let volatile = self.volatile.window.back_mut().expect("window should never be empty");
        let entry = volatile.entry(account.clone()).or_default();
        let update = RegistrationUpdate {
            tx_identifier: *tx_identifier,
            deregistered,
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
