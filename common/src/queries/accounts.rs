use std::collections::HashMap;

use crate::{DRepChoice, KeyHash, PoolLiveStakeInfo, StakeAddress};

pub const DEFAULT_ACCOUNTS_QUERY_TOPIC: (&str, &str) =
    ("accounts-state-query-topic", "cardano.query.accounts");

pub const DEFAULT_HISTORICAL_ACCOUNTS_QUERY_TOPIC: (&str, &str) = (
    "historical-accounts-state-query-topic",
    "cardano.query.historical.accounts",
);

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum AccountsStateQuery {
    GetAccountInfo { stake_address: StakeAddress },
    GetAccountRewardHistory { stake_key: Vec<u8> },
    GetAccountHistory { stake_key: Vec<u8> },
    GetAccountDelegationHistory { stake_key: Vec<u8> },
    GetAccountRegistrationHistory { stake_key: Vec<u8> },
    GetAccountWithdrawalHistory { stake_key: Vec<u8> },
    GetAccountMIRHistory { stake_key: Vec<u8> },
    GetAccountAssociatedAddresses { stake_key: Vec<u8> },
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
    GetSPDDByEpochAndPool { epoch: u64, pool_id: KeyHash },

    // Pools related queries
    GetOptimalPoolSizing,
    GetPoolsLiveStakes { pools_operators: Vec<KeyHash> },
    GetPoolDelegators { pool_operator: KeyHash },
    GetPoolLiveStake { pool_operator: KeyHash },

    // Dreps related queries
    GetDrepDelegators { drep: DRepChoice },
    GetAccountsDrepDelegationsMap { stake_addresses: Vec<StakeAddress> },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum AccountsStateQueryResponse {
    AccountInfo(AccountInfo),
    AccountRewardHistory(AccountRewardHistory),
    AccountHistory(AccountHistory),
    AccountDelegationHistory(AccountDelegationHistory),
    AccountRegistrationHistory(AccountRegistrationHistory),
    AccountWithdrawalHistory(AccountWithdrawalHistory),
    AccountMIRHistory(AccountMIRHistory),
    AccountAssociatedAddresses(AccountAssociatedAddresses),
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
    SPDDByEpoch(Vec<(KeyHash, KeyHash, u64)>),
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
    pub delegated_spo: Option<KeyHash>,
    pub delegated_drep: Option<DRepChoice>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AccountRewardHistory {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AccountHistory {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AccountDelegationHistory {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AccountRegistrationHistory {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AccountWithdrawalHistory {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AccountMIRHistory {}

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
