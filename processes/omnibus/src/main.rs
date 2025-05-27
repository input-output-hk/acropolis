//! 'main' for the Acropolis omnibus process

use caryatid_process::Process;
use anyhow::Result;
use config::{Config, File, Environment};
use tracing::info;
use tracing_subscriber;
use std::sync::Arc;
use acropolis_common::messages::Message;

// External modules
use acropolis_module_genesis_bootstrapper::GenesisBootstrapper;
use acropolis_module_mithril_snapshot_fetcher::MithrilSnapshotFetcher;
use acropolis_module_upstream_chain_fetcher::UpstreamChainFetcher;
use acropolis_module_block_unpacker::BlockUnpacker;
use acropolis_module_tx_unpacker::TxUnpacker;
use acropolis_module_utxo_state::UTXOState;
use acropolis_module_spo_state::SPOState;
use acropolis_module_drep_state::DRepState;
use acropolis_module_governance_state::GovernanceState;
use acropolis_module_stake_delta_filter::StakeDeltaFilter;
use acropolis_module_epoch_activity_counter::EpochActivityCounter;
use acropolis_module_accounts_state::AccountsState;

use caryatid_module_clock::Clock;
use caryatid_module_rest_server::RESTServer;
use caryatid_module_spy::Spy;

/// Standard main
#[tokio::main]
pub async fn main() -> Result<()> {

    // Initialise tracing
    tracing_subscriber::fmt::init();

    info!("Acropolis omnibus process");

    // Read the config
    let config = Arc::new(Config::builder()
        .add_source(File::with_name("omnibus"))
        .add_source(Environment::with_prefix("ACROPOLIS"))
        .build()
        .unwrap());

    // Create the process
    let mut process = Process::<Message>::create(config).await;

    // Register modules
    GenesisBootstrapper::register(&mut process);
    MithrilSnapshotFetcher::register(&mut process);
    UpstreamChainFetcher::register(&mut process);
    BlockUnpacker::register(&mut process);
    TxUnpacker::register(&mut process);
    UTXOState::register(&mut process);
    SPOState::register(&mut process);
    DRepState::register(&mut process);
    GovernanceState::register(&mut process);
    StakeDeltaFilter::register(&mut process);
    EpochActivityCounter::register(&mut process);
    AccountsState::register(&mut process);

    Clock::<Message>::register(&mut process);
    RESTServer::<Message>::register(&mut process);
    Spy::<Message>::register(&mut process);

    // Run it
    process.run().await?;

    // Bye!
    info!("Exiting");
    Ok(())
}

