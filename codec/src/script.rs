use acropolis_common::{ExUnits, Redeemer, RedeemerTag};
use anyhow::{Result, anyhow};
use pallas::codec::minicbor;
use pallas_primitives::ExUnits as PallasExUnits;
use pallas_traverse::{MultiEraRedeemer, MultiEraRedeemerTag};

fn map_redeemer_tag(tag: &MultiEraRedeemerTag) -> Result<RedeemerTag> {
    match tag {
        MultiEraRedeemerTag::Spend => Ok(RedeemerTag::Spend),
        MultiEraRedeemerTag::Mint => Ok(RedeemerTag::Mint),
        MultiEraRedeemerTag::Cert => Ok(RedeemerTag::Cert),
        MultiEraRedeemerTag::Reward => Ok(RedeemerTag::Reward),
        MultiEraRedeemerTag::Vote => Ok(RedeemerTag::Vote),
        MultiEraRedeemerTag::Propose => Ok(RedeemerTag::Propose),
        // TODO: Dijkstra is not properly supported yet. The `Guarding` tag has no
        // `RedeemerTag` variant in acropolis_common, so it is rejected here.
        other => Err(anyhow!("Unsupported redeemer tag {other:?}")),
    }
}

fn map_ex_units(ex_units: &PallasExUnits) -> ExUnits {
    ExUnits {
        mem: ex_units.mem,
        steps: ex_units.steps,
    }
}

pub fn map_redeemer(redeemer: &MultiEraRedeemer) -> Result<Redeemer> {
    let tag = redeemer.multi_era_tag();
    let index = redeemer.index();
    let plutus_data = redeemer.data();
    let ex_units = redeemer.ex_units();
    let mut raw_plutus_data = Vec::new();
    minicbor::encode(plutus_data, &mut raw_plutus_data)
        .map_err(|_| anyhow!("Failed to encode plutus data of redeemer index {}", index))?;

    Ok(Redeemer {
        tag: map_redeemer_tag(&tag)?,
        index,
        data: raw_plutus_data,
        ex_units: map_ex_units(&ex_units),
    })
}
