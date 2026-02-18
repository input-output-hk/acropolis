//! Acropolis transaction unpacker module for Caryatid
//! Unpacks transaction bodies into UTXO events

use std::sync::Arc;

use acropolis_common::{
    messages::{
        AssetDeltasMessage, CardanoMessage, GovernanceProceduresMessage, Message,
        StateTransitionMessage, TxCertificatesMessage, UTXODeltasMessage, WithdrawalsMessage,
    },
    state_history::{StateHistory, StateHistoryStore},
    validation::ValidationOutcomes,
    *,
};
use anyhow::Result;
use caryatid_sdk::{module, Context, Subscription};
use config::Config;
use futures::future::join_all;
use pallas::codec::minicbor::encode;
use pallas::ledger::traverse::MultiEraTx;
use tokio::sync::Mutex;
use tracing::{debug, error, info, info_span, Instrument};

use crate::state::State;
mod crypto;
pub mod state;
pub mod validations;

#[cfg(test)]
mod test_utils;

const DEFAULT_TRANSACTIONS_SUBSCRIBE_TOPIC: (&str, &str) =
    ("transactions-subscribe-topic", "cardano.txs");

const CIP25_METADATA_LABEL: u64 = 721;

/// Tx unpacker module
/// Parameterised by the outer message enum used on the bus
#[module(
    message_type(Message),
    name = "tx-unpacker",
    description = "Transaction to UTXO event unpacker"
)]
pub struct TxUnpacker;

impl TxUnpacker {
    #[allow(clippy::too_many_arguments)]
    async fn run(
        context: Arc<Context<Message>>,
        network_id: NetworkId,
        history: Arc<Mutex<StateHistory<State>>>,
        phase2_enabled: bool,
        // publishers
        publish_utxo_deltas_topic: Option<String>,
        publish_asset_deltas_topic: Option<String>,
        publish_withdrawals_topic: Option<String>,
        publish_certificates_topic: Option<String>,
        publish_governance_procedures_topic: Option<String>,
        publish_tx_validation_topic: Option<String>,
        // subscribers
        mut txs_sub: Box<dyn Subscription<Message>>,
        bootstrapped_sub: Option<Box<dyn Subscription<Message>>>,
        mut protocol_params_sub: Option<Box<dyn Subscription<Message>>>,
    ) -> Result<()> {
        let genesis = match bootstrapped_sub {
            Some(mut sub) => {
                let (_, bootstrapped_message) = sub.read().await?;
                let genesis = match bootstrapped_message.as_ref() {
                    Message::Cardano((_, CardanoMessage::GenesisComplete(complete))) => {
                        complete.values.clone()
                    }
                    _ => panic!(
                        "Unexpected message in genesis completion topic: {bootstrapped_message:?}"
                    ),
                };

                Some(genesis)
            }
            None => None,
        };

        loop {
            let mut state = history
                .lock()
                .await
                .get_or_init_with(|| State::with_phase2_enabled(phase2_enabled));
            let mut current_block: Option<BlockInfo> = None;

            let Ok((_, message)) = txs_sub.read().await else {
                return Err(anyhow::anyhow!("Failed to read txs subscription"));
            };

            let new_epoch = match message.as_ref() {
                Message::Cardano((block_info, _)) => {
                    // Handle rollbacks on this topic only
                    if block_info.status == BlockStatus::RolledBack {
                        state = history.lock().await.get_rolled_back_state(block_info.number);
                    }
                    current_block = Some(block_info.clone());

                    // new_epoch? first_epoch?
                    block_info.new_epoch
                }

                _ => {
                    error!("Unexpected message type: {message:?}");
                    false
                }
            };

            match message.as_ref() {
                Message::Cardano((block, CardanoMessage::ReceivedTxs(txs_msg))) => {
                    if tracing::enabled!(tracing::Level::DEBUG) {
                        debug!("Received {} txs for slot {}", txs_msg.txs.len(), block.slot);
                    }

                    let mut utxo_deltas = Vec::new();
                    let mut total_asset_deltas = Vec::new();
                    let mut cip25_metadata_updates = Vec::new();
                    let mut total_withdrawals = Vec::new();
                    let mut total_certificates = Vec::new();
                    let mut total_voting_procedures = Vec::new();
                    let mut total_proposal_procedures = Vec::new();
                    let mut total_alonzo_babbage_update_proposals = Vec::new();
                    let mut total_output: u128 = 0;
                    let block_number = block.number as u32;

                    let span: tracing::Span =
                        info_span!("tx_unpacker.handle_txs", block = block.number);
                    span.in_scope(|| {
                        for (tx_index, raw_tx) in txs_msg.txs.iter().enumerate() {
                            let tx_index = tx_index as u16;

                            // Parse the tx
                            match MultiEraTx::decode(raw_tx) {
                                Ok(tx) => {
                                    let tx_hash: TxHash =
                                        tx.hash().to_vec().try_into().expect("invalid tx hash length");
                                    let tx_identifier = TxIdentifier::new(block_number, tx_index);

                                    let mapped_tx = acropolis_codec::map_transaction(&tx, raw_tx, tx_identifier, network_id.clone(), block.era);
                                    let tx_output = mapped_tx.calculate_tx_output();

                                    // sum up total output lovelace for a block
                                    total_output += tx_output.coin() as u128;

                                    if tracing::enabled!(tracing::Level::DEBUG) {
                                        debug!("Decoded tx with inputs={}, outputs={}, certs={}, total_output_coin={}",
                                               &mapped_tx.consumes.len(), &mapped_tx.produces.len(), &mapped_tx.certs.len(), tx_output.coin());
                                    }

                                    if let Some(error) = mapped_tx.error.as_ref() {
                                        error!(
                                            "Errors decoding transaction {tx_hash}: {error}"
                                        );
                                    }

                                    if publish_asset_deltas_topic.is_some() {
                                        if let Some(metadata) = tx.metadata().find(CIP25_METADATA_LABEL)
                                        {
                                            let mut metadata_raw = Vec::new();
                                            match encode(metadata, &mut metadata_raw) {
                                                Ok(()) => {
                                                    cip25_metadata_updates.push(metadata_raw);
                                                }
                                                Err(e) => {
                                                    error!("failed to encode CIP-25 metadatum: {e:#}");
                                                }
                                            }
                                        }

                                        if !mapped_tx.mint_burn_deltas.is_empty() {
                                            total_asset_deltas.push(
                                                (tx_identifier, mapped_tx.mint_burn_deltas.clone())
                                            );
                                        }
                                    }

                                    if publish_certificates_topic.is_some() {
                                        total_certificates.extend(mapped_tx.certs.clone());
                                    }

                                    if publish_withdrawals_topic.is_some() {
                                        total_withdrawals.extend(mapped_tx.withdrawals.clone());
                                    }

                                   if publish_governance_procedures_topic.is_some() {
                                        if let Some(proposal_update) = mapped_tx.proposal_update.as_ref() {
                                            total_alonzo_babbage_update_proposals.push(proposal_update.clone());
                                        }

                                        if let Some(pps) = mapped_tx.proposal_procedures.as_ref() {
                                            total_proposal_procedures.extend(pps.clone());
                                        }

                                        if let Some(vps) = mapped_tx.voting_procedures.as_ref() {
                                            total_voting_procedures.push((tx_hash, vps.clone()));
                                        }
                                    }

                                    if publish_utxo_deltas_topic.is_some() {
                                        let mut deltas = mapped_tx.convert_to_utxo_deltas(true);
                                        deltas.tx_hash = tx_hash;
                                        utxo_deltas.push(deltas);
                                    }
                                }

                                Err(e) => {
                                    error!("Can't decode transaction in slot {}: {e}", block.slot)
                                }
                            }
                        }
                    });

                    // Publish messages in parallel
                    let mut futures = Vec::new();
                    if let Some(ref topic) = publish_utxo_deltas_topic {
                        let msg = Message::Cardano((
                            block.clone(),
                            CardanoMessage::UTXODeltas(UTXODeltasMessage {
                                deltas: utxo_deltas,
                            }),
                        ));

                        futures.push(context.message_bus.publish(topic, Arc::new(msg)));
                    }

                    if let Some(ref topic) = publish_asset_deltas_topic {
                        let msg = Message::Cardano((
                            block.clone(),
                            CardanoMessage::AssetDeltas(AssetDeltasMessage {
                                deltas: total_asset_deltas,
                                cip25_metadata_updates,
                            }),
                        ));

                        futures.push(context.message_bus.publish(topic, Arc::new(msg)));
                    }

                    if let Some(ref topic) = publish_withdrawals_topic {
                        let msg = Message::Cardano((
                            block.clone(),
                            CardanoMessage::Withdrawals(WithdrawalsMessage {
                                withdrawals: total_withdrawals,
                            }),
                        ));

                        futures.push(context.message_bus.publish(topic, Arc::new(msg)));
                    }

                    if let Some(ref topic) = publish_certificates_topic {
                        let msg = Message::Cardano((
                            block.clone(),
                            CardanoMessage::TxCertificates(TxCertificatesMessage {
                                certificates: total_certificates,
                            }),
                        ));

                        futures.push(context.message_bus.publish(topic, Arc::new(msg)));
                    }

                    if let Some(ref topic) = publish_governance_procedures_topic {
                        let governance_msg = Arc::new(Message::Cardano((
                            block.clone(),
                            CardanoMessage::GovernanceProcedures(GovernanceProceduresMessage {
                                voting_procedures: total_voting_procedures,
                                proposal_procedures: total_proposal_procedures,
                                alonzo_babbage_updates: total_alonzo_babbage_update_proposals,
                            }),
                        )));

                        futures.push(context.message_bus.publish(topic, governance_msg.clone()));
                    }

                    join_all(futures)
                        .await
                        .into_iter()
                        .filter_map(Result::err)
                        .for_each(|e| error!("Failed to publish: {e}"));
                }

                Message::Cardano((
                    _,
                    CardanoMessage::StateTransition(StateTransitionMessage::Rollback(_)),
                )) => {
                    let mut futures = Vec::new();
                    if let Some(ref topic) = publish_utxo_deltas_topic {
                        futures.push(context.message_bus.publish(topic, message.clone()));
                    }

                    if let Some(ref topic) = publish_asset_deltas_topic {
                        futures.push(context.message_bus.publish(topic, message.clone()));
                    }

                    if let Some(ref topic) = publish_withdrawals_topic {
                        futures.push(context.message_bus.publish(topic, message.clone()));
                    }

                    if let Some(ref topic) = publish_certificates_topic {
                        futures.push(context.message_bus.publish(topic, message.clone()));
                    }

                    if let Some(ref topic) = publish_governance_procedures_topic {
                        futures.push(context.message_bus.publish(topic, message.clone()));
                    }

                    join_all(futures)
                        .await
                        .into_iter()
                        .filter_map(Result::err)
                        .for_each(|e| error!("Failed to publish: {e}"));
                }

                _ => error!("Unexpected message type: {message:?}"),
            }

            if new_epoch {
                if let Some(ref mut sub) = protocol_params_sub {
                    let (_, protocol_parameters_msg) = sub.read().await?;
                    if let Message::Cardano((block_info, CardanoMessage::ProtocolParams(params))) =
                        protocol_parameters_msg.as_ref()
                    {
                        Self::check_sync(&current_block, block_info);
                        let span = info_span!(
                            "tx_unpacker.handle_protocol_params",
                            block = block_info.number
                        );
                        span.in_scope(|| {
                            state.handle_protocol_params(params);
                        });
                    }
                }
            }

            if let Some(publish_tx_validation_topic) = publish_tx_validation_topic.as_ref() {
                if let Some(ref genesis) = genesis {
                    if let Message::Cardano((block, CardanoMessage::ReceivedTxs(txs_msg))) =
                        message.as_ref()
                    {
                        let span = info_span!("tx_unpacker.validate", block = block.number);
                        async {
                            let mut validation_outcomes = ValidationOutcomes::new();
                            if let Err(e) = state.validate(block, txs_msg, &genesis.genesis_delegs)
                            {
                                validation_outcomes.push(*e);
                            }

                            validation_outcomes
                                .publish(
                                    &context,
                                    "tx_unpacker",
                                    publish_tx_validation_topic,
                                    block,
                                )
                                .await
                                .unwrap_or_else(|e| error!("Failed to publish tx validation: {e}"));
                        }
                        .instrument(span)
                        .await;
                    }
                }
            }

            // Commit the new state
            if let Some(block_info) = current_block {
                history.lock().await.commit(block_info.number, state);
            }
        }
    }

    /// Main init function
    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        // Publishers
        let publish_utxo_deltas_topic = config.get_string("publish-utxo-deltas-topic").ok();
        if let Some(ref topic) = publish_utxo_deltas_topic {
            info!("Publishing UTXO deltas on '{topic}'");
        }

        let publish_asset_deltas_topic = config.get_string("publish-asset-deltas-topic").ok();
        if let Some(ref topic) = publish_asset_deltas_topic {
            info!("Publishing native asset deltas on '{topic}'");
        }

        let publish_withdrawals_topic = config.get_string("publish-withdrawals-topic").ok();
        if let Some(ref topic) = publish_withdrawals_topic {
            info!("Publishing withdrawals on '{topic}'");
        }

        let publish_certificates_topic = config.get_string("publish-certificates-topic").ok();
        if let Some(ref topic) = publish_certificates_topic {
            info!("Publishing certificates on '{topic}'");
        }

        let publish_governance_procedures_topic =
            config.get_string("publish-governance-topic").ok();
        if let Some(ref topic) = publish_governance_procedures_topic {
            info!("Publishing governance procedures on '{topic}'");
        }

        let publish_block_txs_topic = config.get_string("publish-block-txs-topic").ok();
        if let Some(ref topic) = publish_block_txs_topic {
            info!("Publishing block txs on '{topic}'");
        }

        let publish_tx_validation_topic = config.get_string("publish-tx-validation-topic").ok();

        // Main transaction subscriber
        let transactions_subscribe_topic = config
            .get_string(DEFAULT_TRANSACTIONS_SUBSCRIBE_TOPIC.0)
            .unwrap_or(DEFAULT_TRANSACTIONS_SUBSCRIBE_TOPIC.1.to_string());
        info!("Creating subscriber on '{transactions_subscribe_topic}'");
        let txs_sub = context.subscribe(&transactions_subscribe_topic).await?;

        // Optional subscription for parameters (only needed if we are validating)
        let protocol_params_subscribe_topic =
            config.get_string("protocol-parameters-subscribe-topic").ok();
        let protocol_params_sub = match protocol_params_subscribe_topic {
            Some(topic) => {
                info!("Creating subscriber on '{topic}'");
                Some(context.subscribe(&topic).await?)
            }
            None => None,
        };

        // Optional subscription for bootstrap (only needed if we are validating)
        let bootstrapped_subscribe_topic = config.get_string("bootstrapped-subscribe-topic").ok();
        let bootstrapped_sub = match bootstrapped_subscribe_topic {
            Some(topic) => {
                info!("Creating subscriber on '{topic}'");
                Some(context.subscribe(&topic).await?)
            }
            None => None,
        };

        let network_id: NetworkId =
            config.get_string("network-id").unwrap_or("mainnet".to_string()).into();

        // Phase 2 script validation (disabled by default)
        let phase2_enabled = config.get_bool("phase2-enabled").unwrap_or(false);
        if phase2_enabled {
            info!("Phase 2 script validation enabled");
        }

        // Initialize State
        let history = Arc::new(Mutex::new(StateHistory::<State>::new(
            "tx_unpacker",
            StateHistoryStore::default_block_store(),
        )));

        let context_run = context.clone();
        context.run(async move {
            Self::run(
                context_run,
                network_id,
                history,
                phase2_enabled,
                publish_utxo_deltas_topic,
                publish_asset_deltas_topic,
                publish_withdrawals_topic,
                publish_certificates_topic,
                publish_governance_procedures_topic,
                publish_tx_validation_topic,
                txs_sub,
                bootstrapped_sub,
                protocol_params_sub,
            )
            .await
            .unwrap_or_else(|e| error!("Failed to run Tx Unpacker: {e}"));
        });

        Ok(())
    }

    /// Check for synchronisation
    fn check_sync(expected: &Option<BlockInfo>, actual: &BlockInfo) {
        if let Some(ref block) = expected {
            if block.number != actual.number {
                error!(
                    expected = block.number,
                    actual = actual.number,
                    "Messages out of sync"
                );
            }
        }
    }
}
