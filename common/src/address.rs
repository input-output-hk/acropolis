//! Cardano address definitions for Acropolis
// We don't use these types in the acropolis_common crate itself
#![allow(dead_code)]
use crate::cip19::{VarIntDecoder, VarIntEncoder};
use crate::types::{KeyHash, ScriptHash};
use anyhow::{anyhow, bail, Result};
use crc::{Crc, CRC_32_ISO_HDLC};
use minicbor::data::IanaTag;
use serde_with::{hex::Hex, serde_as};

/// a Byron-era address
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct ByronAddress {
    /// Raw payload
    pub payload: Vec<u8>,
}

impl ByronAddress {
    fn compute_crc32(&self) -> u32 {
        const CRC32: Crc<u32> = Crc::<u32>::new(&CRC_32_ISO_HDLC);
        CRC32.checksum(&self.payload)
    }

    pub fn to_string(&self) -> Result<String> {
        let crc = self.compute_crc32();

        let mut buf = Vec::new();
        {
            let mut enc = minicbor::Encoder::new(&mut buf);
            enc.array(2)?;
            enc.tag(IanaTag::Cbor)?;
            enc.bytes(&self.payload)?;
            enc.u32(crc)?;
        }

        Ok(bs58::encode(buf).into_string())
    }

    pub fn from_string(s: &str) -> Result<Self> {
        let bytes = bs58::decode(s).into_vec()?;
        let mut dec = minicbor::Decoder::new(&bytes);

        let len = dec.array()?.unwrap_or(0);
        if len != 2 {
            anyhow::bail!("Invalid Byron address CBOR array length");
        }

        let tag = dec.tag()?;
        if tag != IanaTag::Cbor.into() {
            anyhow::bail!("Invalid Byron address CBOR tag, expected 24");
        }

        let payload = dec.bytes()?.to_vec();
        let crc = dec.u32()?;

        let address = ByronAddress { payload };
        let computed = address.compute_crc32();

        if crc != computed {
            anyhow::bail!("Byron address CRC mismatch");
        }

        Ok(address)
    }

    pub fn to_bytes_key(&self) -> Result<Vec<u8>> {
        let crc = self.compute_crc32();

        let mut buf = Vec::new();
        {
            let mut enc = minicbor::Encoder::new(&mut buf);
            enc.array(2)?;
            enc.tag(minicbor::data::IanaTag::Cbor)?;
            enc.bytes(&self.payload)?;
            enc.u32(crc)?;
        }

        Ok(buf)
    }
}

/// Address network identifier
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum AddressNetwork {
    /// Mainnet
    Main,

    /// Testnet
    Test,
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

    pub fn to_bytes_key(&self) -> Result<Vec<u8>> {
        let network_bits = match self.network {
            AddressNetwork::Main => 1u8,
            AddressNetwork::Test => 0u8,
        };

        let (payment_hash, payment_bits): (&Vec<u8>, u8) = match &self.payment {
            ShelleyAddressPaymentPart::PaymentKeyHash(data) => (data, 0),
            ShelleyAddressPaymentPart::ScriptHash(data) => (data, 1),
        };

        let mut data = Vec::new();

        match &self.delegation {
            ShelleyAddressDelegationPart::None => {
                let header = network_bits | (payment_bits << 4) | (3 << 5);
                data.push(header);
                data.extend(payment_hash);
            }
            ShelleyAddressDelegationPart::StakeKeyHash(hash) => {
                let header = network_bits | (payment_bits << 4) | (0 << 5);
                data.push(header);
                data.extend(payment_hash);
                data.extend(hash);
            }
            ShelleyAddressDelegationPart::ScriptHash(hash) => {
                let header = network_bits | (payment_bits << 4) | (1 << 5);
                data.push(header);
                data.extend(payment_hash);
                data.extend(hash);
            }
            ShelleyAddressDelegationPart::Pointer(pointer) => {
                let header = network_bits | (payment_bits << 4) | (2 << 5);
                data.push(header);
                data.extend(payment_hash);

                let mut encoder = VarIntEncoder::new();
                encoder.push(pointer.slot);
                encoder.push(pointer.tx_index);
                encoder.push(pointer.cert_index);
                data.extend(encoder.to_vec());
            }
        }

        Ok(data)
    }
}

/// Payload of a stake address
#[serde_as]
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum StakeAddressPayload {
    /// Stake key
    StakeKeyHash(#[serde_as(as = "Hex")] Vec<u8>),

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
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct StakeAddress {
    /// Network id
    pub network: AddressNetwork,

    /// Payload
    pub payload: StakeAddressPayload,
}

impl StakeAddress {
    /// Get either hash of the payload
    pub fn get_hash(&self) -> &[u8] {
        match &self.payload {
            StakeAddressPayload::StakeKeyHash(hash) => hash,
            StakeAddressPayload::ScriptHash(hash) => hash,
        }
    }

    /// Read from string format
    pub fn from_string(text: &str) -> Result<Self> {
        let (hrp, data) = bech32::decode(text)?;
        if let Some(header) = data.first() {
            let network = match hrp.as_str().contains("test") {
                true => AddressNetwork::Test,
                false => AddressNetwork::Main,
            };

            let payload = match (header >> 4) & 0x0F {
                0b1110 => StakeAddressPayload::StakeKeyHash(data[1..].to_vec()),
                0b1111 => StakeAddressPayload::ScriptHash(data[1..].to_vec()),
                _ => return Err(anyhow!("Unknown header {header} in stake address")),
            };

            return Ok(StakeAddress { network, payload });
        }

        Err(anyhow!("Empty stake address data"))
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

        return Ok(StakeAddress { network, payload });
    }

    /// Convert to string stake1xxx form
    pub fn to_string(&self) -> Result<String> {
        let (hrp, network_bits) = match self.network {
            AddressNetwork::Main => (bech32::Hrp::parse("stake")?, 1u8),
            AddressNetwork::Test => (bech32::Hrp::parse("stake_test")?, 0u8),
        };

        let (stake_hash, stake_bits): (&Vec<u8>, u8) = match &self.payload {
            StakeAddressPayload::StakeKeyHash(data) => (data, 0b1110),
            StakeAddressPayload::ScriptHash(data) => (data, 0b1111),
        };

        let mut data = vec![network_bits | (stake_bits << 4)];
        data.extend(stake_hash);
        Ok(bech32::encode::<bech32::Bech32>(hrp, &data)?)
    }

    pub fn to_bytes_key(&self) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        let (bits, hash): (u8, &[u8]) = match &self.payload {
            StakeAddressPayload::StakeKeyHash(h) => (0b1110, h),
            StakeAddressPayload::ScriptHash(h) => (0b1111, h),
        };

        let net_bit = match self.network {
            AddressNetwork::Main => 1,
            AddressNetwork::Test => 0,
        };

        let header = net_bit | (bits << 4);
        out.push(header);
        out.extend_from_slice(hash);
        Ok(out)
    }
}

/// A Cardano address
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
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
        return None;
    }

    /// Read from string format
    pub fn from_string(text: &str) -> Result<Self> {
        if text.starts_with("addr1") || text.starts_with("addr_test1") {
            Ok(Self::Shelley(ShelleyAddress::from_string(text)?))
        } else if text.starts_with("stake1") || text.starts_with("stake_test1") {
            Ok(Self::Stake(StakeAddress::from_string(text)?))
        } else {
            match ByronAddress::from_string(text) {
                Ok(byron) => Ok(Self::Byron(byron)),
                Err(_) => Ok(Self::None),
            }
        }
    }

    /// Convert to standard string representation
    pub fn to_string(&self) -> Result<String> {
        match self {
            Self::None => Err(anyhow!("No address")),
            Self::Byron(byron) => byron.to_string(),
            Self::Shelley(shelley) => shelley.to_string(),
            Self::Stake(stake) => stake.to_string(),
        }
    }

    pub fn to_bytes_key(&self) -> Result<Vec<u8>> {
        match self {
            Address::Byron(b) => b.to_bytes_key(),

            Address::Shelley(s) => s.to_bytes_key(),

            Address::Stake(stake) => stake.to_bytes_key(),

            Address::None => Err(anyhow!("No address to convert")),
        }
    }
}

// -- Tests --
#[cfg(test)]
mod tests {
    use super::*;
    use blake2::{
        digest::{Update, VariableOutput},
        Blake2bVar,
    };

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
        let mut hasher = Blake2bVar::new(28).unwrap();
        hasher.update(&pubkey);
        let mut hash = vec![0u8; 28];
        hasher.finalize_variable(&mut hash).unwrap();
        assert_eq!(28, hash.len());
        hash
    }

    fn test_stake_key_hash() -> Vec<u8> {
        let stake_key = "stake_vk1px4j0r2fk7ux5p23shz8f3y5y2qam7s954rgf3lg5merqcj6aetsft99wu";
        let (_, pubkey) = bech32::decode(stake_key).expect("Invalid Bech32 string");

        // pubkey is the raw key - we need the Blake2B hash
        let mut hasher = Blake2bVar::new(28).unwrap();
        hasher.update(&pubkey);
        let mut hash = vec![0u8; 28];
        hasher.finalize_variable(&mut hash).unwrap();
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
}
