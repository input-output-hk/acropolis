//! Acropolis transaction unpacker module for Caryatid
//! Unpacks transaction bodies into UTXO events

use acropolis_common::{
    messages::{
        AssetDeltasMessage, BlockTxsMessage, CardanoMessage, GovernanceProceduresMessage, Message,
        TxCertificatesMessage, UTXODeltasMessage, WithdrawalsMessage,
    },
    *,
};
use caryatid_sdk::{module, Context, Module};
use std::{clone::Clone, fmt::Debug, sync::Arc};

use anyhow::Result;
use config::Config;
use futures::future::join_all;
use pallas::codec::minicbor::encode;
use pallas::ledger::primitives::KeyValuePairs;
use pallas::ledger::{primitives, traverse, traverse::MultiEraTx};
use tracing::{debug, error, info, info_span, Instrument};

mod map_parameters;

const DEFAULT_SUBSCRIBE_TOPIC: &str = "cardano.txs";
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

        for (hash, vote) in proposals.iter() {
            match map(vote) {
                Ok(upd) => update.proposals.push((hash.to_vec(), upd)),
                Err(e) => error!("Cannot convert alonzo protocol param update {vote:?}: {e}"),
            }
        }

        dest.push(update);
    }

    /// Main init function
    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        // Subscribe for tx messages
        // Get configuration
        let subscribe_topic =
            config.get_string("subscribe-topic").unwrap_or(DEFAULT_SUBSCRIBE_TOPIC.to_string());
        info!("Creating subscriber on '{subscribe_topic}'");

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

        let mut subscription = context.subscribe(&subscribe_topic).await?;
        context.clone().run(async move {
            loop {
                let Ok((_, message)) = subscription.read().await else { return; };
                match message.as_ref() {
                    Message::Cardano((block, CardanoMessage::ReceivedTxs(txs_msg))) => {
                        let span = info_span!("utxo_state.handle", block = block.number);

                        async {
                            if tracing::enabled!(tracing::Level::DEBUG) {
                                debug!("Received {} txs for slot {}",
                                    txs_msg.txs.len(), block.slot);
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

                            for (tx_index, raw_tx) in txs_msg.txs.iter().enumerate() {
                                if publish_governance_procedures_topic.is_some() {
                                    //Self::decode_legacy_updates(&mut legacy_update_proposals, &block, &raw_tx);
                                    if block.era >= Era::Shelley && block.era < Era::Babbage {
                                        if let Ok(alonzo) = MultiEraTx::decode_for_era(traverse::Era::Alonzo, &raw_tx) {
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
                                    }
                                    else if block.era >= Era::Babbage && block.era < Era::Conway{
                                        if let Ok(babbage) = MultiEraTx::decode_for_era(traverse::Era::Babbage, &raw_tx) {
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

                                // Parse the tx
                                match MultiEraTx::decode(&raw_tx) {
                                    Ok(tx) => {
                                        let inputs = tx.consumes();
                                        let outputs = tx.produces();
                                        let certs = tx.certs();
                                        let tx_withdrawals = tx.withdrawals_sorted_set();
                                        let mut props = None;
                                        let mut votes = None;

                                        if let Some(conway) = tx.as_conway() {
                                            if let Some(ref v) = conway.transaction_body.voting_procedures {
                                                votes = Some(v);
                                            }

                                            if let Some(ref p) = conway.transaction_body.proposal_procedures {
                                                props = Some(p);
                                            }
                                        }

                                        if tracing::enabled!(tracing::Level::DEBUG) {
                                            debug!("Decoded tx with {} inputs, {} outputs, {} certs",
                                               inputs.len(), outputs.len(), certs.len());
                                        }

                                        if publish_utxo_deltas_topic.is_some() {
                                            // Add all the inputs
                                            for input in inputs {  // MultiEraInput
                                                let oref = input.output_ref();

                                                // Construct message
                                                let tx_input = TxInput {
                                                    tx_hash: **oref.hash(),
                                                    index: oref.index(),
                                                };

                                                utxo_deltas.push(UTXODelta::Input(tx_input));
                                            }

                                            // Add all the outputs
                                            for (index, output) in outputs {  // MultiEraOutput

                                                match output.address() {
                                                    Ok(pallas_address) =>
                                                    {
                                                        match map_parameters::map_address(&pallas_address) {
                                                            Ok(address) => {
                                                                let tx_output = TxOutput {
                                                                    tx_hash: *tx.hash(),
                                                                    index: index as u64,
                                                                    address: address,
                                                                    value: map_parameters::map_value(&output.value()),
                                                                    datum: map_parameters::map_datum(&output.datum())
                                                                };

                                                                utxo_deltas.push(UTXODelta::Output(tx_output));

                                                                // catch all output lovelaces
                                                                total_output += output.value().coin() as u128;
                                                            }

                                                            Err(e) =>
                                                                error!("Output {index} in tx ignored: {e}")
                                                        }
                                                    }

                                                    Err(e) =>
                                                        error!("Can't parse output {index} in tx: {e}")
                                                }
                                            }
                                        }

                                        if publish_asset_deltas_topic.is_some() {
                                            let tx_hash: TxHash = tx.hash().to_vec().try_into().expect("invalid tx hash length");

                                            let mut tx_deltas: Vec<(PolicyId, Vec<NativeAssetDelta>)> = Vec::new();

                                            // Mint deltas
                                            for policy_group in tx.mints().iter() {
                                                if let Some((policy_id, deltas)) = map_parameters::map_mint_burn(policy_group) {
                                                    tx_deltas.push((policy_id, deltas));
                                                }
                                            }

                                            if let Some(metadata) = tx.metadata().find(CIP25_METADATA_LABEL) {
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
                                                asset_deltas.push((tx_hash, tx_deltas));
                                            }
                                        }

                                        if publish_certificates_topic.is_some() {
                                            let tx_hash = tx.hash();
                                            for ( cert_index, cert) in certs.iter().enumerate() {
                                                match map_parameters::map_certificate(&cert, *tx_hash, tx_index, cert_index) {
                                                    Ok(tx_cert) => {
                                                        certificates.push( tx_cert);
                                                    },
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
                                                        });
                                                    }

                                                    Err(e) => error!("Bad stake address: {e:#}"),
                                                }
                                            }
                                        }

                                        if publish_governance_procedures_topic.is_some() {
                                            if let Some(pp) = props {
                                                // Nonempty set -- governance_message.proposal_procedures will not be empty
                                                let mut proc_id = GovActionId { transaction_id: *tx.hash(), action_index: 0 };
                                                for (action_index, pallas_governance_proposals) in pp.iter().enumerate() {
                                                    match proc_id.set_action_index(action_index)
                                                        .and_then (|proc_id| map_parameters::map_governance_proposals_procedures(&proc_id, &pallas_governance_proposals))
                                                    {
                                                        Ok(g) => proposal_procedures.push(g),
                                                        Err(e) => error!("Cannot decode governance proposal procedure {} idx {} in slot {}: {e}", proc_id, action_index, block.slot)
                                                    }
                                                }
                                            }

                                            if let Some(pallas_vp) = votes {
                                                // Nonempty set -- governance_message.voting_procedures will not be empty
                                                match map_parameters::map_all_governance_voting_procedures(pallas_vp) {
                                                    Ok(vp) => voting_procedures.push((*tx.hash(), vp)),
                                                    Err(e) => error!("Cannot decode governance voting procedures in slot {}: {e}", block.slot)
                                                }
                                            }
                                        }

                                        // Capture the fees
                                        if let Some(fee) = tx.fee() {
                                            total_fees += fee;
                                        }
                                    },

                                    Err(e) => error!("Can't decode transaction in slot {}: {e}",
                                                     block.slot)
                                }
                            }

                            // Publish messages in parallel
                            let mut futures = Vec::new();
                            if let Some(ref topic) = publish_utxo_deltas_topic {
                                let msg = Message::Cardano((
                                    block.clone(),
                                    CardanoMessage::UTXODeltas(UTXODeltasMessage {
                                        deltas: utxo_deltas,
                                    })
                                ));

                                futures.push(context.message_bus.publish(&topic, Arc::new(msg)));
                            }

                            if let Some(ref topic) = publish_asset_deltas_topic {
                                let msg = Message::Cardano((
                                    block.clone(),
                                    CardanoMessage::AssetDeltas(AssetDeltasMessage {
                                        deltas: asset_deltas,
                                        cip25_metadata_updates
                                    })
                                ));

                                futures.push(context.message_bus.publish(&topic, Arc::new(msg)));
                            }

                            if let Some(ref topic) = publish_withdrawals_topic {
                                let msg = Message::Cardano((
                                    block.clone(),
                                    CardanoMessage::Withdrawals(WithdrawalsMessage {
                                        withdrawals,
                                    })
                                ));

                                futures.push(context.message_bus.publish(&topic, Arc::new(msg)));
                            }

                            if let Some(ref topic) = publish_certificates_topic {
                                let msg = Message::Cardano((
                                    block.clone(),
                                    CardanoMessage::TxCertificates(TxCertificatesMessage {
                                        certificates,
                                    })
                                ));

                                futures.push(context.message_bus.publish(&topic, Arc::new(msg)));
                            }

                            if let Some(ref topic) = publish_governance_procedures_topic {
                                let governance_msg = Arc::new(Message::Cardano((
                                    block.clone(),
                                    CardanoMessage::GovernanceProcedures(
                                        GovernanceProceduresMessage {
                                            voting_procedures,
                                            proposal_procedures,
                                            alonzo_babbage_updates: alonzo_babbage_update_proposals
                                        })
                                )));

                                futures.push(context.message_bus.publish(&topic,
                                                                         governance_msg.clone()));
                            }

                            if let Some(ref topic) = publish_block_txs_topic {
                                let msg = Message::Cardano((
                                    block.clone(),
                                    CardanoMessage::BlockInfoMessage(BlockTxsMessage {
                                        total_txs,
                                        total_output,
                                        total_fees
                                    })
                                ));

                                futures.push(context.message_bus.publish(&topic, Arc::new(msg)));
                            }

                            join_all(futures)
                                .await
                                .into_iter()
                                .filter_map(Result::err)
                                .for_each(|e| error!("Failed to publish: {e}"));
                        }.instrument(span).await;
                    }

                    _ => error!("Unexpected message type: {message:?}")
                }
            }
        });

        Ok(())
    }
}
