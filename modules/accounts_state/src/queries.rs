use std::sync::Arc;

use acropolis_common::{
    messages::{Message, StateQuery, StateQueryResponse},
    queries::{
        accounts::{
            AccountInfo, AccountsStateQuery, AccountsStateQueryResponse, DrepDelegators,
            PoolDelegators,
        },
        errors::QueryError,
    },
};

use crate::{spo_distribution_store::SPDDStore, state::State};

pub fn handle_accounts_query(
    state: &State,
    spdd_store: Option<&SPDDStore>,
    message: &Message,
) -> Arc<Message> {
    let Message::StateQuery(StateQuery::Accounts(query)) = message else {
        return Arc::new(Message::StateQueryResponse(StateQueryResponse::Accounts(
            AccountsStateQueryResponse::Error(QueryError::internal_error(
                "Invalid message for accounts-state",
            )),
        )));
    };

    let response = match query {
        AccountsStateQuery::GetAccountInfo { account } => match state.get_stake_state(account) {
            Some(account) => AccountsStateQueryResponse::AccountInfo(AccountInfo {
                utxo_value: account.utxo_value,
                rewards: account.rewards,
                delegated_spo: account.delegated_spo,
                delegated_drep: account.delegated_drep.clone(),
            }),
            None => AccountsStateQueryResponse::Error(QueryError::not_found(format!(
                "Account {}",
                account
            ))),
        },

        AccountsStateQuery::GetPoolsLiveStakes { pools_operators } => {
            AccountsStateQueryResponse::PoolsLiveStakes(
                state.get_pools_live_stakes(pools_operators),
            )
        }

        AccountsStateQuery::GetPoolDelegators { pool_operator } => {
            AccountsStateQueryResponse::PoolDelegators(PoolDelegators {
                delegators: state.get_pool_delegators(pool_operator),
            })
        }

        AccountsStateQuery::GetPoolLiveStake { pool_operator } => {
            AccountsStateQueryResponse::PoolLiveStake(state.get_pool_live_stake_info(pool_operator))
        }

        AccountsStateQuery::GetDrepDelegators { drep } => {
            AccountsStateQueryResponse::DrepDelegators(DrepDelegators {
                delegators: state.get_drep_delegators(drep),
            })
        }

        AccountsStateQuery::GetAccountsDrepDelegationsMap { stake_addresses } => {
            match state.get_drep_delegations_map(stake_addresses) {
                Some(map) => AccountsStateQueryResponse::AccountsDrepDelegationsMap(map),
                None => AccountsStateQueryResponse::Error(QueryError::internal_error(
                    "Error retrieving DRep delegations map",
                )),
            }
        }

        AccountsStateQuery::GetOptimalPoolSizing => {
            AccountsStateQueryResponse::OptimalPoolSizing(state.get_optimal_pool_sizing())
        }

        AccountsStateQuery::GetAccountsUtxoValuesMap { stake_addresses } => {
            match state.get_accounts_utxo_values_map(stake_addresses) {
                Some(map) => AccountsStateQueryResponse::AccountsUtxoValuesMap(map),
                None => AccountsStateQueryResponse::Error(QueryError::not_found(
                    "One or more accounts not found",
                )),
            }
        }

        AccountsStateQuery::GetAccountsUtxoValuesSum { stake_addresses } => {
            match state.get_accounts_utxo_values_sum(stake_addresses) {
                Some(sum) => AccountsStateQueryResponse::AccountsUtxoValuesSum(sum),
                None => AccountsStateQueryResponse::Error(QueryError::not_found(
                    "One or more accounts not found",
                )),
            }
        }

        AccountsStateQuery::GetAccountsBalancesMap { stake_addresses } => {
            match state.get_accounts_balances_map(stake_addresses) {
                Some(map) => AccountsStateQueryResponse::AccountsBalancesMap(map),
                None => AccountsStateQueryResponse::Error(QueryError::not_found(
                    "One or more accounts not found",
                )),
            }
        }

        AccountsStateQuery::GetActiveStakes {} => {
            AccountsStateQueryResponse::ActiveStakes(state.get_latest_snapshot_account_balances())
        }

        AccountsStateQuery::GetAccountsBalancesSum { stake_addresses } => {
            match state.get_account_balances_sum(stake_addresses) {
                Some(sum) => AccountsStateQueryResponse::AccountsBalancesSum(sum),
                None => AccountsStateQueryResponse::Error(QueryError::not_found(
                    "One or more accounts not found",
                )),
            }
        }

        AccountsStateQuery::GetSPDDByEpoch { epoch } => match spdd_store {
            Some(spdd_store) => match spdd_store.query_by_epoch(*epoch) {
                Ok(result) => AccountsStateQueryResponse::SPDDByEpoch(result),
                Err(e) => {
                    AccountsStateQueryResponse::Error(QueryError::internal_error(e.to_string()))
                }
            },
            None => AccountsStateQueryResponse::Error(QueryError::storage_disabled("SPDD")),
        },

        AccountsStateQuery::GetSPDDByEpochAndPool { epoch, pool_id } => match spdd_store {
            Some(spdd_store) => match spdd_store.query_by_epoch_and_pool(*epoch, pool_id) {
                Ok(result) => AccountsStateQueryResponse::SPDDByEpochAndPool(result),
                Err(e) => {
                    AccountsStateQueryResponse::Error(QueryError::internal_error(e.to_string()))
                }
            },
            None => AccountsStateQueryResponse::Error(QueryError::storage_disabled("SPDD")),
        },

        _ => AccountsStateQueryResponse::Error(QueryError::not_implemented(format!(
            "Unimplemented query variant: {:?}",
            query
        ))),
    };

    Arc::new(Message::StateQueryResponse(StateQueryResponse::Accounts(
        response,
    )))
}
