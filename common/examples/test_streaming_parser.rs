// Example: Test streaming snapshot parser with large snapshot
//
// Usage: cargo run --example test_streaming_parser --release -- <snapshot_path>

use acropolis_common::snapshot::streaming_snapshot::{
    AccountState, DRepCallback, DRepInfo, GovernanceProposal, PoolCallback, PoolInfo,
    ProposalCallback, SnapshotCallbacks, SnapshotMetadata, StakeCallback, StreamingSnapshotParser,
    UtxoCallback, UtxoEntry,
};
use anyhow::Result;
use std::env;
use std::time::Instant;

// Simple counter callback that doesn't store data in memory
struct CountingCallbacks {
    metadata: Option<SnapshotMetadata>,
    utxo_count: u64,
    pool_count: usize,
    account_count: usize,
    drep_count: usize,
    proposal_count: usize,
    sample_utxos: Vec<UtxoEntry>,
    sample_pools: Vec<PoolInfo>,
    sample_accounts: Vec<AccountState>,
    sample_dreps: Vec<DRepInfo>,
    sample_proposals: Vec<GovernanceProposal>,
}

impl Default for CountingCallbacks {
    fn default() -> Self {
        Self {
            metadata: None,
            utxo_count: 0,
            pool_count: 0,
            account_count: 0,
            drep_count: 0,
            proposal_count: 0,
            sample_utxos: Vec::new(),
            sample_pools: Vec::new(),
            sample_accounts: Vec::new(),
            sample_dreps: Vec::new(),
            sample_proposals: Vec::new(),
        }
    }
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
        if self.utxo_count > 0 && self.utxo_count % 1000000 == 0 {
            eprintln!("  Parsed {} UTXOs...", self.utxo_count);
        }
        Ok(())
    }
}

impl PoolCallback for CountingCallbacks {
    fn on_pools(&mut self, pools: Vec<PoolInfo>) -> Result<()> {
        self.pool_count = pools.len();
        eprintln!("✓ Parsed {} stake pools", pools.len());

        // Show first 10 pools
        for (i, pool) in pools.iter().take(10).enumerate() {
            eprintln!(
                "  Pool #{}: {} (pledge: {}, cost: {}, margin: {:.2}%)",
                i + 1,
                pool.pool_id,
                pool.pledge,
                pool.cost,
                pool.margin * 100.0
            );
        }

        // Keep first 10 for summary
        self.sample_pools = pools.into_iter().take(10).collect();
        Ok(())
    }
}

impl StakeCallback for CountingCallbacks {
    fn on_accounts(&mut self, accounts: Vec<AccountState>) -> Result<()> {
        self.account_count = accounts.len();
        if accounts.len() > 0 {
            eprintln!("✓ Parsed {} stake accounts", accounts.len());

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
        eprintln!("✓ Parsed {} DReps", self.drep_count);

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
        if proposals.len() > 0 {
            eprintln!("✓ Parsed {} governance proposals", proposals.len());

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

impl SnapshotCallbacks for CountingCallbacks {
    fn on_metadata(&mut self, metadata: SnapshotMetadata) -> Result<()> {
        self.metadata = Some(metadata);
        Ok(())
    }

    fn on_complete(&mut self) -> Result<()> {
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

    let snapshot_path = &args[1];
    println!("Streaming Snapshot Parser Test");
    println!("================================");
    println!("Snapshot: {}", snapshot_path);
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
            println!("✓ Parse completed successfully in {:.2?}", duration);
            println!();

            // Display results
            if let Some(metadata) = &callbacks.metadata {
                println!("Metadata:");
                println!("  Epoch: {}", metadata.epoch);
                println!("  Treasury: {} lovelace", metadata.pot_balances.treasury);
                println!("  Reserves: {} lovelace", metadata.pot_balances.reserves);
                println!("  Deposits: {} lovelace", metadata.pot_balances.deposits);
                if let Some(count) = metadata.utxo_count {
                    println!("  UTXO Count (metadata): {}", count);
                }
                println!();
            }

            println!("Parsed Data:");
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
                        "  {}: {} (pledge: {}, cost: {}, margin: {:.2}%)",
                        i + 1,
                        pool.pool_id,
                        pool.pledge,
                        pool.cost,
                        pool.margin * 100.0
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
            println!("  Total time: {:.2?}", duration);
            println!("  UTXOs/second: {:.0}", utxos_per_sec);
            println!();

            std::process::exit(0);
        }
        Err(e) => {
            eprintln!("✗ Parse failed: {:?}", e);
            eprintln!();
            std::process::exit(1);
        }
    }
}
