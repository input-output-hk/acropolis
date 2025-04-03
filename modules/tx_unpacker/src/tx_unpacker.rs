//! Acropolis transaction unpacker module for Caryatid
//! Unpacks transaction bodies into UTXO events

use caryatid_sdk::{Context, Module, module, MessageBusExt};
use acropolis_common::{
    messages::{Message, TxCertificatesMessage, UTXODeltasMessage}, *
};

use std::sync::Arc;
use anyhow::Result;
use config::Config;
use tracing::{debug, info, error};
use pallas::ledger::traverse::{MultiEraCert, MultiEraTx};
use pallas::ledger::addresses;
use pallas::ledger::primitives::{
    alonzo,
    conway,
    StakeCredential as PallasStakeCredential,
    Relay as PallasRelay,
    Nullable,
};
use anyhow::anyhow;

const DEFAULT_SUBSCRIBE_TOPIC: &str = "cardano.txs";

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

    /// Map a Pallas DRep to our DRepChoice
    fn map_drep(drep: &conway::DRep) -> DRepChoice {
        match drep {
            conway::DRep::Key(key_hash) => DRepChoice::Key(key_hash.to_vec()),
            conway::DRep::Script(script_hash) => DRepChoice::Script(script_hash.to_vec()),
            conway::DRep::Abstain => DRepChoice::Abstain,
            conway::DRep::NoConfidence => DRepChoice::NoConfidence,
        }
    }

    /// Map a Pallas Relay to ours
    fn map_relay(relay: &PallasRelay) -> Relay {
        match relay {
            PallasRelay::SingleHostAddr(port, ipv4, ipv6) =>
                Relay::SingleHostAddr(SingleHostAddr{
                    port: match port {
                        Nullable::Some(port) => Some(*port as u16),
                        _ => None,
                    }, 
                    ipv4: match ipv4 {
                        Nullable::Some(ipv4) => ipv4.try_into().ok(),
                        _ => None,
                    }, 
                    ipv6: match ipv6 {
                        Nullable::Some(ipv6) => ipv6.try_into().ok(),
                        _ => None,
                    }, 
                }),
            PallasRelay::SingleHostName(port, dns_name) =>
                Relay::SingleHostName(SingleHostName { 
                    port: match port {
                        Nullable::Some(port) => Some(*port as u16),
                        _ => None,
                    }, 
                    dns_name: dns_name.clone(),
                }),
            PallasRelay::MultiHostName(dns_name) =>
                Relay::MultiHostName(MultiHostName { 
                    dns_name: dns_name.clone()
                }),
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
                        // TODO relays, pool_metadata
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
                                        .collect(),
                                    relays: relays
                                        .into_iter()
                                        .map(|relay| Self::map_relay(relay))
                                        .collect(),
                                    pool_metadata: match pool_metadata {
                                        Nullable::Some(md) => Some(PoolMetadata {
                                            url: md.url.clone(),
                                            hash: md.hash.to_vec(),
                                        }),
                                        _ => None
                                    }
                                })),
                    alonzo::Certificate::PoolRetirement(pool_key_hash, epoch) =>
                                Ok(TxCertificate::PoolRetirement(PoolRetirement {
                                    operator: pool_key_hash.to_vec(), 
                                    epoch: *epoch
                                })),
                    alonzo::Certificate::GenesisKeyDelegation(
                        genesis_hash, genesis_delegate_hash, vrf_key_hash) =>
                                Ok(TxCertificate::GenesisKeyDelegation(GenesisKeyDelegation{
                                    genesis_hash: genesis_hash.to_vec(),
                                    genesis_delegate_hash: genesis_delegate_hash.to_vec(),
                                    vrf_key_hash: vrf_key_hash.to_vec(),
                        })),
                    alonzo::Certificate::MoveInstantaneousRewardsCert(mir) =>
                                Ok(TxCertificate::MoveInstantaneousReward(MoveInstantaneosReward{
                                    source: match mir.source {
                                        alonzo::InstantaneousRewardSource::Reserves =>
                                            InstantaneousRewardSource::Reserves,
                                        alonzo::InstantaneousRewardSource::Treasury =>
                                            InstantaneousRewardSource::Treasury,
                                    },
                                    target: match &mir.target {
                                        alonzo::InstantaneousRewardTarget::StakeCredentials(creds) =>
                                            InstantaneousRewardTarget::StakeCredentials(
                                                creds.iter()
                                                .map(|(sc, v)| (Self::map_stake_credential(&sc),
                                                                *v as u64)) // TODO can be negative?
                                                .collect()),
                                        alonzo::InstantaneousRewardTarget::OtherAccountingPot(n) =>
                                            InstantaneousRewardTarget::OtherAccountingPot(*n),
                                    }
                                })),
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
                        // TODO relays, pool_metadata
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
                                        .collect(),
                                    relays: relays
                                        .into_iter()
                                        .map(|relay| Self::map_relay(relay))
                                        .collect(),
                                    pool_metadata: match pool_metadata {
                                        Nullable::Some(md) => Some(PoolMetadata {
                                            url: md.url.clone(),
                                            hash: md.hash.to_vec(),
                                        }),
                                        _ => None
                                    }
                                })),
                    conway::Certificate::PoolRetirement(pool_key_hash, epoch) =>
                                Ok(TxCertificate::PoolRetirement(PoolRetirement {
                                    operator: pool_key_hash.to_vec(), 
                                    epoch: *epoch
                                })),

                    conway::Certificate::Reg(cred, coin) =>
                                Ok(TxCertificate::Registration(Registration {
                                    credential: Self::map_stake_credential(cred),
                                    deposit: *coin,
                                })),

                    conway::Certificate::UnReg(cred, coin) =>
                                Ok(TxCertificate::Deregistration(Deregistration {
                                    credential: Self::map_stake_credential(cred),
                                    refund: *coin,
                                })),

                    conway::Certificate::VoteDeleg(cred, drep) =>
                                Ok(TxCertificate::VoteDelegation(VoteDelegation {
                                    credential: Self::map_stake_credential(cred),
                                    drep: Self::map_drep(drep),
                                })),

                    conway::Certificate::StakeVoteDeleg(cred, pool_key_hash, drep) =>
                                Ok(TxCertificate::StakeAndVoteDelegation(StakeAndVoteDelegation {
                                    credential: Self::map_stake_credential(cred),
                                    operator: pool_key_hash.to_vec(),
                                    drep: Self::map_drep(drep),
                                })),

                    conway::Certificate::StakeRegDeleg(cred, pool_key_hash, coin) =>
                                Ok(TxCertificate::StakeRegistrationAndDelegation(
                                    StakeRegistrationAndDelegation {
                                        credential: Self::map_stake_credential(cred),
                                        operator: pool_key_hash.to_vec(),
                                        deposit: *coin,
                                })),

                    conway::Certificate::VoteRegDeleg(cred, drep, coin) =>
                                Ok(TxCertificate::StakeRegistrationAndVoteDelegation(
                                    StakeRegistrationAndVoteDelegation {
                                        credential: Self::map_stake_credential(cred),
                                        drep: Self::map_drep(drep),
                                        deposit: *coin,
                                })),

                    conway::Certificate::StakeVoteRegDeleg(cred, pool_key_hash, drep, coin) =>
                                Ok(TxCertificate::StakeRegistrationAndStakeAndVoteDelegation(
                                    StakeRegistrationAndStakeAndVoteDelegation {
                                        credential: Self::map_stake_credential(cred),
                                        operator: pool_key_hash.to_vec(),
                                        drep: Self::map_drep(drep),
                                        deposit: *coin,
                                })),

                    // TODO AuthCommitteeHot
                    // TODO ResignCommitteeCold

                    conway::Certificate::RegDRepCert(cred, coin, anchor) =>
                                Ok(TxCertificate::DRepRegistration(DRepRegistration {
                                    credential: Self::map_stake_credential(cred),
                                    deposit: *coin,
                                    anchor: match anchor {
                                        Nullable::Some(a) => Some(Anchor {
                                            url: a.url.clone(),
                                            data_hash: a.content_hash.to_vec(),
                                        }),
                                        _ => None
                                    }
                                })),

                    conway::Certificate::UnRegDRepCert(cred, coin) =>
                                Ok(TxCertificate::DRepDeregistration(DRepDeregistration {
                                    credential: Self::map_stake_credential(cred),
                                    refund: *coin,
                                })),

                    conway::Certificate::UpdateDRepCert(cred, anchor) =>
                                Ok(TxCertificate::DRepUpdate(DRepUpdate {
                                    credential: Self::map_stake_credential(cred),
                                    anchor: match anchor {
                                        Nullable::Some(a) => Some(Anchor {
                                            url: a.url.clone(),
                                            data_hash: a.content_hash.to_vec(),
                                        }),
                                        _ => None
                                    }
                                })),

                    _ => Err(anyhow!("Unhandled Conway certificate type {:?}", cert))
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

        let publish_utxo_deltas_topic = config.get_string("publish-utxo-deltas-topic").ok();
        if let Some(ref topic) = publish_utxo_deltas_topic {
            info!("Publishing UTXO deltas on '{topic}'");
        }

        let publish_certificates_topic = config.get_string("publish-certificates-topic").ok();
        if let Some(ref topic) = publish_certificates_topic {
            info!("Publishing certificates on '{topic}'");
        }

        context.clone().message_bus.subscribe(&subscribe_topic, move |message: Arc<Message>| {

            let context = context.clone();
            let publish_utxo_deltas_topic = publish_utxo_deltas_topic.clone();
            let publish_certificates_topic = publish_certificates_topic.clone();

            async move {
                match message.as_ref() {
                    Message::ReceivedTxs(txs_msg) => {
                        if tracing::enabled!(tracing::Level::DEBUG) {
                            debug!("Received {} txs for slot {}",
                                txs_msg.txs.len(), txs_msg.block.slot);
                        }

                        // Construct messages which we batch up
                        let mut utxo_deltas_message = UTXODeltasMessage {
                            sequence: txs_msg.sequence,
                            block: txs_msg.block.clone(),
                            deltas: Vec::new(),
                        };

                        let mut certificates_message = TxCertificatesMessage {
                            sequence: txs_msg.sequence,
                            block: txs_msg.block.clone(),
                            certificates: Vec::new(),
                        };

                        for raw_tx in &txs_msg.txs {
                            // Parse the tx
                            match MultiEraTx::decode(&raw_tx) {
                                Ok(tx) => {
                                    let inputs = tx.consumes();
                                    let outputs = tx.produces();
                                    let certs = tx.certs();

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
                                                tx_hash: oref.hash().to_vec(),
                                                index: oref.index(),
                                            };

                                            utxo_deltas_message.deltas
                                                .push(UTXODelta::Input(tx_input));
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

                                                            utxo_deltas_message.deltas
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
                                    }

                                    if publish_certificates_topic.is_some() {
                                        for cert in certs {
                                            match Self::map_certificate(&cert) {
                                                Ok(tx_cert) => {
                                                    certificates_message.certificates.push(tx_cert);
                                                },
                                                Err(_e) => { 
                                                    // TODO error unexpected
                                                    //error!("{e}"); 
                                                }
                                            }
                                        }
                                    }
                                },

                                Err(e) => error!("Can't decode transaction in slot {}: {e}",
                                                 txs_msg.block.slot)
                            }
                        }

                        if let Some(topic) = publish_utxo_deltas_topic {
                            let utxo_deltas_message = Message::UTXODeltas(utxo_deltas_message);
                            context.message_bus.publish(&topic, Arc::new(utxo_deltas_message))
                                .await
                                .unwrap_or_else(|e| error!("Failed to publish: {e}"));
                        }

                        if let Some(topic) = publish_certificates_topic {
                            let certificates_message = Message::TxCertificates(certificates_message);
                            context.message_bus.publish(&topic, Arc::new(certificates_message))
                                .await
                                .unwrap_or_else(|e| error!("Failed to publish: {e}"));
                        }
                    }

                    _ => error!("Unexpected message type: {message:?}")
                }
            }
        })?;

        Ok(())
    }
}
