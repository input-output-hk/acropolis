//! Shelley era transaction validation
//! Reference: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Utxo.hs#L343

use acropolis_common::{
    protocol_params::ShelleyParams, validation::UTxOValidationError, Address, Lovelace, NetworkId,
};
use anyhow::Result;
use pallas::{
    codec as pallas_codec,
    ledger::{addresses::Address as PallasAddress, primitives::alonzo},
};

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

pub fn validate(mtx: &alonzo::MintedTx, shelley_params: &ShelleyParams) -> UTxOValidationResult {
    let network_id = shelley_params.network_id.clone();
    let transaction_body = &mtx.transaction_body;

    validate_input_set_empty_utxo(transaction_body)?;
    validate_wrong_network(transaction_body, network_id.clone())?;
    validate_wrong_network_withdrawal(transaction_body, network_id.clone())?;
    validate_output_too_small_utxo(transaction_body, shelley_params)?;
    Ok(())
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

/// Validate every output address match the network
/// Reference: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Utxo.hs#L481
pub fn validate_wrong_network(
    transaction_body: &alonzo::TransactionBody,
    network_id: NetworkId,
) -> UTxOValidationResult {
    for (index, output) in transaction_body.outputs.iter().enumerate() {
        let pallas_address = PallasAddress::from_bytes(output.address.as_ref()).map_err(|_| {
            Box::new(UTxOValidationError::MalformedOutput {
                output_index: index,
                reason: "Malformed address at output".to_string(),
            })
        })?;

        let address = acropolis_codec::map_address(&pallas_address).map_err(|e| {
            Box::new(UTxOValidationError::MalformedOutput {
                output_index: index,
                reason: format!("Invalid address at output {index}: {}", e),
            })
        })?;

        let is_network_correct = match &address {
            // NOTE:
            // need to parse byron address's attributes and get network magic
            Address::Byron(_) => true,
            Address::Shelley(shelley_address) => shelley_address.network == network_id,
            _ => {
                return Err(Box::new(UTxOValidationError::MalformedOutput {
                    output_index: index,
                    reason: "Not a Shelley Address at output".to_string(),
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
                Box::new(UTxOValidationError::MalformedWithdrawal {
                    withdrawal_index: index,
                    reason: "Malformed reward address at withdrawal".to_string(),
                })
            })?;

        let stake_address = acropolis_codec::map_address(&pallas_reward_adddess).map_err(|e| {
            Box::new(UTxOValidationError::MalformedWithdrawal {
                withdrawal_index: index,
                reason: format!("Invalid reward address at withdrawal: {e}"),
            })
        })?;

        let stake_address = match stake_address {
            Address::Stake(stake_address) => stake_address,
            _ => {
                return Err(Box::new(UTxOValidationError::MalformedWithdrawal {
                    withdrawal_index: index,
                    reason: "Not a Stake Address at withdrawal".to_string(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{test_utils::TestContext, validation_fixture};
    use acropolis_common::{ShelleyAddress, StakeAddress};
    use pallas::ledger::traverse::{Era as PallasEra, MultiEraTx};
    use test_case::test_case;

    #[test_case(validation_fixture!("cd9037018278826d8ee2a80fe233862d0ff20bf61fc9f74543d682828c7cdb9f") =>
        matches Ok(());
        "valid transaction 1"
    )]
    #[test_case(validation_fixture!("20ded0bfef32fc5eefba2c1f43bcd99acc0b1c3284617c3cb355ad0eadccaa6e") =>
        matches Ok(());
        "valid transaction 2"
    )]
    #[test_case(validation_fixture!("20ded0bfef32fc5eefba2c1f43bcd99acc0b1c3284617c3cb355ad0eadccaa6e", "input_set_empty_utxo") =>
        matches Err(UTxOValidationError::InputSetEmptyUTxO);
        "input_set_empty_utxo"
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
        let tx = MultiEraTx::decode_for_era(PallasEra::Shelley, &raw_tx).unwrap();
        let mtx = tx.as_alonzo().unwrap();
        validate(mtx, &ctx.shelley_params).map_err(|e| *e)
    }
}
