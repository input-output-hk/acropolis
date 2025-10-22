use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use acropolis_common::{
    messages::{
        AddressDeltasMessage, StakeRewardDeltasMessage, TxCertificatesMessage, WithdrawalsMessage,
    },
    BlockInfo, MoveInstantaneousReward, PoolId, ShelleyAddress, StakeAddress, StakeCredential,
    TxCertificate, TxIdentifier,
};

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
    tx_identifier: TxIdentifier,
    #[n(1)]
    deregistered: bool,
}

#[derive(Debug, Clone, minicbor::Decode, minicbor::Encode)]
pub struct AccountWithdrawal {
    #[n(0)]
    tx_identifier: TxIdentifier,
    #[n(1)]
    amount: u64,
}

#[derive(Debug, Clone)]
pub struct HistoricalAccountsConfig {
    pub db_path: String,
    pub skip_until: Option<u64>,

    pub store_rewards_history: bool,
    pub store_active_stake_history: bool,
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

    pub fn handle_tx_certificates(&mut self, tx_certs: &TxCertificatesMessage) -> Result<()> {
        // Handle certificates
        for tx_cert in tx_certs.certificates.iter() {
            match tx_cert {
                // Pre-Conway stake registration/deregistration certs
                TxCertificate::StakeRegistration(sc) => {
                    self.handle_stake_registration(&sc.stake_credential, &sc.tx_identifier);
                }
                TxCertificate::StakeDeregistration(sc) => {
                    self.handle_stake_deregistration(&sc.stake_credential, &sc.tx_identifier);
                }

                // Post-Conway stake registration/deregistration certs
                TxCertificate::Registration(reg) => {
                    self.handle_stake_registration(&reg.cert.credential, &reg.tx_identifier);
                }
                TxCertificate::Deregistration(dreg) => {
                    self.handle_stake_deregistration(&dreg.cert.credential, &dreg.tx_identifier);
                }

                // Registration and delegation certs
                TxCertificate::StakeRegistrationAndStakeAndVoteDelegation(delegation) => {
                    self.handle_stake_registration(
                        &delegation.cert.credential,
                        &delegation.tx_identifier,
                    );
                    self.handle_stake_delegation(
                        &delegation.cert.credential,
                        &delegation.cert.operator,
                        &delegation.tx_identifier,
                    );
                }
                TxCertificate::StakeRegistrationAndDelegation(delegation) => {
                    self.handle_stake_registration(
                        &delegation.cert.credential,
                        &delegation.tx_identifier,
                    );
                    self.handle_stake_delegation(
                        &delegation.cert.credential,
                        &delegation.cert.operator,
                        &delegation.tx_identifier,
                    );
                }

                // Delegation certs
                TxCertificate::StakeDelegation(delegation) => {
                    self.handle_stake_delegation(
                        &delegation.cert.credential,
                        &delegation.cert.operator,
                        &delegation.tx_identifier,
                    );
                }
                TxCertificate::StakeAndVoteDelegation(delegation) => {
                    self.handle_stake_delegation(
                        &delegation.cert.credential,
                        &delegation.cert.operator,
                        &delegation.tx_identifier,
                    );
                }
                TxCertificate::StakeRegistrationAndVoteDelegation(delegation) => {
                    self.handle_stake_registration(
                        &delegation.cert.credential,
                        &delegation.tx_identifier,
                    );
                }

                // MIR certs
                TxCertificate::MoveInstantaneousReward(mir) => self.handle_mir(mir),

                _ => (),
            };
        }
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
        _account: StakeAddress,
    ) -> Result<Vec<ActiveStakeHistory>> {
        Ok(Vec::new())
    }

    pub fn _get_delegation_history(&self, _account: StakeAddress) -> Result<Vec<DelegationUpdate>> {
        Ok(Vec::new())
    }

    pub fn _get_registration_history(
        &self,
        _account: StakeAddress,
    ) -> Result<Vec<RegistrationUpdate>> {
        Ok(Vec::new())
    }

    pub fn _get_withdrawal_history(
        &self,
        _account: StakeAddress,
    ) -> Result<Vec<AccountWithdrawal>> {
        Ok(Vec::new())
    }

    pub fn _get_mir_history(&self, _account: StakeAddress) -> Result<Vec<AccountWithdrawal>> {
        Ok(Vec::new())
    }

    pub fn _get_addresses(&self, _account: StakeAddress) -> Result<Vec<ShelleyAddress>> {
        Ok(Vec::new())
    }

    fn handle_stake_registration(
        &mut self,
        account: &StakeCredential,
        tx_identifier: &TxIdentifier,
    ) {
        let volatile = self.volatile.window.back_mut().expect("window should never be empty");

        let entry = volatile.entry(account.clone()).or_default();

        if let Some(registration_history) = &mut entry.registration_history {
            registration_history.push(RegistrationUpdate {
                tx_identifier: *tx_identifier,
                deregistered: false,
            });
        } else {
            entry.registration_history = Some(vec![RegistrationUpdate {
                tx_identifier: *tx_identifier,
                deregistered: false,
            }]);
        }
    }

    fn handle_stake_deregistration(
        &mut self,
        account: &StakeCredential,
        tx_identifier: &TxIdentifier,
    ) {
        let volatile = self.volatile.window.back_mut().expect("window should never be empty");

        let entry = volatile.entry(account.clone()).or_default();

        if let Some(mut registration_history) = entry.registration_history.clone() {
            registration_history.push(RegistrationUpdate {
                tx_identifier: *tx_identifier,
                deregistered: true,
            })
        }
    }

    fn handle_stake_delegation(
        &mut self,
        _account: &StakeCredential,
        _pool: &PoolId,
        _tx_identifier: &TxIdentifier,
    ) {
    }

    fn handle_mir(&mut self, _mir: &MoveInstantaneousReward) {}
}
