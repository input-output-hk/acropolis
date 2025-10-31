//! Cardano address definitions for Acropolis
// We don't use these types in the acropolis_common crate itself
#![allow(dead_code)]

use crate::cip19::{VarIntDecoder, VarIntEncoder};
use crate::{Credential, KeyHash, NetworkId, ScriptHash, StakeCredential};
use anyhow::{anyhow, bail, Result};
use crc::{Crc, CRC_32_ISO_HDLC};
use minicbor::data::IanaTag;
use std::cmp::Ordering;
use std::fmt::{Display, Formatter};

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

/// A Shelley-era address - payment part
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    minicbor::Encode,
    minicbor::Decode,
)]
pub enum ShelleyAddressPaymentPart {
    /// Payment to a key
    #[n(0)]
    PaymentKeyHash(#[n(0)] KeyHash),

    /// Payment to a script
    #[n(1)]
    ScriptHash(#[n(0)] ScriptHash),
}

impl Default for ShelleyAddressPaymentPart {
    fn default() -> Self {
        Self::PaymentKeyHash(KeyHash::default())
    }
}

/// Delegation pointer
#[derive(
    Debug,
    Default,
    Clone,
    Hash,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    minicbor::Encode,
    minicbor::Decode,
)]
pub struct ShelleyAddressPointer {
    /// Slot number
    #[n(0)]
    pub slot: u64,

    /// Transaction index within the slot
    #[n(1)]
    pub tx_index: u64,

    /// Certificate index within the transaction
    #[n(2)]
    pub cert_index: u64,
}

/// A Shelley-era address - delegation part
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    minicbor::Encode,
    minicbor::Decode,
)]
pub enum ShelleyAddressDelegationPart {
    /// No delegation (enterprise addresses)
    #[n(0)]
    None,

    /// Delegation to stake key
    #[n(1)]
    StakeKeyHash(#[n(0)] KeyHash),

    /// Delegation to script key hash
    #[n(2)]
    ScriptHash(#[n(0)] ScriptHash),

    /// Delegation to pointer
    #[n(3)]
    Pointer(#[n(0)] ShelleyAddressPointer),
}

impl Default for ShelleyAddressDelegationPart {
    fn default() -> Self {
        Self::None
    }
}

/// A Shelley-era address
#[derive(
    Debug,
    Default,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    minicbor::Encode,
    minicbor::Decode,
)]
pub struct ShelleyAddress {
    /// Network id
    #[n(0)]
    pub network: NetworkId,

    /// Payment part
    #[n(1)]
    pub payment: ShelleyAddressPaymentPart,

    /// Delegation part
    #[n(2)]
    pub delegation: ShelleyAddressDelegationPart,
}

impl ShelleyAddress {
    /// Read from string format
    pub fn from_string(text: &str) -> Result<Self> {
        let (hrp, data) = bech32::decode(text)?;
        if let Some(header) = data.first() {
            let network = match hrp.as_str().contains("test") {
                true => NetworkId::Testnet,
                false => NetworkId::Mainnet,
            };

            let header = *header;

            let payment_part = match (header >> 4) & 0x01 {
                0 => ShelleyAddressPaymentPart::PaymentKeyHash(
                    data[1..29]
                        .try_into()
                        .map_err(|e| anyhow!("Failed to parse payment key hash: {}", e))?,
                ),
                1 => ShelleyAddressPaymentPart::ScriptHash(
                    data[1..29]
                        .try_into()
                        .map_err(|e| anyhow!("Failed to parse payment script hash: {}", e))?,
                ),
                _ => panic!(),
            };

            let delegation_part = match (header >> 5) & 0x03 {
                0 => ShelleyAddressDelegationPart::StakeKeyHash(
                    data[29..57]
                        .try_into()
                        .map_err(|e| anyhow!("Failed to parse stake key hash: {}", e))?,
                ),
                1 => ShelleyAddressDelegationPart::ScriptHash(
                    data[29..57]
                        .try_into()
                        .map_err(|e| anyhow!("Failed to parse stake script hash: {}", e))?,
                ),
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
            NetworkId::Mainnet => (bech32::Hrp::parse("addr")?, 1u8),
            NetworkId::Testnet => (bech32::Hrp::parse("addr_test")?, 0u8),
        };

        let (payment_hash, payment_bits): (&KeyHash, u8) = match &self.payment {
            ShelleyAddressPaymentPart::PaymentKeyHash(data) => (data, 0),
            ShelleyAddressPaymentPart::ScriptHash(data) => (data, 1),
        };

        let (delegation_hash, delegation_bits): (Vec<u8>, u8) = match &self.delegation {
            ShelleyAddressDelegationPart::None => (Vec::new(), 3),
            ShelleyAddressDelegationPart::StakeKeyHash(hash) => (hash.to_vec(), 0),
            ShelleyAddressDelegationPart::ScriptHash(hash) => (hash.to_vec(), 1),
            ShelleyAddressDelegationPart::Pointer(pointer) => {
                let mut encoder = VarIntEncoder::new();
                encoder.push(pointer.slot);
                encoder.push(pointer.tx_index);
                encoder.push(pointer.cert_index);
                (encoder.to_vec(), 2)
            }
        };

        let mut data = vec![network_bits | (payment_bits << 4) | (delegation_bits << 5)];
        data.extend(payment_hash.as_ref());
        data.extend(&delegation_hash);
        Ok(bech32::encode::<bech32::Bech32>(hrp, &data)?)
    }

    pub fn to_bytes_key(&self) -> Result<Vec<u8>> {
        let network_bits = match self.network {
            NetworkId::Mainnet => 1u8,
            NetworkId::Testnet => 0u8,
        };

        let (payment_hash, payment_bits): (&KeyHash, u8) = match &self.payment {
            ShelleyAddressPaymentPart::PaymentKeyHash(data) => (data, 0),
            ShelleyAddressPaymentPart::ScriptHash(data) => (data, 1),
        };

        let mut data = Vec::new();

        let build_header =
            |variant: u8| -> u8 { network_bits | (payment_bits << 4) | (variant << 5) };

        match &self.delegation {
            ShelleyAddressDelegationPart::None => {
                let header = build_header(3);
                data.push(header);
                data.extend_from_slice(payment_hash.as_ref());
            }
            ShelleyAddressDelegationPart::StakeKeyHash(hash) => {
                let header = build_header(0);
                data.push(header);
                data.extend_from_slice(payment_hash.as_ref());
                data.extend_from_slice(hash.as_ref());
            }
            ShelleyAddressDelegationPart::ScriptHash(hash) => {
                let header = build_header(1);
                data.push(header);
                data.extend_from_slice(payment_hash.as_ref());
                data.extend_from_slice(hash.as_ref());
            }
            ShelleyAddressDelegationPart::Pointer(pointer) => {
                let header = build_header(2);
                data.push(header);
                data.extend_from_slice(payment_hash.as_ref());

                let mut encoder = VarIntEncoder::new();
                encoder.push(pointer.slot);
                encoder.push(pointer.tx_index);
                encoder.push(pointer.cert_index);
                data.extend(encoder.to_vec());
            }
        }

        Ok(data)
    }

    pub fn stake_address_string(&self) -> Result<Option<String>> {
        let network_bit = match self.network {
            NetworkId::Mainnet => 1,
            NetworkId::Testnet => 0,
        };

        match &self.delegation {
            ShelleyAddressDelegationPart::StakeKeyHash(key_hash) => {
                let mut data = Vec::with_capacity(29);
                data.push(network_bit | (0b1110 << 4));
                data.extend_from_slice(key_hash.as_ref());
                let stake = StakeAddress::from_binary(&data)?.to_string()?;
                Ok(Some(stake))
            }
            ShelleyAddressDelegationPart::ScriptHash(script_hash) => {
                let mut data = Vec::with_capacity(29);
                data.push(network_bit | (0b1111 << 4));
                data.extend_from_slice(script_hash.as_ref());
                let stake = StakeAddress::from_binary(&data)?.to_string()?;
                Ok(Some(stake))
            }
            // TODO: Use chain store to resolve pointer delegation addresses
            ShelleyAddressDelegationPart::Pointer(_pointer) => Ok(None),
            ShelleyAddressDelegationPart::None => Ok(None),
        }
    }
}

/// A stake address
#[derive(Debug, Clone, Eq, PartialEq, Hash, serde::Serialize, serde::Deserialize)]
pub struct StakeAddress {
    /// Network id
    pub network: NetworkId,

    /// Credential
    pub credential: StakeCredential,
}

impl StakeAddress {
    pub fn new(credential: StakeCredential, network: NetworkId) -> Self {
        StakeAddress {
            network,
            credential,
        }
    }

    pub fn get_hash(&self) -> &KeyHash {
        match &self.credential {
            StakeCredential::AddrKeyHash(hash) => hash,
            StakeCredential::ScriptHash(hash) => hash,
        }
    }

    pub fn get_credential(&self) -> Credential {
        match &self.credential {
            StakeCredential::AddrKeyHash(hash) => Credential::AddrKeyHash(*hash),
            StakeCredential::ScriptHash(hash) => Credential::ScriptHash(*hash),
        }
    }

    /// Convert to string stake1xxx format
    pub fn to_string(&self) -> Result<String> {
        let hrp = match self.network {
            NetworkId::Mainnet => bech32::Hrp::parse("stake")?,
            NetworkId::Testnet => bech32::Hrp::parse("stake_test")?,
        };

        let data = self.to_binary();
        Ok(bech32::encode::<bech32::Bech32>(hrp, data.as_slice())?)
    }

    /// Read from a string format ("stake1xxx...")
    pub fn from_string(text: &str) -> Result<Self> {
        let (hrp, data) = bech32::decode(text)?;
        if let Some(header) = data.first() {
            let network = match hrp.as_str().contains("test") {
                true => NetworkId::Testnet,
                false => NetworkId::Mainnet,
            };

            let credential = match (header >> 4) & 0x0Fu8 {
                0b1110 => StakeCredential::AddrKeyHash(
                    data[1..].try_into().map_err(|e| anyhow!("Failed to parse key hash: {}", e))?,
                ),
                0b1111 => StakeCredential::ScriptHash(
                    data[1..]
                        .try_into()
                        .map_err(|e| anyhow!("Failed to parse script hash: {}", e))?,
                ),
                _ => return Err(anyhow!("Unknown header {header} in stake address")),
            };

            return Ok(StakeAddress {
                network,
                credential,
            });
        }

        Err(anyhow!("Empty stake address data"))
    }

    /// Convert to binary format (29 bytes)
    pub fn to_binary(&self) -> Vec<u8> {
        let network_bits = match self.network {
            NetworkId::Mainnet => 0b1u8,
            NetworkId::Testnet => 0b0u8,
        };

        let (stake_bits, stake_hash) = match &self.credential {
            StakeCredential::AddrKeyHash(data) => (0b1110, data.as_ref()),
            StakeCredential::ScriptHash(data) => (0b1111, data.as_ref()),
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
            0b1 => NetworkId::Mainnet,
            _ => NetworkId::Testnet,
        };

        let credential = match (data[0] >> 4) & 0x0F {
            0b1110 => StakeCredential::AddrKeyHash(
                data[1..].try_into().map_err(|_| anyhow!("Invalid key hash size"))?,
            ),
            0b1111 => StakeCredential::ScriptHash(
                data[1..].try_into().map_err(|_| anyhow!("Invalid script hash size"))?,
            ),
            _ => bail!("Unknown header byte {:x} in stake address", data[0]),
        };

        Ok(StakeAddress {
            network,
            credential,
        })
    }

    pub fn to_bytes_key(&self) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        let (bits, hash): (u8, &[u8]) = match &self.credential {
            StakeCredential::AddrKeyHash(h) => (0b1110, h.as_slice()),
            StakeCredential::ScriptHash(h) => (0b1111, h.as_slice()),
        };

        let net_bit = match self.network {
            NetworkId::Mainnet => 1,
            NetworkId::Testnet => 0,
        };

        let header = net_bit | (bits << 4);
        out.push(header);
        out.extend_from_slice(hash);
        Ok(out)
    }
}

impl Display for StakeAddress {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", hex::encode(self.to_binary()))
    }
}

impl<C> minicbor::Encode<C> for StakeAddress {
    fn encode<W: minicbor::encode::Write>(
        &self,
        e: &mut minicbor::Encoder<W>,
        _ctx: &mut C,
    ) -> Result<(), minicbor::encode::Error<W::Error>> {
        let data = self.to_binary();
        e.bytes(data.as_slice())?;
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
            network: NetworkId::Mainnet,
            credential: StakeCredential::AddrKeyHash(KeyHash::default()),
        }
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
        None
    }

    /// Read from string format ("addr1...")
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

    pub fn kind(&self) -> &'static str {
        match self {
            Address::Byron(_) => "byron",
            Address::Shelley(_) => "shelley",
            Address::Stake(_) => "stake",
            Address::None => "none",
        }
    }

    pub fn is_script(&self) -> bool {
        match self {
            Address::Shelley(shelley) => match shelley.payment {
                ShelleyAddressPaymentPart::PaymentKeyHash(_) => false,
                ShelleyAddressPaymentPart::ScriptHash(_) => true,
            },
            Address::Stake(stake) => match stake.credential {
                StakeCredential::AddrKeyHash(_) => false,
                StakeCredential::ScriptHash(_) => true,
            },
            Address::Byron(_) | Address::None => false,
        }
    }
}

/// Used for ordering addresses by their bech representation
#[derive(Eq, PartialEq)]
pub struct BechOrdAddress(pub Address);

impl Ord for BechOrdAddress {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.to_string().into_iter().cmp(other.0.to_string())
    }
}

impl PartialOrd for BechOrdAddress {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
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
        assert_eq!(text, "8MMy4x9jE734Gz");

        let unpacked = Address::from_string(&text).unwrap();
        assert_eq!(address, unpacked);
    }

    // Standard keys from CIP-19
    fn test_payment_key_hash() -> KeyHash {
        let payment_key = "addr_vk1w0l2sr2zgfm26ztc6nl9xy8ghsk5sh6ldwemlpmp9xylzy4dtf7st80zhd";
        let (_, pubkey) = bech32::decode(payment_key).expect("Invalid Bech32 string");

        // pubkey is the raw key - we need the Blake2B hash
        let hash = keyhash_224(&pubkey);
        assert_eq!(28, hash.len());
        hash.as_slice().try_into().expect("Invalid hash size")
    }

    fn test_stake_key_hash() -> KeyHash {
        let stake_key = "stake_vk1px4j0r2fk7ux5p23shz8f3y5y2qam7s954rgf3lg5merqcj6aetsft99wu";
        let (_, pubkey) = bech32::decode(stake_key).expect("Invalid Bech32 string");

        // pubkey is the raw key - we need the Blake2B hash
        let hash = keyhash_224(&pubkey);
        assert_eq!(28, hash.len());
        hash.as_slice().try_into().expect("Invalid hash size")
    }

    fn test_script_hash() -> KeyHash {
        let script_hash = "script1cda3khwqv60360rp5m7akt50m6ttapacs8rqhn5w342z7r35m37";
        let (_, hash) = bech32::decode(script_hash).expect("Invalid Bech32 string");
        // This is already a hash
        assert_eq!(28, hash.len());
        hash.as_slice().try_into().expect("Invalid hash size")
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
            network: NetworkId::Mainnet,
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
            network: NetworkId::Mainnet,
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
            network: NetworkId::Mainnet,
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
            network: NetworkId::Mainnet,
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
            network: NetworkId::Mainnet,
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
            network: NetworkId::Mainnet,
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
            network: NetworkId::Mainnet,
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
            network: NetworkId::Mainnet,
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
            network: NetworkId::Mainnet,
            credential: StakeCredential::AddrKeyHash(KeyHash::from(test_stake_key_hash())),
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
            network: NetworkId::Mainnet,
            credential: StakeCredential::ScriptHash(KeyHash::from(test_script_hash())),
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
    fn shelley_to_stake_address_string_mainnet() {
        let normal_address = ShelleyAddress::from_string("addr1q82peck5fynytkgjsp9vnpul59zswsd4jqnzafd0mfzykma625r684xsx574ltpznecr9cnc7n9e2hfq9lyart3h5hpszffds5").expect("valid normal address");
        let script_address = ShelleyAddress::from_string("addr1zx0whlxaw4ksygvuljw8jxqlw906tlql06ern0gtvvzhh0c6409492020k6xml8uvwn34wrexagjh5fsk5xk96jyxk2qhlj6gf").expect("valid script address");

        let normal_stake_address = normal_address
            .stake_address_string()
            .expect("stake_address_string should not fail")
            .expect("normal address should have stake credential");
        let script_stake_address = script_address
            .stake_address_string()
            .expect("stake_address_string should not fail")
            .expect("script address should have stake credential");

        assert_eq!(
            normal_stake_address,
            "stake1uxa92par6ngr202l4s3fuupjufu0fju4t5szljw34cm6tscq40449"
        );
        assert_eq!(
            script_stake_address,
            "stake1uyd2hj6j4848mdrdln7x8fc6hpunw5ft6yct2rtzafzrt9qh0m28h"
        );
    }

    #[test]
    fn stake_address_from_binary_mainnet_stake() {
        // First withdrawal on Mainnet
        let binary =
            hex::decode("e1558f3ee09b26d88fac2eddc772a9eda94cce6dbadbe9fee439bd6001").unwrap();
        let sa = StakeAddress::from_binary(&binary).unwrap();
        assert_eq!(sa.network, NetworkId::Mainnet);
        assert_eq!(
            match sa.credential {
                StakeCredential::AddrKeyHash(key) => hex::encode(key),
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
        assert_eq!(sa.network, NetworkId::Mainnet);
        assert_eq!(
            match sa.credential {
                StakeCredential::ScriptHash(key) => hex::encode(key),
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
        assert_eq!(sa.network, NetworkId::Testnet);
        assert_eq!(
            match sa.credential {
                StakeCredential::AddrKeyHash(key) => hex::encode(key),
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
        let expected = [[0x58, 0x1d].as_slice(), binary.as_slice()].concat();

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
            v.extend_from_slice(mainnet_stake_address().to_binary().as_slice());
            v
        };

        let mut decoder = minicbor::Decoder::new(&binary);
        let decoded = StakeAddress::decode(&mut decoder, &mut ()).unwrap();

        assert_eq!(decoded.network, NetworkId::Mainnet);
        assert_eq!(
            match decoded.credential {
                StakeCredential::AddrKeyHash(key) => hex::encode(key),
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

        assert_eq!(decoded.network, NetworkId::Mainnet);
        assert_eq!(
            match decoded.credential {
                StakeCredential::ScriptHash(key) => hex::encode(key),
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

        assert_eq!(decoded.network, NetworkId::Testnet);
        assert_eq!(
            match decoded.credential {
                StakeCredential::ScriptHash(key) => hex::encode(key),
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
