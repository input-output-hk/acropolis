//! Shelley era transaction validation
//! Reference: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Utxo.hs#L343

use acropolis_common::{
    protocol_params::ShelleyParams, validation::UTxOValidationError, Address, Era, Lovelace,
    NetworkId, TxIdentifier, UTxOIdentifier,
};
use anyhow::Result;
use pallas::{
    codec as pallas_codec,
    ledger::{
        addresses::Address as PallasAddress,
        primitives::alonzo,
        traverse::{Era as PallasEra, MultiEraTx},
    },
};
use tracing::error;

fn get_lovelace_from_alonzo_value(val: &alonzo::Value) -> Lovelace {
    match val {
        alonzo::Value::Coin(res) => *res,
        alonzo::Value::Multiasset(res, _) => *res,
    }
}

fn get_value_size_in_bytes(val: &alonzo::Value) -> u64 {
    let mut buf = Vec::new();
    let _ = pallas_codec::minicbor::encode(val, &mut buf);
    (buf.len() as u64).div_ceil(8)
}

/// Reference: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/mary/impl/src/Cardano/Ledger/Mary/TxOut.hs#L52
fn compute_min_lovelace(value: &alonzo::Value, shelley_params: &ShelleyParams) -> Lovelace {
    match value {
        alonzo::Value::Coin(_) => shelley_params.protocol_params.min_utxo_value,
        alonzo::Value::Multiasset(lovelace, _) => {
            let utxo_entry_size = 27 + get_value_size_in_bytes(value);
            let coins_per_utxo_word = shelley_params.protocol_params.min_utxo_value / 27;
            (*lovelace).max(coins_per_utxo_word * utxo_entry_size)
        }
    }
}

pub type UTxOValidationResult = Result<(), Box<UTxOValidationError>>;

pub fn validate_shelley_tx<F>(
    tx: &MultiEraTx,
    shelley_params: &ShelleyParams,
    current_slot: u64,
    lookup_by_hash: F,
) -> UTxOValidationResult
where
    F: Fn(UTxOIdentifier) -> Result<TxIdentifier>,
{
    let network_id = shelley_params.network_id.clone();
    let tx_size = tx.size() as u32;

    let mtx = match tx {
        MultiEraTx::AlonzoCompatible(mtx, PallasEra::Shelley) => mtx,
        _ => {
            error!("Not a Shelley transaction: {:?}", tx);
            return Err(Box::new(UTxOValidationError::MalformedUTxO {
                era: Era::Shelley,
                reason: "Not a Shelley transaction".to_string(),
            }));
        }
    };
    let transaction_body = &mtx.transaction_body;

    validate_time_to_live(mtx, current_slot)?;
    validate_input_set_empty_utxo(transaction_body)?;
    validate_fee_too_small_utxo(transaction_body, tx_size, shelley_params)?;
    validate_bad_inputs_utxo(transaction_body, lookup_by_hash)?;
    validate_wrong_network(transaction_body, network_id.clone())?;
    validate_wrong_network_withdrawal(transaction_body, network_id.clone())?;
    validate_output_too_small_utxo(transaction_body, shelley_params)?;
    validate_max_tx_size_utxo(tx_size, shelley_params)?;
    Ok(())
}

/// Validate transaction's TTL field
/// pass if ttl >= current_slot
/// Reference: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Utxo.hs#L421
pub fn validate_time_to_live(tx: &alonzo::MintedTx, current_slot: u64) -> UTxOValidationResult {
    if let Some(ttl) = tx.transaction_body.ttl {
        if ttl >= current_slot {
            Ok(())
        } else {
            Err(Box::new(UTxOValidationError::ExpiredUTxO {
                ttl,
                current_slot,
            }))
        }
    } else {
        Err(Box::new(UTxOValidationError::MalformedUTxO {
            era: Era::Shelley,
            reason: "TTL is missing".to_string(),
        }))
    }
}

/// Validate every transaction must consume at least one UTxO
/// Reference: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Utxo.hs#L435
pub fn validate_input_set_empty_utxo(
    transaction_body: &alonzo::TransactionBody,
) -> UTxOValidationResult {
    if transaction_body.inputs.is_empty() {
        Err(Box::new(UTxOValidationError::InputSetEmptyUTxO))
    } else {
        Ok(())
    }
}

/// Validate every transaction has minimum fee required
/// Fee calculation:
/// minFee = (tx_size_in_bytes * min_a) + min_b + ref_script_fee (this is after Alonzo Era)
/// Reference: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Utxo.hs#L447
pub fn validate_fee_too_small_utxo(
    transaction_body: &alonzo::TransactionBody,
    tx_size: u32,
    shelley_params: &ShelleyParams,
) -> UTxOValidationResult {
    let min_fee = shelley_params.min_fee(tx_size);
    if transaction_body.fee < min_fee {
        Err(Box::new(UTxOValidationError::FeeTooSmallUTxO {
            supplied: transaction_body.fee,
            required: min_fee,
        }))
    } else {
        Ok(())
    }
}

/// Validate every transaction's input exists in the current UTxO set.
/// This prevents double spending.
/// Reference: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Utxo.hs#L468
pub fn validate_bad_inputs_utxo<F>(
    transaction_body: &alonzo::TransactionBody,
    lookup_by_hash: F,
) -> UTxOValidationResult
where
    F: Fn(UTxOIdentifier) -> Result<TxIdentifier>,
{
    for (index, input) in transaction_body.inputs.iter().enumerate() {
        let tx_ref = UTxOIdentifier::new((*input.transaction_id).into(), input.index as u16);
        if lookup_by_hash(tx_ref).is_err() {
            return Err(Box::new(UTxOValidationError::BadInputsUTxO {
                bad_input: tx_ref,
                bad_input_index: index,
            }));
        }
    }
    Ok(())
}

/// Validate every output address match the network
/// Reference: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Utxo.hs#L481
pub fn validate_wrong_network(
    transaction_body: &alonzo::TransactionBody,
    network_id: NetworkId,
) -> UTxOValidationResult {
    for (index, output) in transaction_body.outputs.iter().enumerate() {
        let pallas_address = PallasAddress::from_bytes(output.address.as_ref()).map_err(|_| {
            Box::new(UTxOValidationError::MalformedUTxO {
                era: Era::Shelley,
                reason: format!("Malformed address at output {index}"),
            })
        })?;

        let address = acropolis_codec::map_address(&pallas_address).map_err(|e| {
            Box::new(UTxOValidationError::MalformedUTxO {
                era: Era::Shelley,
                reason: format!("Invalid address at output {index}: {}", e),
            })
        })?;

        let is_network_correct = match &address {
            // NOTE:
            // need to parse byron address's attributes and get network magic
            Address::Byron(_) => true,
            Address::Shelley(shelley_address) => shelley_address.network == network_id,
            _ => {
                return Err(Box::new(UTxOValidationError::MalformedUTxO {
                    era: Era::Shelley,
                    reason: format!("Not a Shelley Address at output {index}"),
                }))
            }
        };
        if !is_network_correct {
            return Err(Box::new(UTxOValidationError::WrongNetwork {
                expected: network_id,
                wrong_address: address,
                output_index: index,
            }));
        }
    }

    Ok(())
}

/// Validate every withdrawal account addresses match the network
/// Reference: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Utxo.hs#L497
pub fn validate_wrong_network_withdrawal(
    transaction_body: &alonzo::TransactionBody,
    network_id: NetworkId,
) -> UTxOValidationResult {
    let Some(withdrawals) = transaction_body.withdrawals.as_ref() else {
        return Ok(());
    };
    for (index, (stake_address_bytes, _)) in withdrawals.iter().enumerate() {
        let pallas_reward_adddess =
            PallasAddress::from_bytes(stake_address_bytes).map_err(|_| {
                Box::new(UTxOValidationError::MalformedUTxO {
                    era: Era::Shelley,
                    reason: format!("Malformed reward address at withdrawal {index}"),
                })
            })?;

        let stake_address = acropolis_codec::map_address(&pallas_reward_adddess).map_err(|e| {
            Box::new(UTxOValidationError::MalformedUTxO {
                era: Era::Shelley,
                reason: format!("Invalid reward address at withdrawal {index}: {}", e),
            })
        })?;

        let stake_address = match stake_address {
            Address::Stake(stake_address) => stake_address,
            _ => {
                return Err(Box::new(UTxOValidationError::MalformedUTxO {
                    era: Era::Shelley,
                    reason: format!("Not a Stake Address at withdrawal {index}"),
                }));
            }
        };

        if stake_address.network != network_id {
            return Err(Box::new(UTxOValidationError::WrongNetworkWithdrawal {
                expected: network_id,
                wrong_account: stake_address,
                withdrawal_index: index,
            }));
        }
    }

    Ok(())
}

/// Validate every output has minimum required lovelace
/// Reference: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Utxo.hs#L531
pub fn validate_output_too_small_utxo(
    transaction_body: &alonzo::TransactionBody,
    shelley_params: &ShelleyParams,
) -> UTxOValidationResult {
    for (index, output) in transaction_body.outputs.iter().enumerate() {
        let lovelace = get_lovelace_from_alonzo_value(&output.amount);
        let required_lovelace = compute_min_lovelace(&output.amount, shelley_params);
        if lovelace < required_lovelace {
            return Err(Box::new(UTxOValidationError::OutputTooSmallUTxO {
                output_index: index,
                lovelace,
                required_lovelace,
            }));
        }
    }
    Ok(())
}

/// Validate transaction size is under the limit
/// Reference: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Utxo.hs#L575
pub fn validate_max_tx_size_utxo(
    tx_size: u32,
    shelley_params: &ShelleyParams,
) -> UTxOValidationResult {
    let max_tx_size = shelley_params.protocol_params.max_tx_size;
    if tx_size > max_tx_size {
        Err(Box::new(UTxOValidationError::MaxTxSizeUTxO {
            supplied: tx_size,
            max: max_tx_size,
        }))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{test_utils::TestContext, validation_fixture};
    use acropolis_common::{ShelleyAddress, StakeAddress, TxHash};
    use pallas::ledger::traverse;
    use std::str::FromStr;
    use test_case::test_case;

    #[test_case(validation_fixture!("cd9037018278826d8ee2a80fe233862d0ff20bf61fc9f74543d682828c7cdb9f") =>
        matches Ok(());
        "valid transaction 1"
    )]
    #[test_case(validation_fixture!("20ded0bfef32fc5eefba2c1f43bcd99acc0b1c3284617c3cb355ad0eadccaa6e") =>
        matches Ok(());
        "valid transaction 2"
    )]
    #[test_case(validation_fixture!("20ded0bfef32fc5eefba2c1f43bcd99acc0b1c3284617c3cb355ad0eadccaa6e", "expired_utxo") =>
        matches Err(UTxOValidationError::ExpiredUTxO { ttl: 7084747, current_slot: 7084748 });
        "expired_utxo"
    )]
    #[test_case(validation_fixture!("20ded0bfef32fc5eefba2c1f43bcd99acc0b1c3284617c3cb355ad0eadccaa6e", "input_set_empty_utxo") =>
        matches Err(UTxOValidationError::InputSetEmptyUTxO);
        "input_set_empty_utxo"
    )]
    #[test_case(validation_fixture!("20ded0bfef32fc5eefba2c1f43bcd99acc0b1c3284617c3cb355ad0eadccaa6e", "fee_too_small_utxo") =>
        matches Err(UTxOValidationError::FeeTooSmallUTxO { supplied: 22541, required: 172277 });
        "fee_too_small_utxo"
    )]
    #[test_case(validation_fixture!("20ded0bfef32fc5eefba2c1f43bcd99acc0b1c3284617c3cb355ad0eadccaa6e", "bad_inputs_utxo") =>
        matches Err(UTxOValidationError::BadInputsUTxO { bad_input, bad_input_index })
        if bad_input == UTxOIdentifier::new(TxHash::from_str("d93625fb30376a1eaf90e6232296b0a31b7e63fac2af01381ffe58a574aae537").unwrap(), 1) && bad_input_index == 0;
        "bad_inputs_utxo"
    )]
    #[test_case(validation_fixture!("20ded0bfef32fc5eefba2c1f43bcd99acc0b1c3284617c3cb355ad0eadccaa6e", "wrong_network") =>
        matches Err(UTxOValidationError::WrongNetwork { expected: NetworkId::Mainnet, wrong_address, output_index })
        if wrong_address == Address::Shelley(ShelleyAddress::from_string("addr_test1qzvsy7ftzmrqj3hfs6ppczx263rups3fy3q0z0msnfw2e7s663nkrm3jz3sre0aupn4mdmdz8tdakdhgppaz58qkwe0q680lcj").unwrap()) 
            && output_index == 1;
        "wrong_network"
    )]
    #[test_case(validation_fixture!("20ded0bfef32fc5eefba2c1f43bcd99acc0b1c3284617c3cb355ad0eadccaa6e", "output_too_small_utxo") =>
        matches Err(UTxOValidationError::OutputTooSmallUTxO { output_index: 1, lovelace: 1, required_lovelace: 1000000 });
        "output_too_small_utxo"
    )]
    #[test_case(validation_fixture!("20ded0bfef32fc5eefba2c1f43bcd99acc0b1c3284617c3cb355ad0eadccaa6e", "max_tx_size_utxo") =>
        matches Err(UTxOValidationError::MaxTxSizeUTxO { supplied: 17983, max: 16384 });
        "max_tx_size_utxo"
    )]
    /// This tx contains withdrawal
    #[test_case(validation_fixture!("a1aaa9c239f17e6feab5767f61457a3e6251cd0bb94a00a5d41847435caaa42a") =>
        matches Ok(());
        "valid transaction 3 with withdrawal"
    )]
    #[test_case(validation_fixture!("a1aaa9c239f17e6feab5767f61457a3e6251cd0bb94a00a5d41847435caaa42a", "wrong_network_withdrawal") =>
        matches Err(UTxOValidationError::WrongNetworkWithdrawal { expected: NetworkId::Mainnet, wrong_account, withdrawal_index })
        if wrong_account == StakeAddress::from_string("stake_test1upfe3tuzexk65edjy8t4dsfjcs2scyhwwucwkf7qmmg3mmqx3st08").unwrap() 
            && withdrawal_index == 0;
        "wrong_network_withdrawal"
    )]
    #[allow(clippy::result_large_err)]
    fn shelley_test((ctx, raw_tx): (TestContext, Vec<u8>)) -> Result<(), UTxOValidationError> {
        let tx = MultiEraTx::decode_for_era(traverse::Era::Shelley, &raw_tx).unwrap();

        let lookup_by_hash = |tx_ref: UTxOIdentifier| -> Result<TxIdentifier> {
            ctx.utxos.get(&tx_ref).copied().ok_or_else(|| {
                anyhow::anyhow!(
                    "TxHash not found or already spent: {:?}",
                    hex::encode(tx_ref.tx_hash)
                )
            })
        };
        validate_shelley_tx(&tx, &ctx.shelley_params, ctx.current_slot, lookup_by_hash)
            .map_err(|e| *e)
    }
}
