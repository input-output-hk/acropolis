use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use acropolis_common::{messages::{
    AddressDeltasMessage, StakeRewardDeltasMessage, TxCertificatesMessage, WithdrawalsMessage,
}, BlockInfo, PoolId, ShelleyAddress, StakeAddress, StakeCredential, TxIdentifier};

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

#[derive(Debug, Clone, minicbor::Decode, minicbor::Encode)]
pub struct DelegationUpdate {
    #[n(0)]
    pub active_epoch: u32,
    #[n(1)]
    pub tx_identifier: TxIdentifier,
    #[n(2)]
    pub amount: u64,
    #[n(3)]
    pub pool: PoolId,
}

#[derive(Debug, Clone, minicbor::Decode, minicbor::Encode)]
pub struct RegistrationUpdate {
    #[n(0)]
    pub tx_identifier: TxIdentifier,
    #[n(1)]
    pub deregistered: bool,
}

#[derive(Debug, Clone, minicbor::Decode, minicbor::Encode)]
pub struct AccountWithdrawal {
    #[n(0)]
    pub tx_identifier: TxIdentifier,
    #[n(1)]
    pub amount: u64,
}

#[derive(Debug, Clone)]
pub struct HistoricalAccountsConfig {
    pub db_path: String,

    pub store_rewards_history: bool,
    pub store_active_stake_history: bool,
    pub store_delegation_history: bool,
    pub store_registration_history: bool,
    pub store_withdrawal_history: bool,
    pub store_mir_history: bool,
    pub store_addresses: bool,
}

impl HistoricalAccountsConfig {
    pub fn any_enabled(&self) -> bool {
        self.store_rewards_history
            || self.store_active_stake_history
            || self.store_delegation_history
            || self.store_registration_history
            || self.store_withdrawal_history
            || self.store_mir_history
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

    pub fn handle_tx_certificates(&mut self, _tx_certs: &TxCertificatesMessage) -> Result<()> {
        Ok(())
    }

    pub fn handle_address_deltas(&mut self, _address_deltas: &AddressDeltasMessage) -> Result<()> {
        Ok(())
    }

    pub fn handle_withdrawals(&mut self, _withdrawals: &WithdrawalsMessage) -> Result<()> {
        Ok(())
    }

    pub fn _get_reward_history(&self, _account: StakeAddress) -> Result<Vec<RewardHistory>> {
        Ok(Vec::new())
    }

    pub fn _get_active_stake_history(
        &self,
        _account: StakeCredential,
    ) -> Result<Vec<ActiveStakeHistory>> {
        Ok(Vec::new())
    }

    pub fn _get_delegation_history(
        &self,
        _account: StakeCredential,
    ) -> Result<Vec<DelegationUpdate>> {
        Ok(Vec::new())
    }

    pub fn _get_registration_history(
        &self,
        _account: StakeCredential,
    ) -> Result<Vec<RegistrationUpdate>> {
        Ok(Vec::new())
    }

    pub fn _get_withdrawal_history(
        &self,
        _account: StakeCredential,
    ) -> Result<Vec<AccountWithdrawal>> {
        Ok(Vec::new())
    }

    pub fn _get_mir_history(&self, _account: StakeCredential) -> Result<Vec<AccountWithdrawal>> {
        Ok(Vec::new())
    }

    pub fn _get_addresses(&self, _account: StakeCredential) -> Result<Vec<ShelleyAddress>> {
        Ok(Vec::new())
    }
}
