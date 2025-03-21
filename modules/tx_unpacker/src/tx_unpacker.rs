//! Acropolis transaction unpacker module for Caryatid
//! Unpacks transaction bodies into UTXO events

use caryatid_sdk::{Context, Module, module, MessageBusExt};
use acropolis_common::{
    Address, AddressNetwork, ByronAddress, ShelleyAddress, 
    ShelleyAddressDelegationPart, ShelleyAddressPaymentPart, ShelleyAddressPointer, 
    StakeAddress, StakeAddressPayload, 
    TxInput, TxOutput, UTXODelta,
    TxCertificate, StakeCredential, StakeDelegation, Ratio,
    PoolRegistration, PoolRetirement, 
    messages::{Message, UTXODeltasMessage},
};
use std::sync::Arc;
use anyhow::Result;
use config::Config;
use tracing::{debug, info, error};
use pallas::ledger::traverse::{MultiEraTx, MultiEraCert};
use pallas::ledger::addresses;
use pallas::ledger::primitives::{
    alonzo,
    conway,
    StakeCredential as PallasStakeCredential,
};
use anyhow::anyhow;

const DEFAULT_SUBSCRIBE_TOPIC: &str = "cardano.txs";
const DEFAULT_PUBLISH_UTXO_DELTAS_TOPIC: &str = "cardano.utxo.deltas";

/// Tx unpacker module
/// Parameterised by the outer message enum used on the bus
#[module(
    message_type(Message),
    name = "tx-unpacker",
    description = "Transaction to UTXO event unpacker"
)]
pub struct TxUnpacker;

impl TxUnpacker
{
    /// Map Pallas Network to our AddressNetwork
    fn map_network(network: addresses::Network) -> Result<AddressNetwork> {
        match network {
            addresses::Network::Mainnet => Ok(AddressNetwork::Main),
            addresses::Network::Testnet => Ok(AddressNetwork::Test),
            _ => return Err(anyhow!("Unknown network in address"))
        }
    }

    /// Derive our Address from a Pallas address
    // This is essentially a 1:1 mapping but makes the Message definitions independent
    // of Pallas
    fn map_address(address: &addresses::Address) -> Result<Address> {
        match address {
            addresses::Address::Byron(byron_address) => Ok(Address::Byron(ByronAddress {
                payload: byron_address.payload.to_vec(),
            })),

            addresses::Address::Shelley(shelley_address) => Ok(Address::Shelley(ShelleyAddress {
                network: Self::map_network(shelley_address.network())?, 

                payment: match shelley_address.payment() {
                    addresses::ShelleyPaymentPart::Key(hash) => 
                        ShelleyAddressPaymentPart::PaymentKeyHash(hash.to_vec()),
                    addresses::ShelleyPaymentPart::Script(hash) => 
                        ShelleyAddressPaymentPart::ScriptHash(hash.to_vec()),

                },

                delegation: match shelley_address.delegation() {
                    addresses::ShelleyDelegationPart::Null =>
                        ShelleyAddressDelegationPart::None,
                    addresses::ShelleyDelegationPart::Key(hash) =>
                        ShelleyAddressDelegationPart::StakeKeyHash(hash.to_vec()),
                    addresses::ShelleyDelegationPart::Script(hash) =>
                        ShelleyAddressDelegationPart::ScriptHash(hash.to_vec()),
                    addresses::ShelleyDelegationPart::Pointer(pointer) =>
                        ShelleyAddressDelegationPart::Pointer(ShelleyAddressPointer {
                            slot: pointer.slot(),
                            tx_index: pointer.tx_idx(),
                            cert_index: pointer.cert_idx()
                        })
                }
            })),

            addresses::Address::Stake(stake_address) => Ok(Address::Stake(StakeAddress {
                network: Self::map_network(stake_address.network())?,
                payload: match stake_address.payload() {
                    addresses::StakePayload::Stake(hash) => 
                        StakeAddressPayload::StakeKeyHash(hash.to_vec()),
                    addresses::StakePayload::Script(hash) => 
                        StakeAddressPayload::ScriptHash(hash.to_vec()),
                }
            })),

        }
    }

    /// Map a Pallas StakeCredential to ours
    fn map_stake_credential(cred: &PallasStakeCredential) -> StakeCredential {
        match cred {
            PallasStakeCredential::AddrKeyhash(key_hash) =>
                StakeCredential::AddrKeyHash(key_hash.to_vec()),
            PallasStakeCredential::ScriptHash(script_hash) =>
                StakeCredential::ScriptHash(script_hash.to_vec()),
        }
    }

    /// Derive our TxCertificate from a Pallas Certificate
    fn map_certificate(cert: &MultiEraCert) -> Result<TxCertificate> {
        match cert {
            MultiEraCert::NotApplicable => Err(anyhow!("Not applicable cert!")),

            MultiEraCert::AlonzoCompatible(cert) => {
                match cert.as_ref().as_ref() {
                    alonzo::Certificate::StakeRegistration(cred) =>
                        Ok(TxCertificate::StakeRegistration(
                            Self::map_stake_credential(cred))),
                    alonzo::Certificate::StakeDeregistration(cred) =>
                            Ok(TxCertificate::StakeDeregistration(
                                Self::map_stake_credential(cred))),
                    alonzo::Certificate::StakeDelegation(cred, pool_key_hash) =>
                                Ok(TxCertificate::StakeDelegation(StakeDelegation {
                                    credential: Self::map_stake_credential(cred),
                                    operator: pool_key_hash.to_vec()
                                })),
                    alonzo::Certificate::PoolRegistration { 
                        operator, vrf_keyhash, pledge, cost, margin, 
                        reward_account, pool_owners, relays, pool_metadata } =>
                                Ok(TxCertificate::PoolRegistration(PoolRegistration { 
                                    operator: operator.to_vec(), 
                                    vrf_key_hash: vrf_keyhash.to_vec(),
                                    pledge: *pledge,
                                    cost: *cost,
                                    margin: Ratio {
                                        numerator: margin.numerator,
                                        denominator: margin.denominator,
                                    },
                                    reward_account: reward_account.to_vec(),
                                    pool_owners: pool_owners
                                        .into_iter()
                                        .map(|v| v.to_vec())
                                        .collect()
                                })),
                    alonzo::Certificate::PoolRetirement(pool_key_hash, epoch) =>
                                Ok(TxCertificate::PoolRetirement(PoolRetirement {
                                    operator: pool_key_hash.to_vec(), 
                                    epoch: *epoch
                                })),
                    _ => Err(anyhow!("Alonzo certificate type {:?} ignored", cert)),
                }
            }

            // Now repeated for a different type!
            MultiEraCert::Conway(cert) => {
                match cert.as_ref().as_ref() {
                    conway::Certificate::StakeRegistration(cred) =>
                        Ok(TxCertificate::StakeRegistration(
                            Self::map_stake_credential(cred))),
                    conway::Certificate::StakeDeregistration(cred) =>
                            Ok(TxCertificate::StakeDeregistration(
                                Self::map_stake_credential(cred))),
                    conway::Certificate::StakeDelegation(cred, pool_key_hash) =>
                                Ok(TxCertificate::StakeDelegation(StakeDelegation {
                                    credential: Self::map_stake_credential(cred),
                                    operator: pool_key_hash.to_vec()
                                })),
                    conway::Certificate::PoolRegistration { 
                        operator, vrf_keyhash, pledge, cost, margin, 
                        reward_account, pool_owners, relays, pool_metadata } =>
                                Ok(TxCertificate::PoolRegistration(PoolRegistration { 
                                    operator: operator.to_vec(), 
                                    vrf_key_hash: vrf_keyhash.to_vec(),
                                    pledge: *pledge,
                                    cost: *cost,
                                    margin: Ratio {
                                        numerator: margin.numerator,
                                        denominator: margin.denominator,
                                    },
                                    reward_account: reward_account.to_vec(),
                                    pool_owners: pool_owners
                                        .into_iter()
                                        .map(|v| v.to_vec())
                                        .collect()
                                })),
                    conway::Certificate::PoolRetirement(pool_key_hash, epoch) =>
                                Ok(TxCertificate::PoolRetirement(PoolRetirement {
                                    operator: pool_key_hash.to_vec(), 
                                    epoch: *epoch
                                })),
                    _ => Err(anyhow!("Conway certificate type {:?} ignored", cert)),
                }
            }

            _ => Err(anyhow!("Unknown certificate era {:?} ignored", cert)),
        }

    }

    /// Main init function
    pub fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {

        // Subscribe for tx messages
        // Get configuration
        let subscribe_topic = config.get_string("subscribe-topic")
            .unwrap_or(DEFAULT_SUBSCRIBE_TOPIC.to_string());
        info!("Creating subscriber on '{subscribe_topic}'");

        let publish_utxo_deltas_topic = config.get_string("publish-utxo-deltas-topic")
            .unwrap_or(DEFAULT_PUBLISH_UTXO_DELTAS_TOPIC.to_string());
        info!("Publishing UTXO deltas on '{publish_utxo_deltas_topic}'");

        context.clone().message_bus.subscribe(&subscribe_topic, move |message: Arc<Message>| {

            let context = context.clone();
            let publish_utxo_deltas_topic = publish_utxo_deltas_topic.clone();

            async move {
                match message.as_ref() {
                    Message::ReceivedTxs(txs_msg) => {
                        if tracing::enabled!(tracing::Level::DEBUG) {
                            debug!("Received {} txs for slot {}",
                                txs_msg.txs.len(), txs_msg.block.slot);
                        }

                        // Construct message
                        let mut message = UTXODeltasMessage {
                            block: txs_msg.block.clone(),
                            deltas: Vec::new(),
                        };

                        for raw_tx in &txs_msg.txs {
                            // Parse the tx
                            match MultiEraTx::decode(&raw_tx) {
                                Ok(tx) => {
                                    let inputs = tx.consumes();
                                    let outputs = tx.produces();
                                    if tracing::enabled!(tracing::Level::DEBUG) {
                                        debug!("Decoded transaction with {} inputs, {} outputs",
                                           inputs.len(), outputs.len());
                                    }

                                    // Add all the inputs
                                    for input in inputs {  // MultiEraInput

                                        let oref = input.output_ref();

                                        // Construct message
                                        let tx_input = TxInput {
                                            tx_hash: oref.hash().to_vec(),
                                            index: oref.index(),
                                        };

                                        message.deltas.push(UTXODelta::Input(tx_input));
                                    }

                                    // Add all the outputs
                                    for (index, output) in outputs {  // MultiEraOutput

                                        match output.address() {
                                            Ok(pallas_address) =>
                                            {
                                                match Self::map_address(&pallas_address) {
                                                    Ok(address) => {
                                                        let tx_output = TxOutput {
                                                            tx_hash: tx.hash().to_vec(),
                                                            index: index as u64,
                                                            address: address,
                                                            value: output.value().coin(),
                                                            // !!! datum
                                                        };

                                                        message.deltas
                                                            .push(UTXODelta::Output(tx_output));
                                                    }

                                                    Err(e) => 
                                                        error!("Output {index} in tx ignored: {e}")
                                                }
                                            }

                                            Err(e) => 
                                                error!("Can't parse output {index} in tx: {e}")
                                        }
                                    }
                                },

                                Err(e) => error!("Can't decode transaction in slot {}: {e}",
                                                 txs_msg.block.slot)
                            }
                        }

                        let message_enum = Message::UTXODeltas(message);
                        context.message_bus.publish(&publish_utxo_deltas_topic,
                                                    Arc::new(message_enum))
                            .await
                            .unwrap_or_else(|e| error!("Failed to publish: {e}"));
                    }

                    _ => error!("Unexpected message type: {message:?}")
                }
            }
        })?;

        Ok(())
    }
}
