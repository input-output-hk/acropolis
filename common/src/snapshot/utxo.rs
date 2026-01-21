// SPDX-License-Identifier: Apache-2.0
// Copyright © 2025, Acropolis team.

//! UTXO types and CBOR decoding for snapshot parsing.
//!
//! This module handles the UTXO structures from the NewEpochState ledger state.
//!
//! CDDL specification:
//! ```cddl
//! utxo = {* transaction_input => transaction_output }
//!
//! transaction_input = [txin_transaction_id : transaction_id, txin_index : uint .size 2]
//! transaction_id = bytes
//!
//! transaction_output = shelley_transaction_output / babbage_transaction_output
//! shelley_transaction_output = [address, amount : value, ? hash32]
//! babbage_transaction_output = {0 : address, 1 : value, ? 2 : datum_option, ? 3 : script_ref}
//!
//! address = bytes
//! value = coin / [coin, multiasset<positive_coin>]
//! coin = uint
//! multiasset<a0> = {* policy_id => {* asset_name => a0 } }
//!
//! datum_option = [0, hash32] / [1, data]
//! script_ref = #6.24(bytes .cbor script)
//! ```

use minicbor::data::Type;
use minicbor::Decoder;
use serde::{Deserialize, Serialize};

use crate::{
    Address, AssetName, ByronAddress, Datum, DatumHash, NativeAsset, NativeAssets, NativeScript,
    PolicyId, ReferenceScript, ShelleyAddress, StakeAddress, StakeCredential, TxHash, UTXOValue,
    UTxOIdentifier, Value,
};

// =============================================================================
// Public Types
// =============================================================================

/// UTXO entry combining transaction input reference and output value.
///
/// This is the primary type exposed to consumers of the snapshot parser.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UtxoEntry {
    /// UTxO identifier (transaction hash + output index)
    pub id: UTxOIdentifier,
    /// UTxO value (address, lovelace, assets, datum, script_ref)
    pub value: UTXOValue,
}

impl UtxoEntry {
    /// Get the transaction hash as a hex string
    #[inline]
    pub fn tx_hash_hex(&self) -> String {
        self.id.tx_hash_hex()
    }

    /// Get the output index
    #[inline]
    pub fn output_index(&self) -> u16 {
        self.id.output_index
    }

    /// Get the coin (lovelace) value of this UTXO
    #[inline]
    pub fn coin(&self) -> u64 {
        self.value.coin()
    }

    /// Get the address bytes
    #[inline]
    pub fn address_bytes(&self) -> Vec<u8> {
        self.value.address_bytes()
    }

    /// Extract the stake credential from the UTXO's address, if present.
    #[inline]
    pub fn extract_stake_credential(&self) -> Option<StakeCredential> {
        self.value.extract_stake_credential()
    }
}

// =============================================================================
// CBOR Decoding Wrappers
// =============================================================================
//
// These wrapper types provide minicbor::Decode implementations for parsing
// the snapshot CBOR format. They wrap the public types from crate::types.
//
// The wrappers are crate-private since consumers should use UtxoEntry directly.

/// Wrapper for decoding a complete UTXO entry (input + output pair)
pub(crate) struct SnapshotUTxO(pub UtxoEntry);

impl<'b, C> minicbor::Decode<'b, C> for SnapshotUTxO {
    fn decode(d: &mut Decoder<'b>, _: &mut C) -> Result<Self, minicbor::decode::Error> {
        let id: SnapshotUTxOIdentifier = d.decode()?;
        let value: SnapshotUTXOValue = d.decode()?;
        Ok(Self(UtxoEntry {
            id: id.0,
            value: value.0,
        }))
    }
}

// =============================================================================
// Transaction Input Decoding
// =============================================================================

/// Wrapper for decoding transaction input (TxIn)
/// CDDL: transaction_input = [txin_transaction_id, txin_index]
struct SnapshotUTxOIdentifier(pub UTxOIdentifier);

impl<'b, C> minicbor::Decode<'b, C> for SnapshotUTxOIdentifier {
    fn decode(d: &mut Decoder<'b>, _: &mut C) -> Result<Self, minicbor::decode::Error> {
        let tx_hash = TxHash::try_from(d.bytes()?)
            .map_err(|_| minicbor::decode::Error::message("Invalid TxHash (wrong size?)"))?;
        let output_index = d.u64()? as u16;
        Ok(Self(UTxOIdentifier {
            tx_hash,
            output_index,
        }))
    }
}

// =============================================================================
// Transaction Output Decoding
// =============================================================================

/// Wrapper for decoding transaction output (TxOut)
/// CDDL: transaction_output = shelley_transaction_output / babbage_transaction_output
struct SnapshotUTXOValue(pub UTXOValue);

impl<'b, C> minicbor::Decode<'b, C> for SnapshotUTXOValue {
    fn decode(d: &mut Decoder<'b>, _: &mut C) -> Result<Self, minicbor::decode::Error> {
        let datatype = d
            .datatype()
            .map_err(|_| minicbor::decode::Error::message("Failed to read TxOut datatype"))?;

        match datatype {
            // Shelley format: [address, value, ? datum_hash]
            Type::Array | Type::ArrayIndef => Self::decode_array_format(d),
            // Babbage/Conway format: {0: address, 1: value, ? 2: datum, ? 3: script_ref}
            Type::Map | Type::MapIndef => Self::decode_map_format(d),
            _ => Err(minicbor::decode::Error::message("unexpected TxOut type")),
        }
    }
}

impl SnapshotUTXOValue {
    /// Decode Shelley-era array format: [address, value, ? datum_hash]
    fn decode_array_format(d: &mut Decoder) -> Result<Self, minicbor::decode::Error> {
        let arr_len = d
            .array()
            .map_err(|_| minicbor::decode::Error::message("Failed to parse TxOut array"))?;

        if arr_len == Some(0) {
            return Err(minicbor::decode::Error::message("empty TxOut array"));
        }

        // Element 0: Address
        let address: SnapshotAddress = d.decode()?;

        // Element 1: Value
        let value: SnapshotValue = d.decode()?;

        // Element 2 (optional): Datum hash (Shelley style - just the hash, not datum_option)
        let datum = if arr_len.map(|l| l > 2).unwrap_or(true) {
            match d.datatype() {
                Ok(Type::Bytes) => {
                    let hash_bytes = d.bytes()?;
                    if hash_bytes.len() == 32 {
                        Some(Datum::Hash(DatumHash::try_from(hash_bytes).unwrap()))
                    } else {
                        None
                    }
                }
                Ok(Type::Break) => None,
                _ => {
                    d.skip().ok();
                    None
                }
            }
        } else {
            None
        };

        // Skip remaining fields
        if let Some(len) = arr_len {
            for _ in 3..len {
                d.skip()
                    .map_err(|_| minicbor::decode::Error::message("Failed to skip TxOut field"))?;
            }
        }

        Ok(Self(UTXOValue {
            address: address.0,
            value: value.0,
            datum,
            reference_script: None,
        }))
    }

    /// Decode Babbage/Conway-era map format: {0: address, 1: value, ? 2: datum, ? 3: script_ref}
    fn decode_map_format(d: &mut Decoder) -> Result<Self, minicbor::decode::Error> {
        let map_len =
            d.map().map_err(|_| minicbor::decode::Error::message("Failed to parse TxOut map"))?;

        let mut address: Option<Address> = None;
        let mut value: Option<Value> = None;
        let mut datum: Option<Datum> = None;
        let mut reference_script: Option<ReferenceScript> = None;

        let entries = map_len.unwrap_or(4);
        for _ in 0..entries {
            // Check for break in indefinite map
            if map_len.is_none() && matches!(d.datatype(), Ok(Type::Break)) {
                d.skip().ok();
                break;
            }

            let key = match d.u32() {
                Ok(k) => k,
                Err(_) => {
                    d.skip().ok();
                    d.skip().ok();
                    continue;
                }
            };

            match key {
                0 => address = Some(d.decode::<SnapshotAddress>()?.0),
                1 => value = Some(d.decode::<SnapshotValue>()?.0),
                2 => datum = decode_datum_option(d)?,
                3 => reference_script = decode_script_ref(d)?,
                _ => {
                    d.skip().ok();
                }
            }
        }

        match (address, value) {
            (Some(address), Some(value)) => Ok(Self(UTXOValue {
                address,
                value,
                datum,
                reference_script,
            })),
            _ => Err(minicbor::decode::Error::message(
                "map-based TxOut missing required fields",
            )),
        }
    }
}

// =============================================================================
// Datum Decoding
// =============================================================================

/// Decode datum_option: [0, hash32] / [1, data]
fn decode_datum_option(d: &mut Decoder) -> Result<Option<Datum>, minicbor::decode::Error> {
    d.array()?;
    let variant = d.u8()?;

    match variant {
        0 => {
            // Datum hash: [0, hash32]
            let hash_bytes = d.bytes()?;
            let hash = DatumHash::try_from(hash_bytes)
                .map_err(|_| minicbor::decode::Error::message("Invalid datum hash"))?;
            Ok(Some(Datum::Hash(hash)))
        }
        1 => {
            // Inline datum: [1, #6.24(bytes)]
            // The datum may be wrapped in CBOR tag 24 (encoded CBOR)
            if matches!(d.datatype(), Ok(Type::Tag)) {
                let tag = d.tag()?;
                if tag.as_u64() == 24 {
                    let datum_bytes = d.bytes()?.to_vec();
                    return Ok(Some(Datum::Inline(datum_bytes)));
                }
            }
            // Not tagged, read raw bytes or skip
            match d.datatype() {
                Ok(Type::Bytes) => {
                    let datum_bytes = d.bytes()?.to_vec();
                    Ok(Some(Datum::Inline(datum_bytes)))
                }
                _ => {
                    // Complex inline datum - capture the CBOR
                    // For now, skip it
                    d.skip()?;
                    Ok(None)
                }
            }
        }
        _ => {
            d.skip()?;
            Ok(None)
        }
    }
}

// =============================================================================
// Script Reference Decoding
// =============================================================================

/// Decode script_ref: #6.24(bytes .cbor script)
/// Script format: [script_type, script_bytes]
/// script_type: 0 = Native, 1 = PlutusV1, 2 = PlutusV2, 3 = PlutusV3
fn decode_script_ref(d: &mut Decoder) -> Result<Option<ReferenceScript>, minicbor::decode::Error> {
    // Script ref is wrapped in CBOR tag 24 (encoded CBOR)
    if !matches!(d.datatype(), Ok(Type::Tag)) {
        d.skip()?;
        return Ok(None);
    }

    let tag = d.tag()?;
    if tag.as_u64() != 24 {
        d.skip()?;
        return Ok(None);
    }

    // The content is CBOR-encoded bytes containing [script_type, script_bytes]
    let script_cbor = d.bytes()?;
    let mut script_decoder = Decoder::new(script_cbor);

    // Parse [script_type, script_bytes]
    if script_decoder.array().is_err() {
        return Ok(None);
    }

    let script_type = match script_decoder.u8() {
        Ok(t) => t,
        Err(_) => return Ok(None),
    };

    let script_bytes = match script_decoder.bytes() {
        Ok(b) => b.to_vec(),
        Err(_) => return Ok(None),
    };

    let reference_script = match script_type {
        0 => ReferenceScript::Native(minicbor::decode::<NativeScript>(&script_bytes)?),
        1 => ReferenceScript::PlutusV1(script_bytes),
        2 => ReferenceScript::PlutusV2(script_bytes),
        3 => ReferenceScript::PlutusV3(script_bytes),
        _ => return Ok(None),
    };

    Ok(Some(reference_script))
}

// =============================================================================
// Address Decoding
// =============================================================================

/// Wrapper for decoding addresses from raw bytes
/// Handles Byron, Shelley, and Stake address formats
struct SnapshotAddress(pub Address);

impl<'b, C> minicbor::Decode<'b, C> for SnapshotAddress {
    fn decode(d: &mut Decoder<'b>, _: &mut C) -> Result<Self, minicbor::decode::Error> {
        let bytes = d
            .bytes()
            .map_err(|_| minicbor::decode::Error::message("Failed to read address bytes"))?;

        if bytes.is_empty() {
            return Err(minicbor::decode::Error::message("Empty utxo address"));
        }

        Self::parse_address_bytes(bytes)
    }
}

impl SnapshotAddress {
    fn parse_address_bytes(bytes: &[u8]) -> Result<Self, minicbor::decode::Error> {
        match bytes[0] {
            // Byron addresses start with 0x82 (CBOR array of 2)
            0x82 => Self::decode_byron(bytes),
            // Shelley addresses: check header nibble
            _ => Self::decode_shelley(bytes),
        }
    }

    fn decode_byron(bytes: &[u8]) -> Result<Self, minicbor::decode::Error> {
        let mut dec = minicbor::Decoder::new(bytes);
        let byron = ByronAddress::from_cbor(&mut dec)
            .map_err(|_| minicbor::decode::Error::message("Failed to read Byron address"))?;
        Ok(Self(Address::Byron(byron)))
    }

    fn decode_shelley(bytes: &[u8]) -> Result<Self, minicbor::decode::Error> {
        let header_type = (bytes[0] >> 4) & 0x0F;

        match header_type {
            // Stake/reward addresses (type 14, 15)
            0b1110 | 0b1111 => {
                let stake = StakeAddress::from_binary(bytes).map_err(|_| {
                    minicbor::decode::Error::message("Failed to read stake address")
                })?;
                Ok(Self(Address::Stake(stake)))
            }
            // Base, enterprise, pointer addresses (types 0-7)
            _ => {
                let shelley = ShelleyAddress::from_bytes_key(bytes).map_err(|_| {
                    minicbor::decode::Error::message("Failed to read Shelley address")
                })?;
                Ok(Self(Address::Shelley(shelley)))
            }
        }
    }
}

// =============================================================================
// Value Decoding
// =============================================================================

/// Wrapper for decoding value (coin or multi-asset)
/// CDDL: value = coin / [coin, multiasset<positive_coin>]
struct SnapshotValue(pub Value);

impl<'b, C> minicbor::Decode<'b, C> for SnapshotValue {
    fn decode(d: &mut Decoder<'b>, _: &mut C) -> Result<Self, minicbor::decode::Error> {
        let datatype = d
            .datatype()
            .map_err(|_| minicbor::decode::Error::message("Failed to read Value datatype"))?;

        match datatype {
            // Simple coin-only value
            Type::U8 | Type::U16 | Type::U32 | Type::U64 => {
                let lovelace = d
                    .u64()
                    .map_err(|_| minicbor::decode::Error::message("Failed to parse coin amount"))?;
                Ok(Self(Value {
                    lovelace,
                    assets: NativeAssets::default(),
                }))
            }
            // Multi-asset: [coin, multiasset]
            Type::Array | Type::ArrayIndef => {
                d.array()
                    .map_err(|_| minicbor::decode::Error::message("Failed to parse value array"))?;

                let lovelace = d
                    .u64()
                    .map_err(|_| minicbor::decode::Error::message("Failed to parse coin amount"))?;

                let assets = decode_multiasset(d)?;

                Ok(Self(Value { lovelace, assets }))
            }
            _ => Err(minicbor::decode::Error::message(
                "Unexpected Value datatype",
            )),
        }
    }
}

// =============================================================================

/// Decode multiasset: {* policy_id => {* asset_name => amount } }
fn decode_multiasset(d: &mut Decoder) -> Result<NativeAssets, minicbor::decode::Error> {
    let mut assets: NativeAssets = Vec::new();

    let policy_map_len = d.map()?;

    match policy_map_len {
        Some(len) => {
            for _ in 0..len {
                let (policy_id, policy_assets) = decode_policy_assets(d)?;
                assets.push((policy_id, policy_assets));
            }
        }
        None => {
            // Indefinite-length map
            while !matches!(d.datatype(), Ok(Type::Break)) {
                let (policy_id, policy_assets) = decode_policy_assets(d)?;
                assets.push((policy_id, policy_assets));
            }
            d.skip()?; // consume break
        }
    }

    Ok(assets)
}

/// Decode a single policy's assets: policy_id => {* asset_name => amount }
fn decode_policy_assets(
    d: &mut Decoder,
) -> Result<(PolicyId, Vec<NativeAsset>), minicbor::decode::Error> {
    // Decode policy ID (28 bytes)
    let policy_bytes = d.bytes()?;
    if policy_bytes.len() != 28 {
        return Err(minicbor::decode::Error::message(format!(
            "invalid policy_id length: expected 28, got {}",
            policy_bytes.len()
        )));
    }
    let policy_id: PolicyId = policy_bytes
        .try_into()
        .map_err(|_| minicbor::decode::Error::message("Failed to convert policy_id bytes"))?;

    // Decode asset map: {* asset_name => amount }
    let mut policy_assets: Vec<NativeAsset> = Vec::new();
    let asset_map_len = d.map()?;

    match asset_map_len {
        Some(len) => {
            for _ in 0..len {
                let asset = decode_native_asset(d)?;
                policy_assets.push(asset);
            }
        }
        None => {
            // Indefinite-length map
            while !matches!(d.datatype(), Ok(Type::Break)) {
                let asset = decode_native_asset(d)?;
                policy_assets.push(asset);
            }
            d.skip()?; // consume break
        }
    }

    Ok((policy_id, policy_assets))
}

/// Decode a single native asset: asset_name => amount
fn decode_native_asset(d: &mut Decoder) -> Result<NativeAsset, minicbor::decode::Error> {
    let name_bytes = d.bytes()?;
    let name = AssetName::new(name_bytes)
        .ok_or_else(|| minicbor::decode::Error::message("Asset name too long (max 32 bytes)"))?;

    let amount = d.u64()?;

    Ok(NativeAsset { name, amount })
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{NetworkId, ShelleyAddressDelegationPart, ShelleyAddressPaymentPart};

    fn make_shelley_base_address() -> ShelleyAddress {
        let payment_hash: [u8; 28] = [0x11; 28];
        let stake_hash: [u8; 28] = [0x22; 28];
        ShelleyAddress {
            network: NetworkId::Mainnet,
            payment: ShelleyAddressPaymentPart::PaymentKeyHash(payment_hash.into()),
            delegation: ShelleyAddressDelegationPart::StakeKeyHash(stake_hash.into()),
        }
    }

    fn make_enterprise_address() -> ShelleyAddress {
        let payment_hash: [u8; 28] = [0x33; 28];
        ShelleyAddress {
            network: NetworkId::Mainnet,
            payment: ShelleyAddressPaymentPart::PaymentKeyHash(payment_hash.into()),
            delegation: ShelleyAddressDelegationPart::None,
        }
    }

    // Helper to create a UTxOIdentifier
    fn make_utxo_id(tx_hash_byte: u8, output_index: u16) -> UTxOIdentifier {
        let tx_hash = TxHash::try_from([tx_hash_byte; 32].as_slice()).unwrap();
        UTxOIdentifier::new(tx_hash, output_index)
    }

    // Helper to create a simple UTXOValue
    fn make_utxo_value(lovelace: u64, address: Address) -> UTXOValue {
        UTXOValue {
            address,
            value: Value {
                lovelace,
                assets: NativeAssets::default(),
            },
            datum: None,
            reference_script: None,
        }
    }

    #[test]
    fn utxo_entry_tx_hash_hex() {
        let id = make_utxo_id(0xAB, 5);
        let value = make_utxo_value(1_000_000, Address::None);
        let entry = UtxoEntry { id, value };

        let hex = entry.tx_hash_hex();
        assert!(hex.starts_with("abab"));
        assert_eq!(hex.len(), 64);
    }

    #[test]
    fn utxo_entry_output_index() {
        let id = make_utxo_id(0x00, 42);
        let value = make_utxo_value(0, Address::None);
        let entry = UtxoEntry { id, value };

        assert_eq!(entry.output_index(), 42);
    }

    #[test]
    fn utxo_entry_coin() {
        let id = make_utxo_id(0x00, 0);
        let value = make_utxo_value(5_000_000, Address::None);
        let entry = UtxoEntry { id, value };

        assert_eq!(entry.coin(), 5_000_000);
    }

    #[test]
    fn utxo_entry_address_bytes_shelley() {
        let shelley = make_shelley_base_address();
        let expected_bytes = shelley.to_bytes_key();

        let id = make_utxo_id(0x00, 0);
        let value = make_utxo_value(0, Address::Shelley(shelley));
        let entry = UtxoEntry { id, value };

        assert_eq!(entry.address_bytes(), expected_bytes);
    }

    #[test]
    fn utxo_entry_address_bytes_none() {
        let id = make_utxo_id(0x00, 0);
        let value = make_utxo_value(0, Address::None);
        let entry = UtxoEntry { id, value };

        assert!(entry.address_bytes().is_empty());
    }

    #[test]
    fn utxo_entry_extract_stake_credential_base_address() {
        let shelley = make_shelley_base_address();
        let id = make_utxo_id(0x00, 0);
        let value = make_utxo_value(0, Address::Shelley(shelley));
        let entry = UtxoEntry { id, value };

        let credential = entry.extract_stake_credential();
        assert!(credential.is_some());

        match credential.unwrap() {
            StakeCredential::AddrKeyHash(hash) => {
                assert_eq!(hash.as_ref(), &[0x22; 28]);
            }
            _ => panic!("Expected AddrKeyHash"),
        }
    }

    #[test]
    fn utxo_entry_extract_stake_credential_enterprise_address() {
        let shelley = make_enterprise_address();
        let id = make_utxo_id(0x00, 0);
        let value = make_utxo_value(0, Address::Shelley(shelley));
        let entry = UtxoEntry { id, value };

        // Enterprise addresses have no stake credential
        assert!(entry.extract_stake_credential().is_none());
    }

    #[test]
    fn utxo_entry_extract_stake_credential_none_address() {
        let id = make_utxo_id(0x00, 0);
        let value = make_utxo_value(0, Address::None);
        let entry = UtxoEntry { id, value };

        assert!(entry.extract_stake_credential().is_none());
    }

    #[test]
    fn decode_shelley_txout_array_format() {
        let addr = make_shelley_base_address();
        let addr_bytes = addr.to_bytes_key();

        let mut cbor = Vec::new();
        let mut enc = minicbor::Encoder::new(&mut cbor);
        enc.array(2).unwrap();
        enc.bytes(&addr_bytes).unwrap();
        enc.u64(2_000_000).unwrap();

        let mut dec = minicbor::Decoder::new(&cbor);
        let result: Result<SnapshotUTXOValue, _> = dec.decode();
        assert!(result.is_ok());

        let utxo_value = result.unwrap().0;
        assert_eq!(utxo_value.value.lovelace, 2_000_000);
        assert!(utxo_value.datum.is_none());
        assert!(utxo_value.reference_script.is_none());
    }

    #[test]
    fn decode_babbage_txout_map_format() {
        let addr = make_shelley_base_address();
        let addr_bytes = addr.to_bytes_key();

        let mut cbor = Vec::new();
        let mut enc = minicbor::Encoder::new(&mut cbor);
        enc.map(2).unwrap();
        enc.u32(0).unwrap();
        enc.bytes(&addr_bytes).unwrap();
        enc.u32(1).unwrap();
        enc.u64(3_000_000).unwrap();

        let mut dec = minicbor::Decoder::new(&cbor);
        let result: Result<SnapshotUTXOValue, _> = dec.decode();
        assert!(result.is_ok());

        let utxo_value = result.unwrap().0;
        assert_eq!(utxo_value.value.lovelace, 3_000_000);
    }

    #[test]
    fn decode_value_coin_only() {
        let mut cbor = Vec::new();
        let mut enc = minicbor::Encoder::new(&mut cbor);
        enc.u64(1_500_000).unwrap();

        let mut dec = minicbor::Decoder::new(&cbor);
        let result: Result<SnapshotValue, _> = dec.decode();
        assert!(result.is_ok());

        let value = result.unwrap().0;
        assert_eq!(value.lovelace, 1_500_000);
        assert!(value.assets.is_empty());
    }

    #[test]
    fn decode_value_with_multiasset() {
        let policy_id = PolicyId::from([0x44; 28]);
        let asset_name = b"TestToken";

        let mut cbor = Vec::new();
        let mut enc = minicbor::Encoder::new(&mut cbor);
        enc.array(2).unwrap();
        enc.u64(1_000_000).unwrap();
        enc.map(1).unwrap();
        enc.bytes(policy_id.as_ref()).unwrap();
        enc.map(1).unwrap();
        enc.bytes(asset_name).unwrap();
        enc.u64(100).unwrap();

        let mut dec = minicbor::Decoder::new(&cbor);
        let result: Result<SnapshotValue, _> = dec.decode();
        assert!(result.is_ok());

        let value = result.unwrap().0;
        assert_eq!(value.lovelace, 1_000_000);
        assert_eq!(value.assets.len(), 1);

        let (decoded_policy, assets) = &value.assets[0];
        assert_eq!(decoded_policy, &policy_id);
        assert_eq!(assets.len(), 1);
        assert_eq!(assets[0].amount, 100);
    }

    #[test]
    fn decode_utxo_identifier() {
        let tx_hash: [u8; 32] = [0x55; 32];

        let mut cbor = Vec::new();
        let mut enc = minicbor::Encoder::new(&mut cbor);
        enc.bytes(&tx_hash).unwrap();
        enc.u64(7).unwrap();

        let mut dec = minicbor::Decoder::new(&cbor);
        let result: Result<SnapshotUTxOIdentifier, _> = dec.decode();
        assert!(result.is_ok());

        let id = result.unwrap().0;
        assert_eq!(id.tx_hash.as_inner(), &tx_hash);
        assert_eq!(id.output_index, 7);
    }
}
