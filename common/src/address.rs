//! Cardano address definitions for Acropolis
// We don't use these types in the acropolis_common crate itself
#![allow(dead_code)]

use crate::cip19::{VarIntDecoder, VarIntEncoder};
use crate::types::{KeyHash, ScriptHash};
use crate::{Credential, NetworkId};
use anyhow::{anyhow, bail, Result};
use serde_with::{hex::Hex, serde_as};
use std::fmt::{Display, Formatter};

/// a Byron-era address
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ByronAddress {
    /// Raw payload
    pub payload: Vec<u8>,
}

/// Address network identifier
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum AddressNetwork {
    /// Mainnet
    Main,

    /// Testnet
    Test,
}

impl From<NetworkId> for AddressNetwork {
    fn from(network: NetworkId) -> Self {
        match network {
            NetworkId::Mainnet => Self::Main,
            NetworkId::Testnet => Self::Test,
        }
    }
}

impl Default for AddressNetwork {
    fn default() -> Self {
        Self::Main
    }
}

/// A Shelley-era address - payment part
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ShelleyAddressPaymentPart {
    /// Payment to a key
    PaymentKeyHash(KeyHash),

    /// Payment to a script
    ScriptHash(ScriptHash),
}

impl Default for ShelleyAddressPaymentPart {
    fn default() -> Self {
        Self::PaymentKeyHash(Vec::new())
    }
}

/// Delegation pointer
#[derive(Debug, Default, Clone, Hash, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ShelleyAddressPointer {
    /// Slot number
    pub slot: u64,

    /// Transaction index within the slot
    pub tx_index: u64,

    /// Certificate index within the transaction
    pub cert_index: u64,
}

/// A Shelley-era address - delegation part
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ShelleyAddressDelegationPart {
    /// No delegation (enterprise addresses)
    None,

    /// Delegation to stake key
    StakeKeyHash(Vec<u8>),

    /// Delegation to script key
    ScriptHash(ScriptHash),

    /// Delegation to pointer
    Pointer(ShelleyAddressPointer),
}

impl Default for ShelleyAddressDelegationPart {
    fn default() -> Self {
        Self::None
    }
}

/// A Shelley-era address
#[derive(Debug, Default, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct ShelleyAddress {
    /// Network id
    pub network: AddressNetwork,

    /// Payment part
    pub payment: ShelleyAddressPaymentPart,

    /// Delegation part
    pub delegation: ShelleyAddressDelegationPart,
}

impl ShelleyAddress {
    /// Read from string format
    pub fn from_string(text: &str) -> Result<Self> {
        let (hrp, data) = bech32::decode(text)?;
        if let Some(header) = data.first() {
            let network = match hrp.as_str().contains("test") {
                true => AddressNetwork::Test,
                false => AddressNetwork::Main,
            };

            let header = *header;

            let payment_part = match (header >> 4) & 0x01 {
                0 => ShelleyAddressPaymentPart::PaymentKeyHash(data[1..29].to_vec()),
                1 => ShelleyAddressPaymentPart::ScriptHash(data[1..29].to_vec()),
                _ => panic!(),
            };

            let delegation_part = match (header >> 5) & 0x03 {
                0 => ShelleyAddressDelegationPart::StakeKeyHash(data[29..57].to_vec()),
                1 => ShelleyAddressDelegationPart::ScriptHash(data[29..57].to_vec()),
                2 => {
                    let mut decoder = VarIntDecoder::new(&data[29..]);
                    let slot = decoder.read()?;
                    let tx_index = decoder.read()?;
                    let cert_index = decoder.read()?;

                    ShelleyAddressDelegationPart::Pointer(ShelleyAddressPointer {
                        slot,
                        tx_index,
                        cert_index,
                    })
                }
                3 => ShelleyAddressDelegationPart::None,
                _ => panic!(),
            };

            return Ok(ShelleyAddress {
                network,
                payment: payment_part,
                delegation: delegation_part,
            });
        }

        Err(anyhow!("Empty address data"))
    }

    /// Convert to addr1xxx form
    pub fn to_string(&self) -> Result<String> {
        let (hrp, network_bits) = match self.network {
            AddressNetwork::Main => (bech32::Hrp::parse("addr")?, 1u8),
            AddressNetwork::Test => (bech32::Hrp::parse("addr_test")?, 0u8),
        };

        let (payment_hash, payment_bits): (&Vec<u8>, u8) = match &self.payment {
            ShelleyAddressPaymentPart::PaymentKeyHash(data) => (data, 0),
            ShelleyAddressPaymentPart::ScriptHash(data) => (data, 1),
        };

        let (delegation_hash, delegation_bits): (&Vec<u8>, u8) = match &self.delegation {
            ShelleyAddressDelegationPart::None => (&Vec::new(), 3),
            ShelleyAddressDelegationPart::StakeKeyHash(hash) => (hash, 0),
            ShelleyAddressDelegationPart::ScriptHash(hash) => (hash, 1),
            ShelleyAddressDelegationPart::Pointer(pointer) => {
                let mut encoder = VarIntEncoder::new();
                encoder.push(pointer.slot);
                encoder.push(pointer.tx_index);
                encoder.push(pointer.cert_index);
                (&encoder.to_vec(), 2)
            }
        };

        let mut data = vec![network_bits | (payment_bits << 4) | (delegation_bits << 5)];
        data.extend(payment_hash);
        data.extend(delegation_hash);
        Ok(bech32::encode::<bech32::Bech32>(hrp, &data)?)
    }
}

/// Payload of a stake address
#[serde_as]
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, Hash)]
pub enum StakeAddressPayload {
    /// Stake key
    StakeKeyHash(#[serde_as(as = "Hex")] KeyHash),

    /// Script hash
    ScriptHash(#[serde_as(as = "Hex")] ScriptHash),
}

impl StakeAddressPayload {
    // Convert to string - note different encoding from when used as part of a StakeAddress
    pub fn to_string(&self) -> Result<String> {
        let (hrp, data) = match &self {
            Self::StakeKeyHash(data) => (bech32::Hrp::parse("stake_vkh")?, data),
            Self::ScriptHash(data) => (bech32::Hrp::parse("script")?, data),
        };

        Ok(bech32::encode::<bech32::Bech32>(hrp, data)?)
    }
}

/// A stake address
#[derive(Debug, Clone, Eq, PartialEq, Hash, serde::Serialize, serde::Deserialize)]
pub struct StakeAddress {
    /// Network id
    pub network: AddressNetwork,

    /// Payload
    pub payload: StakeAddressPayload,
}

impl StakeAddress {
    pub fn new(payload: StakeAddressPayload, network: AddressNetwork) -> Self {
        StakeAddress { network, payload }
    }

    pub fn get_hash(&self) -> &[u8] {
        match &self.payload {
            StakeAddressPayload::StakeKeyHash(hash) => hash,
            StakeAddressPayload::ScriptHash(hash) => hash,
        }
    }

    pub fn get_credential(&self) -> Credential {
        match &self.payload {
            StakeAddressPayload::StakeKeyHash(hash) => Credential::AddrKeyHash(hash.clone()),
            StakeAddressPayload::ScriptHash(hash) => Credential::ScriptHash(hash.clone()),
        }
    }

    /// Convert to string stake1xxx format
    pub fn to_string(&self) -> Result<String> {
        let hrp = match self.network {
            AddressNetwork::Main => bech32::Hrp::parse("stake")?,
            AddressNetwork::Test => bech32::Hrp::parse("stake_test")?,
        };

        let data = self.to_binary();
        Ok(bech32::encode::<bech32::Bech32>(hrp, &data)?)
    }

    /// Read from a string format ("stake1xxx...")
    pub fn from_string(text: &str) -> Result<Self> {
        let (hrp, data) = bech32::decode(text)?;
        if let Some(header) = data.first() {
            let network = match hrp.as_str().contains("test") {
                true => AddressNetwork::Test,
                false => AddressNetwork::Main,
            };

            let payload = match (header >> 4) & 0x0Fu8 {
                0b1110 => StakeAddressPayload::StakeKeyHash(data[1..].to_vec()),
                0b1111 => StakeAddressPayload::ScriptHash(data[1..].to_vec()),
                _ => return Err(anyhow!("Unknown header {header} in stake address")),
            };

            return Ok(StakeAddress { network, payload });
        }

        Err(anyhow!("Empty stake address data"))
    }

    /// Convert to binary format (29 bytes)
    pub fn to_binary(&self) -> Vec<u8> {
        let network_bits = match self.network {
            AddressNetwork::Main => 0b1u8,
            AddressNetwork::Test => 0b0u8,
        };

        let (stake_bits, stake_hash): (u8, &Vec<u8>) = match &self.payload {
            StakeAddressPayload::StakeKeyHash(data) => (0b1110, data),
            StakeAddressPayload::ScriptHash(data) => (0b1111, data),
        };

        let mut data = vec![network_bits | (stake_bits << 4)];
        data.extend(stake_hash);
        data
    }

    /// Read from binary format (29 bytes)
    pub fn from_binary(data: &[u8]) -> Result<Self> {
        if data.len() != 29 {
            bail!("Bad stake address length: {}", data.len());
        }

        let network = match data[0] & 0x01 {
            0b1 => AddressNetwork::Main,
            _ => AddressNetwork::Test,
        };

        let payload = match (data[0] >> 4) & 0x0F {
            0b1110 => StakeAddressPayload::StakeKeyHash(data[1..].to_vec()),
            0b1111 => StakeAddressPayload::ScriptHash(data[1..].to_vec()),
            _ => bail!("Unknown header byte {:x} in stake address", data[0]),
        };

        Ok(StakeAddress { network, payload })
    }
}

impl Display for StakeAddress {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_string().unwrap())
    }
}

impl<C> minicbor::Encode<C> for StakeAddress {
    fn encode<W: minicbor::encode::Write>(
        &self,
        e: &mut minicbor::Encoder<W>,
        _ctx: &mut C,
    ) -> Result<(), minicbor::encode::Error<W::Error>> {
        e.bytes(&self.to_binary())?;
        Ok(())
    }
}

impl<'b, C> minicbor::Decode<'b, C> for StakeAddress {
    fn decode(
        d: &mut minicbor::Decoder<'b>,
        _ctx: &mut C,
    ) -> Result<Self, minicbor::decode::Error> {
        let bytes = d.bytes()?;
        Self::from_binary(bytes)
            .map_err(|e| minicbor::decode::Error::message(format!("invalid stake address: {e}")))
    }
}

impl Default for StakeAddress {
    fn default() -> Self {
        StakeAddress {
            network: AddressNetwork::Main,
            payload: StakeAddressPayload::StakeKeyHash(vec![0u8; 28]),
        }
    }
}

/// A Cardano address
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Address {
    None,
    Byron(ByronAddress),
    Shelley(ShelleyAddress),
    Stake(StakeAddress),
}

impl Default for Address {
    fn default() -> Self {
        Self::None
    }
}

impl Address {
    /// Get the stake pointer if there is one
    pub fn get_pointer(&self) -> Option<ShelleyAddressPointer> {
        if let Address::Shelley(shelley) = self {
            if let ShelleyAddressDelegationPart::Pointer(ptr) = &shelley.delegation {
                return Some(ptr.clone());
            }
        }
        None
    }

    /// Read from string format ("addr1...")
    pub fn from_string(text: &str) -> Result<Self> {
        if text.starts_with("addr1") || text.starts_with("addr_test1") {
            Ok(Self::Shelley(ShelleyAddress::from_string(text)?))
        } else if text.starts_with("stake1") || text.starts_with("stake_test1") {
            Ok(Self::Stake(StakeAddress::from_string(text)?))
        } else {
            if let Ok(bytes) = bs58::decode(text).into_vec() {
                Ok(Self::Byron(ByronAddress { payload: bytes }))
            } else {
                Ok(Self::None)
            }
        }
    }

    /// Convert to standard string representation
    pub fn to_string(&self) -> Result<String> {
        match self {
            Self::None => Err(anyhow!("No address")),
            Self::Byron(byron) => Ok(bs58::encode(&byron.payload).into_string()),
            Self::Shelley(shelley) => shelley.to_string(),
            Self::Stake(stake) => stake.to_string(),
        }
    }
}

// -- Tests --
#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::keyhash_224;
    use minicbor::{Decode, Encode};

    #[test]
    fn byron_address() {
        let payload = vec![42];
        let address = Address::Byron(ByronAddress { payload });
        let text = address.to_string().unwrap();
        assert_eq!(text, "j");

        let unpacked = Address::from_string(&text).unwrap();
        assert_eq!(address, unpacked);
    }

    // Standard keys from CIP-19
    fn test_payment_key_hash() -> Vec<u8> {
        let payment_key = "addr_vk1w0l2sr2zgfm26ztc6nl9xy8ghsk5sh6ldwemlpmp9xylzy4dtf7st80zhd";
        let (_, pubkey) = bech32::decode(payment_key).expect("Invalid Bech32 string");

        // pubkey is the raw key - we need the Blake2B hash
        let hash = keyhash_224(&pubkey);
        assert_eq!(28, hash.len());
        hash
    }

    fn test_stake_key_hash() -> Vec<u8> {
        let stake_key = "stake_vk1px4j0r2fk7ux5p23shz8f3y5y2qam7s954rgf3lg5merqcj6aetsft99wu";
        let (_, pubkey) = bech32::decode(stake_key).expect("Invalid Bech32 string");

        // pubkey is the raw key - we need the Blake2B hash
        let hash = keyhash_224(&pubkey);
        assert_eq!(28, hash.len());
        hash
    }

    fn test_script_hash() -> Vec<u8> {
        let script_hash = "script1cda3khwqv60360rp5m7akt50m6ttapacs8rqhn5w342z7r35m37";
        let (_, hash) = bech32::decode(script_hash).expect("Invalid Bech32 string");
        // This is already a hash
        assert_eq!(28, hash.len());
        hash
    }

    fn test_pointer() -> ShelleyAddressPointer {
        ShelleyAddressPointer {
            slot: 2498243,
            tx_index: 27,
            cert_index: 3,
        }
    }

    // Test vectors from CIP-19
    #[test]
    fn shelley_type_0() {
        let address = Address::Shelley(ShelleyAddress {
            network: AddressNetwork::Main,
            payment: ShelleyAddressPaymentPart::PaymentKeyHash(test_payment_key_hash()),
            delegation: ShelleyAddressDelegationPart::StakeKeyHash(test_stake_key_hash()),
        });

        let text = address.to_string().unwrap();
        assert_eq!(text, "addr1qx2fxv2umyhttkxyxp8x0dlpdt3k6cwng5pxj3jhsydzer3n0d3vllmyqwsx5wktcd8cc3sq835lu7drv2xwl2wywfgse35a3x");

        let unpacked = Address::from_string(&text).unwrap();
        assert_eq!(address, unpacked);
    }

    #[test]
    fn shelley_type_1() {
        let address = Address::Shelley(ShelleyAddress {
            network: AddressNetwork::Main,
            payment: ShelleyAddressPaymentPart::ScriptHash(test_script_hash()),
            delegation: ShelleyAddressDelegationPart::StakeKeyHash(test_stake_key_hash()),
        });

        let text = address.to_string().unwrap();
        assert_eq!(text, "addr1z8phkx6acpnf78fuvxn0mkew3l0fd058hzquvz7w36x4gten0d3vllmyqwsx5wktcd8cc3sq835lu7drv2xwl2wywfgs9yc0hh");

        let unpacked = Address::from_string(&text).unwrap();
        assert_eq!(address, unpacked);
    }

    #[test]
    fn shelley_type_2() {
        let address = Address::Shelley(ShelleyAddress {
            network: AddressNetwork::Main,
            payment: ShelleyAddressPaymentPart::PaymentKeyHash(test_payment_key_hash()),
            delegation: ShelleyAddressDelegationPart::ScriptHash(test_script_hash()),
        });

        let text = address.to_string().unwrap();
        assert_eq!(text, "addr1yx2fxv2umyhttkxyxp8x0dlpdt3k6cwng5pxj3jhsydzerkr0vd4msrxnuwnccdxlhdjar77j6lg0wypcc9uar5d2shs2z78ve");

        let unpacked = Address::from_string(&text).unwrap();
        assert_eq!(address, unpacked);
    }

    #[test]
    fn shelley_type_3() {
        let address = Address::Shelley(ShelleyAddress {
            network: AddressNetwork::Main,
            payment: ShelleyAddressPaymentPart::ScriptHash(test_script_hash()),
            delegation: ShelleyAddressDelegationPart::ScriptHash(test_script_hash()),
        });

        let text = address.to_string().unwrap();
        assert_eq!(text, "addr1x8phkx6acpnf78fuvxn0mkew3l0fd058hzquvz7w36x4gt7r0vd4msrxnuwnccdxlhdjar77j6lg0wypcc9uar5d2shskhj42g");

        let unpacked = Address::from_string(&text).unwrap();
        assert_eq!(address, unpacked);
    }

    #[test]
    fn shelley_type_4() {
        let address = Address::Shelley(ShelleyAddress {
            network: AddressNetwork::Main,
            payment: ShelleyAddressPaymentPart::PaymentKeyHash(test_payment_key_hash()),
            delegation: ShelleyAddressDelegationPart::Pointer(test_pointer()),
        });

        let text = address.to_string().unwrap();
        assert_eq!(
            text,
            "addr1gx2fxv2umyhttkxyxp8x0dlpdt3k6cwng5pxj3jhsydzer5pnz75xxcrzqf96k"
        );

        let unpacked = Address::from_string(&text).unwrap();
        assert_eq!(address, unpacked);
    }

    #[test]
    fn shelley_type_5() {
        let address = Address::Shelley(ShelleyAddress {
            network: AddressNetwork::Main,
            payment: ShelleyAddressPaymentPart::ScriptHash(test_script_hash()),
            delegation: ShelleyAddressDelegationPart::Pointer(test_pointer()),
        });

        let text = address.to_string().unwrap();
        assert_eq!(
            text,
            "addr128phkx6acpnf78fuvxn0mkew3l0fd058hzquvz7w36x4gtupnz75xxcrtw79hu"
        );

        let unpacked = Address::from_string(&text).unwrap();
        assert_eq!(address, unpacked);
    }

    #[test]
    fn shelley_type_6() {
        let address = Address::Shelley(ShelleyAddress {
            network: AddressNetwork::Main,
            payment: ShelleyAddressPaymentPart::PaymentKeyHash(test_payment_key_hash()),
            delegation: ShelleyAddressDelegationPart::None,
        });

        let text = address.to_string().unwrap();
        assert_eq!(
            text,
            "addr1vx2fxv2umyhttkxyxp8x0dlpdt3k6cwng5pxj3jhsydzers66hrl8"
        );

        let unpacked = Address::from_string(&text).unwrap();
        assert_eq!(address, unpacked);
    }

    #[test]
    fn shelley_type_7() {
        let address = Address::Shelley(ShelleyAddress {
            network: AddressNetwork::Main,
            payment: ShelleyAddressPaymentPart::ScriptHash(test_script_hash()),
            delegation: ShelleyAddressDelegationPart::None,
        });

        let text = address.to_string().unwrap();
        assert_eq!(
            text,
            "addr1w8phkx6acpnf78fuvxn0mkew3l0fd058hzquvz7w36x4gtcyjy7wx"
        );

        let unpacked = Address::from_string(&text).unwrap();
        assert_eq!(address, unpacked);
    }

    #[test]
    fn shelley_type_14() {
        let address = Address::Stake(StakeAddress {
            network: AddressNetwork::Main,
            payload: StakeAddressPayload::StakeKeyHash(test_stake_key_hash()),
        });

        let text = address.to_string().unwrap();
        assert_eq!(
            text,
            "stake1uyehkck0lajq8gr28t9uxnuvgcqrc6070x3k9r8048z8y5gh6ffgw"
        );

        let unpacked = Address::from_string(&text).unwrap();
        assert_eq!(address, unpacked);
    }

    #[test]
    fn shelley_type_15() {
        let address = Address::Stake(StakeAddress {
            network: AddressNetwork::Main,
            payload: StakeAddressPayload::ScriptHash(test_script_hash()),
        });

        let text = address.to_string().unwrap();
        assert_eq!(
            text,
            "stake178phkx6acpnf78fuvxn0mkew3l0fd058hzquvz7w36x4gtcccycj5"
        );

        let unpacked = Address::from_string(&text).unwrap();
        assert_eq!(address, unpacked);
    }

    #[test]
    fn stake_address_from_binary_mainnet_stake() {
        // First withdrawal on Mainnet
        let binary =
            hex::decode("e1558f3ee09b26d88fac2eddc772a9eda94cce6dbadbe9fee439bd6001").unwrap();
        let sa = StakeAddress::from_binary(&binary).unwrap();
        assert_eq!(sa.network, AddressNetwork::Main);
        assert_eq!(
            match sa.payload {
                StakeAddressPayload::StakeKeyHash(key) => hex::encode(&key),
                _ => "SCRIPT".to_string(),
            },
            "558f3ee09b26d88fac2eddc772a9eda94cce6dbadbe9fee439bd6001"
        );
    }

    #[test]
    fn stake_address_from_binary_mainnet_script() {
        // Fudged script hash from above
        let binary =
            hex::decode("f1558f3ee09b26d88fac2eddc772a9eda94cce6dbadbe9fee439bd6001").unwrap();
        let sa = StakeAddress::from_binary(&binary).unwrap();
        assert_eq!(sa.network, AddressNetwork::Main);
        assert_eq!(
            match sa.payload {
                StakeAddressPayload::ScriptHash(key) => hex::encode(&key),
                _ => "STAKE".to_string(),
            },
            "558f3ee09b26d88fac2eddc772a9eda94cce6dbadbe9fee439bd6001"
        );
    }

    #[test]
    fn stake_address_from_binary_testnet_stake() {
        // Fudged testnet from above
        let binary =
            hex::decode("e0558f3ee09b26d88fac2eddc772a9eda94cce6dbadbe9fee439bd6001").unwrap();
        let sa = StakeAddress::from_binary(&binary).unwrap();
        assert_eq!(sa.network, AddressNetwork::Test);
        assert_eq!(
            match sa.payload {
                StakeAddressPayload::StakeKeyHash(key) => hex::encode(&key),
                _ => "SCRIPT".to_string(),
            },
            "558f3ee09b26d88fac2eddc772a9eda94cce6dbadbe9fee439bd6001"
        );
    }

    fn mainnet_stake_address() -> StakeAddress {
        let binary =
            hex::decode("e1558f3ee09b26d88fac2eddc772a9eda94cce6dbadbe9fee439bd6001").unwrap();
        StakeAddress::from_binary(&binary).unwrap()
    }

    fn testnet_script_address() -> StakeAddress {
        let binary =
            hex::decode("f0558f3ee09b26d88fac2eddc772a9eda94cce6dbadbe9fee439bd6001").unwrap();
        StakeAddress::from_binary(&binary).unwrap()
    }

    #[test]
    fn stake_addresses_encode_mainnet_stake() {
        let address = mainnet_stake_address();
        let binary = address.to_binary();

        // CBOR encoding wraps the raw 29-byte stake address in a byte string:
        // - 0x58: CBOR major type 2 (byte string) with 1-byte length follows
        // - 0x1d: Length of 29 bytes (the stake address data)
        // - [29 bytes]: The actual stake address (network header + 28-byte hash)
        // Total: 31 bytes (2-byte CBOR framing + 29-byte payload)
        let expected = [[0x58, 0x1d].as_slice(), &binary].concat();

        let mut actual = Vec::new();
        let mut encoder = minicbor::Encoder::new(&mut actual);
        address.encode(&mut encoder, &mut ()).unwrap();

        assert_eq!(actual.len(), 31);
        assert_eq!(actual, expected);
    }

    #[test]
    fn stake_addresses_decode_mainnet_stake() {
        let binary = {
            let mut v = vec![0x58, 0x1d];
            v.extend_from_slice(&mainnet_stake_address().to_binary());
            v
        };

        let mut decoder = minicbor::Decoder::new(&binary);
        let decoded = StakeAddress::decode(&mut decoder, &mut ()).unwrap();

        assert_eq!(decoded.network, AddressNetwork::Main);
        assert_eq!(
            match decoded.payload {
                StakeAddressPayload::StakeKeyHash(key) => hex::encode(&key),
                _ => "STAKE".to_string(),
            },
            "558f3ee09b26d88fac2eddc772a9eda94cce6dbadbe9fee439bd6001"
        );
    }

    #[test]
    fn stake_addresses_round_trip_mainnet_stake() {
        let binary =
            hex::decode("f1558f3ee09b26d88fac2eddc772a9eda94cce6dbadbe9fee439bd6001").unwrap();
        let original = StakeAddress::from_binary(&binary).unwrap();

        let mut encoded = Vec::new();
        let mut encoder = minicbor::Encoder::new(&mut encoded);
        original.encode(&mut encoder, &mut ()).unwrap();

        let mut decoder = minicbor::Decoder::new(&encoded);
        let decoded = StakeAddress::decode(&mut decoder, &mut ()).unwrap();

        assert_eq!(decoded.network, AddressNetwork::Main);
        assert_eq!(
            match decoded.payload {
                StakeAddressPayload::ScriptHash(key) => hex::encode(&key),
                _ => "STAKE".to_string(),
            },
            "558f3ee09b26d88fac2eddc772a9eda94cce6dbadbe9fee439bd6001"
        );
    }

    #[test]
    fn stake_addresses_roundtrip_testnet_script() {
        let original = testnet_script_address();

        let mut encoded = Vec::new();
        let mut encoder = minicbor::Encoder::new(&mut encoded);
        original.encode(&mut encoder, &mut ()).unwrap();

        let mut decoder = minicbor::Decoder::new(&encoded);
        let decoded = StakeAddress::decode(&mut decoder, &mut ()).unwrap();

        assert_eq!(decoded.network, AddressNetwork::Test);
        assert_eq!(
            match decoded.payload {
                StakeAddressPayload::ScriptHash(key) => hex::encode(&key),
                _ => "SCRIPT".to_string(),
            },
            "558f3ee09b26d88fac2eddc772a9eda94cce6dbadbe9fee439bd6001"
        );
    }

    #[test]
    fn stake_addresses_decode_invalid_length() {
        let bad_data = vec![0xe1, 0x00, 0x01, 0x02, 0x03];
        let mut decoder = minicbor::Decoder::new(&bad_data);

        let result = StakeAddress::decode(&mut decoder, &mut ());
        assert!(result.is_err());
    }
}
