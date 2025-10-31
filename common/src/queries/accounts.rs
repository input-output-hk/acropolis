use std::collections::HashMap;

use crate::{
    DRepChoice, KeyHash, PoolId, PoolLiveStakeInfo, RewardType, ShelleyAddress, StakeAddress,
    TxIdentifier,
};

pub const DEFAULT_ACCOUNTS_QUERY_TOPIC: (&str, &str) =
    ("accounts-state-query-topic", "cardano.query.accounts");

pub const DEFAULT_HISTORICAL_ACCOUNTS_QUERY_TOPIC: (&str, &str) = (
    "historical-accounts-state-query-topic",
    "cardano.query.historical.accounts",
);

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum AccountsStateQuery {
    GetAccountInfo { account: StakeAddress },
    GetAccountRewardHistory { account: StakeAddress },
    GetAccountHistory { stake_key: Vec<u8> },
    GetAccountRegistrationHistory { account: StakeAddress },
    GetAccountDelegationHistory { account: StakeAddress },
    GetAccountMIRHistory { account: StakeAddress },
    GetAccountWithdrawalHistory { account: StakeAddress },
    GetAccountAssociatedAddresses { account: StakeAddress },
    GetAccountAssets { stake_key: Vec<u8> },
    GetAccountAssetsTotals { stake_key: Vec<u8> },
    GetAccountUTxOs { stake_key: Vec<u8> },
    GetAccountsUtxoValuesMap { stake_addresses: Vec<StakeAddress> },
    GetAccountsUtxoValuesSum { stake_addresses: Vec<StakeAddress> },
    GetAccountsBalancesMap { stake_addresses: Vec<StakeAddress> },
    GetAccountsBalancesSum { stake_addresses: Vec<StakeAddress> },

    // Epochs-related queries
    GetActiveStakes {},
    GetSPDDByEpoch { epoch: u64 },
    GetSPDDByEpochAndPool { epoch: u64, pool_id: PoolId },

    // Pools related queries
    GetOptimalPoolSizing,
    GetPoolsLiveStakes { pools_operators: Vec<PoolId> },
    GetPoolDelegators { pool_operator: PoolId },
    GetPoolLiveStake { pool_operator: PoolId },

    // Dreps related queries
    GetDrepDelegators { drep: DRepChoice },
    GetAccountsDrepDelegationsMap { stake_addresses: Vec<StakeAddress> },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum AccountsStateQueryResponse {
    AccountInfo(AccountInfo),
    AccountRewardHistory(Vec<AccountReward>),
    AccountHistory(AccountHistory),
    AccountRegistrationHistory(Vec<RegistrationUpdate>),
    AccountDelegationHistory(Vec<DelegationUpdate>),
    AccountMIRHistory(Vec<AccountWithdrawal>),
    AccountWithdrawalHistory(Vec<AccountWithdrawal>),
    AccountAssociatedAddresses(Vec<ShelleyAddress>),
    AccountAssets(AccountAssets),
    AccountAssetsTotals(AccountAssetsTotals),
    AccountUTxOs(AccountUTxOs),
    AccountsUtxoValuesMap(HashMap<KeyHash, u64>),
    AccountsUtxoValuesSum(u64),
    AccountsBalancesMap(HashMap<KeyHash, u64>),
    AccountsBalancesSum(u64),

    // Epochs-related responses
    ActiveStakes(u64),
    /// Vec<(PoolId, StakeKey, ActiveStakeAmount)>
    SPDDByEpoch(Vec<(PoolId, KeyHash, u64)>),
    /// Vec<(StakeKey, ActiveStakeAmount)>
    SPDDByEpochAndPool(Vec<(KeyHash, u64)>),

    // Pools-related responses
    OptimalPoolSizing(Option<OptimalPoolSizing>),
    PoolsLiveStakes(Vec<u64>),
    PoolDelegators(PoolDelegators),
    PoolLiveStake(PoolLiveStakeInfo),

    // DReps-related responses
    DrepDelegators(DrepDelegators),
    AccountsDrepDelegationsMap(HashMap<KeyHash, Option<DRepChoice>>),

    NotFound,
    Error(String),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AccountInfo {
    pub utxo_value: u64,
    pub rewards: u64,
    pub delegated_spo: Option<PoolId>,
    pub delegated_drep: Option<DRepChoice>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AccountHistory {}

#[derive(
    Debug, Clone, serde::Serialize, serde::Deserialize, minicbor::Decode, minicbor::Encode,
)]
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

#[derive(
    Debug, Clone, serde::Serialize, serde::Deserialize, minicbor::Decode, minicbor::Encode,
)]
pub struct RegistrationUpdate {
    #[n(0)]
    pub tx_identifier: TxIdentifier,
    #[n(1)]
    pub status: RegistrationStatus,
}

#[derive(
    Debug, Clone, serde::Serialize, serde::Deserialize, minicbor::Decode, minicbor::Encode,
)]
pub enum RegistrationStatus {
    #[n(0)]
    Registered,
    #[n(1)]
    Deregistered,
}

impl std::fmt::Display for RegistrationStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RegistrationStatus::Registered => write!(f, "registered"),
            RegistrationStatus::Deregistered => write!(f, "deregistered"),
        }
    }
}

#[derive(
    Debug, Clone, serde::Serialize, serde::Deserialize, minicbor::Decode, minicbor::Encode,
)]
pub struct AccountWithdrawal {
    #[n(0)]
    pub tx_identifier: TxIdentifier,
    #[n(1)]
    pub amount: u64,
}

#[derive(
    Debug, Clone, minicbor::Decode, minicbor::Encode, serde::Serialize, serde::Deserialize,
)]
pub struct AccountReward {
    #[n(0)]
    pub epoch: u32,
    #[n(1)]
    pub amount: u64,
    #[n(2)]
    pub pool: PoolId,
    #[n(3)]
    pub reward_type: RewardType,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AccountWithdrawalHistory {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AccountAssociatedAddresses {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AccountAssets {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AccountAssetsTotals {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AccountUTxOs {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OptimalPoolSizing {
    pub total_supply: u64, // total_supply - reserves
    pub nopt: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PoolDelegators {
    pub delegators: Vec<(KeyHash, u64)>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DrepDelegators {
    pub delegators: Vec<(KeyHash, u64)>,
}
