use acropolis_common::{rest_helper::ToCheckedF64, PoolEpochState, VotingProcedure};
use rust_decimal::Decimal;
use serde::Serialize;

/// REST response structure for proposal votes
#[derive(Serialize)]
pub struct VoteRest {
    pub transaction: String,
    pub voting_procedure: VotingProcedure,
}

#[derive(Serialize)]
pub struct PoolExtendedRest {
    pub pool_id: String,
    pub hex: String,
    pub active_stake: String, // u64 in string
    pub live_stake: String,   // u64 in string
    pub blocks_minted: u64,
    pub live_saturation: Decimal,
    pub declared_pledge: String, // u64 in string
    pub margin_cost: f32,
    pub fixed_cost: String, // u64 in string
}

#[derive(Serialize)]
pub struct PoolEpochStateRest {
    pub epoch: u64,
    pub blocks: u64,
    pub active_stake: String, // u64 in string
    pub active_size: f64,
    pub delegators_count: u64,
    pub rewards: String, // u64 in string
    pub fees: String,    // u64 in string
}

impl From<PoolEpochState> for PoolEpochStateRest {
    fn from(state: PoolEpochState) -> Self {
        Self {
            epoch: state.epoch,
            blocks: state.blocks_minted,
            active_stake: state.active_stake.to_string(),
            active_size: state.active_size.to_checked_f64("active_size").unwrap_or(0.0),
            delegators_count: state.delegators_count,
            rewards: state.pool_reward.to_string(),
            fees: state.spo_reward.to_string(),
        }
    }
}
