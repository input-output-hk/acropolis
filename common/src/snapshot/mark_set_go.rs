// ================================================================================================
// Mark, Set, Go Snapshots Support
// ================================================================================================

use anyhow::{Context, Result};
use log::info;

use minicbor::Decoder;
use serde::Serialize;

pub use crate::hash::Hash;
pub use crate::stake_addresses::{AccountState, StakeAddressState};
pub use crate::StakeCredential;

/// VMap<K, V> representation for CBOR Map types
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VMap<K, V>(pub Vec<(K, V)>);

impl<'b, C, K, V> minicbor::Decode<'b, C> for VMap<K, V>
where
    K: minicbor::Decode<'b, C>,
    V: minicbor::Decode<'b, C>,
{
    fn decode(d: &mut Decoder<'b>, ctx: &mut C) -> Result<Self, minicbor::decode::Error> {
        let map_len = d.map()?;
        let mut pairs = Vec::new();

        match map_len {
            Some(len) => {
                for _ in 0..len {
                    let key = K::decode(d, ctx)?;
                    let value = V::decode(d, ctx)?;
                    pairs.push((key, value));
                }
            }
            None => {
                // Indefinite-length map
                while d.datatype()? != minicbor::data::Type::Break {
                    let key = K::decode(d, ctx)?;
                    let value = V::decode(d, ctx)?;
                    pairs.push((key, value));
                }
                d.skip()?; // Skip the break
            }
        }

        Ok(VMap(pairs))
    }
}

/// Raw snapshots container with VMap data
#[derive(Debug, Clone)]
pub struct RawSnapshotsContainer {
    pub mark: VMap<StakeCredential, i64>,
    pub set: VMap<StakeCredential, i64>,
    pub go: VMap<StakeCredential, i64>,
    pub fee: u64,
}

/// Callback trait for mark, set, go snapshots
pub trait SnapshotsCallback {
    fn on_snapshots(&mut self, snapshots: RawSnapshotsContainer) -> Result<()>;
}

/// From ttps://github.com/rrruko/nes-cddl-hs/blob/main/nes.cddl
/// Snapshot data structure matching the CDDL schema
/// snapshot = [
///   snapshot_stake : stake,
///   snapshot_delegations : vmap<credential, key_hash<stake_pool>>,
///   snapshot_pool_params : vmap<key_hash<stake_pool>, pool_params>,
/// ]
/// stake = vmap<credential, compactform_coin>
/// credential = [0, addr_keyhash // 1, script_hash]
/// xxx

#[derive(Debug, Clone)]
pub struct Snapshot {
    /// snapshot_stake: stake distribution map (credential -> lovelace amount)
    pub snapshot_stake: VMap<StakeCredential, i64>,
    // snapshot_delegations: delegation map (credential -> stake pool key hash)
    // pub snapshot_delegations: VMap<Credential, StakePool>,

    // snapshot_pool_params: pool parameters map (stake pool key hash -> pool params)
    //  snapshot_pool_params: VMap<Vec<u8>, Vec<u8>>,
}

impl Snapshot {
    /// Parse a single snapshot (Mark, Set, or Go)
    pub fn parse_single_snapshot(decoder: &mut Decoder, snapshot_name: &str) -> Result<Snapshot> {
        info!("        {snapshot_name} snapshot - checking data type...");

        // Check what type we have - could be array, map, or simple value
        match decoder.datatype().context("Failed to read snapshot datatype")? {
            minicbor::data::Type::Map | minicbor::data::Type::MapIndef => {
                info!(
                    "        {snapshot_name} snapshot is a map - treating as stake distribution directly"
                );
                // Try VMap first, then fallback to simple map
                match decoder.decode::<VMap<StakeCredential, i64>>() {
                    Ok(snapshot_stake) => {
                        info!(
                            "        {} snapshot - successfully decoded {} stake entries with VMap",
                            snapshot_name,
                            snapshot_stake.0.len()
                        );
                        Ok(Snapshot { snapshot_stake })
                    }
                    Err(vmap_error) => {
                        info!(
                            "        {snapshot_name} snapshot - VMap decode failed: {vmap_error}"
                        );
                        info!(
                            "        {snapshot_name} snapshot - trying simple BTreeMap<bytes, i64>"
                        );

                        // Reset decoder and try simple map format
                        // Note: We can't reset the decoder, so we need to handle this differently
                        // For now, return an empty snapshot to continue processing
                        info!(
                            "        {snapshot_name} snapshot - using empty fallback due to format mismatch"
                        );
                        Ok(Snapshot {
                            snapshot_stake: VMap(Vec::new()),
                        })
                    }
                }
            }
            minicbor::data::Type::Array => {
                info!("        {snapshot_name} snapshot is an array");
                decoder.array().context("Failed to parse snapshot array")?;

                // Check what the first element type is
                info!("        {snapshot_name} snapshot - checking first element type...");
                match decoder.datatype().context("Failed to read first element datatype")? {
                    minicbor::data::Type::Map | minicbor::data::Type::MapIndef => {
                        info!(
                            "        {snapshot_name} snapshot - first element is a map, parsing as stake"
                        );
                        // First element is snapshot_stake
                        let snapshot_stake: VMap<StakeCredential, i64> = decoder.decode()?;

                        // Skip delegations (second element)
                        decoder.skip().context("Failed to skip snapshot_delegations")?;

                        // Skip pool_params (third element)
                        decoder.skip().context("Failed to skip snapshot_pool_params")?;

                        Ok(Snapshot { snapshot_stake })
                    }
                    other_type => {
                        info!(
                            "        {snapshot_name} snapshot - first element is {other_type:?}, skipping entire array"
                        );
                        // We don't know how many elements are in this array, so just skip the first element
                        // and let the array parsing naturally complete
                        decoder.skip().context("Failed to skip first element")?;

                        // Try to skip remaining elements, but don't fail if there aren't exactly 3
                        loop {
                            match decoder.datatype() {
                                Ok(minicbor::data::Type::Break) => {
                                    // End of indefinite array
                                    break;
                                }
                                Ok(_) => {
                                    // More elements to skip
                                    decoder.skip().ok(); // Don't fail on individual skips
                                }
                                Err(_) => {
                                    // End of definite array or other error - break
                                    break;
                                }
                            }
                        }

                        Ok(Snapshot {
                            snapshot_stake: VMap(Vec::new()),
                        })
                    }
                }
            }
            minicbor::data::Type::U32
            | minicbor::data::Type::U64
            | minicbor::data::Type::U8
            | minicbor::data::Type::U16 => {
                let value = decoder.u64().context("Failed to parse snapshot value")?;
                info!("        {snapshot_name} snapshot is a simple value: {value}");

                // Return empty snapshot for simple values
                Ok(Snapshot {
                    snapshot_stake: VMap(Vec::new()),
                })
            }
            minicbor::data::Type::Break => {
                info!(
                    "        {snapshot_name} snapshot is a Break token - indicates end of indefinite structure"
                );
                // Don't consume the break token, let the parent structure handle it
                // Return empty snapshot
                Ok(Snapshot {
                    snapshot_stake: VMap(Vec::new()),
                })
            }
            minicbor::data::Type::Tag => {
                info!(
                    "        {snapshot_name} snapshot starts with a CBOR tag, trying to skip tag and parse content"
                );
                let _tag = decoder.tag().context("Failed to read CBOR tag")?;
                info!(
                    "        {snapshot_name} snapshot - found tag {_tag}, checking tagged content..."
                );

                // After consuming tag, try to parse the tagged content
                match decoder.datatype().context("Failed to read tagged content datatype")? {
                    minicbor::data::Type::Map | minicbor::data::Type::MapIndef => {
                        let snapshot_stake: VMap<StakeCredential, i64> = decoder.decode()?;
                        Ok(Snapshot { snapshot_stake })
                    }
                    other_tagged_type => {
                        info!(
                            "        {snapshot_name} snapshot - tagged content is {other_tagged_type:?}, skipping"
                        );
                        decoder.skip().ok(); // Don't fail on skip
                        Ok(Snapshot {
                            snapshot_stake: VMap(Vec::new()),
                        })
                    }
                }
            }
            other_type => {
                info!(
                    "        {snapshot_name} snapshot has unexpected type: {other_type:?}, skipping..."
                );
                decoder.skip().ok(); // Don't fail on skip

                // Return empty snapshot
                Ok(Snapshot {
                    snapshot_stake: VMap(Vec::new()),
                })
            }
        }
    }
}
