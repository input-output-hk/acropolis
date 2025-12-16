//! Acropolis transaction unpacker module for Caryatid
//! Unpacks transaction bodies into UTXO events

use std::{collections::HashSet, sync::Arc};

use acropolis_common::{
    messages::{
        AssetDeltasMessage, BlockTxsMessage, CardanoMessage, GovernanceProceduresMessage, Message,
        StateTransitionMessage, TxCertificatesMessage, UTXODeltasMessage, WithdrawalsMessage,
    },
    protocol_params::ProtocolParams,
    state_history::{StateHistory, StateHistoryStore},
    *,
};
use anyhow::Result;
use caryatid_sdk::{module, Context, Subscription};
use config::Config;
use futures::future::join_all;
use pallas::codec::minicbor::encode;
use pallas::ledger::traverse::MultiEraTx;
use tokio::sync::Mutex;
use tracing::{debug, error, info, info_span};

use crate::state::State;
mod state;
mod tx_validation_publisher;
mod validations;
use tx_validation_publisher::TxValidationPublisher;
mod crypto;

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
        publish_block_txs_topic: Option<String>,
        tx_validation_publisher: Option<TxValidationPublisher>,
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
                    let mut total_fees: u64 = 0;
                    let total_txs = txs_msg.txs.len() as u64;
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

                                    let (
                                        tx_inputs,
                                        tx_outputs,
                                        tx_total_output,
                                        tx_certs,
                                        tx_withdrawals,
                                        tx_proposal_update,
                                        vkey_witnesses,
                                        native_scripts,
                                        tx_error
                                    ) = acropolis_codec::map_transaction(&tx, raw_tx, tx_identifier, network_id.clone(), block.era);
                                    let mut props = None;
                                    let mut votes = None;

                                    let mut vkey_hashes_needed = HashSet::new();
                                    let mut script_hashes_needed = HashSet::new();
                                    Self::get_vkey_script_needed(
                                        &tx_certs,
                                        &tx_withdrawals,
                                        &tx_proposal_update,
                                        &state.protocol_params,
                                        &mut vkey_hashes_needed,
                                        &mut script_hashes_needed,
                                    );
                                    let vkey_hashes_provided = vkey_witnesses.iter().map(|w| w.key_hash()).collect::<Vec<_>>();
                                    let script_hashes_provided = native_scripts.iter().map(|s| s.compute_hash()).collect::<Vec<_>>();

                                    // sum up total output lovelace for a block
                                    total_output += tx_total_output;

                                    if tracing::enabled!(tracing::Level::DEBUG) {
                                        debug!("Decoded tx with inputs={}, outputs={}, certs={}, total_output={}",
                                               tx_inputs.len(), tx_outputs.len(), tx_certs.len(), tx_total_output);
                                    }

                                    if let Some(error) = tx_error {
                                        error!(
                                            "Errors decoding transaction {tx_hash}: {error}"
                                        );
                                    }

                                    if publish_utxo_deltas_topic.is_some() {
                                        // Group deltas by tx
                                        utxo_deltas.push(TxUTxODeltas {
                                            tx_identifier,
                                            inputs: tx_inputs,
                                            outputs: tx_outputs,
                                            vkey_hashes_needed,
                                            script_hashes_needed,
                                            vkey_hashes_provided,
                                            script_hashes_provided,
                                        });
                                    }

                                    if publish_asset_deltas_topic.is_some() {
                                        let mut tx_deltas: Vec<(PolicyId, Vec<NativeAssetDelta>)> =
                                            Vec::new();

                                        // Mint deltas
                                        for policy_group in tx.mints().iter() {
                                            if let Some((policy_id, deltas)) =
                                                acropolis_codec::map_mint_burn(policy_group)
                                            {
                                                tx_deltas.push((policy_id, deltas));
                                            }
                                        }

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

                                        if !tx_deltas.is_empty() {
                                            asset_deltas.push((tx_identifier, tx_deltas));
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

                                    // Capture the fees
                                    if let Some(fee) = tx.fee() {
                                        total_fees += fee;
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

                    if let Some(ref topic) = publish_block_txs_topic {
                        let msg = Message::Cardano((
                            block.clone(),
                            CardanoMessage::BlockInfoMessage(BlockTxsMessage {
                                total_txs,
                                total_output,
                                total_fees,
                            }),
                        ));

                        futures.push(context.message_bus.publish(topic, Arc::new(msg)));
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

                    if let Some(ref topic) = publish_block_txs_topic {
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

            if let Some(tx_validation_publisher) = tx_validation_publisher.as_ref() {
                if let Message::Cardano((block, CardanoMessage::ReceivedTxs(txs_msg))) =
                    message.as_ref()
                {
                    let mut tx_errors = Vec::new();
                    for (tx_index, raw_tx) in txs_msg.txs.iter().enumerate() {
                        let tx_index = tx_index as u16;

                        // Validate transaction
                        if let Err(e) =
                            state.validate_transaction(block, raw_tx, &genesis.genesis_delegs)
                        {
                            tx_errors.push((tx_index, e));
                        }
                    }
                    if !tx_errors.is_empty() {
                        error!(
                            "Validation failed: block={}, bad_transactions={}",
                            block.number,
                            tx_errors
                                .iter()
                                .map(|(tx_index, error)| format!(
                                    "tx-index={tx_index}, error={error}"
                                ))
                                .collect::<Vec<_>>()
                                .join("; "),
                        );
                    }
                    tx_validation_publisher
                        .publish_tx_validation(block, tx_errors)
                        .await
                        .unwrap_or_else(|e| error!("Failed to publish tx validation: {e}"));
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
        let tx_validation_publisher = if let Some(ref topic) = publish_tx_validation_topic {
            info!("Publishing tx validation on '{topic}'");
            Some(TxValidationPublisher::new(context.clone(), topic.clone()))
        } else {
            None
        };

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
                publish_block_txs_topic,
                tx_validation_publisher,
                txs_sub,
                bootstrapped_sub,
                protocol_params_sub,
            )
            .await
            .unwrap_or_else(|e| error!("Failed to run Tx Unpacker: {e}"));
        });

        Ok(())
    }

    /// Get VKey Witnesses needed for transaction
    /// Get Scripts needed for transaction
    /// Reference: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/shelley/impl/src/Cardano/Ledger/Shelley/UTxO.hs#L274
    /// https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/shelley/impl/src/Cardano/Ledger/Shelley/UTxO.hs#L226
    /// https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/shelley/impl/src/Cardano/Ledger/Shelley/UTxO.hs#L103
    ///
    /// VKey Witnesses needed
    /// 1. UTxO authors: keys that own the UTxO being spent
    /// 2. Certificate authors: keys authorizing certificates
    /// 3. Pool owners: owners that must sign pool registration
    /// 4. Withdrawal authors: keys authorizing reward withdrawals
    /// 5. Governance authors: keys authorizing governance actions (e.g. protocol update)
    ///
    /// Script Witnesses needed
    /// 1. Input scripts: scripts locking UTxO being spent
    /// 2. Withdrawal scripts: scripts controlling reward accounts
    /// 3. Certificate scripts: scripts in certificate credentials.
    ///
    /// NOTE:
    /// This doesn't count `inputs`
    /// which will be considered in the utxos_state
    fn get_vkey_script_needed(
        certs: &[TxCertificateWithPos],
        withdrawals: &[Withdrawal],
        proposal_update: &Option<AlonzoBabbageUpdateProposal>,
        protocol_params: &ProtocolParams,
        vkey_hashes: &mut HashSet<KeyHash>,
        script_hashes: &mut HashSet<ScriptHash>,
    ) {
        let genesis_delegs =
            protocol_params.shelley.as_ref().map(|shelley_params| &shelley_params.gen_delegs);
        // for each certificate, get the required vkey and script hashes
        for cert_with_pos in certs.iter() {
            cert_with_pos.cert.get_cert_authors(vkey_hashes, script_hashes);
        }

        // for each withdrawal, get the required vkey and script hashes
        for withdrawal in withdrawals.iter() {
            withdrawal.get_withdrawal_authors(vkey_hashes, script_hashes);
        }

        // for each governance action, get the required vkey hashes
        if let Some(proposal_update) = proposal_update.as_ref() {
            if let Some(genesis_delegs) = genesis_delegs {
                proposal_update.get_governance_authors(vkey_hashes, genesis_delegs);
            } else {
                error!("Genesis delegates not found in protocol parameters");
            }
        }
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
