use acropolis_common::{validation::ScriptContextError, NativeAssetsDelta, Value};
use uplc_turbo::{arena::Arena, data::PlutusData, machine::PlutusVersion};

use super::to_plutus_data::*;

impl ToPlutusData for Value {
    fn to_plutus_data<'a>(
        &self,
        arena: &'a Arena,
        version: PlutusVersion,
    ) -> Result<&'a PlutusData<'a>, ScriptContextError> {
        let include_ada = match version {
            PlutusVersion::V3 => self.lovelace != 0,
            _ => true,
        };

        let mut entries = Vec::new();

        if include_ada {
            let empty = bytes(arena, &[]);
            let lovelace = integer(arena, self.lovelace as i128);
            let inner = map(arena, vec![(empty, lovelace)]);
            entries.push((bytes(arena, &[]), inner));
        }

        let mut sorted_assets: Vec<_> = self.assets.iter().collect();
        sorted_assets.sort_by(|(a, _), (b, _)| a.as_ref().cmp(b.as_ref()));

        for (policy_id, assets) in sorted_assets {
            let policy = policy_id.to_plutus_data(arena, version)?;
            let mut sorted_assets: Vec<_> = assets.iter().collect();
            sorted_assets.sort_by(|a, b| a.name.as_slice().cmp(b.name.as_slice()));
            let asset_pairs: Vec<_> = sorted_assets
                .iter()
                .map(|asset| {
                    let name = bytes(arena, asset.name.as_slice());
                    let amount = integer(arena, asset.amount as i128);
                    (name, amount)
                })
                .collect();
            entries.push((policy, map(arena, asset_pairs)));
        }

        Ok(map(arena, entries))
    }
}

pub fn encode_mint_value<'a>(
    mint: &NativeAssetsDelta,
    arena: &'a Arena,
    version: PlutusVersion,
) -> Result<&'a PlutusData<'a>, ScriptContextError> {
    let include_zero_ada = match version {
        PlutusVersion::V3 => false,
        _ => true, // V1/V2 require ADA entry in mint value
    };

    let mut entries = Vec::new();

    if include_zero_ada {
        let empty = bytes(arena, &[]);
        let zero = integer(arena, 0);
        let inner = map(arena, vec![(empty, zero)]);
        entries.push((bytes(arena, &[]), inner));
    }

    let mut sorted_mint: Vec<_> = mint.iter().collect();
    sorted_mint.sort_by(|(a, _), (b, _)| a.as_ref().cmp(b.as_ref()));

    for (policy_id, assets) in sorted_mint {
        let policy = policy_id.to_plutus_data(arena, version)?;
        let mut sorted_assets: Vec<_> = assets.iter().collect();
        sorted_assets.sort_by(|a, b| a.name.as_slice().cmp(b.name.as_slice()));
        let asset_pairs: Vec<_> = sorted_assets
            .iter()
            .map(|asset| {
                let name = bytes(arena, asset.name.as_slice());
                let amount = integer(arena, asset.amount as i128);
                (name, amount)
            })
            .collect();
        entries.push((policy, map(arena, asset_pairs)));
    }
    Ok(map(arena, entries))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_v1_always_includes_ada() {
        let arena = Arena::new();
        let val = Value::new(5_000_000, vec![]);
        let pd = val.to_plutus_data(&arena, PlutusVersion::V1).unwrap();

        // Map([(empty, Map([(empty, 5000000)]))])
        if let PlutusData::Map(entries) = pd {
            assert_eq!(entries.len(), 1);
            if let PlutusData::ByteString(bs) = entries[0].0 {
                assert!(bs.is_empty(), "ADA currency symbol should be empty bytes");
            }
        } else {
            panic!("Value should be Map");
        }
    }

    #[test]
    fn value_v1_includes_ada_even_if_zero() {
        let arena = Arena::new();
        let val = Value::new(0, vec![]);
        let pd = val.to_plutus_data(&arena, PlutusVersion::V1).unwrap();

        if let PlutusData::Map(entries) = pd {
            assert_eq!(entries.len(), 1, "V1 should include ADA even when zero");
        } else {
            panic!("Value should be Map");
        }
    }

    #[test]
    fn value_v3_excludes_zero_ada() {
        let arena = Arena::new();
        let val = Value::new(0, vec![]);
        let pd = val.to_plutus_data(&arena, PlutusVersion::V3).unwrap();

        if let PlutusData::Map(entries) = pd {
            assert_eq!(entries.len(), 0, "V3 should exclude zero ADA");
        } else {
            panic!("Value should be Map");
        }
    }

    #[test]
    fn mint_v1_includes_zero_ada_entry() {
        let arena = Arena::new();
        let mint: NativeAssetsDelta = vec![];
        let pd = encode_mint_value(&mint, &arena, PlutusVersion::V1).unwrap();

        if let PlutusData::Map(entries) = pd {
            assert_eq!(entries.len(), 1, "V1 empty mint should have zero ADA entry");
        } else {
            panic!("Mint should be Map");
        }
    }

    #[test]
    fn mint_v3_no_ada_entry() {
        let arena = Arena::new();
        let mint: NativeAssetsDelta = vec![];
        let pd = encode_mint_value(&mint, &arena, PlutusVersion::V3).unwrap();

        if let PlutusData::Map(entries) = pd {
            assert_eq!(entries.len(), 0, "V3 empty mint should have no entries");
        } else {
            panic!("Mint should be Map");
        }
    }
}
