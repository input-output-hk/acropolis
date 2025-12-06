use acropolis_common::{AssetName, Metadata, MetadataInt, NativeAssetDelta, NetworkId, PolicyId};
use anyhow::{Result, anyhow};
use pallas::ledger::addresses;
use pallas_primitives::Metadatum as PallasMetadatum;
use pallas_traverse::MultiEraPolicyAssets;

/// Map Pallas Network to our NetworkId
pub fn map_network(network: addresses::Network) -> Result<NetworkId> {
    match network {
        addresses::Network::Mainnet => Ok(NetworkId::Mainnet),
        addresses::Network::Testnet => Ok(NetworkId::Testnet),
        _ => Err(anyhow!("Unknown network in address")),
    }
}

pub fn map_mint_burn(
    policy_group: &MultiEraPolicyAssets<'_>,
) -> Option<(PolicyId, Vec<NativeAssetDelta>)> {
    match policy_group {
        MultiEraPolicyAssets::AlonzoCompatibleMint(policy, kvps) => {
            let policy_id: PolicyId = match policy.as_ref().try_into() {
                Ok(id) => id,
                Err(_) => {
                    tracing::error!(
                        "Invalid policy id length: expected 28 bytes, got {}",
                        policy.len()
                    );
                    return None;
                }
            };

            let deltas = kvps
                .iter()
                .filter_map(|(name, amt)| {
                    AssetName::new(name).map(|asset_name| NativeAssetDelta {
                        name: asset_name,
                        amount: *amt,
                    })
                })
                .collect::<Vec<_>>();

            Some((policy_id, deltas))
        }

        MultiEraPolicyAssets::ConwayMint(policy, kvps) => {
            let policy_id: PolicyId = match policy.as_ref().try_into() {
                Ok(id) => id,
                Err(_) => {
                    tracing::error!(
                        "Invalid policy id length: expected 28 bytes, got {}",
                        policy.len()
                    );
                    return None;
                }
            };

            let deltas = kvps
                .iter()
                .filter_map(|(name, amt)| {
                    AssetName::new(name).map(|asset_name| NativeAssetDelta {
                        name: asset_name,
                        amount: i64::from(*amt),
                    })
                })
                .collect::<Vec<_>>();
            Some((policy_id, deltas))
        }

        _ => None,
    }
}

pub fn map_metadata(metadata: &PallasMetadatum) -> Metadata {
    match metadata {
        PallasMetadatum::Int(pallas_primitives::Int(i)) => Metadata::Int(MetadataInt(*i)),
        PallasMetadatum::Bytes(b) => Metadata::Bytes(b.to_vec()),
        PallasMetadatum::Text(s) => Metadata::Text(s.clone()),
        PallasMetadatum::Array(a) => Metadata::Array(a.iter().map(map_metadata).collect()),
        PallasMetadatum::Map(m) => {
            Metadata::Map(m.iter().map(|(k, v)| (map_metadata(k), map_metadata(v))).collect())
        }
    }
}
