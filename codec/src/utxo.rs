use crate::{utils::to_hash, witness::map_native_script};
use acropolis_common::*;
use pallas_primitives::conway;
use pallas_traverse::{MultiEraPolicyAssets, MultiEraValue};

pub fn map_value(pallas_value: &MultiEraValue) -> Value {
    let lovelace = pallas_value.coin();
    let pallas_assets = pallas_value.assets();

    let mut assets: NativeAssets = Vec::new();

    for policy_group in pallas_assets {
        match policy_group {
            MultiEraPolicyAssets::AlonzoCompatibleOutput(policy, kvps) => {
                match policy.as_ref().try_into() {
                    Ok(policy_id) => {
                        let native_assets = kvps
                            .iter()
                            .filter_map(|(name, amt)| {
                                AssetName::new(name).map(|asset_name| NativeAsset {
                                    name: asset_name,
                                    amount: *amt,
                                })
                            })
                            .collect::<Vec<_>>();

                        assets.push((policy_id, native_assets));
                    }
                    Err(_) => {
                        tracing::error!(
                            "Invalid policy id length: expected 28 bytes, got {}",
                            policy.len()
                        );
                        continue;
                    }
                }
            }
            MultiEraPolicyAssets::ConwayOutput(policy, kvps) => match policy.as_ref().try_into() {
                Ok(policy_id) => {
                    let native_assets = kvps
                        .iter()
                        .filter_map(|(name, amt)| {
                            AssetName::new(name).map(|asset_name| NativeAsset {
                                name: asset_name,
                                amount: u64::from(*amt),
                            })
                        })
                        .collect();

                    assets.push((policy_id, native_assets));
                }
                Err(_) => {
                    tracing::error!(
                        "Invalid policy id length: expected 28 bytes, got {}",
                        policy.len()
                    );
                    continue;
                }
            },
            _ => {}
        }
    }
    Value::new(lovelace, assets)
}

pub fn map_datum(datum: &Option<conway::MintedDatumOption>) -> Option<Datum> {
    match datum {
        Some(conway::MintedDatumOption::Hash(h)) => Some(Datum::Hash(h.to_vec())),
        Some(conway::MintedDatumOption::Data(d)) => Some(Datum::Inline(d.raw_cbor().to_vec())),
        None => None,
    }
}

pub fn map_reference_script(script: &Option<conway::MintedScriptRef>) -> Option<ReferenceScript> {
    match script {
        Some(conway::PseudoScript::NativeScript(script)) => {
            Some(ReferenceScript::Native(map_native_script(script)))
        }
        Some(conway::PseudoScript::PlutusV1Script(script)) => {
            Some(ReferenceScript::PlutusV1(script.as_ref().to_vec()))
        }
        Some(conway::PseudoScript::PlutusV2Script(script)) => {
            Some(ReferenceScript::PlutusV2(script.as_ref().to_vec()))
        }
        Some(conway::PseudoScript::PlutusV3Script(script)) => {
            Some(ReferenceScript::PlutusV3(script.as_ref().to_vec()))
        }
        None => None,
    }
}

pub fn map_mint_burn(
    policy_group: &MultiEraPolicyAssets<'_>,
) -> Option<(PolicyId, Vec<NativeAssetDelta>)> {
    match policy_group {
        MultiEraPolicyAssets::AlonzoCompatibleMint(policy, kvps) => {
            let policy_id: PolicyId = to_hash(*policy);

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
            let policy_id: PolicyId = to_hash(*policy);

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
