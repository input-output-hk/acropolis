use acropolis_common::{
    Address, ByronAddress, NetworkId, ShelleyAddress, ShelleyAddressDelegationPart,
    ShelleyAddressPaymentPart, ShelleyAddressPointer, StakeAddress, StakeCredential,
};
use anyhow::{Result, anyhow};
use pallas::ledger::{
    addresses as pallas_addresses, primitives::StakeCredential as PallasStakeCredential,
};

use crate::utils::to_hash;

/// Map Pallas Network to our NetworkId
pub fn map_network(network: pallas_addresses::Network) -> Result<NetworkId> {
    match network {
        pallas_addresses::Network::Mainnet => Ok(NetworkId::Mainnet),
        pallas_addresses::Network::Testnet => Ok(NetworkId::Testnet),
        _ => Err(anyhow!("Unknown network in address")),
    }
}

/// Derive our Address from a Pallas address
// This is essentially a 1:1 mapping but makes the Message definitions independent
// of Pallas
pub fn map_address(address: &pallas_addresses::Address) -> Result<Address> {
    match address {
        pallas_addresses::Address::Byron(byron_address) => Ok(Address::Byron(ByronAddress {
            payload: byron_address.payload.to_vec(),
        })),

        pallas_addresses::Address::Shelley(shelley_address) => {
            Ok(Address::Shelley(ShelleyAddress {
                network: map_network(shelley_address.network())?,

                payment: match shelley_address.payment() {
                    pallas_addresses::ShelleyPaymentPart::Key(hash) => {
                        ShelleyAddressPaymentPart::PaymentKeyHash(to_hash(hash))
                    }
                    pallas_addresses::ShelleyPaymentPart::Script(hash) => {
                        ShelleyAddressPaymentPart::ScriptHash(to_hash(hash))
                    }
                },

                delegation: match shelley_address.delegation() {
                    pallas_addresses::ShelleyDelegationPart::Null => {
                        ShelleyAddressDelegationPart::None
                    }
                    pallas_addresses::ShelleyDelegationPart::Key(hash) => {
                        ShelleyAddressDelegationPart::StakeKeyHash(to_hash(hash))
                    }
                    pallas_addresses::ShelleyDelegationPart::Script(hash) => {
                        ShelleyAddressDelegationPart::ScriptHash(to_hash(hash))
                    }
                    pallas_addresses::ShelleyDelegationPart::Pointer(pointer) => {
                        ShelleyAddressDelegationPart::Pointer(ShelleyAddressPointer {
                            slot: pointer.slot(),
                            tx_index: pointer.tx_idx(),
                            cert_index: pointer.cert_idx(),
                        })
                    }
                },
            }))
        }

        pallas_addresses::Address::Stake(stake_address) => Ok(Address::Stake(StakeAddress {
            network: map_network(stake_address.network())?,
            credential: match stake_address.payload() {
                pallas_addresses::StakePayload::Stake(hash) => {
                    StakeCredential::AddrKeyHash(to_hash(hash))
                }
                pallas_addresses::StakePayload::Script(hash) => {
                    StakeCredential::ScriptHash(to_hash(hash))
                }
            },
        })),
    }
}

/// Map a Pallas StakeCredential to ours
pub fn map_stake_credential(cred: &PallasStakeCredential) -> StakeCredential {
    match cred {
        PallasStakeCredential::AddrKeyhash(key_hash) => {
            StakeCredential::AddrKeyHash(to_hash(key_hash))
        }
        PallasStakeCredential::ScriptHash(script_hash) => {
            StakeCredential::ScriptHash(to_hash(script_hash))
        }
    }
}

/// Map a PallasStakeCredential to our StakeAddress
pub fn map_stake_address(cred: &PallasStakeCredential, network_id: NetworkId) -> StakeAddress {
    let payload = match cred {
        PallasStakeCredential::AddrKeyhash(key_hash) => {
            StakeCredential::AddrKeyHash(to_hash(key_hash))
        }
        PallasStakeCredential::ScriptHash(script_hash) => {
            StakeCredential::ScriptHash(to_hash(script_hash))
        }
    };

    StakeAddress::new(payload, network_id)
}
