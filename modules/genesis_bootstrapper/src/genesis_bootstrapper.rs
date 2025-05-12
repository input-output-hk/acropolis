//! Acropolis genesis bootstrapper module for Caryatid
//! Reads genesis files and outputs initial UTXO events

use std::collections::HashMap;
use caryatid_sdk::{module, Context, MessageBusExt, Module};
use acropolis_common::{
    messages::{
        GenesisCompleteMessage, Message, Sequence, UTXODeltasMessage
    },
    Address, Anchor, BlockInfo, BlockStatus, ByronAddress,
    Committee, Constitution, ConwayGenesisParams, Credential,
    DRepVotingThresholds, PoolVotingThresholds,
    TxOutput, UTXODelta
};
use hex::decode;
use fraction::Fraction;
use std::sync::Arc;
use anyhow::{anyhow, Result};
use config::Config;
use tracing::{error, info};
use pallas::ledger::configs::{byron::genesis_utxos, *};
use acropolis_common::rational_number::RationalNumber;

const DEFAULT_STARTUP_TOPIC: &str = "cardano.sequence.start";
const DEFAULT_PUBLISH_UTXO_DELTAS_TOPIC: &str = "cardano.utxo.deltas";
const DEFAULT_COMPLETION_TOPIC: &str = "cardano.sequence.bootstrapped";

// Include genesis data (downloaded by build.rs)
const MAINNET_BYRON_GENESIS: &[u8] = include_bytes!("../downloads/mainnet-byron-genesis.json");
const MAINNET_CONWAY_GENESIS: &[u8] = include_bytes!("../downloads/mainnet-conway-genesis.json");

/// Genesis bootstrapper module
#[module(
    message_type(Message),
    name = "genesis-bootstrapper",
    description = "Genesis bootstrap UTXO event generator"
)]
pub struct GenesisBootstrapper;

fn decode_hex_string(s: &str, len: usize) -> Result<Vec<u8>> {
    let key_hash = decode(s.to_owned().into_bytes())?;
    if key_hash.len() == len {
        Ok(key_hash)
    }
    else {
        Err(anyhow!("Incorrect hex length: {} instead of {}", key_hash.len(), len))
    }
}

fn map_anchor(anchor: &conway::Anchor) -> Result<Anchor> {
    Ok(Anchor {
        url: anchor.url.clone(),
        data_hash: decode_hex_string(&anchor.data_hash, 32)?,
    })
}

pub fn map_fraction(fraction: &conway::Fraction) -> RationalNumber {
    RationalNumber {
        numerator: fraction.numerator,
        denominator: fraction.denominator,
    }
}

pub fn map_f32_to_rational(value: f32) -> Result<RationalNumber> {
    if value.is_sign_negative() {
        return Err(anyhow!("Value {} must be greater than 0", value));
    }
    let fract = Fraction::from(value);
    Ok(RationalNumber {
        numerator: *fract.numer().ok_or_else(|| anyhow!("Cannot get numerator for {}", value))?,
        denominator: *fract.denom().ok_or_else(|| anyhow!("Cannot get denominator for {}", value))?
    })
}

fn map_pool_thresholds(thresholds: &conway::PoolVotingThresholds) -> Result<PoolVotingThresholds> {
    Ok(PoolVotingThresholds {
        motion_no_confidence: map_f32_to_rational(thresholds.motion_no_confidence)?,
        committee_normal: map_f32_to_rational(thresholds.committee_normal)?,
        committee_no_confidence: map_f32_to_rational(thresholds.committee_no_confidence)?,
        hard_fork_initiation: map_f32_to_rational(thresholds.hard_fork_initiation)?,
        security_voting_threshold: map_f32_to_rational(thresholds.pp_security_group)?,
    })
}

fn map_drep_thresholds(thresholds: &conway::DRepVotingThresholds) -> Result<DRepVotingThresholds> {
    Ok(DRepVotingThresholds {
        motion_no_confidence: map_f32_to_rational(thresholds.motion_no_confidence)?,
        committee_normal: map_f32_to_rational(thresholds.committee_normal)?,
        committee_no_confidence: map_f32_to_rational(thresholds.committee_normal)?,
        update_constitution: map_f32_to_rational(thresholds.update_to_constitution)?,
        hard_fork_initiation: map_f32_to_rational(thresholds.hard_fork_initiation)?,
        pp_network_group: map_f32_to_rational(thresholds.pp_network_group)?,
        pp_economic_group: map_f32_to_rational(thresholds.pp_economic_group)?,
        pp_technical_group: map_f32_to_rational(thresholds.pp_technical_group)?,
        pp_governance_group: map_f32_to_rational(thresholds.pp_gov_group)?,
        treasury_withdrawal: map_f32_to_rational(thresholds.treasury_withdrawal)?,
    })
}

pub fn map_constitution(constitution: &conway::Constitution) -> Result<Constitution> {
    Ok(Constitution {
        anchor: map_anchor(&constitution.anchor)?,
        guardrail_script: Some(decode_hex_string(&constitution.script, 28)?)
    })
}

pub fn map_committee(committee: &conway::Committee) -> Result<Committee> {
    let mut members = HashMap::new();

    for (member, expiry_epoch) in committee.members.iter() {
        members.insert(Credential::from_json_string(member)?, *expiry_epoch);
    };

    Ok(Committee {
        members,
        threshold: map_fraction(&committee.threshold),
    })
}

fn map_conway_genesis (genesis: &conway::GenesisFile) -> Result<ConwayGenesisParams> {
    Ok(ConwayGenesisParams {
        pool_voting_thresholds: map_pool_thresholds(&genesis.pool_voting_thresholds)?,
        d_rep_voting_thresholds: map_drep_thresholds(&genesis.d_rep_voting_thresholds)?,
        committee_min_size: genesis.committee_min_size,
        committee_max_term_length: genesis.committee_max_term_length,
        gov_action_lifetime: genesis.gov_action_lifetime,
        gov_action_deposit: genesis.gov_action_deposit,
        d_rep_deposit: genesis.d_rep_deposit,
        d_rep_activity: genesis.d_rep_activity,
        min_fee_ref_script_cost_per_byte: RationalNumber::from(genesis.min_fee_ref_script_cost_per_byte),
        plutus_v3_cost_model: genesis.plutus_v3_cost_model.clone(),
        constitution: map_constitution(&genesis.constitution)?,
        committee: map_committee(&genesis.committee)?,
    })
}

impl GenesisBootstrapper
{
    /// Main init function
    pub fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {

        let startup_topic = config.get_string("startup-topic")
            .unwrap_or(DEFAULT_STARTUP_TOPIC.to_string());
        info!("Creating startup subscriber on '{startup_topic}'");

        context.clone().message_bus.subscribe(&startup_topic, 
            move |_message: Arc<Message>| {
                let context = context.clone();
                let config = config.clone();
                info!("Received startup message");

                tokio::spawn(async move {
                    let publish_utxo_deltas_topic = config.get_string("publish-utxo-deltas-topic")
                        .unwrap_or(DEFAULT_PUBLISH_UTXO_DELTAS_TOPIC.to_string());
                    info!("Publishing UTXO deltas on '{publish_utxo_deltas_topic}'");

                    let completion_topic = config.get_string("completion-topic")
                        .unwrap_or(DEFAULT_COMPLETION_TOPIC.to_string());
                    info!("Completing with '{completion_topic}'");

                    // Read genesis data
                    let genesis: byron::GenesisFile = serde_json::from_slice(MAINNET_BYRON_GENESIS)
                        .expect("Invalid JSON in MAINNET_BYRON_GENESIS file");

                    let conway_genesis: Option<conway::GenesisFile> = match serde_json::from_slice(MAINNET_CONWAY_GENESIS) {
                        Ok(file) => Some(file),
                        Err(e) => {
                            info!("Cannot read JSON in MAINNET_CONWAY_GENESIS file: {e}");
                            None
                        }
                    };

                    // Construct message
                    let mut message = UTXODeltasMessage {
                        sequence: Sequence {
                            number: 0,      // We are always the first
                            previous: None,
                        },
                        block: BlockInfo {
                            status: BlockStatus::Bootstrap,
                            slot: 0,
                            number: 0,
                            hash: Vec::new(),
                            epoch: 0,
                            new_epoch: false
                        },
                        deltas: Vec::new(),
                    };

                    // Convert the AVVM distributions into pseudo-UTXOs
                    let gen_utxos = genesis_utxos(&genesis);
                    for (hash, address, amount) in gen_utxos {
                        let tx_output = TxOutput {
                            tx_hash: hash.to_vec(),
                            index: 0,
                            address: Address::Byron(ByronAddress{
                                payload: address.payload.to_vec(),
                            }),
                            value: amount
                        };

                        message.deltas.push(UTXODelta::Output(tx_output));
                    }

                    let message_enum = Message::UTXODeltas(message);
                    context.message_bus.publish(&publish_utxo_deltas_topic,
                                                Arc::new(message_enum))
                        .await
                        .unwrap_or_else(|e| error!("Failed to publish: {e}"));

                    // Send completion message
                    let completion_message = GenesisCompleteMessage {
                        final_sequence: Some(0),
                        conway_genesis: conway_genesis
                            .map(|g| map_conway_genesis(&g))
                            .transpose().unwrap_or_else(|e| {
                                error!("Failure to parse conway genesis block: {e}");
                                None
                            }),
                    };

                    let message_enum = Message::GenesisComplete(completion_message);
                    context.message_bus.publish(&completion_topic, Arc::new(message_enum))
                        .await
                        .unwrap_or_else(|e| error!("Failed to publish: {e}"));
                });

                async {}
            }
        )?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use acropolis_common::rational_number::RationalNumber;
    use crate::map_f32_to_rational;

    #[test]
    fn test_fractions() -> Result<(), anyhow::Error> {
        assert_eq!(map_f32_to_rational(0.51)?, RationalNumber { numerator: 51, denominator: 100 });
        assert_eq!(map_f32_to_rational(0.67)?, RationalNumber { numerator: 67, denominator: 100 });
        assert_eq!(map_f32_to_rational(0.6)?, RationalNumber { numerator: 3, denominator: 5 });
        assert_eq!(map_f32_to_rational(0.75)?, RationalNumber { numerator: 3, denominator: 4 });
        assert_eq!(map_f32_to_rational(0.5)?, RationalNumber { numerator: 1, denominator: 2 });
        Ok(())
    }
}