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
//! multiasset<a0> = {* bytes => {* bytes => int } }
//!
//! datum_option = [0, hash32 // 1, data]
//! script_ref = #6.24(bytes .cbor script)
//! ```

use anyhow::{Context, Result};
use minicbor::data::Type;
use minicbor::Decoder;
use serde::{Deserialize, Serialize};

use crate::StakeCredential;

// =============================================================================
// TransactionInput
// =============================================================================

/// Transaction input reference (TxIn)
/// CDDL: transaction_input = [txin_transaction_id : transaction_id, txin_index : uint .size 2]
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TransactionInput {
    /// Transaction hash (32 bytes, hex-encoded for display)
    pub tx_hash: [u8; 32],
    /// Output index within the transaction
    pub output_index: u16,
}

impl TransactionInput {
    /// Get the transaction hash as a hex string
    pub fn tx_hash_hex(&self) -> String {
        hex::encode(self.tx_hash)
    }
}

impl<'b, C> minicbor::Decode<'b, C> for TransactionInput {
    fn decode(d: &mut Decoder<'b>, _ctx: &mut C) -> Result<Self, minicbor::decode::Error> {
        d.array()?;

        let tx_hash_bytes = d.bytes()?;
        if tx_hash_bytes.len() != 32 {
            return Err(minicbor::decode::Error::message(format!(
                "invalid tx_hash length: expected 32, got {}",
                tx_hash_bytes.len()
            )));
        }
        let mut tx_hash = [0u8; 32];
        tx_hash.copy_from_slice(tx_hash_bytes);

        let output_index = d.u16()?;

        Ok(TransactionInput {
            tx_hash,
            output_index,
        })
    }
}

// =============================================================================
// Value (coin or multi-asset)
// =============================================================================

/// Policy ID (28-byte hash)
pub type PolicyId = [u8; 28];

/// Asset name (up to 32 bytes)
pub type AssetName = Vec<u8>;

/// Multi-asset map: policy_id -> [(asset_name, amount)]
pub type MultiAsset = Vec<(PolicyId, Vec<(AssetName, u64)>)>;

/// Lovelace value, potentially with multi-assets
/// CDDL: value = coin / [coin, multiasset<positive_coin>]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Value {
    /// Lovelace (ADA) amount
    pub coin: u64,
    /// Multi-assets (policy_id -> asset_name -> amount)
    /// None if this is a simple coin-only value
    pub multiasset: Option<MultiAsset>,
}

impl Value {
    /// Create a simple coin-only value
    pub fn coin_only(coin: u64) -> Self {
        Value {
            coin,
            multiasset: None,
        }
    }
}

impl<'b, C> minicbor::Decode<'b, C> for Value {
    fn decode(d: &mut Decoder<'b>, _ctx: &mut C) -> Result<Self, minicbor::decode::Error> {
        match d.datatype()? {
            // Simple coin value
            Type::U8 | Type::U16 | Type::U32 | Type::U64 => {
                let coin = d.u64()?;
                Ok(Value::coin_only(coin))
            }
            // Multi-asset: [coin, multiasset]
            Type::Array | Type::ArrayIndef => {
                d.array()?;
                let coin = d.u64()?;

                // Parse multiasset map: {* policy_id => {* asset_name => amount } }
                let mut multiasset = Vec::new();
                let policy_map_len = d.map()?;

                match policy_map_len {
                    Some(len) => {
                        for _ in 0..len {
                            let policy_id = decode_policy_id(d)?;
                            let assets = decode_asset_map(d)?;
                            multiasset.push((policy_id, assets));
                        }
                    }
                    None => {
                        while d.datatype()? != Type::Break {
                            let policy_id = decode_policy_id(d)?;
                            let assets = decode_asset_map(d)?;
                            multiasset.push((policy_id, assets));
                        }
                        d.skip()?; // consume break
                    }
                }

                Ok(Value {
                    coin,
                    multiasset: if multiasset.is_empty() {
                        None
                    } else {
                        Some(multiasset)
                    },
                })
            }
            other => Err(minicbor::decode::Error::message(format!(
                "unexpected value type: {:?}",
                other
            ))),
        }
    }
}

fn decode_policy_id(d: &mut Decoder) -> Result<PolicyId, minicbor::decode::Error> {
    let bytes = d.bytes()?;
    if bytes.len() != 28 {
        return Err(minicbor::decode::Error::message(format!(
            "invalid policy_id length: expected 28, got {}",
            bytes.len()
        )));
    }
    let mut policy_id = [0u8; 28];
    policy_id.copy_from_slice(bytes);
    Ok(policy_id)
}

fn decode_asset_map(d: &mut Decoder) -> Result<Vec<(AssetName, u64)>, minicbor::decode::Error> {
    let mut assets = Vec::new();
    let asset_map_len = d.map()?;

    match asset_map_len {
        Some(len) => {
            for _ in 0..len {
                let asset_name = d.bytes()?.to_vec();
                let amount = d.u64()?;
                assets.push((asset_name, amount));
            }
        }
        None => {
            while d.datatype()? != Type::Break {
                let asset_name = d.bytes()?.to_vec();
                let amount = d.u64()?;
                assets.push((asset_name, amount));
            }
            d.skip()?; // consume break
        }
    }

    Ok(assets)
}

// =============================================================================
// TransactionOutput
// =============================================================================

/// Transaction output (TxOut)
/// CDDL: transaction_output = shelley_transaction_output / babbage_transaction_output
/// shelley_transaction_output = [address, amount : value, ? hash32]
/// babbage_transaction_output = {0 : address, 1 : value, ? 2 : datum_option, ? 3 : script_ref}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TransactionOutput {
    /// Address (raw bytes)
    pub address: Vec<u8>,
    /// Value (coin + optional multi-assets)
    pub value: Value,
    /// Optional datum hash or inline datum
    pub datum: Option<Datum>,
    /// Optional script reference
    pub script_ref: Option<Vec<u8>>,
}

/// Datum attached to a transaction output
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Datum {
    /// Datum hash (32 bytes)
    Hash([u8; 32]),
    /// Inline datum (raw CBOR bytes)
    Inline(Vec<u8>),
}

impl TransactionOutput {
    /// Get the address as a hex string
    pub fn address_hex(&self) -> String {
        hex::encode(&self.address)
    }

    /// Get the coin (lovelace) value
    pub fn coin(&self) -> u64 {
        self.value.coin
    }
}

impl<'b, C> minicbor::Decode<'b, C> for TransactionOutput {
    fn decode(d: &mut Decoder<'b>, _ctx: &mut C) -> Result<Self, minicbor::decode::Error> {
        match d.datatype()? {
            // Shelley format: [address, value, ? datum_hash]
            Type::Array | Type::ArrayIndef => {
                let arr_len = d.array()?;
                if arr_len == Some(0) {
                    return Err(minicbor::decode::Error::message("empty TxOut array"));
                }

                let address = d.bytes()?.to_vec();
                let value: Value = d.decode()?;

                // Optional datum hash (Shelley style)
                let datum = if arr_len.map(|l| l > 2).unwrap_or(true) {
                    match d.datatype() {
                        Ok(Type::Bytes) => {
                            let hash_bytes = d.bytes()?;
                            if hash_bytes.len() == 32 {
                                let mut hash = [0u8; 32];
                                hash.copy_from_slice(hash_bytes);
                                Some(Datum::Hash(hash))
                            } else {
                                None
                            }
                        }
                        Ok(Type::Break) => {
                            // End of indefinite array
                            None
                        }
                        _ => {
                            // Skip unknown field
                            d.skip().ok();
                            None
                        }
                    }
                } else {
                    None
                };

                // Skip remaining fields in Shelley format
                if let Some(len) = arr_len {
                    for _ in 3..len {
                        d.skip()?;
                    }
                }

                Ok(TransactionOutput {
                    address,
                    value,
                    datum,
                    script_ref: None,
                })
            }
            // Babbage format: {0: address, 1: value, ? 2: datum_option, ? 3: script_ref}
            Type::Map | Type::MapIndef => {
                let map_len = d.map()?;

                let mut address = Vec::new();
                let mut value = Value::coin_only(0);
                let mut datum = None;
                let mut script_ref = None;

                let entries = map_len.unwrap_or(4);
                for _ in 0..entries {
                    // Check for break in indefinite map
                    if map_len.is_none() && matches!(d.datatype(), Ok(Type::Break)) {
                        d.skip()?;
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
                        0 => {
                            // Address
                            address = d.bytes()?.to_vec();
                        }
                        1 => {
                            // Value
                            value = d.decode()?;
                        }
                        2 => {
                            // Datum option: [0, hash32] or [1, data]
                            datum = decode_datum_option(d)?;
                        }
                        3 => {
                            // Script ref: #6.24(bytes .cbor script)
                            script_ref = decode_script_ref(d)?;
                        }
                        _ => {
                            d.skip()?;
                        }
                    }
                }

                Ok(TransactionOutput {
                    address,
                    value,
                    datum,
                    script_ref,
                })
            }
            other => Err(minicbor::decode::Error::message(format!(
                "unexpected TxOut type: {:?}",
                other
            ))),
        }
    }
}

fn decode_datum_option(d: &mut Decoder) -> Result<Option<Datum>, minicbor::decode::Error> {
    d.array()?;
    let variant = d.u8()?;

    match variant {
        0 => {
            // Datum hash
            let hash_bytes = d.bytes()?;
            if hash_bytes.len() == 32 {
                let mut hash = [0u8; 32];
                hash.copy_from_slice(hash_bytes);
                Ok(Some(Datum::Hash(hash)))
            } else {
                Ok(None)
            }
        }
        1 => {
            // Inline datum - store raw CBOR
            // The datum is wrapped in #6.24 tag
            if d.datatype()? == Type::Tag {
                let tag = d.tag()?;
                if tag.as_u64() == 24 {
                    let datum_bytes = d.bytes()?.to_vec();
                    return Ok(Some(Datum::Inline(datum_bytes)));
                }
            }
            // Fallback: skip and return None
            d.skip()?;
            Ok(None)
        }
        _ => {
            d.skip()?;
            Ok(None)
        }
    }
}

fn decode_script_ref(d: &mut Decoder) -> Result<Option<Vec<u8>>, minicbor::decode::Error> {
    // Script ref is #6.24(bytes .cbor script)
    if d.datatype()? == Type::Tag {
        let tag = d.tag()?;
        if tag.as_u64() == 24 {
            let script_bytes = d.bytes()?.to_vec();
            return Ok(Some(script_bytes));
        }
    }
    d.skip()?;
    Ok(None)
}

// =============================================================================
// UtxoEntry
// =============================================================================

/// Complete UTXO entry combining input reference and output
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UtxoEntry {
    /// Transaction input (reference to the UTXO)
    pub input: TransactionInput,
    /// Transaction output (the actual UTXO data)
    pub output: TransactionOutput,
}

impl UtxoEntry {
    /// Parse a single UTXO entry from a CBOR decoder
    /// This decodes one key-value pair from the UTXO map
    pub fn decode(d: &mut Decoder) -> Result<Self> {
        let input: TransactionInput = d.decode().context("Failed to decode TransactionInput")?;
        let output: TransactionOutput = d.decode().context("Failed to decode TransactionOutput")?;

        Ok(UtxoEntry { input, output })
    }

    /// Get the transaction hash as a hex string
    pub fn tx_hash_hex(&self) -> String {
        self.input.tx_hash_hex()
    }

    /// Get the output index
    pub fn output_index(&self) -> u16 {
        self.input.output_index
    }

    /// Get the address as a hex string
    pub fn address_hex(&self) -> String {
        self.output.address_hex()
    }

    /// Get the coin (lovelace) value
    pub fn coin(&self) -> u64 {
        self.output.coin()
    }

    /// Extract the stake credential from the UTXO's address, if present.
    ///
    /// Returns `Some(StakeCredential)` for Shelley base addresses that have
    /// a stake credential embedded. Returns `None` for:
    /// - Byron addresses
    /// - Enterprise addresses (no stake part)
    /// - Pointer addresses (stake part is a pointer, not a credential)
    /// - Reward addresses
    /// - Invalid/malformed addresses
    ///
    /// Address format (Shelley):
    /// - Header byte encodes network and address type
    /// - Base addresses (type 0-3): 1 byte header + 28 bytes payment + 28 bytes stake
    /// - For types 0,1: stake part is key hash
    /// - For types 2,3: stake part is script hash
    pub fn extract_stake_credential(&self) -> Option<StakeCredential> {
        let addr = &self.output.address;

        // Minimum length for base address: 1 (header) + 28 (payment) + 28 (stake) = 57
        if addr.len() < 57 {
            return None;
        }

        let header = addr[0];
        let addr_type = (header & 0xF0) >> 4;

        // Only base addresses (types 0-3) have embedded stake credentials
        match addr_type {
            0 | 1 => {
                // Base address with stake key hash
                let stake_bytes: [u8; 28] = addr[29..57].try_into().ok()?;
                Some(StakeCredential::AddrKeyHash(stake_bytes.into()))
            }
            2 | 3 => {
                // Base address with stake script hash
                let stake_bytes: [u8; 28] = addr[29..57].try_into().ok()?;
                Some(StakeCredential::ScriptHash(stake_bytes.into()))
            }
            _ => None, // Enterprise, pointer, reward, or Byron addresses
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transaction_input_decode() {
        // [tx_hash (32 bytes), output_index (u16)]
        let tx_hash = [0xab; 32];
        let mut cbor = vec![0x82]; // array of 2
        cbor.push(0x58); // bytes with 1-byte length
        cbor.push(32);
        cbor.extend_from_slice(&tx_hash);
        cbor.push(0x05); // output_index = 5

        let mut decoder = Decoder::new(&cbor);
        let input: TransactionInput = decoder.decode().unwrap();

        assert_eq!(input.tx_hash, tx_hash);
        assert_eq!(input.output_index, 5);
    }

    #[test]
    fn test_value_coin_only() {
        let cbor = [0x1a, 0x00, 0x0f, 0x42, 0x40]; // 1_000_000
        let mut decoder = Decoder::new(&cbor);
        let value: Value = decoder.decode().unwrap();

        assert_eq!(value.coin, 1_000_000);
        assert!(value.multiasset.is_none());
    }

    #[test]
    fn test_extract_stake_credential_base_address() {
        // Construct a base address (type 0): header + 28 payment + 28 stake
        let mut address = vec![0x01]; // header: type 0, network 1
        address.extend_from_slice(&[0x11; 28]); // payment key hash
        address.extend_from_slice(&[0x22; 28]); // stake key hash

        let entry = UtxoEntry {
            input: TransactionInput {
                tx_hash: [0; 32],
                output_index: 0,
            },
            output: TransactionOutput {
                address,
                value: Value::coin_only(1_000_000),
                datum: None,
                script_ref: None,
            },
        };

        let stake_cred = entry.extract_stake_credential();
        assert!(stake_cred.is_some());

        if let Some(StakeCredential::AddrKeyHash(hash)) = stake_cred {
            assert_eq!(hash.as_ref(), &[0x22; 28]);
        } else {
            panic!("Expected AddrKeyHash");
        }
    }

    #[test]
    fn test_extract_stake_credential_enterprise_address() {
        // Enterprise address (type 6): header + 28 payment (no stake part)
        let mut address = vec![0x61]; // header: type 6, network 1
        address.extend_from_slice(&[0x11; 28]); // payment key hash

        let entry = UtxoEntry {
            input: TransactionInput {
                tx_hash: [0; 32],
                output_index: 0,
            },
            output: TransactionOutput {
                address,
                value: Value::coin_only(1_000_000),
                datum: None,
                script_ref: None,
            },
        };

        assert!(entry.extract_stake_credential().is_none());
    }
}
