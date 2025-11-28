//! Acropolis transaction unpacker module for Caryatid
//! Unpacks transaction bodies into UTXO events

use acropolis_codec::*;
use acropolis_common::{
    messages::{
        AssetDeltasMessage, BlockTxsMessage, CardanoMessage, GovernanceProceduresMessage, Message,
        TxCertificatesMessage, UTXODeltasMessage, WithdrawalsMessage,
    },
    state_history::{StateHistory, StateHistoryStore},
    *,
};
use anyhow::Result;
use caryatid_sdk::{module, Context, Subscription};
use config::Config;
use futures::future::join_all;
use pallas::codec::minicbor::encode;
use pallas::ledger::primitives::KeyValuePairs;
use pallas::ledger::{primitives, traverse, traverse::MultiEraTx};
use std::{clone::Clone, fmt::Debug, sync::Arc};
use tokio::sync::Mutex;
use tracing::{debug, error, info, info_span};
mod state;
mod utxo_registry;
mod validations;
use crate::{state::State, utxo_registry::UTxORegistry};
mod test_utils;

const DEFAULT_TRANSACTIONS_SUBSCRIBE_TOPIC: &str = "cardano.txs";
const DEFAULT_GENESIS_SUBSCRIBE_TOPIC: &str = "cardano.genesis.utxos";
const DEFAULT_PROTOCOL_PARAMS_SUBSCRIBE_TOPIC: &str = "cardano.protocol.parameters";

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
        mut utxo_registry: UTxORegistry,
        // publishers
        publish_utxo_deltas_topic: Option<String>,
        publish_asset_deltas_topic: Option<String>,
        publish_withdrawals_topic: Option<String>,
        publish_certificates_topic: Option<String>,
        publish_governance_procedures_topic: Option<String>,
        publish_block_txs_topic: Option<String>,
        // subscribers
        mut genesis_sub: Box<dyn Subscription<Message>>,
        mut txs_sub: Box<dyn Subscription<Message>>,
        mut protocol_params_sub: Box<dyn Subscription<Message>>,
    ) -> Result<()> {
        // Initialize TxRegistry with genesis utxos
        let (_, message) = genesis_sub.read().await.expect("failed to read genesis utxos");
        match message.as_ref() {
            Message::Cardano((_block, CardanoMessage::GenesisUTxOs(genesis_msg))) => {
                utxo_registry.bootstrap_from_genesis_utxos(&genesis_msg.utxos);
                info!(
                    "Seeded registry with {} genesis utxos",
                    genesis_msg.utxos.len()
                );
            }
            other => panic!("expected GenesisUTxOs, got {:?}", other),
        }

        loop {
            let mut state = history.lock().await.get_or_init_with(State::new);
            let mut current_block: Option<BlockInfo> = None;

            let Ok((_, message)) = txs_sub.read().await else {
                return Err(anyhow::anyhow!("failed to read txs"));
            };
            let new_epoch = match message.as_ref() {
                Message::Cardano((block_info, _)) => {
                    // Handle rollbacks on this topic only
                    if block_info.status == BlockStatus::RolledBack {
                        state = history.lock().await.get_rolled_back_state(block_info.number);
                    }
                    current_block = Some(block_info.clone());

                    // new_epoch?
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

                    // handle rollback or advance registry to the next block
                    let block_number = block.number as u32;
                    if block.status == BlockStatus::RolledBack {
                        if let Err(e) = utxo_registry.rollback_before(block_number) {
                            error!("rollback_before({}) failed: {}", block_number, e);
                        }
                        utxo_registry.next_block();
                        state = history.lock().await.get_rolled_back_state(block.number);
                        current_block = Some(block.clone());
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

                    let span = info_span!("tx_unpacker.handle_txs", block = block.number);
                    span.in_scope(|| {
                        for (tx_index, raw_tx) in txs_msg.txs.iter().enumerate() {
                            let tx_index = tx_index as u16;

                            // Parse the tx
                            match MultiEraTx::decode(raw_tx) {
                                Ok(tx) => {
                                    // Validate transaction
                                    let _ = state.validate_transaction(block, &tx, &utxo_registry);

                                    let tx_hash: TxHash =
                                        tx.hash().to_vec().try_into().expect("invalid tx hash length");
                                    let tx_identifier = TxIdentifier::new(block_number, tx_index);

                                    let certs = tx.certs();
                                    let tx_withdrawals = tx.withdrawals_sorted_set();
                                    let mut props = None;
                                    let mut votes = None;

                                    let (tx_inputs, tx_outputs, errors) =
                                        map_parameters::map_transaction_inputs_outputs(
                                            block_number,
                                            tx_index,
                                            &tx,
                                        );

                                    if tracing::enabled!(tracing::Level::DEBUG) {
                                        debug!(
                                                "Decoded tx with {} inputs, {} outputs, {} certs, {} errors",
                                                tx_inputs.len(),
                                                tx_outputs.len(),
                                                certs.len(),
                                                errors.len()
                                            )
                                    }

                                    if publish_utxo_deltas_topic.is_some() {
                                        // Lookup and remove UTxOIdentifier from registry
                                        // Group deltas by tx
                                        let mut tx_utxo_deltas = TxUTxODeltas {
                                            tx_identifier,
                                            inputs: Vec::new(),
                                            outputs: Vec::new(),
                                        };

                                        // Remove inputs from UTxORegistry and push to UTxOIdentifiers to delta
                                        for tx_ref in tx_inputs {
                                            match utxo_registry.consume(&tx_ref) {
                                                Ok(tx_identifier) => {
                                                    // Add TxInput to utxo_deltas
                                                    tx_utxo_deltas.inputs.push(UTxOIdentifier::new(
                                                        tx_identifier.block_number(),
                                                        tx_identifier.tx_index(),
                                                        tx_ref.output_index,
                                                    ));
                                                }
                                                Err(e) => {
                                                    error!(
                                                        "Failed to consume input {}: {e}",
                                                        tx_ref.output_index
                                                    );
                                                }
                                            }
                                        }

                                        for (tx_ref, output) in tx_outputs.into_iter() {
                                            total_output += output.value.coin() as u128;

                                            if let Err(e) =
                                                utxo_registry.add(block_number, tx_index, tx_ref)
                                            {
                                                error!("Failed to insert output into registry: {e}");
                                            } else {
                                                tx_utxo_deltas.outputs.push(output);
                                            }
                                        }

                                        utxo_deltas.push(tx_utxo_deltas);
                                    }

                                    if publish_asset_deltas_topic.is_some() {
                                        let mut tx_deltas: Vec<(PolicyId, Vec<NativeAssetDelta>)> =
                                            Vec::new();

                                        // Mint deltas
                                        for policy_group in tx.mints().iter() {
                                            if let Some((policy_id, deltas)) =
                                                map_parameters::map_mint_burn(policy_group)
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
                                        for (cert_index, cert) in certs.iter().enumerate() {
                                            match map_parameters::map_certificate(
                                                cert,
                                                tx_identifier,
                                                cert_index,
                                                network_id,
                                            ) {
                                                Ok(tx_cert) => {
                                                    certificates.push(tx_cert);
                                                }
                                                Err(_e) => {
                                                    // TODO error unexpected
                                                    //error!("{e}");
                                                }
                                            }
                                        }
                                    }

                                    if publish_withdrawals_topic.is_some() {
                                        for (key, value) in tx_withdrawals {
                                            match StakeAddress::from_binary(key) {
                                                Ok(stake_address) => {
                                                    withdrawals.push(Withdrawal {
                                                        address: stake_address,
                                                        value,
                                                        tx_identifier,
                                                    });
                                                }
                                                Err(e) => error!("Bad stake address: {e:#}"),
                                            }
                                        }
                                    }

                                    if publish_governance_procedures_topic.is_some() {
                                        //Self::decode_legacy_updates(&mut legacy_update_proposals, &block, &raw_tx);
                                        if block.era >= Era::Shelley && block.era < Era::Babbage {
                                            if let Ok(alonzo) = MultiEraTx::decode_for_era(
                                                traverse::Era::Alonzo,
                                                raw_tx,
                                            ) {
                                                if let Some(update) = alonzo.update() {
                                                    if let Some(alonzo_update) = update.as_alonzo() {
                                                        Self::decode_updates(
                                                                &mut alonzo_babbage_update_proposals,
                                                                &alonzo_update.proposed_protocol_parameter_updates,
                                                                alonzo_update.epoch,
                                                                map_parameters::map_alonzo_protocol_param_update
                                                            );
                                                    }
                                                }
                                            }
                                        } else if block.era >= Era::Babbage && block.era < Era::Conway {
                                            if let Ok(babbage) = MultiEraTx::decode_for_era(
                                                traverse::Era::Babbage,
                                                raw_tx,
                                            ) {
                                                if let Some(update) = babbage.update() {
                                                    if let Some(babbage_update) = update.as_babbage() {
                                                        Self::decode_updates(
                                                                &mut alonzo_babbage_update_proposals,
                                                                &babbage_update.proposed_protocol_parameter_updates,
                                                                babbage_update.epoch,
                                                                map_parameters::map_babbage_protocol_param_update
                                                            );
                                                    }
                                                }
                                            }
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
                                                        .and_then (|proc_id| map_parameters::map_governance_proposals_procedures(proc_id, pallas_governance_proposals))
                                                    {
                                                        Ok(g) => proposal_procedures.push(g),
                                                        Err(e) => error!("Cannot decode governance proposal procedure {} idx {} in slot {}: {e}", proc_id, action_index, block.slot)
                                                    }
                                            }
                                        }

                                        if let Some(pallas_vp) = votes {
                                            // Nonempty set -- governance_message.voting_procedures will not be empty
                                            match map_parameters::map_all_governance_voting_procedures(pallas_vp) {
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

                    utxo_registry.next_block();

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

            // Commit the new state
            if let Some(block_info) = current_block {
                history.lock().await.commit(block_info.number, state);
            }
        }
    }

    fn decode_updates<EraSpecificUpdateProposals: Clone + Debug>(
        dest: &mut Vec<AlonzoBabbageUpdateProposal>,
        proposals: &KeyValuePairs<primitives::Bytes, EraSpecificUpdateProposals>,
        epoch: u64,
        map: impl Fn(&EraSpecificUpdateProposals) -> Result<Box<ProtocolParamUpdate>>,
    ) {
        let mut update = AlonzoBabbageUpdateProposal {
            proposals: Vec::new(),
            enactment_epoch: epoch,
        };

        for (hash_bytes, vote) in proposals.iter() {
            let hash = match GenesisKeyhash::try_from(hash_bytes.as_ref()) {
                Ok(h) => h,
                Err(e) => {
                    error!("Invalid genesis keyhash in protocol parameter update: {e}");
                    continue;
                }
            };

            match map(vote) {
                Ok(upd) => update.proposals.push((hash, upd)),
                Err(e) => error!("Cannot convert protocol param update {vote:?}: {e}"),
            }
        }

        dest.push(update);
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

        // Subscribers
        let genesis_utxos_subscribe_topic = config
            .get_string("genesis-utxos-subscribe-topic")
            .unwrap_or(DEFAULT_GENESIS_SUBSCRIBE_TOPIC.to_string());
        info!("Creating subscriber on '{genesis_utxos_subscribe_topic}'");

        let transactions_subscribe_topic = config
            .get_string("subscribe-topic")
            .unwrap_or(DEFAULT_TRANSACTIONS_SUBSCRIBE_TOPIC.to_string());
        info!("Creating subscriber on '{transactions_subscribe_topic}'");

        let protocol_params_subscribe_topic = config
            .get_string("protocol-params-subscribe-topic")
            .unwrap_or(DEFAULT_PROTOCOL_PARAMS_SUBSCRIBE_TOPIC.to_string());
        info!("Creating subscriber on '{protocol_params_subscribe_topic}'");

        let genesis_sub = context.subscribe(&genesis_utxos_subscribe_topic).await?;
        let txs_sub = context.subscribe(&transactions_subscribe_topic).await?;
        let protocol_params_sub = context.subscribe(&protocol_params_subscribe_topic).await?;

        let network_id: NetworkId =
            config.get_string("network-id").unwrap_or("mainnet".to_string()).into();

        // Initialize State
        let history = Arc::new(Mutex::new(StateHistory::<State>::new(
            "tx_unpacker",
            StateHistoryStore::default_block_store(),
        )));

        // Initialize UTxORegistry
        let utxo_registry = UTxORegistry::default();

        let context_run = context.clone();
        context.run(async move {
            Self::run(
                context_run,
                network_id,
                history,
                utxo_registry,
                publish_utxo_deltas_topic,
                publish_asset_deltas_topic,
                publish_withdrawals_topic,
                publish_certificates_topic,
                publish_governance_procedures_topic,
                publish_block_txs_topic,
                genesis_sub,
                txs_sub,
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
