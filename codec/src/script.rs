use acropolis_common::{ExUnits, Redeemer, RedeemerTag};
use anyhow::{Result, anyhow};
use pallas::codec::minicbor;
use pallas_primitives::{ExUnits as PallasExUnits, conway};
use pallas_traverse::MultiEraRedeemer;

fn map_redeemer_tag(tag: &conway::RedeemerTag) -> RedeemerTag {
    match tag {
        conway::RedeemerTag::Spend => RedeemerTag::Spend,
        conway::RedeemerTag::Mint => RedeemerTag::Mint,
        conway::RedeemerTag::Cert => RedeemerTag::Cert,
        conway::RedeemerTag::Reward => RedeemerTag::Reward,
        conway::RedeemerTag::Vote => RedeemerTag::Vote,
        conway::RedeemerTag::Propose => RedeemerTag::Propose,
    }
}

fn map_ex_units(ex_units: &PallasExUnits) -> ExUnits {
    ExUnits {
        mem: ex_units.mem,
        steps: ex_units.steps,
    }
}

pub fn map_redeemer(redeemer: &MultiEraRedeemer) -> Result<Redeemer> {
    let tag = redeemer.tag();
    let index = redeemer.index();
    let plutus_data = redeemer.data();
    let ex_units = redeemer.ex_units();
    let mut raw_plutus_data = Vec::new();
    minicbor::encode(plutus_data, &mut raw_plutus_data)
        .map_err(|_| anyhow!("Failed to encode plutus data of redeemer index {}", index))?;

    Ok(Redeemer {
        tag: map_redeemer_tag(&tag),
        index,
        data: raw_plutus_data,
        ex_units: map_ex_units(&ex_units),
    })
}
