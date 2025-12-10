// ================================================================================================
// Mark, Set, Go Snapshots Support
// ================================================================================================

use anyhow::{Context, Error, Result};
use log::{error, info};

use minicbor::Decoder;
use serde::Serialize;

use types::Ratio;

pub use crate::hash::Hash;
use crate::snapshot::pool_params::PoolParams;
use crate::snapshot::streaming_snapshot;
pub use crate::stake_addresses::{AccountState, StakeAddressState};
use crate::PoolId;
pub use crate::StakeCredential;
use crate::{address::StakeAddress, types, NetworkId, PoolRegistration};

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
    // snapshot_delegations: vmap<credential, key_hash<stake_pool>>,)
    pub snapshot_delegations: VMap<StakeCredential, PoolId>,
    // snapshot_pool_params: vmap<key_hash<stake_pool>, pool_params>,
    pub snapshot_pool_params: VMap<PoolId, PoolRegistration>,
}

impl Snapshot {
    /// Parse a single snapshot (Mark, Set, or Go)
    pub fn parse_single_snapshot(decoder: &mut Decoder, snapshot_name: &str) -> Result<Snapshot> {
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
                        let pools: VMap<PoolId, PoolParams> = decoder
                            .decode()
                            .context("Failed to parse snapshot_pool_registration")?;
                        let registration = VMap(
                            pools
                                .0
                                .into_iter()
                                .map(|(pool_id, params)| {
                                    // Convert RewardAccount (Vec<u8>) to StakeAddress (arbitralily chosen over ScripHash)
                                    let reward_account =
                                        StakeAddress::from_binary(&params.reward_account.0)
                                            .unwrap_or_else(|_|
                                                {
                                                    error!("Failed to parse reward account for pool {pool_id}, using default");
                                                    StakeAddress::default()
                                                }
                                            );

                                    // Convert Set<AddrKeyhash> to Vec<StakeAddress>
                                    let pool_owners: Vec<StakeAddress> = params
                                        .owners
                                        .0
                                        .into_iter()
                                        .map(|keyhash| {
                                            StakeAddress::new(
                                                StakeCredential::AddrKeyHash(keyhash),
                                                NetworkId::Mainnet, // TODO: Make network configurable or get it from parameters
                                            )
                                        })
                                        .collect();

                                    // Convert Vec<streaming_snapshot::Relay> to Vec<types::Relay>
                                    let relays: Vec<types::Relay> = params
                                        .relays
                                        .into_iter()
                                        .map(|relay| match relay {
                                            streaming_snapshot::Relay::SingleHostAddr(
                                                port,
                                                ipv4,
                                                ipv6,
                                            ) => {
                                                let port_opt = match port {
                                                    streaming_snapshot::Nullable::Some(p) => {
                                                        Some(p as u16)
                                                    }
                                                    _ => None,
                                                };
                                                let ipv4_opt = match ipv4 {
                                                    streaming_snapshot::Nullable::Some(ip)
                                                        if ip.0.len() == 4 =>
                                                    {
                                                        Some(std::net::Ipv4Addr::new(
                                                            ip.0[0], ip.0[1], ip.0[2], ip.0[3],
                                                        ))
                                                    }
                                                    _ => None,
                                                };
                                                let ipv6_opt = match ipv6 {
                                                    streaming_snapshot::Nullable::Some(ip)
                                                        if ip.0.len() == 16 =>
                                                    {
                                                        let b = &ip.0;
                                                        Some(std::net::Ipv6Addr::from([
                                                            b[0], b[1], b[2], b[3], b[4], b[5],
                                                            b[6], b[7], b[8], b[9], b[10], b[11],
                                                            b[12], b[13], b[14], b[15],
                                                        ]))
                                                    }
                                                    _ => None,
                                                };
                                                types::Relay::SingleHostAddr(
                                                    types::SingleHostAddr {
                                                        port: port_opt,
                                                        ipv4: ipv4_opt,
                                                        ipv6: ipv6_opt,
                                                    },
                                                )
                                            }
                                            streaming_snapshot::Relay::SingleHostName(
                                                port,
                                                hostname,
                                            ) => {
                                                let port_opt = match port {
                                                    streaming_snapshot::Nullable::Some(p) => {
                                                        Some(p as u16)
                                                    }
                                                    _ => None,
                                                };
                                                types::Relay::SingleHostName(
                                                    types::SingleHostName {
                                                        port: port_opt,
                                                        dns_name: hostname,
                                                    },
                                                )
                                            }
                                            streaming_snapshot::Relay::MultiHostName(hostname) => {
                                                types::Relay::MultiHostName(types::MultiHostName {
                                                    dns_name: hostname,
                                                })
                                            }
                                        })
                                        .collect();

                                    // Convert Nullable<PoolMetadata> to Option<PoolMetadata>
                                    let pool_metadata = match params.metadata {
                                        streaming_snapshot::Nullable::Some(meta) => {
                                            Some(types::PoolMetadata {
                                                url: meta.url,
                                                hash: meta.hash.to_vec(),
                                            })
                                        }
                                        _ => None,
                                    };

                                    (
                                        pool_id,
                                        PoolRegistration {
                                            operator: params.id,
                                            vrf_key_hash: params.vrf,
                                            pledge: params.pledge,
                                            cost: params.cost,
                                            margin: Ratio {
                                                numerator: params.margin.numerator,
                                                denominator: params.margin.denominator,
                                            },
                                            reward_account,
                                            pool_owners,
                                            relays,
                                            pool_metadata,
                                        },
                                    )
                                })
                                .collect(),
                        );

                        info!("        {snapshot_name} snapshot - parse completed successfully.");

                        Ok(Snapshot {
                            snapshot_stake,
                            snapshot_delegations: delegations,
                            snapshot_pool_params: registration,
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
