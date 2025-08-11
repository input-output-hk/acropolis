//! Acropolis Parameter State module
//! Alonzo Genesis struct: replacement for Pallas Alonzo (unluckily, Pallas structure
//! is incompatible with SanchoNet genesis)

use acropolis_common::{
    rational_number::{rational_number_from_f32, RationalNumber},
    AlonzoParams, ExUnitPrices, ExUnits,
};
use anyhow::{bail, Result};
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Deserialize, Clone)]
#[serde(untagged)]
pub enum CostModel {
    Map(HashMap<String, i64>),
    Vector(Vec<i64>),
}

impl CostModel {
    pub fn to_vec(&self) -> Vec<i64> {
        match self {
            CostModel::Map(hm) => {
                let mut keys =
                    hm.iter().map(|(k, v)| (k.as_str(), *v)).collect::<Vec<(&str, i64)>>();
                keys.sort();
                keys.into_iter().map(|(_, n)| n).collect::<Vec<i64>>()
            }
            CostModel::Vector(v) => v.clone(),
        }
    }
}

#[derive(Deserialize, PartialEq, Eq, Hash, Clone)]
pub enum Language {
    PlutusV1,
    PlutusV2,
}

#[derive(Deserialize, Clone)]
pub struct CostModelPerLanguage(HashMap<Language, CostModel>);

impl CostModelPerLanguage {
    fn get_plutus_v1(&self) -> Result<Option<Vec<i64>>> {
        let mut res = None;
        for (k, v) in self.0.iter() {
            if *k != Language::PlutusV1 {
                bail!("Only PlutusV1 language cost model is allowed in Alonzo Genesis!")
            }
            res = Some(v.to_vec());
        }
        Ok(res)
    }
}

#[derive(Deserialize, Clone)]
#[serde(untagged)]
pub enum AlonzoFraction {
    Float(f32),
    Fraction { numerator: u64, denominator: u64 },
}

impl AlonzoFraction {
    fn get_rational(&self) -> Result<RationalNumber> {
        match self {
            AlonzoFraction::Fraction {
                numerator: n,
                denominator: d,
            } => Ok(RationalNumber::new(*n, *d)),
            AlonzoFraction::Float(v) => rational_number_from_f32(*v),
        }
    }
}

#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AlonzoExecutionPrices {
    pub pr_steps: AlonzoFraction,
    pub pr_mem: AlonzoFraction,
}

#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AlonzoExUnits {
    pub ex_units_mem: u64,
    pub ex_units_steps: u64,
}

#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Genesis {
    #[serde(rename = "lovelacePerUTxOWord")]
    pub lovelace_per_utxo_word: u64,
    pub execution_prices: AlonzoExecutionPrices,
    pub max_tx_ex_units: AlonzoExUnits,
    pub max_block_ex_units: AlonzoExUnits,
    pub max_value_size: u32,
    pub collateral_percentage: u32,
    pub max_collateral_inputs: u32,
    pub cost_models: CostModelPerLanguage,
}

fn map_ex_units(e: &AlonzoExUnits) -> Result<ExUnits> {
    Ok(ExUnits {
        mem: e.ex_units_mem,
        steps: e.ex_units_steps,
    })
}

fn map_alonzo_fraction(fr: &AlonzoFraction) -> Result<RationalNumber> {
    fr.get_rational()
}

fn map_execution_prices(e: &AlonzoExecutionPrices) -> Result<ExUnitPrices> {
    Ok(ExUnitPrices {
        mem_price: map_alonzo_fraction(&e.pr_mem)?,
        step_price: map_alonzo_fraction(&e.pr_steps)?,
    })
}

pub fn map_alonzo(genesis: &Genesis) -> Result<AlonzoParams> {
    Ok(AlonzoParams {
        lovelace_per_utxo_word: genesis.lovelace_per_utxo_word,
        execution_prices: map_execution_prices(&genesis.execution_prices)?,
        max_tx_ex_units: map_ex_units(&genesis.max_tx_ex_units)?,
        max_block_ex_units: map_ex_units(&genesis.max_block_ex_units)?,
        max_value_size: genesis.max_value_size,
        collateral_percentage: genesis.collateral_percentage,
        max_collateral_inputs: genesis.max_collateral_inputs,
        plutus_v1_cost_model: genesis.cost_models.get_plutus_v1()?,
        plutus_v2_cost_model: None,
    })
}
