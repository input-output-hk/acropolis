//! Cardano address definitions for Acropolis
// We don't use these types in the acropolis_common crate itself
#![allow(dead_code)]
use anyhow::{Result, anyhow};
use crate::types::{KeyHash, ScriptHash};
use crate::varint_encoder::VarIntEncoder;

/// a Byron-era address
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ByronAddress {
    /// Raw payload
    pub payload: Vec<u8>,
}

/// Address network identifier
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum AddressNetwork {
    /// Mainnet
    Main,

    /// Testnet
    Test,
}

impl Default for AddressNetwork {
    fn default() -> Self { Self::Main }
}

/// A Shelley-era address - payment part
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ShelleyAddressPaymentPart {
    /// Payment to a key
    PaymentKeyHash(KeyHash),

    /// Payment to a script
    ScriptHash(ScriptHash),
}

impl Default for ShelleyAddressPaymentPart {
    fn default() -> Self { Self::PaymentKeyHash(Vec::new()) }
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
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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
    fn default() -> Self { Self::None }
}

/// A Shelley-era address
#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ShelleyAddress {
    /// Network id
    pub network: AddressNetwork,

    /// Payment part
    pub payment: ShelleyAddressPaymentPart,

    /// Delegation part
    pub delegation: ShelleyAddressDelegationPart,
}

impl ShelleyAddress {
    /// Convert to addr1xxx form
    pub fn to_string(&self) -> Result<String> {
        let (hrp, network_bits) = match self.network {
            AddressNetwork::Main => (bech32::Hrp::parse("addr")?, 1u8),
            AddressNetwork::Test => (bech32::Hrp::parse("addr_test")?, 0u8),
        };

        let (payment_hash, payment_bits): (&Vec<u8>, u8) = match &self.payment {
            ShelleyAddressPaymentPart::PaymentKeyHash(data) => (data, 0),
            ShelleyAddressPaymentPart::ScriptHash(data) => (data, 1)
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

        let mut data = vec!( network_bits | (payment_bits << 4) | (delegation_bits << 5) );
        data.extend(payment_hash);
        data.extend(delegation_hash);
        Ok(bech32::encode::<bech32::Bech32>(hrp, &data)?)
    }
}

/// Payload of a stake address
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum StakeAddressPayload {
    /// Stake key
    StakeKeyHash(Vec<u8>),

    /// Script hash
    ScriptHash(ScriptHash),
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
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StakeAddress {
    /// Network id
    pub network: AddressNetwork,

    /// Payload
    pub payload: StakeAddressPayload,
}

impl StakeAddress {
    /// Convert to string stake1xxx form
    pub fn to_string(&self) -> Result<String> {
        let (hrp, network_bits) = match self.network {
            AddressNetwork::Main => (bech32::Hrp::parse("stake")?, 1u8),
            AddressNetwork::Test => (bech32::Hrp::parse("stake_test")?, 0u8)
        };

        let (stake_hash, stake_bits): (&Vec<u8>, u8) = match &self.payload {
            StakeAddressPayload::StakeKeyHash(data) => (data, 0b1110),
            StakeAddressPayload::ScriptHash(data) => (data, 0b1111)
        };

        let mut data = vec!( network_bits | (stake_bits << 4) );
        data.extend(stake_hash);
        Ok(bech32::encode::<bech32::Bech32>(hrp, &data)?)
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
    fn default() -> Self { Self::None }
}

impl Address {
    /// Get the stake pointer if there is one
    pub fn get_pointer(&self) -> Option<ShelleyAddressPointer> {
        if let Address::Shelley(shelley) = self {
            if let ShelleyAddressDelegationPart::Pointer(ptr) = &shelley.delegation {
                return Some(ptr.clone())
            }
        }
        return None
    }

    /// Convert to standard string representation
    pub fn to_string(&self) -> Result<String> {
        match self {
            Address::None => Err(anyhow!("No address")),
            Address::Byron(byron) => Ok(bs58::encode(&byron.payload).into_string()),
            Address::Shelley(shelley) => shelley.to_string(),
            Address::Stake(stake) => stake.to_string(),
        }
    }
}

// -- Tests --
#[cfg(test)]
mod tests {
    use super::*;
    use blake2::{Blake2bVar, digest::{Update, VariableOutput}};

    #[test]
    fn byron_address() {
        let payload = vec!(42);
        let address = Address::Byron(ByronAddress{ payload });
        assert_eq!(address.to_string().unwrap(), "j");
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
            cert_index: 3
        }
    }

    // Test vectors from CIP-19
    #[test]
    fn shelley_type_0() {
        let address = Address::Shelley(ShelleyAddress{
            network: AddressNetwork::Main,
            payment: ShelleyAddressPaymentPart::PaymentKeyHash(test_payment_key_hash()),
            delegation: ShelleyAddressDelegationPart::StakeKeyHash(test_stake_key_hash()),
        });

        assert_eq!(address.to_string().unwrap(),
                   "addr1qx2fxv2umyhttkxyxp8x0dlpdt3k6cwng5pxj3jhsydzer3n0d3vllmyqwsx5wktcd8cc3sq835lu7drv2xwl2wywfgse35a3x");
    }

    #[test]
    fn shelley_type_1() {
        let address = Address::Shelley(ShelleyAddress{
            network: AddressNetwork::Main,
            payment: ShelleyAddressPaymentPart::ScriptHash(test_script_hash()),
            delegation: ShelleyAddressDelegationPart::StakeKeyHash(test_stake_key_hash()),
        });

        assert_eq!(address.to_string().unwrap(),
                   "addr1z8phkx6acpnf78fuvxn0mkew3l0fd058hzquvz7w36x4gten0d3vllmyqwsx5wktcd8cc3sq835lu7drv2xwl2wywfgs9yc0hh");
    }

    #[test]
    fn shelley_type_2() {
        let address = Address::Shelley(ShelleyAddress{
            network: AddressNetwork::Main,
            payment: ShelleyAddressPaymentPart::PaymentKeyHash(test_payment_key_hash()),
            delegation: ShelleyAddressDelegationPart::ScriptHash(test_script_hash()),
        });

        assert_eq!(address.to_string().unwrap(),
                   "addr1yx2fxv2umyhttkxyxp8x0dlpdt3k6cwng5pxj3jhsydzerkr0vd4msrxnuwnccdxlhdjar77j6lg0wypcc9uar5d2shs2z78ve");
    }

    #[test]
    fn shelley_type_3() {
        let address = Address::Shelley(ShelleyAddress{
            network: AddressNetwork::Main,
            payment: ShelleyAddressPaymentPart::ScriptHash(test_script_hash()),
            delegation: ShelleyAddressDelegationPart::ScriptHash(test_script_hash()),
        });

        assert_eq!(address.to_string().unwrap(),
                   "addr1x8phkx6acpnf78fuvxn0mkew3l0fd058hzquvz7w36x4gt7r0vd4msrxnuwnccdxlhdjar77j6lg0wypcc9uar5d2shskhj42g");
    }

    #[test]
    fn shelley_type_4() {
        let address = Address::Shelley(ShelleyAddress{
            network: AddressNetwork::Main,
            payment: ShelleyAddressPaymentPart::PaymentKeyHash(test_payment_key_hash()),
            delegation: ShelleyAddressDelegationPart::Pointer(test_pointer()),
        });

        assert_eq!(address.to_string().unwrap(),
                   "addr1gx2fxv2umyhttkxyxp8x0dlpdt3k6cwng5pxj3jhsydzer5pnz75xxcrzqf96k");
    }

    #[test]
    fn shelley_type_5() {
        let address = Address::Shelley(ShelleyAddress{
            network: AddressNetwork::Main,
            payment: ShelleyAddressPaymentPart::ScriptHash(test_script_hash()),
            delegation: ShelleyAddressDelegationPart::Pointer(test_pointer()),
        });

        assert_eq!(address.to_string().unwrap(),
                   "addr128phkx6acpnf78fuvxn0mkew3l0fd058hzquvz7w36x4gtupnz75xxcrtw79hu");
    }

    #[test]
    fn shelley_type_6() {
        let address = Address::Shelley(ShelleyAddress{
            network: AddressNetwork::Main,
            payment: ShelleyAddressPaymentPart::PaymentKeyHash(test_payment_key_hash()),
            delegation: ShelleyAddressDelegationPart::None,
        });

        assert_eq!(address.to_string().unwrap(),
                   "addr1vx2fxv2umyhttkxyxp8x0dlpdt3k6cwng5pxj3jhsydzers66hrl8");
    }

    #[test]
    fn shelley_type_7() {
        let address = Address::Shelley(ShelleyAddress{
            network: AddressNetwork::Main,
            payment: ShelleyAddressPaymentPart::ScriptHash(test_script_hash()),
            delegation: ShelleyAddressDelegationPart::None,
        });

        assert_eq!(address.to_string().unwrap(),
                   "addr1w8phkx6acpnf78fuvxn0mkew3l0fd058hzquvz7w36x4gtcyjy7wx");
    }

    #[test]
    fn shelley_type_14() {
        let address = Address::Stake(StakeAddress{
            network: AddressNetwork::Main,
            payload: StakeAddressPayload::StakeKeyHash(test_stake_key_hash()),
        });

        assert_eq!(address.to_string().unwrap(),
                   "stake1uyehkck0lajq8gr28t9uxnuvgcqrc6070x3k9r8048z8y5gh6ffgw");
    }

    #[test]
    fn shelley_type_15() {
        let address = Address::Stake(StakeAddress{
            network: AddressNetwork::Main,
            payload: StakeAddressPayload::ScriptHash(test_script_hash()),
        });

        assert_eq!(address.to_string().unwrap(),
                   "stake178phkx6acpnf78fuvxn0mkew3l0fd058hzquvz7w36x4gtcccycj5");
    }
}
