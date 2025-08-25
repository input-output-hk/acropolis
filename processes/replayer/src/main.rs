//! 'main' for the Acropolis omnibus process

use acropolis_common::messages::Message;
use anyhow::Result;
use caryatid_process::Process;
use caryatid_sdk::ModuleRegistry;
use config::{Config, Environment, File};
use std::{env, sync::Arc};
use tracing::info;
use tracing_subscriber::prelude::*;
use tracing_subscriber::{filter, fmt, EnvFilter, Registry};

// External modules
use acropolis_module_accounts_state::AccountsState;
use acropolis_module_block_unpacker::BlockUnpacker;
use acropolis_module_drep_state::DRepState;
use acropolis_module_epoch_activity_counter::EpochActivityCounter;
use acropolis_module_genesis_bootstrapper::GenesisBootstrapper;
use acropolis_module_governance_state::GovernanceState;
use acropolis_module_mithril_snapshot_fetcher::MithrilSnapshotFetcher;
use acropolis_module_parameters_state::ParametersState;
use acropolis_module_spo_state::SPOState;
use acropolis_module_stake_delta_filter::StakeDeltaFilter;
use acropolis_module_tx_unpacker::TxUnpacker;
use acropolis_module_upstream_chain_fetcher::UpstreamChainFetcher;
use acropolis_module_utxo_state::UTXOState;

use caryatid_module_clock::Clock;
use caryatid_module_rest_server::RESTServer;
use caryatid_module_spy::Spy;

mod playback;
mod recorder;
mod recorder_alonzo_governance;
mod replayer_config;

use playback::Playback;
use recorder::Recorder;
use recorder_alonzo_governance::RecorderAlonzoGovernance;

fn setup_governance_collect(process: &mut dyn ModuleRegistry<Message>) {
    tracing::info!("Collecting");
    GenesisBootstrapper::register(process);
    MithrilSnapshotFetcher::register(process);
    UpstreamChainFetcher::register(process);
    BlockUnpacker::register(process);
    TxUnpacker::register(process);
    UTXOState::register(process);
    SPOState::register(process);
    DRepState::register(process);
    GovernanceState::register(process);
    ParametersState::register(process);
    StakeDeltaFilter::register(process);
    EpochActivityCounter::register(process);
    AccountsState::register(process);

    Recorder::register(process);

    Clock::<Message>::register(process);
    RESTServer::<Message>::register(process);
    Spy::<Message>::register(process);
}

fn setup_alonzo_governance_collect(process: &mut dyn ModuleRegistry<Message>) {
    tracing::info!("Collecting");
    GenesisBootstrapper::register(process);
    MithrilSnapshotFetcher::register(process);
    UpstreamChainFetcher::register(process);
    BlockUnpacker::register(process);
    TxUnpacker::register(process);
    /*
        UTXOState::register(process);
        SPOState::register(process);
        DRepState::register(process);
        GovernanceState::register(process);
        ParametersState::register(process);
        StakeDeltaFilter::register(process);
        EpochActivityCounter::register(process);
        AccountsState::register(process);
    */
    RecorderAlonzoGovernance::register(process);

    Clock::<Message>::register(process);
    RESTServer::<Message>::register(process);
    Spy::<Message>::register(process);
}

fn setup_governance_replay(process: &mut dyn ModuleRegistry<Message>) {
    //TxUnpacker::register(process);
    GovernanceState::register(process);
    ParametersState::register(process);

    Playback::register(process);

    Clock::<Message>::register(process);
    RESTServer::<Message>::register(process);
    Spy::<Message>::register(process);
}

#[tokio::main]
pub async fn main() -> Result<()> {
    // Initialise tracing
    let fmt_layer = fmt::layer()
        .with_filter(EnvFilter::from_default_env().add_directive(filter::LevelFilter::INFO.into()))
        .with_filter(filter::filter_fn(|meta| meta.is_event()));
    //tracing_subscriber::fmt::init();
    Registry::default().with(fmt_layer).init();

    info!("Acropolis omnibus process");

    let mut args = env::args();
    let _executable_name = args.next();

    // Read the config
    let config = Arc::new(
        Config::builder()
            .add_source(File::with_name("replayer"))
            .add_source(Environment::with_prefix("ACROPOLIS"))
            .build()
            .unwrap(),
    );

    // Create the process
    let mut process = Process::<Message>::create(config).await;

    if let Some(key) = args.next() {
        match key.as_str() {
            "--governance-collect" => setup_governance_collect(&mut process),
            "--governance-replay" => setup_governance_replay(&mut process),
            "--alonzo-governance-collect" => setup_alonzo_governance_collect(&mut process),
            a => {
                tracing::error!(
                    "Unknown command line argument: {a}, \
                    expected --governance-collect or --governance-replay"
                );
                return Ok(());
            }
        }
    } else {
        tracing::error!("Please, specify command: command line must have at least one argument");
        return Ok(());
    }

    // Run it
    tracing::info!("Running");
    process.run().await?;

    // Bye!
    info!("Exiting");
    Ok(())
}
