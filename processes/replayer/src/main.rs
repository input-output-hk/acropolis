//! 'main' for the Acropolis omnibus process

use caryatid_process::Process;
use caryatid_sdk::ModuleRegistry;
use anyhow::Result;
use config::{Config, File, Environment};
use tracing::info;
use tracing_subscriber;
use std::{env, sync::Arc};
use acropolis_common::messages::Message;

// External modules
use acropolis_module_genesis_bootstrapper::GenesisBootstrapper;
use acropolis_module_mithril_snapshot_fetcher::MithrilSnapshotFetcher;
use acropolis_module_upstream_chain_fetcher::UpstreamChainFetcher;
use acropolis_module_block_unpacker::BlockUnpacker;
use acropolis_module_tx_unpacker::TxUnpacker;
//use acropolis_module_utxo_state::UTXOState;
use acropolis_module_spo_state::SPOState;
use acropolis_module_drep_state::DRepState;
use acropolis_module_governance_state::GovernanceState;

use caryatid_module_playback::Playback;
use caryatid_module_clock::Clock;
use caryatid_module_rest_server::RESTServer;
use caryatid_module_spy::Spy;

fn setup_governance_collect(process: &mut dyn ModuleRegistry::<Message>) {
    GenesisBootstrapper::register(process);
    MithrilSnapshotFetcher::register(process);
    UpstreamChainFetcher::register(process);
    BlockUnpacker::register(process);
    TxUnpacker::register(process);

    Clock::<Message>::register(process);
    RESTServer::<Message>::register(process);
    Spy::<Message>::register(process);
}

fn setup_governance_replay(process: &mut dyn ModuleRegistry::<Message>) {
    GenesisBootstrapper::register(process);

    TxUnpacker::register(process);
    SPOState::register(process);
    DRepState::register(process);
    GovernanceState::register(process);

    Playback::<Message>::register(process);

    Clock::<Message>::register(process);
    RESTServer::<Message>::register(process);
    Spy::<Message>::register(process);
}

#[tokio::main]
pub async fn main() -> Result<()> {

    // Initialise tracing
    tracing_subscriber::fmt::init();

    info!("Acropolis omnibus process");

    // Read the config
    let config = Arc::new(Config::builder()
        .add_source(File::with_name("replayer"))
        .add_source(Environment::with_prefix("ACROPOLIS"))
        .build()
        .unwrap());

    // Create the process
    let mut process = Process::<Message>::create(config).await;

    let mut args = env::args();
    let _executable_name = args.next();
    if let Some(key) = args.next() {
        match key.as_str() {
            "--governance-collect" => setup_governance_collect(&mut process),
            "--governance-replay" => setup_governance_replay(&mut process),
            a => {
                tracing::error!("Unknown command line argument: {a}");
                return Ok(());
            }
        }
    }
    else {
        tracing::error!("Please, specify command: command line must have at least one argument");
        return Ok(());
    }

    // Run it
    process.run().await?;

    // Bye!
    info!("Exiting");
    Ok(())
}
