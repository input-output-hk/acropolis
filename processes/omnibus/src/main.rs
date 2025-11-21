//! 'main' for the Acropolis omnibus process

use acropolis_common::messages::Message;
use anyhow::Result;
use caryatid_process::Process;
use config::{Config, Environment, File};
use std::sync::Arc;
use tracing::info;

// External modules
use acropolis_module_accounts_state::AccountsState;
use acropolis_module_address_state::AddressState;
use acropolis_module_assets_state::AssetsState;
use acropolis_module_block_kes_validator::BlockKesValidator;
use acropolis_module_block_unpacker::BlockUnpacker;
use acropolis_module_block_vrf_validator::BlockVrfValidator;
use acropolis_module_chain_store::ChainStore;
use acropolis_module_consensus::Consensus;
use acropolis_module_drdd_state::DRDDState;
use acropolis_module_drep_state::DRepState;
use acropolis_module_epochs_state::EpochsState;
use acropolis_module_genesis_bootstrapper::GenesisBootstrapper;
use acropolis_module_governance_state::GovernanceState;
use acropolis_module_historical_accounts_state::HistoricalAccountsState;
use acropolis_module_historical_epochs_state::HistoricalEpochsState;
use acropolis_module_mithril_snapshot_fetcher::MithrilSnapshotFetcher;
use acropolis_module_parameters_state::ParametersState;
use acropolis_module_peer_network_interface::PeerNetworkInterface;
use acropolis_module_rest_blockfrost::BlockfrostREST;
use acropolis_module_snapshot_bootstrapper::SnapshotBootstrapper;
use acropolis_module_spdd_state::SPDDState;
use acropolis_module_spo_state::SPOState;
use acropolis_module_stake_delta_filter::StakeDeltaFilter;
use acropolis_module_tx_unpacker::TxUnpacker;
use acropolis_module_utxo_state::UTXOState;

use caryatid_module_clock::Clock;
use caryatid_module_rest_server::RESTServer;
use caryatid_module_spy::Spy;

use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::SpanExporter;
use opentelemetry_sdk::trace::SdkTracerProvider;
use tracing_opentelemetry::OpenTelemetryLayer;
use tracing_subscriber::prelude::*;
use tracing_subscriber::{filter, fmt, EnvFilter, Registry};

#[cfg(not(target_env = "msvc"))]
use tikv_jemallocator::Jemalloc;
#[cfg(not(target_env = "msvc"))]
#[global_allocator]
static GLOBAL: Jemalloc = Jemalloc;

#[derive(Debug, clap::Parser)]
#[command(name = "acropolis_process_omnibus")]
struct Args {
    #[arg(long, value_name = "PATH", default_value_t = option_env!("ACROPOLIS_OMNIBUS_DEFAULT_CONFIG").unwrap_or("omnibus.toml").to_string())]
    config: String,
}

/// Standard main
#[tokio::main]
pub async fn main() -> Result<()> {
    let args = <self::Args as clap::Parser>::parse();

    // Standard logging using RUST_LOG for log levels default to INFO for events only
    let fmt_layer = fmt::layer()
        .with_filter(EnvFilter::try_from_default_env().unwrap_or(EnvFilter::new("info")))
        .with_filter(filter::filter_fn(|meta| meta.is_event()));

    // Only turn on tracing if some OTEL environment variables exist
    if std::env::vars().any(|(name, _)| name.starts_with("OTEL_")) {
        // Send span tracing to opentelemetry
        // Should pick up standard OTEL_* environment variables
        let otel_exporter = SpanExporter::builder().with_tonic().build()?;
        let otel_tracer = SdkTracerProvider::builder()
            .with_batch_exporter(otel_exporter)
            .build()
            .tracer("rust-otel-otlp");
        let otel_layer = OpenTelemetryLayer::new(otel_tracer)
            .with_filter(
                EnvFilter::from_default_env().add_directive(filter::LevelFilter::INFO.into()),
            )
            .with_filter(filter::filter_fn(|meta| meta.is_span()));
        Registry::default().with(fmt_layer).with(otel_layer).init();
    } else {
        Registry::default().with(fmt_layer).init();
    }

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
    let mut process = Process::<Message>::create(config.clone()).await;

    // Get startup method from config
    let startup_method =
        config.get_string("startup.method").unwrap_or_else(|_| "snapshot".to_string());

    info!("Using startup method: {}", startup_method);

    // Register bootstrap modules based on startup method
    match startup_method.as_str() {
        "genesis" => {
            info!("Registering GenesisBootstrapper");
            GenesisBootstrapper::register(&mut process);
        }
        "snapshot" => {
            info!("Registering SnapshotBootstrapper");
            SnapshotBootstrapper::register(&mut process);
        }
        _ => {
            panic!(
                "Invalid startup method: {}. Must be one of: genesis, snapshot",
                startup_method
            );
        }
    }

    // Register modules
    MithrilSnapshotFetcher::register(&mut process);
    BlockUnpacker::register(&mut process);
    PeerNetworkInterface::register(&mut process);
    TxUnpacker::register(&mut process);
    UTXOState::register(&mut process);
    SPOState::register(&mut process);
    DRepState::register(&mut process);
    GovernanceState::register(&mut process);
    ParametersState::register(&mut process);
    StakeDeltaFilter::register(&mut process);
    EpochsState::register(&mut process);
    AccountsState::register(&mut process);
    AddressState::register(&mut process);
    AssetsState::register(&mut process);
    HistoricalAccountsState::register(&mut process);
    HistoricalEpochsState::register(&mut process);
    BlockfrostREST::register(&mut process);
    SPDDState::register(&mut process);
    DRDDState::register(&mut process);
    Consensus::register(&mut process);
    ChainStore::register(&mut process);
    BlockVrfValidator::register(&mut process);
    BlockKesValidator::register(&mut process);

    Clock::<Message>::register(&mut process);
    RESTServer::<Message>::register(&mut process);
    Spy::<Message>::register(&mut process);

    // Run it
    process.run().await?;

    // Bye!
    info!("Exiting");

    Ok(())
}
