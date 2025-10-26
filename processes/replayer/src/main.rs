//! 'main' for the Acropolis omnibus process

use acropolis_common::messages::Message;
use anyhow::Result;
use caryatid_process::Process;
use caryatid_sdk::ModuleRegistry;
use config::{Config, Environment, File};
use std::sync::Arc;
use tracing::info;
use tracing_subscriber::prelude::*;
use tracing_subscriber::{filter, fmt, EnvFilter, Registry};

// External modules
use acropolis_module_accounts_state::AccountsState;
//use acropolis_module_address_state::AddressState;
//use acropolis_module_assets_state::AssetsState;
use acropolis_module_block_unpacker::BlockUnpacker;
//use acropolis_module_chain_store::ChainStore;
//use acropolis_module_consensus::Consensus;
use acropolis_module_drdd_state::DRDDState;
use acropolis_module_drep_state::DRepState;
use acropolis_module_epochs_state::EpochsState;
use acropolis_module_genesis_bootstrapper::GenesisBootstrapper;
use acropolis_module_governance_state::GovernanceState;
//use acropolis_module_historical_accounts_state::HistoricalAccountsState;
use acropolis_module_mithril_snapshot_fetcher::MithrilSnapshotFetcher;
use acropolis_module_parameters_state::ParametersState;
use acropolis_module_rest_blockfrost::BlockfrostREST;
use acropolis_module_spdd_state::SPDDState;
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
    EpochsState::register(process);
    AccountsState::register(process);
    //AddressState::register(process);
    //AssetsState::register(process);
    //HistoricalAccountsState::register(process);
    BlockfrostREST::register(process);
    SPDDState::register(process);
    DRDDState::register(process);
    //Consensus::register(process);
    //ChainStore::register(process);

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
        EpochsState::register(process);
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
    BlockfrostREST::register(process);

    Clock::<Message>::register(process);
    RESTServer::<Message>::register(process);
    Spy::<Message>::register(process);
}

#[derive(Debug, clap::Parser)]
#[command(
    name = "acropolis_process_replayer",
    group(clap::ArgGroup::new("mode").required(true).args(&["governance_collect", "governance_replay", "alonzo_governance_collect"])),
)]
struct Args {
    #[arg(long, value_name = "PATH", default_value_t = option_env!("ACROPOLIS_REPLAYER_DEFAULT_CONFIG").unwrap_or("replayer.toml").to_string())]
    config: String,

    // FIXME: typically, these should be real [`clap::Command`] commands, not
    // flags, but @michalrus kept `--` for backwards compatibility.
    #[arg(long)]
    governance_collect: bool,

    #[arg(long)]
    governance_replay: bool,

    #[arg(long)]
    alonzo_governance_collect: bool,
}

#[tokio::main]
pub async fn main() -> Result<()> {
    let args = <self::Args as clap::Parser>::parse();

    // Initialise tracing
    let fmt_layer = fmt::layer()
        .with_filter(EnvFilter::from_default_env().add_directive(filter::LevelFilter::INFO.into()))
        .with_filter(filter::filter_fn(|meta| meta.is_event()));
    //tracing_subscriber::fmt::init();
    Registry::default().with(fmt_layer).init();

    info!("Acropolis omnibus process");

    // Read the config
    let config = Arc::new(
        Config::builder()
            .add_source(File::with_name(&args.config))
            .add_source(Environment::with_prefix("ACROPOLIS"))
            .build()
            .unwrap(),
    );

    // Create the process
    let mut process = Process::<Message>::create(config).await;

    if args.governance_collect {
        setup_governance_collect(&mut process)
    } else if args.governance_replay {
        setup_governance_replay(&mut process)
    } else if args.alonzo_governance_collect {
        setup_alonzo_governance_collect(&mut process)
    } else {
        unreachable!()
    }

    // Run it
    tracing::info!("Running");
    process.run().await?;

    // Bye!
    info!("Exiting");
    Ok(())
}
