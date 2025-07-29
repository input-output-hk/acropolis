use crate::{DRepChoice, KeyHash};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum AccountsStateQuery {
    GetAccountInfo { stake_key: Vec<u8> },
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
