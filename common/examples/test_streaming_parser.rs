// Example: Test streaming snapshot parser with large snapshot
//
// Usage: cargo run --example test_streaming_parser --release -- <snapshot_path>
use acropolis_common::{
    epoch_snapshot::SnapshotsContainer,
    ledger_state::SPOState,
    snapshot::{
        streaming_snapshot::{AccountsCallback, GovernanceProtocolParametersCallback},
        utxo::UtxoEntry,
        AccountState, DRepCallback, EpochCallback, GovernanceProposal, PoolCallback,
        ProposalCallback, SnapshotCallbacks, SnapshotMetadata, SnapshotsCallback,
        StreamingSnapshotParser, UtxoCallback,
    },
    DRepCredential, NetworkId, PoolRegistration, ProtocolParamUpdate, RewardParams,
};
use anyhow::Result;
use std::collections::HashMap;
use std::env;
use std::time::Instant;
use tracing::info;

use acropolis_common::{DRepRecord, EpochBootstrapData};
use env_logger::Env;

// Simple counter callback that doesn't store data in memory
#[derive(Default)]
struct CountingCallbacks {
    metadata: Option<SnapshotMetadata>,
    utxo_count: u64,
    pool_count: usize,
    future_pool_count: usize,
    retiring_pool_count: usize,
    account_count: usize,
    drep_count: usize,
    proposal_count: usize,
    sample_utxos: Vec<UtxoEntry>,
    sample_pools: Vec<PoolRegistration>,
    sample_accounts: Vec<AccountState>,
    sample_dreps: Vec<(DRepCredential, DRepRecord)>,
    sample_proposals: Vec<GovernanceProposal>,
    previous_reward_params: Option<RewardParams>,
    current_reward_params: Option<RewardParams>,
    params: Option<ProtocolParamUpdate>,
}

impl UtxoCallback for CountingCallbacks {
    fn on_utxo(&mut self, utxo: UtxoEntry) -> Result<()> {
        self.utxo_count += 1;
        // Keep first 10 for display
        if self.sample_utxos.len() < 10 {
            if self.sample_utxos.len() < 10 {
                let addr_bytes = utxo.address_bytes();
                eprintln!(
                    "  UTXO #{}: {}:{} → {} ({} lovelace)",
                    self.utxo_count,
                    &utxo.tx_hash_hex()[..16],
                    utxo.output_index(),
                    hex::encode(&addr_bytes[..addr_bytes.len().min(16)]),
                    utxo.coin()
                );
            }
            self.sample_utxos.push(utxo);
        }
        // Progress reporting every million UTXOs
        if self.utxo_count > 0 && self.utxo_count.is_multiple_of(1000000) {
            eprintln!("  Parsed {} UTXOs...", self.utxo_count);
        }
        Ok(())
    }
}

impl PoolCallback for CountingCallbacks {
    fn on_pools(&mut self, pools: SPOState) -> Result<()> {
        self.pool_count = pools.pools.len();
        self.future_pool_count = pools.updates.len();
        self.retiring_pool_count = pools.retiring.len();
        eprintln!(
            "Parsed {} stake pools (future: {}, retiring: {}))",
            pools.pools.len(),
            pools.updates.len(),
            pools.retiring.len()
        );

        // Keep first 10 for summary
        self.sample_pools = pools.pools.into_iter().take(10).map(|(_, v)| v).collect();

        // Show sample pools
        for (i, pool) in self.sample_pools.clone().iter().enumerate() {
            eprintln!(
                "  Pool #{}: {} (pledge: {}, cost: {}, margin: {:?})",
                i + 1,
                pool.operator,
                pool.pledge,
                pool.cost,
                pool.margin
            );
        }

        Ok(())
    }
}

impl AccountsCallback for CountingCallbacks {
    fn on_accounts(
        &mut self,
        data: acropolis_common::snapshot::AccountsBootstrapData,
    ) -> Result<()> {
        self.account_count = data.accounts.len();
        if !data.accounts.is_empty() {
            eprintln!("Parsed {} stake accounts", data.accounts.len());

            // Show first 10 accounts
            for (i, account) in data.accounts.iter().take(10).enumerate() {
                eprintln!(
                    "  Account #{}: {} (utxo: {}, rewards: {}, pool: {:?}, drep: {:?})",
                    i + 1,
                    &account.stake_address.to_string().unwrap(),
                    account.address_state.utxo_value,
                    account.address_state.rewards,
                    account.address_state.delegated_spo.as_ref().map(|s| &s[..16]),
                    account.address_state.delegated_drep
                );
            }
        }

        eprintln!(
            "AccountsBootstrapData: epoch={}, pools={}, retiring={}, dreps={}, snapshots={}",
            data.epoch,
            data.pools.len(),
            data.retiring_pools.len(),
            data.dreps.len(),
            data.snapshots
        );

        // Keep first 10 for summary
        self.sample_accounts = data.accounts.into_iter().take(10).collect();
        Ok(())
    }
}

impl DRepCallback for CountingCallbacks {
    fn on_dreps(&mut self, epoch: u64, dreps: HashMap<DRepCredential, DRepRecord>) -> Result<()> {
        self.drep_count = dreps.len();
        eprintln!("Parsed {} DReps for epoch {}", self.drep_count, epoch);

        // Show first 10 DReps
        for (i, (cred, record)) in dreps.iter().take(10).enumerate() {
            let drep_id = cred.to_drep_bech32().unwrap_or_else(|_| "invalid_cred".to_string());
            if let Some(anchor) = &record.anchor {
                eprintln!(
                    "  DRep #{}: {} (deposit: {}) - {}",
                    i + 1,
                    drep_id,
                    record.deposit,
                    anchor.url
                );
            } else {
                eprintln!(
                    "  DRep #{}: {} (deposit: {})",
                    i + 1,
                    drep_id,
                    record.deposit
                );
            }
        }

        // Keep first 10 for summary
        self.sample_dreps = dreps.into_iter().take(10).collect();
        Ok(())
    }
}

impl ProposalCallback for CountingCallbacks {
    fn on_proposals(&mut self, proposals: Vec<GovernanceProposal>) -> Result<()> {
        self.proposal_count = proposals.len();
        if !proposals.is_empty() {
            eprintln!("Parsed {} governance proposals", proposals.len());

            // Show first 10 proposals
            for (i, proposal) in proposals.iter().take(10).enumerate() {
                eprintln!(
                    "  Proposal #{}: {} (deposit: {}, action: {}, by: {})",
                    i + 1,
                    proposal.gov_action_id,
                    proposal.deposit,
                    proposal.gov_action,
                    &proposal.reward_account[..32]
                );
            }
        }

        // Keep first 10 for summary
        self.sample_proposals = proposals.into_iter().take(10).collect();
        Ok(())
    }
}

impl GovernanceProtocolParametersCallback for CountingCallbacks {
    fn on_gs_protocol_parameters(
        &mut self,
        epoch: u64,
        previous_reward_params: RewardParams,
        current_reward_params: RewardParams,
        params: ProtocolParamUpdate,
    ) -> Result<()> {
        eprintln!("\n=== Governance Protocol Parameters for epoch {epoch} ===\n");

        eprintln!("Previous Reward Parameters:");
        eprintln!("  Expansion rate {}", previous_reward_params.expansion_rate);
        eprintln!(
            "  Treasury growth rate {}",
            previous_reward_params.treasury_growth_rate
        );
        eprintln!(
            "  Desired number of pools: {}",
            previous_reward_params.desired_number_of_stake_pools
        );
        eprintln!(
            "  Pledge influence: {}",
            previous_reward_params.pool_pledge_influence
        );
        eprintln!("  Min pool cost: {}", previous_reward_params.min_pool_cost);

        eprintln!("\nCurrent Reward Parameters:");
        eprintln!("  Expansion rate: {}", current_reward_params.expansion_rate);
        eprintln!(
            "  Treasury growth rate: {}",
            current_reward_params.treasury_growth_rate
        );
        eprintln!(
            "  Desired number of pools: {}",
            current_reward_params.desired_number_of_stake_pools
        );
        eprintln!(
            "  Pledge influence: {}",
            current_reward_params.pool_pledge_influence
        );
        eprintln!("  Min pool cost: {}", current_reward_params.min_pool_cost);

        eprintln!("\nBootstrap Parameters:");
        eprintln!("  Min fee a: {}", params.minfee_a.unwrap());
        eprintln!("  Min fee b: {}", params.minfee_b.unwrap());
        eprintln!(
            "  Max block body size: {}",
            params.max_block_body_size.unwrap()
        );
        eprintln!(
            "  Max transaction size: {}",
            params.max_transaction_size.unwrap()
        );
        eprintln!(
            "  Max block header size: {}",
            params.max_block_header_size.unwrap()
        );
        eprintln!("  Key deposit: {}", params.key_deposit.unwrap());
        eprintln!("  Pool deposit: {}", params.pool_deposit.unwrap());
        eprintln!("  Pool max retire epoch: {}", params.maximum_epoch.unwrap());
        eprintln!(
            "  Desired number of stake pools: {}",
            params.desired_number_of_stake_pools.unwrap()
        );
        eprintln!(
            "  Pool pledge influence: {}",
            params.pool_pledge_influence.clone().unwrap()
        );
        eprintln!(
            "  Monetary expansion rate: {}",
            params.expansion_rate.clone().unwrap()
        );
        eprintln!(
            "  Treasury growth rate: {}",
            params.treasury_growth_rate.clone().unwrap()
        );
        eprintln!("  Min pool cost: {}", params.min_pool_cost.unwrap());
        eprintln!(
            "  Execution prices: {:?}",
            params.execution_costs.clone().unwrap()
        );
        eprintln!(
            "  Max execution units: {:?}",
            params.max_block_ex_units.unwrap()
        );
        eprintln!("  Max value size: {:?}", params.max_value_size.unwrap());
        eprintln!(
            "  Collateral percentage: {:?}",
            params.collateral_percentage.unwrap()
        );
        eprintln!(
            "  Max collateral inputs: {:?}",
            params.max_collateral_inputs.unwrap()
        );
        eprintln!(
            "  Coins per UTxO byte: {:?}",
            params.coins_per_utxo_byte.unwrap()
        );
        eprintln!(
            "  Min committee size: {:?}",
            params.min_committee_size.unwrap()
        );
        eprintln!(
            "  Max committee term limit: {:?}",
            params.committee_term_limit.unwrap()
        );
        eprintln!(
            "  Governance action validity period: {:?}",
            params.governance_action_validity_period.unwrap()
        );
        eprintln!(
            "  Governance action deposit: {:?}",
            params.governance_action_deposit.unwrap()
        );
        eprintln!("  DRep deposit: {:?}", params.drep_deposit.unwrap());
        eprintln!(
            "  DRep inactivity period: {:?}",
            params.drep_inactivity_period.unwrap()
        );
        eprintln!(
            "  Min fee refscript cost per byte: {:?}",
            params.minfee_refscript_cost_per_byte.clone().unwrap()
        );
        eprintln!(
            "  Decentralization constant: {:?}",
            params.decentralisation_constant.clone().unwrap()
        );
        eprintln!(
            "  Protocol version: {:?}",
            params.protocol_version.clone().unwrap()
        );

        // Store for later display
        self.previous_reward_params = Some(previous_reward_params);
        self.current_reward_params = Some(current_reward_params);
        self.params = Some(params);

        eprintln!("\n=== End Protocol Parameters ===\n");
        Ok(())
    }
}

impl acropolis_common::snapshot::GovernanceStateCallback for CountingCallbacks {
    fn on_governance_state(
        &mut self,
        state: acropolis_common::snapshot::GovernanceState,
    ) -> Result<()> {
        eprintln!(
            "Governance State: epoch {}, {} proposals, {} votes",
            state.epoch,
            state.proposals.len(),
            state.votes.len()
        );
        Ok(())
    }
}

impl EpochCallback for CountingCallbacks {
    fn on_epoch(&mut self, data: EpochBootstrapData) -> Result<()> {
        info!(
            "Received epoch bootstrap data for epoch {}: {} current epoch blocks, {} previous epoch blocks",
            data.epoch,
            data.total_blocks_current,
            data.total_blocks_previous
        );
        Ok(())
    }
}

impl SnapshotCallbacks for CountingCallbacks {
    fn on_metadata(&mut self, metadata: SnapshotMetadata) -> Result<()> {
        eprintln!("Snapshot Metadata:");
        eprintln!("  Epoch: {}", metadata.epoch);
        eprintln!(
            "  Treasury: {} ADA",
            metadata.pot_balances.treasury as f64 / 1_000_000.0
        );
        eprintln!(
            "  Reserves: {} ADA",
            metadata.pot_balances.reserves as f64 / 1_000_000.0
        );
        eprintln!(
            "  Deposits: {} ADA",
            metadata.pot_balances.deposits as f64 / 1_000_000.0
        );
        if let Some(count) = metadata.utxo_count {
            eprintln!("  UTXO count: {count}");
        }
        // Calculate total blocks produced
        let total_blocks_previous: u32 =
            metadata.blocks_previous_epoch.iter().map(|p| p.block_count as u32).sum();
        let total_blocks_current: u32 =
            metadata.blocks_current_epoch.iter().map(|p| p.block_count as u32).sum();

        eprintln!(
            "  Block production previous epoch: {} pools produced {} blocks total",
            metadata.blocks_previous_epoch.len(),
            total_blocks_previous
        );
        eprintln!(
            "  Block production current epoch: {} pools produced {} blocks total",
            metadata.blocks_current_epoch.len(),
            total_blocks_current
        );

        // Show top block producers if any
        if !metadata.blocks_previous_epoch.is_empty() {
            eprintln!("  Previous epoch top producers (first 3):");
            let mut sorted_previous = metadata.blocks_previous_epoch.clone();
            sorted_previous.sort_by(|a, b| b.block_count.cmp(&a.block_count));
            for (i, production) in sorted_previous.iter().take(3).enumerate() {
                eprintln!(
                    "    [{}] Pool {} produced {} blocks (epoch {})",
                    i + 1,
                    &production.pool_id,
                    production.block_count,
                    production.epoch
                );
            }
            if metadata.blocks_previous_epoch.len() > 3 {
                eprintln!(
                    "    ... and {} more pools",
                    metadata.blocks_previous_epoch.len() - 3
                );
            }
        }

        if !metadata.blocks_current_epoch.is_empty() {
            eprintln!("  Current epoch top producers (first 3):");
            let mut sorted_current = metadata.blocks_current_epoch.clone();
            sorted_current.sort_by(|a, b| b.block_count.cmp(&a.block_count));
            for (i, production) in sorted_current.iter().take(3).enumerate() {
                eprintln!(
                    "    [{}] Pool {} produced {} blocks (epoch {})",
                    i + 1,
                    &production.pool_id,
                    production.block_count,
                    production.epoch
                );
            }
            if metadata.blocks_current_epoch.len() > 3 {
                eprintln!(
                    "    ... and {} more pools",
                    metadata.blocks_current_epoch.len() - 3
                );
            }
        }
        eprintln!();

        self.metadata = Some(metadata);
        Ok(())
    }

    fn on_complete(&mut self) -> Result<()> {
        Ok(())
    }
}

impl SnapshotsCallback for CountingCallbacks {
    fn on_snapshots(&mut self, snapshots: SnapshotsContainer) -> Result<()> {
        eprintln!("Snapshots Data:");
        eprintln!();

        eprintln!("Mark Snapshot (epoch {}):", snapshots.mark.epoch);
        eprintln!("  SPOs: {}", snapshots.mark.spos.len());
        let mark_total: u64 = snapshots.mark.spos.values().map(|spo| spo.total_stake).sum();
        eprintln!("  Total stake: {:.2} ADA", mark_total as f64 / 1_000_000.0);
        eprintln!();

        eprintln!("Set Snapshot (epoch {}):", snapshots.set.epoch);
        eprintln!("  SPOs: {}", snapshots.set.spos.len());
        let set_total: u64 = snapshots.set.spos.values().map(|spo| spo.total_stake).sum();
        eprintln!("  Total stake: {:.2} ADA", set_total as f64 / 1_000_000.0);
        eprintln!();

        eprintln!("Go Snapshot (epoch {}):", snapshots.go.epoch);
        eprintln!("  SPOs: {}", snapshots.go.spos.len());
        let go_total: u64 = snapshots.go.spos.values().map(|spo| spo.total_stake).sum();
        eprintln!("  Total stake: {:.2} ADA", go_total as f64 / 1_000_000.0);
        eprintln!();

        Ok(())
    }
}

fn main() {
    // Get snapshot path from command line
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <snapshot_path>", args[0]);
        eprintln!("Example: {} tests/fixtures/134092758.*.cbor", args[0]);
        std::process::exit(1);
    }

    // Initialize env_logger to read RUST_LOG environment variable
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    let snapshot_path = &args[1];
    println!("Streaming Snapshot Parser Test with Block Parsing");
    println!("====================================================");
    println!("Snapshot: {snapshot_path}");
    println!("Features: UTXOs, Pools, Accounts, DReps, Proposals, and BLOCKS!");
    println!();

    // Create parser and callbacks
    let parser = StreamingSnapshotParser::new(snapshot_path);
    let mut callbacks = CountingCallbacks::default();

    // Parse with timing
    println!("Starting parse...");
    let start = Instant::now();

    match parser.parse(&mut callbacks, NetworkId::Mainnet) {
        Ok(()) => {
            let duration = start.elapsed();
            println!("Parse completed successfully in {duration:.2?}");
            println!();

            // Display results
            if let Some(metadata) = &callbacks.metadata {
                println!("Final Metadata Summary:");
                println!("  Epoch: {}", metadata.epoch);
                println!("  Treasury: {} lovelace", metadata.pot_balances.treasury);
                println!("  Reserves: {} lovelace", metadata.pot_balances.reserves);
                println!("  Deposits: {} lovelace", metadata.pot_balances.deposits);
                if let Some(count) = metadata.utxo_count {
                    println!("  UTXO Count (metadata): {count}");
                }
                let total_blocks_previous: u32 =
                    metadata.blocks_previous_epoch.iter().map(|p| p.block_count as u32).sum();
                let total_blocks_current: u32 =
                    metadata.blocks_current_epoch.iter().map(|p| p.block_count as u32).sum();
                println!(
                    "  Block production previous epoch: {} pools, {} blocks total",
                    metadata.blocks_previous_epoch.len(),
                    total_blocks_previous
                );
                println!(
                    "  Block production current epoch: {} pools, {} blocks total",
                    metadata.blocks_current_epoch.len(),
                    total_blocks_current
                );

                println!();
            }

            println!("Parsed Data Summary:");
            println!("  UTXOs: {}", callbacks.utxo_count);
            println!("  Stake Pools: {}", callbacks.pool_count);
            println!("  Stake Accounts: {}", callbacks.account_count);
            println!("  DReps: {}", callbacks.drep_count);
            println!("  Governance Proposals: {}", callbacks.proposal_count);
            println!();

            // Show sample UTXOs
            if !callbacks.sample_utxos.is_empty() {
                println!("Sample UTXOs (first 10):");
                for (i, utxo) in callbacks.sample_utxos.iter().enumerate() {
                    let addr_bytes = utxo.address_bytes();
                    println!(
                        "  {}: {}:{} → {} ({} lovelace)",
                        i + 1,
                        &utxo.tx_hash_hex()[..16],
                        utxo.output_index(),
                        hex::encode(&addr_bytes[..addr_bytes.len().min(16)]),
                        utxo.coin()
                    );
                }
                println!();
            }

            // Show sample pools
            if !callbacks.sample_pools.is_empty() {
                println!("Sample Pools (first 10):");
                for (i, pool) in callbacks.sample_pools.iter().enumerate() {
                    println!(
                        "  {}: {} (pledge: {}, cost: {}, margin: {:?})",
                        i + 1,
                        pool.operator,
                        pool.pledge,
                        pool.cost,
                        pool.margin
                    );
                }
                println!();
            }

            // Show sample accounts
            if !callbacks.sample_accounts.is_empty() {
                println!("Sample Accounts (first 10):");
                for (i, account) in callbacks.sample_accounts.iter().enumerate() {
                    println!(
                        "  {}: {} (utxo: {}, rewards: {})",
                        i + 1,
                        &account.stake_address.to_string().unwrap(),
                        account.address_state.utxo_value,
                        account.address_state.rewards
                    );
                }
                println!();
            }

            // Show sample DReps
            if !callbacks.sample_dreps.is_empty() {
                println!("Sample DReps (first 10):");
                for (i, (cred, record)) in callbacks.sample_dreps.iter().enumerate() {
                    let drep_id =
                        cred.to_drep_bech32().unwrap_or_else(|_| "invalid_cred".to_string());
                    print!(
                        "  {}: {} (deposit: {} lovelace)",
                        i + 1,
                        drep_id,
                        record.deposit
                    );
                    if let Some(anchor) = &record.anchor {
                        println!(" - {}", anchor.url);
                    } else {
                        println!();
                    }
                }
                println!();
            }

            // Show sample proposals
            if !callbacks.sample_proposals.is_empty() {
                println!("Sample Proposals (first 10):");
                for (i, proposal) in callbacks.sample_proposals.iter().enumerate() {
                    println!(
                        "  {}: {} (deposit: {}, action: {})",
                        i + 1,
                        proposal.gov_action_id,
                        proposal.deposit,
                        proposal.gov_action
                    );
                }
                println!();
            }

            // Performance stats
            let utxos_per_sec = callbacks.utxo_count as f64 / duration.as_secs_f64();
            println!("Performance:");
            println!("  Total time: {duration:.2?}");
            println!("  UTXOs/second: {utxos_per_sec:.0}");
            println!();

            std::process::exit(0);
        }
        Err(e) => {
            eprintln!("Parse failed: {e:?}");
            eprintln!();
            std::process::exit(1);
        }
    }
}
