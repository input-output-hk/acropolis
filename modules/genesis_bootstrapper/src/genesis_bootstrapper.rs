//! Acropolis genesis bootstrapper module for Caryatid
//! Reads genesis files and outputs initial UTXO events

use acropolis_common::{
    configuration::StartupMode,
    genesis_values::GenesisValues,
    hash::Hash,
    messages::{
        BootstrapPotDeltas, CardanoMessage, GenesisCompleteMessage, GenesisUTxOsMessage, Message,
        PotDeltasMessage, UTXODeltasMessage,
    },
    Address, BlockHash, BlockInfo, BlockIntent, BlockStatus, ByronAddress, Era, GenesisDelegates,
    LovelaceDelta, MagicNumber, TxHash, TxIdentifier, TxOutput, TxUTxODeltas, UTxOIdentifier,
    Value,
};
use anyhow::Result;
use blake2::{digest::consts::U32, Blake2b, Digest};
use caryatid_sdk::{module, Context};
use config::Config;
use pallas::ledger::configs::{
    byron::{genesis_utxos, GenesisFile as ByronGenesisFile},
    shelley::GenesisFile as ShelleyGenesisFile,
};
use std::borrow::Cow;
use std::sync::Arc;
use tracing::{error, info, info_span, Instrument};

const DEFAULT_STARTUP_TOPIC: &str = "cardano.sequence.start";
const DEFAULT_PUBLISH_UTXO_DELTAS_TOPIC: &str = "cardano.utxo.deltas";
const DEFAULT_PUBLISH_POT_DELTAS_TOPIC: &str = "cardano.pot.deltas";
const DEFAULT_PUBLISH_GENESIS_UTXO_REGISTRY_TOPIC: &str = "cardano.genesis.utxos";
const DEFAULT_COMPLETION_TOPIC: &str = "cardano.sequence.bootstrapped";
const DEFAULT_NETWORK_NAME: &str = "mainnet";

// Include genesis data (downloaded by build.rs)
const MAINNET_BYRON_GENESIS: &[u8] = include_bytes!("../downloads/mainnet-byron-genesis.json");
const MAINNET_SHELLEY_GENESIS: &[u8] = include_bytes!("../downloads/mainnet-shelley-genesis.json");
const MAINNET_SHELLEY_START_EPOCH: u64 = 208;
const PREVIEW_BYRON_GENESIS: &[u8] = include_bytes!("../downloads/preview-byron-genesis.json");
const PREVIEW_SHELLEY_GENESIS: &[u8] = include_bytes!("../downloads/preview-shelley-genesis.json");
const PREVIEW_SHELLEY_START_EPOCH: u64 = 0;
const SANCHONET_BYRON_GENESIS: &[u8] = include_bytes!("../downloads/sanchonet-byron-genesis.json");
const SANCHONET_SHELLEY_GENESIS: &[u8] =
    include_bytes!("../downloads/sanchonet-shelley-genesis.json");
const SANCHONET_SHELLEY_START_EPOCH: u64 = 0;

const MAINNET_FIRST_BLOCK_ERA: Era = Era::Byron;
const PREVIEW_FIRST_BLOCK_ERA: Era = Era::Shelley;
const SANCHONET_FIRST_BLOCK_ERA: Era = Era::Conway;

fn hash_genesis_bytes(raw_bytes: &[u8]) -> Hash<32> {
    let mut hasher = Blake2b::<U32>::new();
    hasher.update(raw_bytes);
    let hash: [u8; 32] = hasher.finalize().into();
    Hash::<32>::new(hash)
}

fn approximate_rational(num: u64, den: u64) -> f64 {
    let scale = 10u128.pow(3);

    let scaled = (num as u128 * scale + den as u128 / 2) / den as u128;

    scaled as f64 / scale as f64
}

/// Genesis bootstrapper module
#[module(
    message_type(Message),
    name = "genesis-bootstrapper",
    description = "Genesis bootstrap UTXO event generator"
)]
pub struct GenesisBootstrapper;

impl GenesisBootstrapper {
    /// Main init function
    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        let startup_topic =
            config.get_string("startup-topic").unwrap_or(DEFAULT_STARTUP_TOPIC.to_string());
        info!("Creating startup subscriber on '{startup_topic}'");

        let snapshot_bootstrap = StartupMode::from_config(config.as_ref()).is_snapshot();

        let mut subscription = context.subscribe(&startup_topic).await?;
        context.clone().run(async move {
            let Ok(_) = subscription.read().await else {
                return;
            };
            let span = info_span!("genesis_bootstrapper", block = 0);
            async {
                info!("Received startup message");

                let completion_topic = config
                    .get_string("completion-topic")
                    .unwrap_or(DEFAULT_COMPLETION_TOPIC.to_string());
                info!("Completing with '{completion_topic}'");

                let network_name = config
                    .get_string("startup.network-name")
                    .unwrap_or(DEFAULT_NETWORK_NAME.to_string());

                let (byron_genesis_bytes, shelley_genesis_bytes, shelley_start_epoch, first_block_era):
                    (Cow<'static, [u8]>, Cow<'static, [u8]>, u64, Era) = match network_name.as_ref()
                {
                    "mainnet" => (
                        Cow::Borrowed(MAINNET_BYRON_GENESIS),
                        Cow::Borrowed(MAINNET_SHELLEY_GENESIS),
                        MAINNET_SHELLEY_START_EPOCH,
                        MAINNET_FIRST_BLOCK_ERA,
                    ),
                    "preview" => (
                        Cow::Borrowed(PREVIEW_BYRON_GENESIS),
                        Cow::Borrowed(PREVIEW_SHELLEY_GENESIS),
                        PREVIEW_SHELLEY_START_EPOCH,
                        PREVIEW_FIRST_BLOCK_ERA,
                    ),
                    "sanchonet" => (
                        Cow::Borrowed(SANCHONET_BYRON_GENESIS),
                        Cow::Borrowed(SANCHONET_SHELLEY_GENESIS),
                        SANCHONET_SHELLEY_START_EPOCH,
                        SANCHONET_FIRST_BLOCK_ERA,
                    ),
                    _ => {
                        let byron_path = config.get_string("byron-genesis-file");
                        let shelley_path = config.get_string("shelley-genesis-file");
                        match (byron_path, shelley_path) {
                            (Ok(bp), Ok(sp)) => {
                                info!("Loading custom genesis files: byron={bp}, shelley={sp}");
                                let byron = match std::fs::read(&bp) {
                                    Ok(data) => data,
                                    Err(e) => { error!("Cannot read byron genesis file {bp}: {e}"); return; }
                                };
                                let shelley = match std::fs::read(&sp) {
                                    Ok(data) => data,
                                    Err(e) => { error!("Cannot read shelley genesis file {sp}: {e}"); return; }
                                };
                                let shelley_start_epoch = config.get::<u64>("shelley-start-epoch").unwrap_or(0);
                                let first_block_era = config.get_string("first-block-era")
                                    .ok()
                                    .and_then(|s| match s.as_ref() {
                                        "byron" => Some(Era::Byron),
                                        "shelley" => Some(Era::Shelley),
                                        "allegra" => Some(Era::Allegra),
                                        "mary" => Some(Era::Mary),
                                        "alonzo" => Some(Era::Alonzo),
                                        "babbage" => Some(Era::Babbage),
                                        "conway" => Some(Era::Conway),
                                        _ => None,
                                    })
                                    .unwrap_or(Era::Byron);
                                (Cow::Owned(byron), Cow::Owned(shelley), shelley_start_epoch, first_block_era)
                            }
                            _ => {
                                error!("Cannot find genesis for {network_name}; set byron-genesis-file and shelley-genesis-file for custom networks");
                                return;
                            }
                        }
                    }
                };

                info!("Reading genesis for '{network_name}'");
                let shelley_genesis_hash = hash_genesis_bytes(&shelley_genesis_bytes);

                // Read genesis data
                let byron_genesis: ByronGenesisFile = serde_json::from_slice(&byron_genesis_bytes)
                    .expect("Invalid JSON in BYRON_GENESIS file");
                let shelley_genesis: ShelleyGenesisFile = serde_json::from_slice(&shelley_genesis_bytes)
                    .expect("Invalid JSON in SHELLEY_GENESIS file");
                let initial_reserves = shelley_genesis
                    .max_lovelace_supply
                    .expect("max_lovelace_supply not set in SHELLEY_GENESIS file");

                // Construct messages
                let block_info = BlockInfo {
                    status: BlockStatus::Bootstrap,
                    intent: BlockIntent::Apply,
                    slot: 0,
                    number: 0,
                    hash: BlockHash::default(),
                    epoch: 0,
                    epoch_slot: 0,
                    new_epoch: false,
                    is_new_era: true,
                    timestamp: byron_genesis.start_time,
                    era: first_block_era,
                    tip_slot: None,
                };

                if !snapshot_bootstrap {
                    let publish_utxo_deltas_topic = config
                        .get_string("publish-utxo-deltas-topic")
                        .unwrap_or(DEFAULT_PUBLISH_UTXO_DELTAS_TOPIC.to_string());
                    info!("Publishing UTXO deltas on '{publish_utxo_deltas_topic}'");

                    let publish_pot_deltas_topic = config
                        .get_string("publish-pot-deltas-topic")
                        .unwrap_or(DEFAULT_PUBLISH_POT_DELTAS_TOPIC.to_string());
                    info!("Publishing pot deltas on '{publish_pot_deltas_topic}'");

                    let publish_genesis_utxos_topic = config
                        .get_string("publish-genesis-utxos-topic")
                        .unwrap_or(DEFAULT_PUBLISH_GENESIS_UTXO_REGISTRY_TOPIC.to_string());
                    info!("Publishing genesis transactions on '{publish_genesis_utxos_topic}'");

                    let mut utxo_deltas_message = UTXODeltasMessage { deltas: Vec::new() };

                    // Convert the AVVM distributions into pseudo-UTXOs
                    let gen_utxos = genesis_utxos(&byron_genesis);
                    let mut gen_utxo_identifiers = Vec::new();
                    let mut total_allocated: u64 = 0;
                    for (tx_index, (hash, address, amount)) in gen_utxos.iter().enumerate() {
                        let tx_identifier = TxIdentifier::new(0, tx_index as u16);
                        let utxo_identifier = UTxOIdentifier::new(TxHash::from(**hash), 0);

                        gen_utxo_identifiers.push((utxo_identifier, tx_identifier));

                        let tx_output = TxOutput {
                            utxo_identifier,
                            address: Address::Byron(ByronAddress {
                                payload: address.payload.to_vec(),
                            }),
                            value: Value::new(*amount, Vec::new()),
                            datum: None,
                            reference_script_hash: None,
                        };

                        utxo_deltas_message.deltas.push(TxUTxODeltas {
                            tx_identifier,
                            consumes: Vec::new(),
                            produces: vec![tx_output],
                            fee: 0,
                            is_valid: true,
                            ..TxUTxODeltas::default()
                        });
                        total_allocated += amount;
                    }

                    info!(
                        total_allocated,
                        count = gen_utxos.len(),
                        "AVVM genesis UTXOs"
                    );

                    let message_enum = Message::Cardano((
                        block_info.clone(),
                        CardanoMessage::UTXODeltas(utxo_deltas_message),
                    ));
                    context
                        .publish(&publish_utxo_deltas_topic, Arc::new(message_enum))
                        .await
                        .unwrap_or_else(|e| error!("Failed to publish: {e}"));

                    // Send the pot update message with the remaining reserves
                    let pot_deltas = if first_block_era < Era::Shelley {
                        BootstrapPotDeltas {
                            delta_reserves: (initial_reserves - total_allocated) as LovelaceDelta,
                            delta_treasury: 0,
                            delta_deposits: 0,
                        }
                    } else {
                        // When booting directly into Shelley (e.g. Preview), apply the first epoch's
                        // treasury cut immediately. We approximate tau and rho to 3 decimal places to reflect
                        // their intended decimal values instead of the binary scaled rationals encoded in genesis.
                        let reserves_after_allocation = (initial_reserves - total_allocated) as f64;

                        let tau = approximate_rational(
                            shelley_genesis.protocol_params.tau.numerator,
                            shelley_genesis.protocol_params.tau.denominator,
                        );

                        let rho = approximate_rational(
                            shelley_genesis.protocol_params.rho.numerator,
                            shelley_genesis.protocol_params.rho.denominator,
                        );

                        let treasury_delta = reserves_after_allocation * tau * rho;

                        BootstrapPotDeltas {
                            delta_reserves: (initial_reserves
                                - total_allocated
                                - treasury_delta as u64)
                                as LovelaceDelta,
                            delta_treasury: treasury_delta as LovelaceDelta,
                            delta_deposits: 0,
                        }
                    };

                    let pot_deltas_message = PotDeltasMessage { deltas: pot_deltas };

                    let message_enum = Message::Cardano((
                        block_info.clone(),
                        CardanoMessage::PotDeltas(pot_deltas_message),
                    ));
                    context
                        .publish(&publish_pot_deltas_topic, Arc::new(message_enum))
                        .await
                        .unwrap_or_else(|e| error!("Failed to publish: {e}"));

                    let gen_utxos_message = Message::Cardano((
                        block_info.clone(),
                        CardanoMessage::GenesisUTxOs(GenesisUTxOsMessage {
                            utxos: gen_utxo_identifiers,
                        }),
                    ));
                    context
                        .publish(&publish_genesis_utxos_topic, Arc::new(gen_utxos_message))
                        .await
                        .unwrap_or_else(|e| error!("Failed to publish: {e}"));
                }

                let values = GenesisValues {
                    byron_timestamp: byron_genesis.start_time,
                    shelley_epoch: shelley_start_epoch,
                    shelley_epoch_len: shelley_genesis.epoch_length.unwrap() as u64,
                    shelley_genesis_hash,
                    genesis_delegs: GenesisDelegates::try_from(
                        shelley_genesis
                            .gen_delegs
                            .unwrap()
                            .iter()
                            .map(|(key, value)| {
                                (
                                    key.as_str(),
                                    (
                                        value.delegate.as_ref().unwrap().as_str(),
                                        value.vrf.as_ref().unwrap().as_str(),
                                    ),
                                )
                            })
                            .collect::<Vec<(&str, (&str, &str))>>(),
                    )
                    .unwrap(),
                    magic_number: MagicNumber::new(byron_genesis.protocol_consts.protocol_magic),
                };

                // Send completion message
                let message_enum = Message::Cardano((
                    block_info,
                    CardanoMessage::GenesisComplete(GenesisCompleteMessage { values }),
                ));
                context
                    .message_bus
                    .publish(&completion_topic, Arc::new(message_enum))
                    .await
                    .unwrap_or_else(|e| error!("Failed to publish: {e}"));
                info!("Publishing genesis complete message on '{completion_topic}'");
            }
            .instrument(span)
            .await;
        });

        Ok(())
    }
}
