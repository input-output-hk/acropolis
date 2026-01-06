// ================================================================================================
// Mark and Set Snapshots - CBOR Parsing Support
// ================================================================================================

use anyhow::{Context, Error, Result};
use log::info;

use minicbor::Decoder;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::epoch_snapshot::{EpochSnapshot, SnapshotsContainer};
pub use crate::hash::Hash;
use crate::snapshot::streaming_snapshot::SnapshotContext;
use crate::snapshot::streaming_snapshot::SnapshotPoolRegistration;
use crate::{NetworkId, PoolId, PoolRegistration, Pots, StakeCredential};

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

/// Raw snapshot from CBOR parsing (before processing into final Snapshot format)
/// From https://github.com/rrruko/nes-cddl-hs/blob/main/nes.cddl
/// snapshot = [
///   snapshot_stake : stake,
///   snapshot_delegations : vmap<credential, key_hash<stake_pool>>,
///   snapshot_pool_params : vmap<key_hash<stake_pool>, pool_params>,
/// ]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RawSnapshot {
    /// snapshot_stake: stake distribution map (credential -> lovelace amount)
    pub snapshot_stake: VMap<StakeCredential, i64>,
    /// snapshot_delegations: vmap<credential, key_hash<stake_pool>>
    pub snapshot_delegations: VMap<StakeCredential, PoolId>,
    /// snapshot_pool_params: vmap<key_hash<stake_pool>, pool_params>
    pub snapshot_pool_params: VMap<PoolId, PoolRegistration>,
}

impl RawSnapshot {
    /// Parse a single snapshot (Mark or Set) from CBOR
    pub fn parse(
        decoder: &mut Decoder,
        ctx: &mut SnapshotContext,
        snapshot_name: &str,
    ) -> Result<RawSnapshot> {
        info!("Parsing snapshot {snapshot_name}");
        match decoder.datatype().context("Failed to read snapshot datatype")? {
            minicbor::data::Type::Array => {
                decoder.array().context("Failed to parse snapshot array")?;
                match decoder.datatype().context("Failed to read first element datatype")? {
                    minicbor::data::Type::Map | minicbor::data::Type::MapIndef => {
                        let snapshot_stake: VMap<StakeCredential, i64> = decoder.decode()?;

                        let delegations: VMap<StakeCredential, PoolId> =
                            decoder.decode().context("Failed to parse snapshot_delegations")?;

                        let pools: VMap<PoolId, SnapshotPoolRegistration> = decoder
                            .decode_with(ctx)
                            .context("Failed to parse snapshot_pool_registration")?;
                        Ok(RawSnapshot {
                            snapshot_stake,
                            snapshot_delegations: delegations,
                            snapshot_pool_params: VMap(
                                pools.0.into_iter().map(|(k, v)| (k, v.0)).collect(),
                            ),
                        })
                    }
                    other_type => Err(Error::msg(format!(
                        "Unexpected first element type in snapshot array: {other_type:?}"
                    ))),
                }
            }
            other_type => Err(Error::msg(format!(
                "Unexpected snapshot data type: {other_type:?}"
            ))),
        }
    }

    /// Convert this raw snapshot to a processed EpochSnapshot
    pub fn into_snapshot(
        self,
        epoch: u64,
        block_counts: &HashMap<PoolId, usize>,
        pots: Pots,
        network: NetworkId,
    ) -> EpochSnapshot {
        let stake_map: HashMap<_, _> = self.snapshot_stake.0.into_iter().collect();
        let delegation_map: HashMap<_, _> = self.snapshot_delegations.0.into_iter().collect();
        let pool_params_map: HashMap<_, _> = self.snapshot_pool_params.0.into_iter().collect();

        EpochSnapshot::from_raw(
            epoch,
            stake_map,
            delegation_map,
            pool_params_map,
            block_counts,
            pots,
            network,
        )
    }
}

/// Raw snapshots container from CBOR parsing
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RawSnapshotsContainer {
    /// Mark snapshot (raw CBOR data)
    pub mark: RawSnapshot,
    /// Set snapshot (raw CBOR data)
    pub set: RawSnapshot,
}

impl RawSnapshotsContainer {
    /// Convert raw snapshots to processed SnapshotsContainer
    ///
    /// Block count assignments:
    /// - Mark (epoch): Uses blocks_current_epoch
    /// - Set (epoch-1): Uses blocks_previous_epoch
    ///
    /// Pots assignment (reserves, treasury, deposits - the global ADA accounting pots):
    /// - Mark and Set: receive zeroed pots (Live pots in accounts state are used for rewards calculation)
    ///
    /// Why this is safe: On the first epoch after bootstrap, we skip monetary change
    /// calculation (pots are already correct from bootstrap). The first `enter_epoch`
    /// creates a fresh snapshot with correct pots that becomes the new mark. Subsequent
    /// epochs will have correct pots propagated through the snapshot rotation.
    pub fn into_snapshots_container(
        self,
        epoch: u64,
        blocks_previous_epoch: &HashMap<PoolId, usize>,
        blocks_current_epoch: &HashMap<PoolId, usize>,
        network: NetworkId,
    ) -> SnapshotsContainer {
        SnapshotsContainer {
            mark: self.mark.into_snapshot(
                epoch,
                blocks_current_epoch,
                Pots::default(),
                network.clone(),
            ),
            set: self.set.into_snapshot(
                epoch.saturating_sub(1),
                blocks_previous_epoch,
                Pots::default(),
                network.clone(),
            ),
        }
    }
}

/// Callback trait for mark and set snapshots
pub trait SnapshotsCallback {
    fn on_snapshots(&mut self, snapshots: SnapshotsContainer) -> Result<()>;
}
