use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use acropolis_common::{
    messages::{
        AddressDeltasMessage, StakeRewardDeltasMessage, TxCertificatesMessage, WithdrawalsMessage,
    },
    PoolId, ShelleyAddress, TxIdentifier,
};

use crate::{
    immutable_historical_account_store::ImmutableHistoricalAccountStore,
    volatile_historical_accounts::VolatileHistoricalAccounts,
};

use anyhow::Result;

#[derive(Debug, Default, Clone)]
pub struct AccountEntry {
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

#[derive(Debug, Clone, minicbor::Decode, minicbor::Encode)]
pub struct DelegationUpdate {
    #[n(0)]
    active_epoch: u32,
    #[n(1)]
    tx_hash: TxIdentifier,
    #[n(2)]
    amount: u64,
    #[n(3)]
    pool: PoolId,
}

#[derive(Debug, Clone, minicbor::Decode, minicbor::Encode)]
pub struct RegistrationUpdate {
    #[n(0)]
    tx_hash: TxIdentifier,
    #[n(1)]
    deregistered: bool,
}

#[derive(Debug, Clone, minicbor::Decode, minicbor::Encode)]
pub struct AccountWithdrawal {
    #[n(0)]
    tx_hash: TxIdentifier,
    #[n(1)]
    amount: u64,
}

#[derive(Debug, Clone)]
pub struct HistoricalAccountsConfig {
    pub db_path: String,
    pub skip_until: Option<u64>,

    pub store_epoch_history: bool,
    pub store_delegation_history: bool,
    pub store_registration_history: bool,
    pub store_withdrawal_history: bool,
    pub store_mir_history: bool,
    pub store_addresses: bool,
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

        let mut config = config.clone();
        config.skip_until = store.get_last_epoch_stored().await?;

        Ok(Self {
            config,
            volatile: VolatileHistoricalAccounts::default(),
            immutable: store,
        })
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
}
