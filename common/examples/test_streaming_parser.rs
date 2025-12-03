// Example: Test streaming snapshot parser with large snapshot
//
// Usage: cargo run --example test_streaming_parser --release -- <snapshot_path>

use acropolis_common::ledger_state::SPOState;
use acropolis_common::snapshot::EpochCallback;
use acropolis_common::snapshot::{
    AccountState, DRepCallback, DRepInfo, GovernanceProposal, PoolCallback, ProposalCallback,
    RawSnapshotsContainer, SnapshotCallbacks, SnapshotMetadata, SnapshotsCallback, StakeCallback,
    StreamingSnapshotParser, UtxoCallback, UtxoEntry,
};
use acropolis_common::PoolRegistration;
use anyhow::Result;
use std::env;
use std::time::Instant;
use tracing::info;

use acropolis_common::EpochBootstrapData;
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
    sample_dreps: Vec<DRepInfo>,
    sample_proposals: Vec<GovernanceProposal>,
}

impl UtxoCallback for CountingCallbacks {
    fn on_utxo(&mut self, utxo: UtxoEntry) -> Result<()> {
        self.utxo_count += 1;
        // Keep first 10 for display
        if self.sample_utxos.len() < 10 {
            if self.sample_utxos.len() < 10 {
                eprintln!(
                    "  UTXO #{}: {}:{} → {} ({} lovelace)",
                    self.utxo_count,
                    &utxo.tx_hash[..16],
                    utxo.output_index,
                    &utxo.address[..utxo.address.len().min(32)],
                    utxo.value
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

impl StakeCallback for CountingCallbacks {
    fn on_accounts(&mut self, accounts: Vec<AccountState>) -> Result<()> {
        self.account_count = accounts.len();
        if !accounts.is_empty() {
            eprintln!("Parsed {} stake accounts", accounts.len());

            // Show first 10 accounts
            for (i, account) in accounts.iter().take(10).enumerate() {
                eprintln!(
                    "  Account #{}: {} (utxo: {}, rewards: {}, pool: {:?}, drep: {:?})",
                    i + 1,
                    &account.stake_address[..32],
                    account.address_state.utxo_value,
                    account.address_state.rewards,
                    account.address_state.delegated_spo.as_ref().map(|s| &s[..16]),
                    account.address_state.delegated_drep
                );
            }
        }

        // Keep first 10 for summary
        self.sample_accounts = accounts.into_iter().take(10).collect();
        Ok(())
    }
}

impl DRepCallback for CountingCallbacks {
    fn on_dreps(&mut self, dreps: Vec<DRepInfo>) -> Result<()> {
        self.drep_count = dreps.len();
        eprintln!("Parsed {} DReps", self.drep_count);

        // Show first 10 DReps
        for (i, drep) in dreps.iter().take(10).enumerate() {
            if let Some(anchor) = &drep.anchor {
                eprintln!(
                    "  DRep #{}: {} (deposit: {}) - {}",
                    i + 1,
                    drep.drep_id,
                    drep.deposit,
                    anchor.url
                );
            } else {
                eprintln!(
                    "  DRep #{}: {} (deposit: {})",
                    i + 1,
                    drep.drep_id,
                    drep.deposit
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

        // Show snapshots info if available
        if let Some(snapshots_info) = &metadata.snapshots {
            eprintln!("  Snapshots Info:");
            eprintln!(
                "    Mark snapshot: {} sections",
                snapshots_info.mark.sections_count
            );
            eprintln!(
                "    Set snapshot: {} sections",
                snapshots_info.set.sections_count
            );
            eprintln!(
                "    Go snapshot: {} sections",
                snapshots_info.go.sections_count
            );
            eprintln!(
                "    Fee value: {} lovelace ({} ADA)",
                snapshots_info.fee,
                snapshots_info.fee as f64 / 1_000_000.0
            );
        } else {
            eprintln!("  No snapshots data available");
        }

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
    fn on_snapshots(&mut self, snapshots: RawSnapshotsContainer) -> Result<()> {
        eprintln!("Raw Snapshots Data:");

        // Calculate total stakes and delegator counts from VMap data
        let mark_total: i64 = snapshots.mark.0.iter().map(|(_, amount)| amount).sum();
        let set_total: i64 = snapshots.set.0.iter().map(|(_, amount)| amount).sum();
        let go_total: i64 = snapshots.go.0.iter().map(|(_, amount)| amount).sum();

        eprintln!(
            "  Mark snapshot: {} delegators, {} total stake (ADA)",
            snapshots.mark.0.len(),
            mark_total as f64 / 1_000_000.0
        );
        eprintln!(
            "  Set snapshot: {} delegators, {} total stake (ADA)",
            snapshots.set.0.len(),
            set_total as f64 / 1_000_000.0
        );
        eprintln!(
            "  Go snapshot: {} delegators, {} total stake (ADA)",
            snapshots.go.0.len(),
            go_total as f64 / 1_000_000.0
        );
        eprintln!("  Fee: {} ADA", snapshots.fee as f64 / 1_000_000.0);
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

    match parser.parse(&mut callbacks) {
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

                // Show snapshots info summary
                if let Some(snapshots_info) = &metadata.snapshots {
                    println!("  Snapshots Summary:");
                    println!(
                        "    Mark: {} sections, Set: {} sections, Go: {} sections, Fee: {} ADA",
                        snapshots_info.mark.sections_count,
                        snapshots_info.set.sections_count,
                        snapshots_info.go.sections_count,
                        snapshots_info.fee as f64 / 1_000_000.0
                    );
                }

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
                    println!(
                        "  {}: {}:{} → {} ({} lovelace)",
                        i + 1,
                        &utxo.tx_hash[..16],
                        utxo.output_index,
                        &utxo.address[..32],
                        utxo.value
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
                        &account.stake_address[..32],
                        account.address_state.utxo_value,
                        account.address_state.rewards
                    );
                }
                println!();
            }

            // Show sample DReps
            if !callbacks.sample_dreps.is_empty() {
                println!("Sample DReps (first 10):");
                for (i, drep) in callbacks.sample_dreps.iter().enumerate() {
                    print!(
                        "  {}: {} (deposit: {} lovelace)",
                        i + 1,
                        drep.drep_id,
                        drep.deposit
                    );
                    if let Some(anchor) = &drep.anchor {
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
