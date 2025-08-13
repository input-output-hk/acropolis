use acropolis_common::VotingProcedure;
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
    pub metadata: Option<PoolMetadataRest>,
}

#[derive(Serialize)]
pub struct PoolMetadataRest {
    pub url: String,
    pub hash: String,
    pub ticker: String,
    pub name: String,
    pub description: String,
    pub homepage: String,
}
