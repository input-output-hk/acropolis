//! Acropolis transaction unpacker module for Caryatid
//! Unpacks transaction bodies into UTXO events

use std::{collections::HashSet, sync::Arc};

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
mod state;
mod utils;
mod validations;

#[cfg(test)]
mod test_utils;

const DEFAULT_TRANSACTIONS_SUBSCRIBE_TOPIC: (&str, &str) =
    ("transactions-subscribe-topic", "cardano.txs");
const DEFAULT_PROTOCOL_PARAMS_SUBSCRIBE_TOPIC: (&str, &str) = (
    "protocol-parameters-subscribe-topic",
    "cardano.protocol.parameters",
);
const DEFAULT_BOOTSTRAPPED_SUBSCRIBE_TOPIC: (&str, &str) = (
    "bootstrapped-subscribe-topic",
    "cardano.sequence.bootstrapped",
);

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
        // publishers
        publish_utxo_deltas_topic: Option<String>,
        publish_asset_deltas_topic: Option<String>,
        publish_withdrawals_topic: Option<String>,
        publish_certificates_topic: Option<String>,
        publish_governance_procedures_topic: Option<String>,
        publish_tx_validation_topic: Option<String>,
        // subscribers
        mut txs_sub: Box<dyn Subscription<Message>>,
        mut bootstrapped_sub: Box<dyn Subscription<Message>>,
        mut protocol_params_sub: Box<dyn Subscription<Message>>,
    ) -> Result<()> {
        let (_, bootstrapped_message) = bootstrapped_sub.read().await?;
        let genesis = match bootstrapped_message.as_ref() {
            Message::Cardano((_, CardanoMessage::GenesisComplete(complete))) => {
                complete.values.clone()
            }
            _ => panic!("Unexpected message in genesis completion topic: {bootstrapped_message:?}"),
        };

        loop {
            let mut state = history.lock().await.get_or_init_with(State::new);
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
                    let mut asset_deltas = Vec::new();
                    let mut cip25_metadata_updates = Vec::new();
                    let mut withdrawals = Vec::new();
                    let mut certificates = Vec::new();
                    let mut voting_procedures = Vec::new();
                    let mut proposal_procedures = Vec::new();
                    let mut alonzo_babbage_update_proposals = Vec::new();
                    let mut total_output: u128 = 0;
                    let block_number = block.number as u32;

                    let span = info_span!("tx_unpacker.handle_txs", block = block.number);
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
                                    let tx_total_output = mapped_tx.calculate_total_output();

                                    let Transaction {
                                        consumes: tx_consumes,
                                        produces: tx_produces,
                                        fee:tx_fee,
                                        is_valid,
                                        certs: tx_certs,
                                        withdrawals: tx_withdrawals,
                                        proposal_update: tx_proposal_update,
                                        vkey_witnesses,
                                        native_scripts,
                                        error: tx_error,
                                    } = mapped_tx;
                                    let mut props = None;
                                    let mut votes = None;

                                    let certs_identifiers = tx_certs.iter().map(|c| c.tx_certificate_identifier()).collect::<Vec<_>>();
                                    let total_withdrawals = tx_withdrawals.iter().map(|w| w.value).sum::<u64>();
                                    let mut vkey_needed = HashSet::new();
                                    let mut script_needed = HashSet::new();
                                    utils::get_vkey_script_needed(
                                        &tx_certs,
                                        &tx_withdrawals,
                                        &tx_proposal_update,
                                        &state.protocol_params,
                                        &mut vkey_needed,
                                        &mut script_needed,
                                    );
                                    let vkey_hashes_provided = vkey_witnesses.iter().map(|w| w.key_hash()).collect::<Vec<_>>();
                                    let script_hashes_provided = native_scripts.iter().map(|s| s.compute_hash()).collect::<Vec<_>>();

                                    // sum up total output lovelace for a block
                                    total_output += tx_total_output.coin() as u128;

                                    // Mint or burn deltas
                                    let mut mint_burn_deltas:NativeAssetsDelta =
                                            Vec::new();

                                    // Mint deltas
                                    for policy_group in tx.mints().iter() {
                                        if let Some((policy_id, deltas)) =
                                            acropolis_codec::map_mint_burn(policy_group)
                                        {
                                            mint_burn_deltas.push((policy_id, deltas));
                                        }
                                    }

                                    if tracing::enabled!(tracing::Level::DEBUG) {
                                        debug!("Decoded tx with inputs={}, outputs={}, certs={}, total_output_coin={}",
                                               tx_consumes.len(), tx_produces.len(), tx_certs.len(), tx_total_output.coin());
                                    }

                                    if let Some(error) = tx_error {
                                        error!(
                                            "Errors decoding transaction {tx_hash}: {error}"
                                        );
                                    }

                                    if publish_utxo_deltas_topic.is_some() {
                                        // Group deltas by tx
                                        let (value_minted, value_burnt) = utils::get_value_minted_burnt_from_deltas(&mint_burn_deltas);
                                        utxo_deltas.push(TxUTxODeltas {
                                            tx_identifier,
                                            consumes: tx_consumes,
                                            produces: tx_produces,
                                            fee: tx_fee,
                                            is_valid,
                                            total_withdrawals: Some(total_withdrawals),
                                            certs_identifiers: Some(certs_identifiers),
                                            value_minted: Some(value_minted),
                                            value_burnt: Some(value_burnt),
                                            vkey_hashes_needed: Some(vkey_needed),
                                            script_hashes_needed: Some(script_needed),
                                            vkey_hashes_provided: Some(vkey_hashes_provided),
                                            script_hashes_provided: Some(script_hashes_provided),
                                        });
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

                                        if !mint_burn_deltas.is_empty() {
                                            asset_deltas.push((tx_identifier, mint_burn_deltas));
                                        }
                                    }

                                    if publish_certificates_topic.is_some() {
                                        certificates.extend(tx_certs);
                                    }

                                    if publish_withdrawals_topic.is_some() {
                                        withdrawals.extend(tx_withdrawals);
                                    }

                                   if publish_governance_procedures_topic.is_some() {
                                    if let Some(proposal_update) = tx_proposal_update {
                                        alonzo_babbage_update_proposals.push(proposal_update);
                                    }
                                    }

                                    if let Some(conway) = tx.as_conway() {
                                        if let Some(ref v) = conway.transaction_body.voting_procedures {
                                            votes = Some(v);
                                        }

                                        if let Some(ref p) = conway.transaction_body.proposal_procedures
                                        {
                                            props = Some(p);
                                        }
                                    }

                                    if publish_governance_procedures_topic.is_some() {
                                        if let Some(pp) = props {
                                            // Nonempty set -- governance_message.proposal_procedures will not be empty
                                            let mut proc_id = GovActionId {
                                                transaction_id: tx_hash,
                                                action_index: 0,
                                            };
                                            for (action_index, pallas_governance_proposals) in
                                                pp.iter().enumerate()
                                            {
                                                match proc_id.set_action_index(action_index)
                                                        .and_then (|proc_id| acropolis_codec::map_governance_proposals_procedures(proc_id, pallas_governance_proposals))
                                                    {
                                                        Ok(g) => proposal_procedures.push(g),
                                                        Err(e) => error!("Cannot decode governance proposal procedure {} idx {} in slot {}: {e}", proc_id, action_index, block.slot)
                                                    }
                                            }
                                        }

                                        if let Some(pallas_vp) = votes {
                                            // Nonempty set -- governance_message.voting_procedures will not be empty
                                            match acropolis_codec::map_all_governance_voting_procedures(pallas_vp) {
                                                    Ok(vp) => voting_procedures.push((tx_hash, vp)),
                                                    Err(e) => error!("Cannot decode governance voting procedures in slot {}: {e}", block.slot)
                                                }
                                        }
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
                                deltas: asset_deltas,
                                cip25_metadata_updates,
                            }),
                        ));

                        futures.push(context.message_bus.publish(topic, Arc::new(msg)));
                    }

                    if let Some(ref topic) = publish_withdrawals_topic {
                        let msg = Message::Cardano((
                            block.clone(),
                            CardanoMessage::Withdrawals(WithdrawalsMessage { withdrawals }),
                        ));

                        futures.push(context.message_bus.publish(topic, Arc::new(msg)));
                    }

                    if let Some(ref topic) = publish_certificates_topic {
                        let msg = Message::Cardano((
                            block.clone(),
                            CardanoMessage::TxCertificates(TxCertificatesMessage { certificates }),
                        ));

                        futures.push(context.message_bus.publish(topic, Arc::new(msg)));
                    }

                    if let Some(ref topic) = publish_governance_procedures_topic {
                        let governance_msg = Arc::new(Message::Cardano((
                            block.clone(),
                            CardanoMessage::GovernanceProcedures(GovernanceProceduresMessage {
                                voting_procedures,
                                proposal_procedures,
                                alonzo_babbage_updates: alonzo_babbage_update_proposals,
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
                let (_, protocol_parameters_msg) = protocol_params_sub.read().await?;
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

            if let Some(publish_tx_validation_topic) = publish_tx_validation_topic.as_ref() {
                if let Message::Cardano((block, CardanoMessage::ReceivedTxs(txs_msg))) =
                    message.as_ref()
                {
                    let span = info_span!("tx_unpacker.validate", block = block.number);
                    async {
                        let mut validation_outcomes = ValidationOutcomes::new();
                        if let Err(e) = state.validate(block, txs_msg, &genesis.genesis_delegs) {
                            validation_outcomes.push(*e);
                        }

                        validation_outcomes
                            .publish(&context, publish_tx_validation_topic, block)
                            .await
                            .unwrap_or_else(|e| error!("Failed to publish tx validation: {e}"));
                    }
                    .instrument(span)
                    .await;
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

        // Subscribers
        let transactions_subscribe_topic = config
            .get_string(DEFAULT_TRANSACTIONS_SUBSCRIBE_TOPIC.0)
            .unwrap_or(DEFAULT_TRANSACTIONS_SUBSCRIBE_TOPIC.1.to_string());
        info!("Creating subscriber on '{transactions_subscribe_topic}'");

        let bootstrapped_subscribe_topic = config
            .get_string(DEFAULT_BOOTSTRAPPED_SUBSCRIBE_TOPIC.0)
            .unwrap_or(DEFAULT_BOOTSTRAPPED_SUBSCRIBE_TOPIC.1.to_string());
        info!("Creating subscriber on '{bootstrapped_subscribe_topic}'");

        let protocol_params_subscribe_topic = config
            .get_string(DEFAULT_PROTOCOL_PARAMS_SUBSCRIBE_TOPIC.0)
            .unwrap_or(DEFAULT_PROTOCOL_PARAMS_SUBSCRIBE_TOPIC.1.to_string());
        info!("Creating subscriber on '{protocol_params_subscribe_topic}'");

        let txs_sub = context.subscribe(&transactions_subscribe_topic).await?;
        let bootstrapped_sub = context.subscribe(&bootstrapped_subscribe_topic).await?;
        let protocol_params_sub = context.subscribe(&protocol_params_subscribe_topic).await?;

        let network_id: NetworkId =
            config.get_string("network-id").unwrap_or("mainnet".to_string()).into();

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
