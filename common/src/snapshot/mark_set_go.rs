//! Mark, Set, Go Snapshots Support

use anyhow::{Context, Error, Result};
use log::info;
use std::collections::HashMap;

use minicbor::Decoder;
use serde::Serialize;

pub use crate::hash::Hash;
use crate::snapshot::streaming_snapshot::{SnapshotContext, SnapshotPoolRegistration};
pub use crate::stake_addresses::{AccountState, StakeAddressState};
pub use crate::StakeCredential;
use crate::{Lovelace, NetworkId, PoolId, PoolRegistration, Ratio, StakeAddress};

/// VMap<K, V> representation for CBOR Map types
#[derive(Debug, Clone, PartialEq, Serialize, serde::Deserialize)]
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
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
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
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub snapshot_stake: VMap<StakeCredential, i64>,
    // snapshot_delegations: vmap<credential, key_hash<stake_pool>>,)
    pub snapshot_delegations: VMap<StakeCredential, PoolId>,
    // snapshot_pool_params: vmap<key_hash<stake_pool>, pool_params>,
    pub snapshot_pool_params: VMap<PoolId, PoolRegistration>,
}

impl Snapshot {
    /// Parse a single snapshot (Mark, Set, or Go)
    pub fn parse_single_snapshot(decoder: &mut Decoder, snapshot_name: &str) -> Result<Snapshot> {
        Self::parse_single_snapshot_with_network(decoder, snapshot_name, NetworkId::Mainnet)
    }

    /// Parse a single snapshot with a specific network ID
    pub fn parse_single_snapshot_with_network(
        decoder: &mut Decoder,
        snapshot_name: &str,
        network: NetworkId,
    ) -> Result<Snapshot> {
        match decoder.datatype().context("Failed to read snapshot datatype")? {
            minicbor::data::Type::Array => {
                decoder.array().context("Failed to parse snapshot array")?;

                match decoder.datatype().context("Failed to read first element datatype")? {
                    minicbor::data::Type::Map | minicbor::data::Type::MapIndef => {
                        let snapshot_stake: VMap<StakeCredential, i64> = decoder.decode()?;
                        let delegations: VMap<StakeCredential, PoolId> =
                            decoder.decode().context("Failed to parse snapshot_delegations")?;

                        let mut ctx = SnapshotContext::new(network);
                        let pool_params = parse_pool_params_map(decoder, &mut ctx)
                            .context("Failed to parse snapshot_pool_params")?;

                        info!("Parsed {snapshot_name} snapshot successfully");

                        Ok(Snapshot {
                            snapshot_stake,
                            snapshot_delegations: delegations,
                            snapshot_pool_params: pool_params,
                        })
                    }
                    other_type => Err(Error::msg(format!(
                        "{snapshot_name} snapshot: unexpected first element type: {other_type:?}"
                    ))),
                }
            }
            other_type => Err(Error::msg(format!(
                "{snapshot_name} snapshot: unexpected data type: {other_type:?}"
            ))),
        }
    }
}

/// SPO data for a bootstrap stake snapshot - serializable version for message passing
#[derive(Debug, Clone, Default, Serialize, serde::Deserialize)]
pub struct BootstrapSnapshotSPO {
    /// List of delegator stake addresses and amounts
    pub delegators: Vec<(StakeAddress, Lovelace)>,

    /// Total stake delegated
    pub total_stake: Lovelace,

    /// Pledge
    pub pledge: Lovelace,

    /// Fixed cost
    pub cost: Lovelace,

    /// Margin
    pub margin: Ratio,

    /// Reward account
    pub reward_account: StakeAddress,

    /// Pool owners
    pub pool_owners: Vec<StakeAddress>,
}

/// Bootstrap snapshot - serializable version for message passing
/// This is the pre-processed format ready for accounts_state to use
#[derive(Debug, Clone, Default, Serialize, serde::Deserialize)]
pub struct BootstrapSnapshot {
    /// Epoch this snapshot is for
    pub epoch: u64,

    /// Map of SPOs by operator ID with their delegators and stake
    pub spos: HashMap<PoolId, BootstrapSnapshotSPO>,

    /// Total stake across all SPOs
    pub total_stake: Lovelace,
}

/// Container for the three bootstrap snapshots (mark, set, go)
#[derive(Debug, Clone, Default, Serialize, serde::Deserialize)]
pub struct BootstrapSnapshots {
    /// Mark snapshot (epoch N-1)
    pub mark: BootstrapSnapshot,

    /// Set snapshot (epoch N-2)
    pub set: BootstrapSnapshot,

    /// Go snapshot (epoch N-3)
    pub go: BootstrapSnapshot,

    /// Fee from snapshots
    pub fee: u64,
}

impl BootstrapSnapshots {
    /// Build bootstrap snapshots from raw snapshot data, accounts, and pools
    ///
    /// # Arguments
    /// * `epoch` - Current epoch we're bootstrapping into
    /// * `raw_snapshots` - Raw stake distributions from CBOR parsing
    /// * `accounts` - Account states with delegation info
    /// * `pools` - Pool registrations
    pub fn from_raw(
        epoch: u64,
        raw_snapshots: RawSnapshotsContainer,
        accounts: &[AccountState],
        pools: &[PoolRegistration],
    ) -> Self {
        // Build delegation map: stake credential -> pool id
        let delegations: HashMap<StakeCredential, PoolId> = accounts
            .iter()
            .filter_map(|account| {
                account
                    .address_state
                    .delegated_spo
                    .map(|pool_id| (account.stake_address.credential.clone(), pool_id))
            })
            .collect();

        // Build pool map for quick lookup
        let pool_map: HashMap<PoolId, &PoolRegistration> =
            pools.iter().map(|p| (p.operator, p)).collect();

        // Build the three snapshots
        let mark = Self::build_snapshot(
            epoch.saturating_sub(1),
            raw_snapshots.mark,
            &delegations,
            &pool_map,
        );

        let set = Self::build_snapshot(
            epoch.saturating_sub(2),
            raw_snapshots.set,
            &delegations,
            &pool_map,
        );

        let go = Self::build_snapshot(
            epoch.saturating_sub(3),
            raw_snapshots.go,
            &delegations,
            &pool_map,
        );

        info!(
            "Built bootstrap snapshots: mark(epoch {}, {} SPOs, {} stake), \
             set(epoch {}, {} SPOs, {} stake), go(epoch {}, {} SPOs, {} stake)",
            mark.epoch,
            mark.spos.len(),
            mark.total_stake,
            set.epoch,
            set.spos.len(),
            set.total_stake,
            go.epoch,
            go.spos.len(),
            go.total_stake
        );

        Self {
            mark,
            set,
            go,
            fee: raw_snapshots.fee,
        }
    }

    fn build_snapshot(
        epoch: u64,
        stake_map: VMap<StakeCredential, i64>,
        delegations: &HashMap<StakeCredential, PoolId>,
        pools: &HashMap<PoolId, &PoolRegistration>,
    ) -> BootstrapSnapshot {
        let mut snapshot = BootstrapSnapshot {
            epoch,
            spos: HashMap::new(),
            total_stake: 0,
        };

        // Initialize SPOs from pool registrations
        for (pool_id, pool_reg) in pools {
            snapshot.spos.insert(
                *pool_id,
                BootstrapSnapshotSPO {
                    delegators: Vec::new(),
                    total_stake: 0,
                    pledge: pool_reg.pledge,
                    cost: pool_reg.cost,
                    margin: pool_reg.margin.clone(),
                    pool_owners: pool_reg.pool_owners.clone(),
                    reward_account: pool_reg.reward_account.clone(),
                },
            );
        }

        // Build delegator lists from stake map + delegation info
        for (credential, amount) in stake_map.0 {
            if amount <= 0 {
                continue;
            }
            let stake = amount as Lovelace;

            if let Some(pool_id) = delegations.get(&credential) {
                if let Some(snap_spo) = snapshot.spos.get_mut(pool_id) {
                    let stake_address = StakeAddress::new(credential, NetworkId::Mainnet);
                    snap_spo.delegators.push((stake_address, stake));
                    snap_spo.total_stake += stake;
                    snapshot.total_stake += stake;
                }
            }
        }

        snapshot
    }
}

/// Parse pool params map using SnapshotPoolRegistration
fn parse_pool_params_map(
    decoder: &mut Decoder,
    ctx: &mut SnapshotContext,
) -> Result<VMap<PoolId, PoolRegistration>, minicbor::decode::Error> {
    let map_len = decoder.map()?;
    let mut pairs = Vec::new();

    match map_len {
        Some(len) => {
            for _ in 0..len {
                let key: PoolId = decoder.decode_with(ctx)?;
                let value: SnapshotPoolRegistration = decoder.decode_with(ctx)?;
                pairs.push((key, value.0));
            }
        }
        None => {
            // Indefinite-length map
            while decoder.datatype()? != minicbor::data::Type::Break {
                let key: PoolId = decoder.decode_with(ctx)?;
                let value: SnapshotPoolRegistration = decoder.decode_with(ctx)?;
                pairs.push((key, value.0));
            }
            decoder.skip()?; // Skip the break
        }
    }
    Ok(VMap(pairs))
}
