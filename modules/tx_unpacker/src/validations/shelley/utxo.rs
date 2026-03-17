//! Shelley era transaction validation
//! Reference: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Utxo.hs#L343

use acropolis_common::{
    protocol_params::ProtocolParams, validation::UTxOValidationError, Address, Era, NetworkId,
    StakeAddress,
};
use anyhow::Result;
use pallas::ledger::traverse::{MultiEraInput, MultiEraOutput, MultiEraTx};

use crate::validations::utils;

pub type UTxOValidationResult = Result<(), Box<UTxOValidationError>>;

pub fn validate(
    tx: &MultiEraTx,
    protocol_params: &ProtocolParams,
    era: Era,
) -> UTxOValidationResult {
    let shelley_params = protocol_params.shelley.as_ref().ok_or_else(|| {
        Box::new(UTxOValidationError::Other(
            "Shelley params are not set".to_string(),
        ))
    })?;
    let network_id = shelley_params.network_id.clone();

    validate_input_set_empty_utxo(&tx.inputs_sorted_set())?;
    validate_output_network(&tx.produces(), network_id.clone())?;
    validate_withdrawal_network(&tx.withdrawals_sorted_set(), network_id.clone())?;
    validate_output_too_small_utxo(&tx.outputs(), &tx.collateral_return(), protocol_params, era)?;
    Ok(())
}

/// Validate every transaction must consume at least one UTxO
/// Reference: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Utxo.hs#L435
pub fn validate_input_set_empty_utxo(inputs: &[MultiEraInput]) -> UTxOValidationResult {
    if inputs.is_empty() {
        Err(Box::new(UTxOValidationError::InputSetEmptyUTxO))
    } else {
        Ok(())
    }
}

/// Validate every output address match the network
/// Reference: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Utxo.hs#L481
pub fn validate_output_network(
    outputs: &[(usize, MultiEraOutput)],
    network_id: NetworkId,
) -> UTxOValidationResult {
    for (index, output) in outputs.iter() {
        let pallas_address = output.address().map_err(|_| {
            Box::new(UTxOValidationError::MalformedOutput {
                output_index: *index,
                reason: "Malformed address".to_string(),
            })
        })?;

        let address = acropolis_codec::map_address(&pallas_address).map_err(|_| {
            Box::new(UTxOValidationError::MalformedOutput {
                output_index: *index,
                reason: "Invalid address".to_string(),
            })
        })?;

        let is_network_correct = match &address {
            // NOTE:
            // need to parse byron address's attributes and get network magic
            Address::Byron(_) => true,
            Address::Shelley(shelley_address) => shelley_address.network == network_id,
            _ => {
                return Err(Box::new(UTxOValidationError::MalformedOutput {
                    output_index: *index,
                    reason: "Not a Shelley Address".to_string(),
                }))
            }
        };
        if !is_network_correct {
            return Err(Box::new(UTxOValidationError::WrongNetwork {
                expected: network_id,
                wrong_address: address,
                output_index: *index,
            }));
        }
    }

    Ok(())
}

/// Validate every withdrawal account addresses match the network
/// Reference: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Utxo.hs#L497
pub fn validate_withdrawal_network(
    withdrawals: &[(&[u8], u64)],
    network_id: NetworkId,
) -> UTxOValidationResult {
    for (index, (stake_address_bytes, _amount)) in withdrawals.iter().enumerate() {
        let stake_address = StakeAddress::from_binary(stake_address_bytes).map_err(|e| {
            Box::new(UTxOValidationError::MalformedWithdrawal {
                withdrawal_index: index,
                reason: format!("Invalid stake address: {e}"),
            })
        })?;

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
    outputs: &[MultiEraOutput],
    collateral_return: &Option<MultiEraOutput>,
    protocol_params: &ProtocolParams,
    era: Era,
) -> UTxOValidationResult {
    let validate_output = |index: usize, output: &MultiEraOutput| {
        let lovelace = output.value().coin();
        let required_lovelace = utils::compute_min_lovelace(output, protocol_params, era)
            .map_err(|e| Box::new(UTxOValidationError::Other(e.to_string())))?;
        if lovelace < required_lovelace {
            return Err(Box::new(UTxOValidationError::OutputTooSmallUTxO {
                output_index: index,
                lovelace,
                required_lovelace,
            }));
        }
        Ok(())
    };

    for (index, output) in outputs.iter().enumerate() {
        validate_output(index, output)?;
    }
    if let Some(collateral_return) = collateral_return {
        // NOTE:
        // Use collateral return index as 0
        validate_output(0, collateral_return)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        test_utils::{to_era, to_pallas_era, TestContext},
        validation_fixture,
    };
    use acropolis_common::{ShelleyAddress, StakeAddress};
    use pallas::ledger::traverse::MultiEraTx;
    use test_case::test_case;

    #[test_case(validation_fixture!(
        "shelley",
        "20ded0bfef32fc5eefba2c1f43bcd99acc0b1c3284617c3cb355ad0eadccaa6e"
    ) =>
        matches Ok(());
        "valid transaction 1 - with byron input & output"
    )]
    #[test_case(validation_fixture!(
        "shelley",
        "da350a9e2a14717172cee9e37df02b14b5718ea1934ce6bea25d739d9226f01b"
    ) =>
        matches Ok(());
        "valid transaction 2"
    )]
    #[test_case(validation_fixture!(
        "shelley",
        "20ded0bfef32fc5eefba2c1f43bcd99acc0b1c3284617c3cb355ad0eadccaa6e",
        "input_set_empty_utxo"
    ) =>
        matches Err(UTxOValidationError::InputSetEmptyUTxO);
        "input_set_empty_utxo"
    )]
    #[test_case(validation_fixture!(
        "shelley",
        "20ded0bfef32fc5eefba2c1f43bcd99acc0b1c3284617c3cb355ad0eadccaa6e",
        "wrong_network"
    ) =>
        matches Err(UTxOValidationError::WrongNetwork { expected: NetworkId::Mainnet, wrong_address, output_index })
        if wrong_address == Address::Shelley(ShelleyAddress::from_string("addr_test1qzvsy7ftzmrqj3hfs6ppczx263rups3fy3q0z0msnfw2e7s663nkrm3jz3sre0aupn4mdmdz8tdakdhgppaz58qkwe0q680lcj").unwrap()) 
            && output_index == 1;
        "wrong_network"
    )]
    #[test_case(validation_fixture!(
        "shelley",
        "20ded0bfef32fc5eefba2c1f43bcd99acc0b1c3284617c3cb355ad0eadccaa6e",
        "output_too_small_utxo"
    ) =>
        matches Err(UTxOValidationError::OutputTooSmallUTxO { output_index: 1, lovelace: 1, required_lovelace: 1000000 });
        "output_too_small_utxo"
    )]
    /// This tx contains withdrawal
    #[test_case(validation_fixture!(
        "shelley",
        "a1aaa9c239f17e6feab5767f61457a3e6251cd0bb94a00a5d41847435caaa42a"
    ) =>
        matches Ok(());
        "valid transaction 3 with withdrawal"
    )]
    #[test_case(validation_fixture!(
        "shelley",
        "a1aaa9c239f17e6feab5767f61457a3e6251cd0bb94a00a5d41847435caaa42a",
        "wrong_network_withdrawal"
    ) =>
        matches Err(UTxOValidationError::WrongNetworkWithdrawal { expected: NetworkId::Mainnet, wrong_account, withdrawal_index })
        if wrong_account == StakeAddress::from_string("stake_test1upfe3tuzexk65edjy8t4dsfjcs2scyhwwucwkf7qmmg3mmqx3st08").unwrap() 
            && withdrawal_index == 0;
        "wrong_network_withdrawal"
    )]
    #[allow(clippy::result_large_err)]
    fn shelley_utxo_test(
        (ctx, raw_tx, era): (TestContext, Vec<u8>, &str),
    ) -> Result<(), UTxOValidationError> {
        let tx = MultiEraTx::decode_for_era(to_pallas_era(era), &raw_tx).unwrap();
        validate(&tx, &ctx.protocol_params, to_era(era)).map_err(|e| *e)
    }
}
