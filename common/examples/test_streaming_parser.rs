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
    sample_dreps: Vec<DRepInfo>,
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
            sample_dreps: Vec::new(),
        }
    }
}

impl UtxoCallback for CountingCallbacks {
    fn on_utxo(&mut self, utxo: UtxoEntry) -> Result<()> {
        self.utxo_count += 1;
        // Keep first 5 for display
        if self.sample_utxos.len() < 5 {
            self.sample_utxos.push(utxo);
        }
        Ok(())
    }
}

impl PoolCallback for CountingCallbacks {
    fn on_pools(&mut self, pools: Vec<PoolInfo>) -> Result<()> {
        self.pool_count = pools.len();
        Ok(())
    }
}

impl StakeCallback for CountingCallbacks {
    fn on_accounts(&mut self, accounts: Vec<AccountState>) -> Result<()> {
        self.account_count = accounts.len();
        Ok(())
    }
}

impl DRepCallback for CountingCallbacks {
    fn on_dreps(&mut self, dreps: Vec<DRepInfo>) -> Result<()> {
        self.drep_count = dreps.len();
        // Keep first 3 for display
        self.sample_dreps = dreps.into_iter().take(3).collect();
        Ok(())
    }
}

impl ProposalCallback for CountingCallbacks {
    fn on_proposals(&mut self, proposals: Vec<GovernanceProposal>) -> Result<()> {
        self.proposal_count = proposals.len();
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
                println!("Sample UTXOs (first 5):");
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

            // Show sample DReps
            if !callbacks.sample_dreps.is_empty() {
                println!("Sample DReps (first 3):");
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
