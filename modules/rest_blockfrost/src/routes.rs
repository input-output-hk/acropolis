//! Route Definitions for Blockfrost-compatible API
//!
//! This module provides a shared registry of all API routes that can be used by both
//! the REST server and the MCP server to ensure consistency.

/// Handler type - determines whether the handler accepts query parameters
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandlerType {
    /// Handler only accepts path parameters
    PathOnly,
    /// Handler accepts both path and query parameters
    WithQuery,
}

/// Definition of an API route
#[derive(Debug, Clone)]
pub struct RouteDefinition {
    /// The topic pattern used for message routing (e.g., "rest.get.epochs.*")
    pub topic_pattern: &'static str,
    /// The REST API path with parameter placeholders (e.g., "/epochs/{number}")
    pub rest_path: &'static str,
    /// The MCP resource URI template (e.g., "blockfrost://epochs/{number}")
    pub mcp_uri_template: &'static str,
    /// Human-readable name for the resource
    pub name: &'static str,
    /// Description of what this endpoint returns
    pub description: &'static str,
    /// Handler type (with or without query parameters)
    pub handler_type: HandlerType,
    /// Handler function name (for reference/documentation)
    pub handler_name: &'static str,
    /// Parameter names in order (extracted from path)
    pub param_names: &'static [&'static str],
}

/// All registered API routes
pub const ROUTES: &[RouteDefinition] = &[
    // ==================== Accounts ====================
    RouteDefinition {
        topic_pattern: "rest.get.accounts.*",
        rest_path: "/accounts/{stake_address}",
        mcp_uri_template: "blockfrost://accounts/{stake_address}",
        name: "Account Information",
        description: "Obtain information about a specific stake account",
        handler_type: HandlerType::PathOnly,
        handler_name: "handle_single_account_blockfrost",
        param_names: &["stake_address"],
    },
    RouteDefinition {
        topic_pattern: "rest.get.accounts.*.registrations",
        rest_path: "/accounts/{stake_address}/registrations",
        mcp_uri_template: "blockfrost://accounts/{stake_address}/registrations",
        name: "Account Registrations",
        description: "Obtain information about the registrations of a specific account",
        handler_type: HandlerType::PathOnly,
        handler_name: "handle_account_registrations_blockfrost",
        param_names: &["stake_address"],
    },
    RouteDefinition {
        topic_pattern: "rest.get.accounts.*.delegations",
        rest_path: "/accounts/{stake_address}/delegations",
        mcp_uri_template: "blockfrost://accounts/{stake_address}/delegations",
        name: "Account Delegations",
        description: "Obtain information about the delegations of a specific account",
        handler_type: HandlerType::PathOnly,
        handler_name: "handle_account_delegations_blockfrost",
        param_names: &["stake_address"],
    },
    RouteDefinition {
        topic_pattern: "rest.get.accounts.*.mirs",
        rest_path: "/accounts/{stake_address}/mirs",
        mcp_uri_template: "blockfrost://accounts/{stake_address}/mirs",
        name: "Account MIRs",
        description: "Obtain information about MIRs of a specific account",
        handler_type: HandlerType::PathOnly,
        handler_name: "handle_account_mirs_blockfrost",
        param_names: &["stake_address"],
    },
    RouteDefinition {
        topic_pattern: "rest.get.accounts.*.withdrawals",
        rest_path: "/accounts/{stake_address}/withdrawals",
        mcp_uri_template: "blockfrost://accounts/{stake_address}/withdrawals",
        name: "Account Withdrawals",
        description: "Obtain information about the withdrawals of a specific account",
        handler_type: HandlerType::PathOnly,
        handler_name: "handle_account_withdrawals_blockfrost",
        param_names: &["stake_address"],
    },
    RouteDefinition {
        topic_pattern: "rest.get.accounts.*.rewards",
        rest_path: "/accounts/{stake_address}/rewards",
        mcp_uri_template: "blockfrost://accounts/{stake_address}/rewards",
        name: "Account Rewards",
        description: "Obtain information about the rewards history of a specific account",
        handler_type: HandlerType::PathOnly,
        handler_name: "handle_account_rewards_blockfrost",
        param_names: &["stake_address"],
    },
    RouteDefinition {
        topic_pattern: "rest.get.accounts.*.addresses",
        rest_path: "/accounts/{stake_address}/addresses",
        mcp_uri_template: "blockfrost://accounts/{stake_address}/addresses",
        name: "Account Addresses",
        description: "Obtain information about the addresses associated with a specific account",
        handler_type: HandlerType::PathOnly,
        handler_name: "handle_account_addresses_blockfrost",
        param_names: &["stake_address"],
    },
    RouteDefinition {
        topic_pattern: "rest.get.accounts.*.addresses.assets",
        rest_path: "/accounts/{stake_address}/addresses/assets",
        mcp_uri_template: "blockfrost://accounts/{stake_address}/addresses/assets",
        name: "Account Assets",
        description: "Obtain information about assets associated with addresses of a specific account",
        handler_type: HandlerType::PathOnly,
        handler_name: "handle_account_assets_blockfrost",
        param_names: &["stake_address"],
    },
    RouteDefinition {
        topic_pattern: "rest.get.accounts.*.addresses.total",
        rest_path: "/accounts/{stake_address}/addresses/total",
        mcp_uri_template: "blockfrost://accounts/{stake_address}/addresses/total",
        name: "Account Totals",
        description: "Obtain summed details about all addresses associated with a specific account",
        handler_type: HandlerType::PathOnly,
        handler_name: "handle_account_totals_blockfrost",
        param_names: &["stake_address"],
    },
    RouteDefinition {
        topic_pattern: "rest.get.accounts.*.utxos",
        rest_path: "/accounts/{stake_address}/utxos",
        mcp_uri_template: "blockfrost://accounts/{stake_address}/utxos",
        name: "Account UTXOs",
        description: "Obtain information about UTXOs of a specific account",
        handler_type: HandlerType::PathOnly,
        handler_name: "handle_account_utxos_blockfrost",
        param_names: &["stake_address"],
    },

    // ==================== Blocks ====================
    RouteDefinition {
        topic_pattern: "rest.get.blocks.*",
        rest_path: "/blocks/{hash_or_number}",
        mcp_uri_template: "blockfrost://blocks/{hash_or_number}",
        name: "Block Information",
        description: "Return the content of a requested block (use 'latest' for most recent)",
        handler_type: HandlerType::PathOnly,
        handler_name: "handle_blocks_latest_hash_number_blockfrost",
        param_names: &["hash_or_number"],
    },
    RouteDefinition {
        topic_pattern: "rest.get.blocks.*.txs",
        rest_path: "/blocks/{hash_or_number}/txs",
        mcp_uri_template: "blockfrost://blocks/{hash_or_number}/txs",
        name: "Block Transactions",
        description: "Return the transactions within the requested block",
        handler_type: HandlerType::WithQuery,
        handler_name: "handle_blocks_latest_hash_number_transactions_blockfrost",
        param_names: &["hash_or_number"],
    },
    RouteDefinition {
        topic_pattern: "rest.get.blocks.*.txs.cbor",
        rest_path: "/blocks/{hash_or_number}/txs/cbor",
        mcp_uri_template: "blockfrost://blocks/{hash_or_number}/txs/cbor",
        name: "Block Transactions CBOR",
        description: "Return the transactions within the requested block in CBOR format",
        handler_type: HandlerType::WithQuery,
        handler_name: "handle_blocks_latest_hash_number_transactions_cbor_blockfrost",
        param_names: &["hash_or_number"],
    },
    RouteDefinition {
        topic_pattern: "rest.get.blocks.*.next",
        rest_path: "/blocks/{hash_or_number}/next",
        mcp_uri_template: "blockfrost://blocks/{hash_or_number}/next",
        name: "Next Blocks",
        description: "Return the list of blocks following a specific block",
        handler_type: HandlerType::WithQuery,
        handler_name: "handle_blocks_hash_number_next_blockfrost",
        param_names: &["hash_or_number"],
    },
    RouteDefinition {
        topic_pattern: "rest.get.blocks.*.previous",
        rest_path: "/blocks/{hash_or_number}/previous",
        mcp_uri_template: "blockfrost://blocks/{hash_or_number}/previous",
        name: "Previous Blocks",
        description: "Return the list of blocks preceding a specific block",
        handler_type: HandlerType::WithQuery,
        handler_name: "handle_blocks_hash_number_previous_blockfrost",
        param_names: &["hash_or_number"],
    },
    RouteDefinition {
        topic_pattern: "rest.get.blocks.slot.*",
        rest_path: "/blocks/slot/{slot_number}",
        mcp_uri_template: "blockfrost://blocks/slot/{slot_number}",
        name: "Block by Slot",
        description: "Return the content of a requested block for a specific slot",
        handler_type: HandlerType::PathOnly,
        handler_name: "handle_blocks_slot_blockfrost",
        param_names: &["slot_number"],
    },
    RouteDefinition {
        topic_pattern: "rest.get.blocks.epoch.*.slot.*",
        rest_path: "/blocks/epoch/{epoch_number}/slot/{slot_number}",
        mcp_uri_template: "blockfrost://blocks/epoch/{epoch_number}/slot/{slot_number}",
        name: "Block by Epoch and Slot",
        description: "Return the content of a requested block for a specific epoch and slot",
        handler_type: HandlerType::PathOnly,
        handler_name: "handle_blocks_epoch_slot_blockfrost",
        param_names: &["epoch_number", "slot_number"],
    },
    RouteDefinition {
        topic_pattern: "rest.get.blocks.*.addresses",
        rest_path: "/blocks/{hash_or_number}/addresses",
        mcp_uri_template: "blockfrost://blocks/{hash_or_number}/addresses",
        name: "Block Addresses",
        description: "Return list of addresses affected in the specified block",
        handler_type: HandlerType::WithQuery,
        handler_name: "handle_blocks_hash_number_addresses_blockfrost",
        param_names: &["hash_or_number"],
    },

    // ==================== Governance - DReps ====================
    RouteDefinition {
        topic_pattern: "rest.get.governance.dreps",
        rest_path: "/governance/dreps",
        mcp_uri_template: "blockfrost://governance/dreps",
        name: "DReps List",
        description: "Return list of registered DReps",
        handler_type: HandlerType::PathOnly,
        handler_name: "handle_dreps_list_blockfrost",
        param_names: &[],
    },
    RouteDefinition {
        topic_pattern: "rest.get.governance.dreps.*",
        rest_path: "/governance/dreps/{drep_id}",
        mcp_uri_template: "blockfrost://governance/dreps/{drep_id}",
        name: "DRep Information",
        description: "Return information about a specific DRep",
        handler_type: HandlerType::PathOnly,
        handler_name: "handle_single_drep_blockfrost",
        param_names: &["drep_id"],
    },
    RouteDefinition {
        topic_pattern: "rest.get.governance.dreps.*.delegators",
        rest_path: "/governance/dreps/{drep_id}/delegators",
        mcp_uri_template: "blockfrost://governance/dreps/{drep_id}/delegators",
        name: "DRep Delegators",
        description: "Return list of delegators to a specific DRep",
        handler_type: HandlerType::PathOnly,
        handler_name: "handle_drep_delegators_blockfrost",
        param_names: &["drep_id"],
    },
    RouteDefinition {
        topic_pattern: "rest.get.governance.dreps.*.metadata",
        rest_path: "/governance/dreps/{drep_id}/metadata",
        mcp_uri_template: "blockfrost://governance/dreps/{drep_id}/metadata",
        name: "DRep Metadata",
        description: "Return metadata of a specific DRep",
        handler_type: HandlerType::PathOnly,
        handler_name: "handle_drep_metadata_blockfrost",
        param_names: &["drep_id"],
    },
    RouteDefinition {
        topic_pattern: "rest.get.governance.dreps.*.updates",
        rest_path: "/governance/dreps/{drep_id}/updates",
        mcp_uri_template: "blockfrost://governance/dreps/{drep_id}/updates",
        name: "DRep Updates",
        description: "Return list of updates to a specific DRep",
        handler_type: HandlerType::PathOnly,
        handler_name: "handle_drep_updates_blockfrost",
        param_names: &["drep_id"],
    },
    RouteDefinition {
        topic_pattern: "rest.get.governance.dreps.*.votes",
        rest_path: "/governance/dreps/{drep_id}/votes",
        mcp_uri_template: "blockfrost://governance/dreps/{drep_id}/votes",
        name: "DRep Votes",
        description: "Return list of votes cast by a specific DRep",
        handler_type: HandlerType::PathOnly,
        handler_name: "handle_drep_votes_blockfrost",
        param_names: &["drep_id"],
    },

    // ==================== Governance - Proposals ====================
    RouteDefinition {
        topic_pattern: "rest.get.governance.proposals",
        rest_path: "/governance/proposals",
        mcp_uri_template: "blockfrost://governance/proposals",
        name: "Proposals List",
        description: "Return list of governance proposals",
        handler_type: HandlerType::PathOnly,
        handler_name: "handle_proposals_list_blockfrost",
        param_names: &[],
    },
    RouteDefinition {
        topic_pattern: "rest.get.governance.proposals.*.*",
        rest_path: "/governance/proposals/{tx_hash}/{cert_index}",
        mcp_uri_template: "blockfrost://governance/proposals/{tx_hash}/{cert_index}",
        name: "Proposal Information",
        description: "Return information about a specific governance proposal",
        handler_type: HandlerType::PathOnly,
        handler_name: "handle_single_proposal_blockfrost",
        param_names: &["tx_hash", "cert_index"],
    },
    RouteDefinition {
        topic_pattern: "rest.get.governance.proposals.*.*.parameters",
        rest_path: "/governance/proposals/{tx_hash}/{cert_index}/parameters",
        mcp_uri_template: "blockfrost://governance/proposals/{tx_hash}/{cert_index}/parameters",
        name: "Proposal Parameters",
        description: "Return parameters of a specific governance proposal",
        handler_type: HandlerType::PathOnly,
        handler_name: "handle_proposal_parameters_blockfrost",
        param_names: &["tx_hash", "cert_index"],
    },
    RouteDefinition {
        topic_pattern: "rest.get.governance.proposals.*.*.withdrawals",
        rest_path: "/governance/proposals/{tx_hash}/{cert_index}/withdrawals",
        mcp_uri_template: "blockfrost://governance/proposals/{tx_hash}/{cert_index}/withdrawals",
        name: "Proposal Withdrawals",
        description: "Return withdrawals of a specific governance proposal",
        handler_type: HandlerType::PathOnly,
        handler_name: "handle_proposal_withdrawals_blockfrost",
        param_names: &["tx_hash", "cert_index"],
    },
    RouteDefinition {
        topic_pattern: "rest.get.governance.proposals.*.*.votes",
        rest_path: "/governance/proposals/{tx_hash}/{cert_index}/votes",
        mcp_uri_template: "blockfrost://governance/proposals/{tx_hash}/{cert_index}/votes",
        name: "Proposal Votes",
        description: "Return votes on a specific governance proposal",
        handler_type: HandlerType::PathOnly,
        handler_name: "handle_proposal_votes_blockfrost",
        param_names: &["tx_hash", "cert_index"],
    },
    RouteDefinition {
        topic_pattern: "rest.get.governance.proposals.*.*.metadata",
        rest_path: "/governance/proposals/{tx_hash}/{cert_index}/metadata",
        mcp_uri_template: "blockfrost://governance/proposals/{tx_hash}/{cert_index}/metadata",
        name: "Proposal Metadata",
        description: "Return metadata of a specific governance proposal",
        handler_type: HandlerType::PathOnly,
        handler_name: "handle_proposal_metadata_blockfrost",
        param_names: &["tx_hash", "cert_index"],
    },

    // ==================== Pools ====================
    RouteDefinition {
        topic_pattern: "rest.get.pools",
        rest_path: "/pools",
        mcp_uri_template: "blockfrost://pools",
        name: "Pools List",
        description: "Return list of registered stake pools",
        handler_type: HandlerType::PathOnly,
        handler_name: "handle_pools_list_blockfrost",
        param_names: &[],
    },
    RouteDefinition {
        topic_pattern: "rest.get.pools.*",
        rest_path: "/pools/{pool_id}",
        mcp_uri_template: "blockfrost://pools/{pool_id}",
        name: "Pool Information",
        description: "Return information about a specific pool (also handles extended/retired/retiring)",
        handler_type: HandlerType::PathOnly,
        handler_name: "handle_pools_extended_retired_retiring_single_blockfrost",
        param_names: &["pool_id"],
    },
    RouteDefinition {
        topic_pattern: "rest.get.pools.*.history",
        rest_path: "/pools/{pool_id}/history",
        mcp_uri_template: "blockfrost://pools/{pool_id}/history",
        name: "Pool History",
        description: "Return history of a specific pool",
        handler_type: HandlerType::PathOnly,
        handler_name: "handle_pool_history_blockfrost",
        param_names: &["pool_id"],
    },
    RouteDefinition {
        topic_pattern: "rest.get.pools.*.metadata",
        rest_path: "/pools/{pool_id}/metadata",
        mcp_uri_template: "blockfrost://pools/{pool_id}/metadata",
        name: "Pool Metadata",
        description: "Return metadata of a specific pool",
        handler_type: HandlerType::PathOnly,
        handler_name: "handle_pool_metadata_blockfrost",
        param_names: &["pool_id"],
    },
    RouteDefinition {
        topic_pattern: "rest.get.pools.*.relays",
        rest_path: "/pools/{pool_id}/relays",
        mcp_uri_template: "blockfrost://pools/{pool_id}/relays",
        name: "Pool Relays",
        description: "Return relays of a specific pool",
        handler_type: HandlerType::PathOnly,
        handler_name: "handle_pool_relays_blockfrost",
        param_names: &["pool_id"],
    },
    RouteDefinition {
        topic_pattern: "rest.get.pools.*.delegators",
        rest_path: "/pools/{pool_id}/delegators",
        mcp_uri_template: "blockfrost://pools/{pool_id}/delegators",
        name: "Pool Delegators",
        description: "Return list of delegators to a specific pool",
        handler_type: HandlerType::PathOnly,
        handler_name: "handle_pool_delegators_blockfrost",
        param_names: &["pool_id"],
    },
    RouteDefinition {
        topic_pattern: "rest.get.pools.*.blocks",
        rest_path: "/pools/{pool_id}/blocks",
        mcp_uri_template: "blockfrost://pools/{pool_id}/blocks",
        name: "Pool Blocks",
        description: "Return list of blocks minted by a specific pool",
        handler_type: HandlerType::PathOnly,
        handler_name: "handle_pool_blocks_blockfrost",
        param_names: &["pool_id"],
    },
    RouteDefinition {
        topic_pattern: "rest.get.pools.*.updates",
        rest_path: "/pools/{pool_id}/updates",
        mcp_uri_template: "blockfrost://pools/{pool_id}/updates",
        name: "Pool Updates",
        description: "Return list of certificate updates to a specific pool",
        handler_type: HandlerType::PathOnly,
        handler_name: "handle_pool_updates_blockfrost",
        param_names: &["pool_id"],
    },
    RouteDefinition {
        topic_pattern: "rest.get.pools.*.votes",
        rest_path: "/pools/{pool_id}/votes",
        mcp_uri_template: "blockfrost://pools/{pool_id}/votes",
        name: "Pool Votes",
        description: "Return list of votes cast by a specific pool",
        handler_type: HandlerType::PathOnly,
        handler_name: "handle_pool_votes_blockfrost",
        param_names: &["pool_id"],
    },

    // ==================== Epochs ====================
    RouteDefinition {
        topic_pattern: "rest.get.epochs.*",
        rest_path: "/epochs/{number}",
        mcp_uri_template: "blockfrost://epochs/{number}",
        name: "Epoch Information",
        description: "Return information about an epoch (use 'latest' for current epoch)",
        handler_type: HandlerType::PathOnly,
        handler_name: "handle_epoch_info_blockfrost",
        param_names: &["number"],
    },
    RouteDefinition {
        topic_pattern: "rest.get.epochs.*.parameters",
        rest_path: "/epochs/{number}/parameters",
        mcp_uri_template: "blockfrost://epochs/{number}/parameters",
        name: "Epoch Parameters",
        description: "Return the protocol parameters for the epoch",
        handler_type: HandlerType::PathOnly,
        handler_name: "handle_epoch_params_blockfrost",
        param_names: &["number"],
    },
    RouteDefinition {
        topic_pattern: "rest.get.epochs.*.next",
        rest_path: "/epochs/{number}/next",
        mcp_uri_template: "blockfrost://epochs/{number}/next",
        name: "Next Epochs",
        description: "Return list of epochs following a specific epoch",
        handler_type: HandlerType::PathOnly,
        handler_name: "handle_epoch_next_blockfrost",
        param_names: &["number"],
    },
    RouteDefinition {
        topic_pattern: "rest.get.epochs.*.previous",
        rest_path: "/epochs/{number}/previous",
        mcp_uri_template: "blockfrost://epochs/{number}/previous",
        name: "Previous Epochs",
        description: "Return list of epochs preceding a specific epoch",
        handler_type: HandlerType::PathOnly,
        handler_name: "handle_epoch_previous_blockfrost",
        param_names: &["number"],
    },
    RouteDefinition {
        topic_pattern: "rest.get.epochs.*.stakes",
        rest_path: "/epochs/{number}/stakes",
        mcp_uri_template: "blockfrost://epochs/{number}/stakes",
        name: "Epoch Stakes",
        description: "Return the stake distribution for the epoch",
        handler_type: HandlerType::PathOnly,
        handler_name: "handle_epoch_total_stakes_blockfrost",
        param_names: &["number"],
    },
    RouteDefinition {
        topic_pattern: "rest.get.epochs.*.stakes.*",
        rest_path: "/epochs/{number}/stakes/{pool_id}",
        mcp_uri_template: "blockfrost://epochs/{number}/stakes/{pool_id}",
        name: "Epoch Pool Stakes",
        description: "Return the stake distribution for a specific pool in the epoch",
        handler_type: HandlerType::PathOnly,
        handler_name: "handle_epoch_pool_stakes_blockfrost",
        param_names: &["number", "pool_id"],
    },
    RouteDefinition {
        topic_pattern: "rest.get.epochs.*.blocks",
        rest_path: "/epochs/{number}/blocks",
        mcp_uri_template: "blockfrost://epochs/{number}/blocks",
        name: "Epoch Blocks",
        description: "Return list of blocks within the epoch",
        handler_type: HandlerType::PathOnly,
        handler_name: "handle_epoch_total_blocks_blockfrost",
        param_names: &["number"],
    },
    RouteDefinition {
        topic_pattern: "rest.get.epochs.*.blocks.*",
        rest_path: "/epochs/{number}/blocks/{pool_id}",
        mcp_uri_template: "blockfrost://epochs/{number}/blocks/{pool_id}",
        name: "Epoch Pool Blocks",
        description: "Return list of blocks minted by a specific pool in the epoch",
        handler_type: HandlerType::PathOnly,
        handler_name: "handle_epoch_pool_blocks_blockfrost",
        param_names: &["number", "pool_id"],
    },

    // ==================== Assets ====================
    RouteDefinition {
        topic_pattern: "rest.get.assets",
        rest_path: "/assets",
        mcp_uri_template: "blockfrost://assets",
        name: "Assets List",
        description: "Return list of assets",
        handler_type: HandlerType::PathOnly,
        handler_name: "handle_assets_list_blockfrost",
        param_names: &[],
    },
    RouteDefinition {
        topic_pattern: "rest.get.assets.*",
        rest_path: "/assets/{asset}",
        mcp_uri_template: "blockfrost://assets/{asset}",
        name: "Asset Information",
        description: "Return information about a specific asset",
        handler_type: HandlerType::PathOnly,
        handler_name: "handle_asset_single_blockfrost",
        param_names: &["asset"],
    },
    RouteDefinition {
        topic_pattern: "rest.get.assets.*.history",
        rest_path: "/assets/{asset}/history",
        mcp_uri_template: "blockfrost://assets/{asset}/history",
        name: "Asset History",
        description: "Return history of a specific asset",
        handler_type: HandlerType::PathOnly,
        handler_name: "handle_asset_history_blockfrost",
        param_names: &["asset"],
    },
    RouteDefinition {
        topic_pattern: "rest.get.assets.*.transactions",
        rest_path: "/assets/{asset}/transactions",
        mcp_uri_template: "blockfrost://assets/{asset}/transactions",
        name: "Asset Transactions",
        description: "Return list of transactions involving a specific asset",
        handler_type: HandlerType::PathOnly,
        handler_name: "handle_asset_transactions_blockfrost",
        param_names: &["asset"],
    },
    RouteDefinition {
        topic_pattern: "rest.get.assets.*.addresses",
        rest_path: "/assets/{asset}/addresses",
        mcp_uri_template: "blockfrost://assets/{asset}/addresses",
        name: "Asset Addresses",
        description: "Return list of addresses holding a specific asset",
        handler_type: HandlerType::PathOnly,
        handler_name: "handle_asset_addresses_blockfrost",
        param_names: &["asset"],
    },
    RouteDefinition {
        topic_pattern: "rest.get.assets.policy.*",
        rest_path: "/assets/policy/{policy_id}",
        mcp_uri_template: "blockfrost://assets/policy/{policy_id}",
        name: "Policy Assets",
        description: "Return list of assets under a specific policy",
        handler_type: HandlerType::PathOnly,
        handler_name: "handle_policy_assets_blockfrost",
        param_names: &["policy_id"],
    },

    // ==================== Addresses ====================
    RouteDefinition {
        topic_pattern: "rest.get.addresses.*",
        rest_path: "/addresses/{address}",
        mcp_uri_template: "blockfrost://addresses/{address}",
        name: "Address Information",
        description: "Return information about a specific address",
        handler_type: HandlerType::PathOnly,
        handler_name: "handle_address_single_blockfrost",
        param_names: &["address"],
    },
    RouteDefinition {
        topic_pattern: "rest.get.addresses.*.extended",
        rest_path: "/addresses/{address}/extended",
        mcp_uri_template: "blockfrost://addresses/{address}/extended",
        name: "Address Extended",
        description: "Return extended information about a specific address",
        handler_type: HandlerType::PathOnly,
        handler_name: "handle_address_extended_blockfrost",
        param_names: &["address"],
    },
    RouteDefinition {
        topic_pattern: "rest.get.addresses.*.total",
        rest_path: "/addresses/{address}/total",
        mcp_uri_template: "blockfrost://addresses/{address}/total",
        name: "Address Totals",
        description: "Return total amounts sent/received by an address",
        handler_type: HandlerType::PathOnly,
        handler_name: "handle_address_totals_blockfrost",
        param_names: &["address"],
    },
    RouteDefinition {
        topic_pattern: "rest.get.addresses.*.utxos",
        rest_path: "/addresses/{address}/utxos",
        mcp_uri_template: "blockfrost://addresses/{address}/utxos",
        name: "Address UTXOs",
        description: "Return UTXOs of a specific address",
        handler_type: HandlerType::PathOnly,
        handler_name: "handle_address_utxos_blockfrost",
        param_names: &["address"],
    },
    RouteDefinition {
        topic_pattern: "rest.get.addresses.*.utxos.*",
        rest_path: "/addresses/{address}/utxos/{asset}",
        mcp_uri_template: "blockfrost://addresses/{address}/utxos/{asset}",
        name: "Address Asset UTXOs",
        description: "Return UTXOs of a specific address containing a specific asset",
        handler_type: HandlerType::PathOnly,
        handler_name: "handle_address_asset_utxos_blockfrost",
        param_names: &["address", "asset"],
    },
    RouteDefinition {
        topic_pattern: "rest.get.addresses.*.transactions",
        rest_path: "/addresses/{address}/transactions",
        mcp_uri_template: "blockfrost://addresses/{address}/transactions",
        name: "Address Transactions",
        description: "Return transactions involving a specific address",
        handler_type: HandlerType::PathOnly,
        handler_name: "handle_address_transactions_blockfrost",
        param_names: &["address"],
    },

    // ==================== Transactions ====================
    RouteDefinition {
        topic_pattern: "rest.get.txs.*",
        rest_path: "/txs/{hash}",
        mcp_uri_template: "blockfrost://txs/{hash}",
        name: "Transaction Information",
        description: "Return content of a specific transaction",
        handler_type: HandlerType::PathOnly,
        handler_name: "handle_transactions_blockfrost",
        param_names: &["hash"],
    },
    RouteDefinition {
        topic_pattern: "rest.get.txs.*.*",
        rest_path: "/txs/{hash}/{sub}",
        mcp_uri_template: "blockfrost://txs/{hash}/{sub}",
        name: "Transaction Sub-resource",
        description: "Return specific sub-resource of a transaction (utxos, stakes, delegations, withdrawals, mirs, pool_updates, pool_retires, metadata, redeemers, required_signers)",
        handler_type: HandlerType::PathOnly,
        handler_name: "handle_transactions_blockfrost",
        param_names: &["hash", "sub"],
    },
    RouteDefinition {
        topic_pattern: "rest.get.txs.metadata.*",
        rest_path: "/txs/metadata/{label}",
        mcp_uri_template: "blockfrost://txs/metadata/{label}",
        name: "Transaction Metadata by Label",
        description: "Return transaction metadata for a specific label",
        handler_type: HandlerType::PathOnly,
        handler_name: "handle_transactions_blockfrost",
        param_names: &["label"],
    },
];

/// Find a route by its topic pattern
pub fn find_route_by_topic(topic: &str) -> Option<&'static RouteDefinition> {
    ROUTES.iter().find(|r| r.topic_pattern == topic)
}

/// Find a route by its REST path pattern
pub fn find_route_by_rest_path(path: &str) -> Option<&'static RouteDefinition> {
    ROUTES.iter().find(|r| r.rest_path == path)
}

/// Find a route by its MCP URI template
pub fn find_route_by_mcp_uri(uri_template: &str) -> Option<&'static RouteDefinition> {
    ROUTES.iter().find(|r| r.mcp_uri_template == uri_template)
}

/// Get all routes for a specific category (accounts, blocks, epochs, etc.)
pub fn get_routes_by_category(category: &str) -> Vec<&'static RouteDefinition> {
    let prefix = format!("rest.get.{category}.");
    let exact = format!("rest.get.{category}");
    ROUTES
        .iter()
        .filter(|r| r.topic_pattern.starts_with(&prefix) || r.topic_pattern == exact)
        .collect()
}

/// Convert an MCP URI to parameters by matching against the template
/// Returns None if the URI doesn't match the template
pub fn extract_params_from_uri(uri: &str, template: &str) -> Option<Vec<String>> {
    // Remove the scheme prefix for matching
    let uri_path = uri.strip_prefix("blockfrost://")?;
    let template_path = template.strip_prefix("blockfrost://")?;

    let uri_parts: Vec<&str> = uri_path.split('/').collect();
    let template_parts: Vec<&str> = template_path.split('/').collect();

    if uri_parts.len() != template_parts.len() {
        return None;
    }

    let mut params = Vec::new();
    for (uri_part, template_part) in uri_parts.iter().zip(template_parts.iter()) {
        if template_part.starts_with('{') && template_part.ends_with('}') {
            // This is a parameter placeholder
            params.push(uri_part.to_string());
        } else if uri_part != template_part {
            // Literal parts don't match
            return None;
        }
    }

    Some(params)
}

/// Match a URI against all routes and return the matching route with extracted parameters
pub fn match_uri(uri: &str) -> Option<(&'static RouteDefinition, Vec<String>)> {
    for route in ROUTES {
        if let Some(params) = extract_params_from_uri(uri, route.mcp_uri_template) {
            return Some((route, params));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_params_simple() {
        let params =
            extract_params_from_uri("blockfrost://epochs/latest", "blockfrost://epochs/{number}");
        assert_eq!(params, Some(vec!["latest".to_string()]));
    }

    #[test]
    fn test_extract_params_multiple() {
        let params = extract_params_from_uri(
            "blockfrost://epochs/507/stakes/pool123",
            "blockfrost://epochs/{number}/stakes/{pool_id}",
        );
        assert_eq!(params, Some(vec!["507".to_string(), "pool123".to_string()]));
    }

    #[test]
    fn test_extract_params_no_match() {
        let params = extract_params_from_uri(
            "blockfrost://epochs/507/blocks",
            "blockfrost://epochs/{number}/stakes",
        );
        assert_eq!(params, None);
    }

    #[test]
    fn test_match_uri() {
        let (route, params) = match_uri("blockfrost://epochs/latest").unwrap();
        assert_eq!(route.handler_name, "handle_epoch_info_blockfrost");
        assert_eq!(params, vec!["latest".to_string()]);
    }

    #[test]
    fn test_find_route_by_topic() {
        let route = find_route_by_topic("rest.get.epochs.*").unwrap();
        assert_eq!(route.name, "Epoch Information");
    }
}
