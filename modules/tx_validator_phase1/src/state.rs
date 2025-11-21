use crate::TxValidatorPhase1StateConfig;
use acropolis_codec::map_parameters;
use acropolis_common::messages::{ProtocolParamsMessage, RawTxsMessage};
use acropolis_common::validation::{ValidationError, ValidationStatus};
use acropolis_common::{
    AssetName, BlockInfo, NativeAsset, NativeAssets, TxHash, TxIdentifier, TxOutRef, TxOutput,
    UTxOIdentifier, Value,
};
use anyhow::Result;
use pallas::ledger::primitives::{alonzo, byron};
use pallas::ledger::traverse::{MultiEraPolicyAssets, MultiEraTx, MultiEraValue};
use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::error;

// TODO: make something with separate utxo registres
#[derive(Clone, Default)]
pub struct UTxORegistry {
    pub live_map: HashMap<TxOutRef, TxIdentifier>,
}

pub struct State {
    pub config: Arc<TxValidatorPhase1StateConfig>,
    params: Option<ProtocolParamsMessage>,
    utxos_registry: UTxORegistry,
}

enum ConversionResult<Res> {
    Ok(Res),
    Error(ValidationError),
}

/*
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
 */

struct Transaction {
    inputs: Vec<UTxOIdentifier>,
    outputs: Vec<TxOutput>,
}

impl State {
    pub fn new(config: Arc<TxValidatorPhase1StateConfig>) -> Self {
        Self {
            config,
            params: None,
            utxos_registry: UTxORegistry::default(),
        }
    }

    pub async fn process_params(
        &mut self,
        _blk: BlockInfo,
        prm: ProtocolParamsMessage,
    ) -> Result<()> {
        self.params = Some(prm);
        Ok(())
    }

    /// Byron validation is not acutally performed, so it's always returns 'Go'
    fn validate_byron<'b>(
        &self,
        _tx: Box<Cow<'b, byron::MintedTxPayload<'b>>>,
    ) -> Result<ValidationStatus> {
        Ok(ValidationStatus::Go)
    }

    fn convert_from_pallas_tx<'b>(
        &self,
        block_info: &BlockInfo,
        tx_index: u16,
        tx: &MultiEraTx,
    ) -> Result<ConversionResult<Transaction>> {
        let _certs = tx.certs();
        let _tx_withdrawals = tx.withdrawals_sorted_set();

        let (tx_in_ref, tx_out, _total, err) =
            map_parameters::map_one_transaction(block_info.number as u32, tx_index, tx);

        if let Some(first_err) = err.into_iter().next() {
            return Ok(ConversionResult::Error(first_err));
        }

        let mut converted_inputs = Vec::new();
        let mut converted_outputs = Vec::new();

        for tx_ref in tx_in_ref {
            // MultiEraInput
            // Lookup and remove UTxOIdentifier from registry
            match self.utxos_registry.live_map.get(&tx_ref) {
                Some(tx_identifier) => {
                    // Add TxInput to utxo_deltas
                    converted_inputs.push(UTxOIdentifier::new(
                        tx_identifier.block_number(),
                        tx_identifier.tx_index(),
                        tx_ref.output_index,
                    ));
                }
                None => {
                    return Ok(ConversionResult::Error(
                        ValidationError::MalformedTransaction(
                            tx_index,
                            format!(
                                "Tx not found, tx {}, output index {}",
                                tx_ref.tx_hash, tx_ref.output_index
                            ),
                        ),
                    ));
                }
            }
        }

        // Add all the outputs
        for (_tx_ref, output) in tx_out {
            converted_outputs.push(output);
        }

        let tx = Transaction {
            inputs: converted_inputs,
            outputs: converted_outputs,
        };

        Ok(ConversionResult::Ok(tx))
    }

    fn validate_tx(&self, tx: &Transaction) -> Result<ValidationStatus> {
        // Do validate transactions
        Ok(ValidationStatus::Go)
    }

    pub fn process_transactions(
        &mut self,
        blk: &BlockInfo,
        txs_msg: &RawTxsMessage,
    ) -> Result<ValidationStatus> {
        for (tx_index, raw_tx) in txs_msg.txs.iter().enumerate() {
            // Parse the tx
            let res = match MultiEraTx::decode(raw_tx) {
                Err(e) => ValidationStatus::NoGo(ValidationError::CborDecodeError(
                    tx_index,
                    e.to_string(),
                )),
                Ok(MultiEraTx::Byron(byron_tx)) => self.validate_byron(byron_tx)?,

                Ok(tx) => {
                    let tx = match self.convert_from_pallas_tx(blk, tx_index as u16, &tx)? {
                        ConversionResult::Ok(res) => res,
                        ConversionResult::Error(err) => return Ok(ValidationStatus::NoGo(err)),
                    };
                    self.validate_tx(&tx)?
                }
            };

            if let ValidationStatus::NoGo(_) = &res {
                return Ok(res);
            }
        }
        Ok(ValidationStatus::Go)
    }
}
