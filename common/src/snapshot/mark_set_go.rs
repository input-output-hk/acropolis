// ================================================================================================
// Mark, Set, Go Snapshots Support
// ================================================================================================

use anyhow::{Context, Error, Result};
use log::info;

use minicbor::Decoder;
use serde::{Deserialize, Serialize};

pub use crate::hash::Hash;
use crate::snapshot::streaming_snapshot::{SnapshotContext, SnapshotPoolRegistration};
pub use crate::stake_addresses::{AccountState, StakeAddressState};
pub use crate::StakeCredential;
use crate::{PoolId, PoolRegistration};

/// VMap<K, V> representation for CBOR Map types
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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
    // snapshot_delegations: vmap<credential, key_hash<stake_pool>>,)
    pub snapshot_delegations: VMap<StakeCredential, PoolId>,
    // snapshot_pool_params: vmap<key_hash<stake_pool>, pool_params>,
    pub snapshot_pool_params: VMap<PoolId, PoolRegistration>,
}

impl Snapshot {
    /// Parse a single snapshot (Mark, Set, or Go)
    pub fn parse_single_snapshot(
        decoder: &mut Decoder,
        ctx: &mut SnapshotContext,
        snapshot_name: &str,
    ) -> Result<Snapshot> {
        info!("        {snapshot_name} snapshot - checking data type...");

        // Check what type we have - could be array, map, or simple value
        match decoder.datatype().context("Failed to read snapshot datatype")? {
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
                        info!("        {snapshot_name} snapshot - parsing snapshot_delegations...");

                        let delegations: VMap<StakeCredential, PoolId> =
                            decoder.decode().context("Failed to parse snapshot_delegations")?;

                        info!(
                            "        {snapshot_name} snapshot - parsing snapshot_pool_registration..."
                        );

                        // pool_registration (third element)
                        let pools: VMap<PoolId, SnapshotPoolRegistration> = decoder
                            .decode_with(ctx)
                            .context("Failed to parse snapshot_pool_registration")?;

                        info!("        {snapshot_name} snapshot - parse completed successfully.");

                        Ok(Snapshot {
                            snapshot_stake,
                            snapshot_delegations: delegations,
                            snapshot_pool_params: VMap(
                                pools.0.into_iter().map(|(k, v)| (k, v.0)).collect(),
                            ),
                        })
                    }
                    other_type => {
                        info!(
                            "        {snapshot_name} snapshot - first element is {other_type:?}, skipping entire array"
                        );
                        Err(Error::msg(
                            "Unexpected first element type in snapshot array",
                        ))
                    }
                }
            }
            other_type => Err(Error::msg(format!(
                "Unexpected snapshot data type: {other_type:?}"
            ))),
        }
    }
}
