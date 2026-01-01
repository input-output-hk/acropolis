// ================================================================================================
// Mark, Set, Go Snapshots - CBOR Parsing Support
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
    /// Parse a single snapshot (Mark, Set, or Go) from CBOR
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

    /// Convert this raw snapshot to a processed EpochSnapshot with registration checking
    pub fn into_snapshot_with_registration_check(
        self,
        epoch: u64,
        block_counts: &HashMap<PoolId, usize>,
        pots: Pots,
        network: NetworkId,
        two_previous_snapshot: Option<&EpochSnapshot>,
        registered_credentials: Option<&std::collections::HashSet<StakeCredential>>,
    ) -> EpochSnapshot {
        let stake_map: HashMap<_, _> = self.snapshot_stake.0.into_iter().collect();
        let delegation_map: HashMap<_, _> = self.snapshot_delegations.0.into_iter().collect();
        let pool_params_map: HashMap<_, _> = self.snapshot_pool_params.0.into_iter().collect();

        EpochSnapshot::from_raw_with_registration_check(
            epoch,
            stake_map,
            delegation_map,
            pool_params_map,
            block_counts,
            pots,
            network,
            two_previous_snapshot,
            registered_credentials,
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
    /// Go snapshot (raw CBOR data)
    pub go: RawSnapshot,
}

impl RawSnapshotsContainer {
    /// Convert raw snapshots to processed SnapshotsContainer
    ///
    /// The CBOR file stores snapshots in order: [Mark, Set, Go, Fee]
    ///
    /// IMPORTANT: The snapshots in the CBOR are taken at epoch BOUNDARIES, not at the
    /// current epoch. For a Mithril snapshot captured during epoch N:
    /// - Mark contains stake distribution from end of epoch N-1 (snapshot taken at N-1→N boundary)
    /// - Set contains stake distribution from end of epoch N-2 (snapshot taken at N-2→N-1 boundary)
    /// - Go contains stake distribution from end of epoch N-3 (snapshot taken at N-3→N-2 boundary)
    ///
    /// In Cardano terminology for rewards calculation:
    /// - Mark = newest snapshot (epoch N-1) - used as "performance" snapshot (blocks_produced)
    /// - Set = middle snapshot (epoch N-2)
    /// - Go = oldest snapshot (epoch N-3) - used as "staking" snapshot for rewards
    ///
    /// Block count assignments:
    /// - Mark (epoch N-1): Uses blocks_previous_epoch (blocks produced in epoch N-1)
    /// - Set (epoch N-2): Uses blocks_current_epoch (actually from epoch before that)
    /// - Go (epoch N-3): No block data available during bootstrap
    ///
    /// Pots assignment:
    /// - Mark: receives the pots from the bootstrap file (most recent)
    /// - Set and Go: receive zeroed pots (we don't have historical pots)
    ///
    /// Note on `two_previous_reward_account_is_registered`:
    /// For bootstrap snapshots, we don't have historical registration data to properly
    /// determine this flag. Therefore, we default to `true` for all SPOs - the conservative
    /// approach that pays rewards when we can't verify registration status.
    pub fn into_snapshots_container(
        self,
        epoch: u64,
        blocks_previous_epoch: &HashMap<PoolId, usize>,
        blocks_current_epoch: &HashMap<PoolId, usize>,
        pots: Pots,
        network: NetworkId,
    ) -> SnapshotsContainer {
        let empty_blocks = HashMap::new();

        // Epoch assignments - snapshots are from epoch boundaries BEFORE the current epoch:
        // - Mark = epoch - 1 (newest, has blocks from previous epoch)
        // - Set = epoch - 2
        // - Go = epoch - 3 (oldest, used for staking in rewards calculation)
        let mark = self.mark.into_snapshot(
            epoch.saturating_sub(1),
            blocks_previous_epoch,
            pots,
            network.clone(),
        );

        let set = self.set.into_snapshot(
            epoch.saturating_sub(2),
            blocks_current_epoch,
            Pots::default(),
            network.clone(),
        );

        let go = self.go.into_snapshot(
            epoch.saturating_sub(3),
            &empty_blocks,
            Pots::default(),
            network,
        );

        SnapshotsContainer { mark, set, go }
    }

    /// Convert raw snapshots to processed SnapshotsContainer with registration checking.
    ///
    /// This version takes a set of registered credentials from DState to properly
    /// determine `two_previous_reward_account_is_registered` for each SPO.
    ///
    /// The snapshots are created in order from oldest to newest so that each can
    /// reference the previous one for registration checking:
    /// 1. Go (epoch-3) - oldest, no previous snapshot available
    /// 2. Set (epoch-2) - uses Go as two_previous
    /// 3. Mark (epoch-1) - uses Set as two_previous
    #[allow(clippy::too_many_arguments)]
    pub fn into_snapshots_container_with_registration_check(
        self,
        epoch: u64,
        blocks_previous_epoch: &HashMap<PoolId, usize>,
        blocks_current_epoch: &HashMap<PoolId, usize>,
        pots: Pots,
        network: NetworkId,
        registered_credentials: Option<&std::collections::HashSet<StakeCredential>>,
    ) -> SnapshotsContainer {
        let empty_blocks = HashMap::new();

        // Create snapshots from oldest to newest so each can reference the previous
        // for two_previous_reward_account_is_registered checking.

        // Go (epoch-3) - oldest, no previous snapshot to reference
        let go = self.go.into_snapshot_with_registration_check(
            epoch.saturating_sub(3),
            &empty_blocks,
            Pots::default(),
            network.clone(),
            None, // No previous snapshot for Go
            registered_credentials,
        );

        // Set (epoch-2) - uses Go as reference for two_previous check
        let set = self.set.into_snapshot_with_registration_check(
            epoch.saturating_sub(2),
            blocks_current_epoch,
            Pots::default(),
            network.clone(),
            Some(&go),
            registered_credentials,
        );

        // Mark (epoch-1) - uses Set as reference for two_previous check
        let mark = self.mark.into_snapshot_with_registration_check(
            epoch.saturating_sub(1),
            blocks_previous_epoch,
            pots,
            network,
            Some(&set),
            registered_credentials,
        );

        SnapshotsContainer { mark, set, go }
    }
}

/// Callback trait for mark, set, go snapshots
pub trait SnapshotsCallback {
    fn on_snapshots(&mut self, snapshots: SnapshotsContainer) -> Result<()>;
}
