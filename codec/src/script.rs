use acropolis_common::{ExUnits, Redeemer, RedeemerTag};
use anyhow::{Result, anyhow};
use pallas::codec::minicbor;
use pallas_primitives::ExUnits as PallasExUnits;
use pallas_traverse::{MultiEraRedeemer, MultiEraRedeemerTag};

fn map_redeemer_tag(tag: MultiEraRedeemerTag) -> Option<RedeemerTag> {
    match tag {
        MultiEraRedeemerTag::Spend => Some(RedeemerTag::Spend),
        MultiEraRedeemerTag::Mint => Some(RedeemerTag::Mint),
        MultiEraRedeemerTag::Cert => Some(RedeemerTag::Cert),
        MultiEraRedeemerTag::Reward => Some(RedeemerTag::Reward),
        MultiEraRedeemerTag::Vote => Some(RedeemerTag::Vote),
        MultiEraRedeemerTag::Propose => Some(RedeemerTag::Propose),
        _ => None,
    }
}

fn map_ex_units(ex_units: &PallasExUnits) -> ExUnits {
    ExUnits {
        mem: ex_units.mem,
        steps: ex_units.steps,
    }
}

pub fn map_redeemer(redeemer: &MultiEraRedeemer) -> Result<Redeemer> {
    let index = redeemer.index();
    let pallas_tag = redeemer.multi_era_tag();
    let tag = map_redeemer_tag(pallas_tag).ok_or_else(|| {
        anyhow!(
            "Unsupported tag {:?} of redeemer index {}",
            pallas_tag,
            index
        )
    })?;
    let plutus_data = redeemer.data();
    let ex_units = redeemer.ex_units();
    let mut raw_plutus_data = Vec::new();
    minicbor::encode(plutus_data, &mut raw_plutus_data)
        .map_err(|_| anyhow!("Failed to encode plutus data of redeemer index {}", index))?;

    Ok(Redeemer {
        tag,
        index,
        data: raw_plutus_data,
        ex_units: map_ex_units(&ex_units),
    })
}
