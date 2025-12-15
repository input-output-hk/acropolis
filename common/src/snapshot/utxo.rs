// SPDX-License-Identifier: Apache-2.0
// Copyright © 2025, Acropolis team.

//! UTXO parsing types for snapshot processing.
//!
//! This module provides CBOR decoding implementations for UTXO-related types
//! used in streaming snapshot parsing.

use minicbor::data::Type;
use minicbor::Decoder;
use serde::{Deserialize, Serialize};

use crate::{
    Address, ByronAddress, NativeAssets, ShelleyAddress, StakeAddress, TxHash, UTXOValue,
    UTxOIdentifier, Value,
};

/// UTXO entry with transaction hash, index, address, and value
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UtxoEntry {
    /// UTxO Identifier
    pub id: UTxOIdentifier,
    /// UTxO Value
    pub value: UTXOValue,
}

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

struct SnapshotUTxOIdentifier(pub UTxOIdentifier);

impl<'b, C> minicbor::Decode<'b, C> for SnapshotUTxOIdentifier {
    fn decode(d: &mut Decoder<'b>, _: &mut C) -> Result<Self, minicbor::decode::Error> {
        let Ok(tx_hash) = TxHash::try_from(d.bytes()?) else {
            return Err(minicbor::decode::Error::message(
                "Invalid TxHash (wrong size?)",
            ));
        };
        let output_index = d.u64()? as u16;
        Ok(Self(UTxOIdentifier {
            tx_hash,
            output_index,
        }))
    }
}

struct SnapshotAddress(pub Address);

impl<'b, C> minicbor::Decode<'b, C> for SnapshotAddress {
    fn decode(d: &mut Decoder<'b>, _: &mut C) -> Result<Self, minicbor::decode::Error> {
        let Ok(bytes) = d.bytes() else {
            return Err(minicbor::decode::Error::message(
                "Failed to read address bytes",
            ));
        };
        if bytes.is_empty() {
            return Err(minicbor::decode::Error::message("Empty utxo address"));
        }
        match bytes[0] {
            // Looks like CBOR, so should be Byron
            0x82 => {
                let mut dec = minicbor::Decoder::new(bytes);
                let Ok(byron) = ByronAddress::from_cbor(&mut dec) else {
                    return Err(minicbor::decode::Error::message(
                        "Failed to read Byron address",
                    ));
                };
                Ok(Self(Address::Byron(byron)))
            }
            // Everything else should be Shelley
            _ => {
                match (bytes[0] >> 4) & 0x0F {
                    // Stake addresses
                    0b1110 | 0b1111 => {
                        let Ok(stake) = StakeAddress::from_binary(bytes) else {
                            return Err(minicbor::decode::Error::message(
                                "Failed to read stake address",
                            ));
                        };
                        Ok(Self(Address::Stake(stake)))
                    }
                    // Other Shelley addresses
                    _ => {
                        let Ok(shelley) = ShelleyAddress::from_bytes_key(bytes) else {
                            return Err(minicbor::decode::Error::message(
                                "Failed to read Shelley address",
                            ));
                        };
                        Ok(Self(Address::Shelley(shelley)))
                    }
                }
            }
        }
    }
}

struct SnapshotValue(pub Value);

impl<'b, C> minicbor::Decode<'b, C> for SnapshotValue {
    fn decode(d: &mut Decoder<'b>, _: &mut C) -> Result<Self, minicbor::decode::Error> {
        let Ok(datatype) = d.datatype() else {
            return Err(minicbor::decode::Error::message(
                "Failed to read Value datatype",
            ));
        };
        match datatype {
            Type::U8 | Type::U16 | Type::U32 | Type::U64 => {
                // Simple ADA-only value
                let Ok(lovelace) = d.u64() else {
                    return Err(minicbor::decode::Error::message(
                        "Failed to parse coin amount",
                    ));
                };
                Ok(Self(Value {
                    lovelace,
                    assets: NativeAssets::default(),
                }))
            }
            Type::Array | Type::ArrayIndef => {
                // Multi-asset: [coin, assets_map]
                if d.array().is_err() {
                    return Err(minicbor::decode::Error::message(
                        "Failed to parse value array",
                    ));
                }
                let Ok(lovelace) = d.u64() else {
                    return Err(minicbor::decode::Error::message(
                        "Failed to parse coin amount",
                    ));
                };
                // TODO: read assets map
                if d.skip().is_err() {
                    return Err(minicbor::decode::Error::message(
                        "Failed to skip assets map",
                    ));
                }
                Ok(Self(Value {
                    lovelace,
                    assets: NativeAssets::default(),
                }))
            }
            _ => Err(minicbor::decode::Error::message(
                "Unexpected Value datatype",
            )),
        }
    }
}

struct SnapshotUTXOValue(pub UTXOValue);

impl<'b, C> minicbor::Decode<'b, C> for SnapshotUTXOValue {
    fn decode(d: &mut Decoder<'b>, _: &mut C) -> Result<Self, minicbor::decode::Error> {
        // TxOut is typically an array [address, value, ...]
        // or a map for Conway with optional fields

        let Ok(datatype) = d.datatype() else {
            return Err(minicbor::decode::Error::message(
                "Failed to read TxOut datatype",
            ));
        };

        // Try array format first (most common)
        match datatype {
            Type::Array | Type::ArrayIndef => {
                let Ok(arr_len) = d.array() else {
                    return Err(minicbor::decode::Error::message(
                        "Failed to parse TxOut array",
                    ));
                };
                if arr_len == Some(0) {
                    return Err(minicbor::decode::Error::message("empty TxOut array"));
                }

                // Element 0: Address (bytes)
                let address: SnapshotAddress = d.decode()?;

                // Element 1: Value (coin or map)
                let value: SnapshotValue = d.decode()?;

                // Skip remaining fields (datum, script_ref)
                // TODO: Read datum, script ref
                if let Some(len) = arr_len {
                    for _ in 2..len {
                        if d.skip().is_err() {
                            return Err(minicbor::decode::Error::message(
                                "Failed to skip TxOut field",
                            ));
                        }
                    }
                }

                Ok(Self(UTXOValue {
                    address: address.0,
                    value: value.0,
                    datum: None,
                    reference_script: None,
                }))
            }
            Type::Map | Type::MapIndef => {
                // Map format (Conway with optional fields)
                // Map keys: 0=address, 1=value, 2=datum, 3=script_ref
                let Ok(map_len) = d.map() else {
                    return Err(minicbor::decode::Error::message(
                        "Failed to parse TxOut map",
                    ));
                };

                let mut address = Option::<Address>::default();
                let mut value = Option::<Value>::default();

                let entries = map_len.unwrap_or(4); // Assume max 4 entries if indefinite
                for _ in 0..entries {
                    // Check for break in indefinite map
                    if map_len.is_none() && matches!(d.datatype(), Ok(Type::Break)) {
                        d.skip().ok(); // consume break
                        break;
                    }

                    // Read key
                    let key = match d.u32() {
                        Ok(k) => k,
                        Err(_) => {
                            // Skip both key and value if key is not u32
                            d.skip().ok();
                            d.skip().ok();
                            continue;
                        }
                    };

                    // Read value based on key
                    match key {
                        0 => {
                            address = Some((d.decode::<SnapshotAddress>()?).0);
                        }
                        1 => {
                            // Value (coin or multi-asset)
                            value = Some((d.decode::<SnapshotValue>()?).0);
                        }
                        _ => {
                            // TODO: read datum, script ref
                            // datum (2), script_ref (3), or unknown - skip
                            d.skip().ok();
                        }
                    }
                }

                if let (Some(address), Some(value)) = (address, value) {
                    Ok(Self(UTXOValue {
                        address,
                        value,
                        datum: None,
                        reference_script: None,
                    }))
                } else {
                    Err(minicbor::decode::Error::message(
                        "map-based TxOut missing required fields",
                    ))
                }
            }
            _ => Err(minicbor::decode::Error::message("unexpected TxOut type")),
        }
    }
}
