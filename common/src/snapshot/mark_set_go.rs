//! Mark, Set, Go Snapshots Support
//!
//! Handles parsing and processing of Cardano's mark/set/go stake snapshots:
//! - CBOR parsing: `VMap`, `Snapshot`, `ParsedSnapshotsContainer`
//! - Processed bootstrap types: `BootstrapSnapshot`, `BootstrapSnapshots`
use anyhow::{Context, Error, Result};
use log::info;
use std::collections::HashMap;

use minicbor::Decoder;
use serde::Serialize;

use crate::snapshot::streaming_snapshot::{SnapshotContext, SnapshotPoolRegistration};
use crate::{Lovelace, NetworkId, PoolId, PoolRegistration, Ratio, StakeAddress, StakeCredential};

/// Generic CBOR Map type represented as a vector of key-value pairs
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
                d.skip()?;
            }
        }

        Ok(VMap(pairs))
    }
}

/// Snapshot data structure matching the CDDL schema from
/// https://github.com/rrruko/nes-cddl-hs/blob/main/nes.cddl
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub snapshot_stake: VMap<StakeCredential, i64>,
    pub snapshot_delegations: VMap<StakeCredential, PoolId>,
    pub snapshot_pool_params: VMap<PoolId, PoolRegistration>,
}

impl Snapshot {
    pub fn parse_single_snapshot(decoder: &mut Decoder, snapshot_name: &str) -> Result<Snapshot> {
        Self::parse_single_snapshot_with_network(decoder, snapshot_name, NetworkId::Mainnet)
    }

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
                        let snapshot_delegations: VMap<StakeCredential, PoolId> =
                            decoder.decode().context("Failed to parse snapshot_delegations")?;

                        let mut ctx = SnapshotContext::new(network);
                        let snapshot_pool_params =
                            Self::parse_pool_params_map(decoder, &mut ctx)
                                .context("Failed to parse snapshot_pool_params")?;

                        info!("Parsed {snapshot_name} snapshot successfully");

                        Ok(Snapshot {
                            snapshot_stake,
                            snapshot_delegations,
                            snapshot_pool_params,
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
                decoder.skip()?;
            }
        }
        Ok(VMap(pairs))
    }
}

/// Raw snapshots container with just stake VMap data (for callbacks/logging)
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct RawSnapshotsContainer {
    pub mark: VMap<StakeCredential, i64>,
    pub set: VMap<StakeCredential, i64>,
    pub go: VMap<StakeCredential, i64>,
    pub fee: u64,
}

/// Callback trait for receiving raw snapshot data during parsing
pub trait SnapshotsCallback {
    fn on_snapshots(&mut self, snapshots: RawSnapshotsContainer) -> Result<()>;
}

/// Full parsed snapshots container with delegations and pool params
#[derive(Debug, Clone)]
pub struct ParsedSnapshotsContainer {
    pub mark: Snapshot,
    pub set: Snapshot,
    pub go: Snapshot,
    pub fee: u64,
}

impl ParsedSnapshotsContainer {
    /// Convert to raw container (this is just for callbacks that only need stake data)
    pub fn to_raw(&self) -> RawSnapshotsContainer {
        RawSnapshotsContainer {
            mark: self.mark.snapshot_stake.clone(),
            set: self.set.snapshot_stake.clone(),
            go: self.go.snapshot_stake.clone(),
            fee: self.fee,
        }
    }
}

/// SPO data within a bootstrap snapshot
#[derive(Debug, Clone, Default, Serialize, serde::Deserialize)]
pub struct BootstrapSnapshotSPO {
    pub delegators: Vec<(StakeAddress, Lovelace)>,
    pub total_stake: Lovelace,
    pub pledge: Lovelace,
    pub cost: Lovelace,
    pub margin: Ratio,
    pub reward_account: StakeAddress,
    pub pool_owners: Vec<StakeAddress>,
}

/// Pre-processed snapshot ready for accounts_state consumption
#[derive(Debug, Clone, Default, Serialize, serde::Deserialize)]
pub struct BootstrapSnapshot {
    pub epoch: u64,
    pub spos: HashMap<PoolId, BootstrapSnapshotSPO>,
    pub total_stake: Lovelace,
}

/// Container for mark/set/go bootstrap snapshots
#[derive(Debug, Clone, Default, Serialize, serde::Deserialize)]
pub struct BootstrapSnapshots {
    /// Epoch N-2
    pub mark: BootstrapSnapshot,
    /// Epoch N-1
    pub set: BootstrapSnapshot,
    /// Epoch N
    pub go: BootstrapSnapshot,
    pub fee: u64,
}

impl BootstrapSnapshots {
    /// Build bootstrap snapshots from fully parsed snapshot data.
    /// Uses the historical delegations and pool params from each snapshot.
    pub fn from_parsed(epoch: u64, parsed: ParsedSnapshotsContainer) -> Self {
        let mark = Self::build_snapshot(epoch.saturating_sub(2), &parsed.mark);
        let set = Self::build_snapshot(epoch.saturating_sub(1), &parsed.set);
        let go = Self::build_snapshot(epoch.saturating_sub(0), &parsed.go);

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
            fee: parsed.fee,
        }
    }

    fn build_snapshot(epoch: u64, parsed: &Snapshot) -> BootstrapSnapshot {
        let mut snapshot = BootstrapSnapshot {
            epoch,
            spos: HashMap::new(),
            total_stake: 0,
        };

        let delegations: HashMap<StakeCredential, PoolId> =
            parsed.snapshot_delegations.0.iter().cloned().collect();

        for (pool_id, pool_reg) in &parsed.snapshot_pool_params.0 {
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

        for (credential, amount) in &parsed.snapshot_stake.0 {
            if *amount <= 0 {
                continue;
            }
            let stake = *amount as Lovelace;

            if let Some(pool_id) = delegations.get(credential) {
                if let Some(snap_spo) = snapshot.spos.get_mut(pool_id) {
                    let stake_address = StakeAddress::new(credential.clone(), NetworkId::Mainnet);
                    snap_spo.delegators.push((stake_address, stake));
                    snap_spo.total_stake += stake;
                    snapshot.total_stake += stake;
                }
            }
        }

        snapshot
    }
}
