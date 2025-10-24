use std::{path::PathBuf, sync::Arc};

use acropolis_common::{
    commands::transactions::TransactionsCommand,
    messages::{Command, Message},
};
use acropolis_module_tx_submitter::TxSubmitter;
use anyhow::{Result, bail};
use caryatid_process::Process;
use caryatid_sdk::{Context, Module, module};
use clap::Parser;
use config::{Config, File};
use tokio::{fs, select, sync::mpsc};
use tracing::info;
use tracing_subscriber::{
    EnvFilter, Layer as _, Registry, filter, fmt, layer::SubscriberExt as _,
    util::SubscriberInitExt as _,
};

fn default_config_path() -> PathBuf {
    PathBuf::from(
        option_env!("ACROPOLIS_TX_SUBMITTER_DEFAULT_CONFIG").unwrap_or("tx-submitter.toml"),
    )
}

#[derive(clap::Parser, Clone)]
struct Args {
    /// Path to configuration.
    #[arg(long, default_value = default_config_path().into_os_string())]
    config: PathBuf,
    /// File containing the raw bytes of a transaction.
    tx_file: PathBuf,
}

#[derive(Clone)]
struct CliState {
    args: Args,
    done: mpsc::Sender<Result<()>>,
}
impl CliState {
    pub fn run<F, Fut>(self, ctx: Arc<Context<Message>>, fut: F)
    where
        F: FnOnce(Args, Arc<Context<Message>>) -> Fut + Send + 'static,
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        let args = self.args.clone();
        let c = ctx.clone();
        ctx.run(async move {
            let result = fut(args, c).await;
            let _ = self.done.send(result).await;
        });
    }
}

tokio::task_local!(static CLI: CliState);
async fn run_process(process: Process<Message>, args: Args) -> Result<()> {
    let (tx, mut rx) = mpsc::channel(1);
    let state = CliState { args, done: tx };
    select! {
        res = CLI.scope(state, process.run()) => {
            res?;
            bail!("process terminated")
        }
        res = rx.recv() => {
            match res {
                Some(result) => {
                    info!("process completed");
                    result
                }
                None => bail!("process terminated")
            }
        }
    }
}

#[tokio::main]
pub async fn main() -> Result<()> {
    let args = Args::try_parse()?;

    // Standard logging using RUST_LOG for log levels default to INFO for events only
    let fmt_layer = fmt::layer()
        .with_filter(EnvFilter::from_default_env().add_directive(filter::LevelFilter::INFO.into()))
        .with_filter(filter::filter_fn(|meta| meta.is_event()));
    Registry::default().with(fmt_layer).init();

    let config = Arc::new(Config::builder().add_source(File::from(args.config.as_path())).build()?);
    let mut process = Process::<Message>::create(config).await;

    TxSubmitter::register(&mut process);
    CliDriver::register(&mut process);

    run_process(process, args).await
}

#[module(
    message_type(Message),
    name = "cli-driver",
    description = "Module to interface with the CLI tool"
)]
struct CliDriver;
impl CliDriver {
    pub async fn init(&self, context: Arc<Context<Message>>, _config: Arc<Config>) -> Result<()> {
        let state = CLI.get();
        state.run(context, move |args, context| async move {
            let tx = fs::read(args.tx_file).await?;
            let request = Arc::new(Message::Command(Command::Transactions(
                TransactionsCommand::Submit { cbor: tx },
            )));
            let response = context.request("cli.tx.submit", request).await?;
            info!("{response:?}");
            Ok(())
        });
        Ok(())
    }
}
