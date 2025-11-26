//! Shelley era transaction validation
//! Reference: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Utxo.hs#L343

use acropolis_codec;
use acropolis_common::{
    protocol_params::ShelleyParams, validation::TransactionValidationError, Address, Era,
    NetworkId, TxIdentifier, TxOutRef,
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

pub fn get_alonzo_comp_tx_size(mtx: &alonzo::MintedTx) -> u32 {
    match &mtx.auxiliary_data {
        pallas_codec::utils::Nullable::Some(aux_data) => {
            (aux_data.raw_cbor().len()
                + mtx.transaction_body.raw_cbor().len()
                + mtx.transaction_witness_set.raw_cbor().len()) as u32
        }
        _ => {
            (mtx.transaction_body.raw_cbor().len() + mtx.transaction_witness_set.raw_cbor().len())
                as u32
        }
    }
}

pub fn validate_shelley_tx(
    tx: &MultiEraTx,
    block_number: u32,
    tx_index: u16,
    shelley_params: &ShelleyParams,
    current_slot: u64,
) -> Result<(), TransactionValidationError> {
    let tx_size = tx.size() as u64;

    let mtx = match tx {
        MultiEraTx::AlonzoCompatible(mtx, PallasEra::Shelley) => mtx,
        _ => {
            return Err(TransactionValidationError::MalformedTransaction {
                era: Era::Shelley,
                reason: "Not a Shelley transaction".to_string(),
            })
        }
    };
    let transaction_body = &mtx.transaction_body;

    // map pallas transaction to acropolis transaction
    let (inputs, outputs, _, errors) =
        acropolis_codec::map_parameters::map_one_transaction(block_number, tx_index, &tx);

    if !errors.is_empty() {
        return Err(TransactionValidationError::MalformedTransaction {
            era: Era::Shelley,
            reason: format!(
                "Errors: {}",
                errors.iter().map(|e| e.to_string()).collect::<Vec<_>>().join(", ")
            ),
        });
    }

    validate_time_to_live(mtx, current_slot)?;
    validate_input_set_empty_utxo(transaction_body)?;
    validate_fee_too_small_utxo(transaction_body, tx_size, shelley_params)?;
    Ok(())
}

/// Validate transaction's TTL field
/// pass if ttl >= current_slot
/// Reference
/// https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Utxo.hs#L421
pub fn validate_time_to_live(
    tx: &alonzo::MintedTx,
    current_slot: u64,
) -> Result<(), TransactionValidationError> {
    if let Some(ttl) = tx.transaction_body.ttl {
        if ttl >= current_slot {
            Ok(())
        } else {
            Err(TransactionValidationError::ExpiredUTxO { ttl, current_slot })
        }
    } else {
        Err(TransactionValidationError::MalformedTransaction {
            era: Era::Shelley,
            reason: "TTL is missing".to_string(),
        })
    }
}

/// Validate every transaction must consume at least one UTxO
/// Reference: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Utxo.hs#L435
pub fn validate_input_set_empty_utxo(
    transaction_body: &alonzo::TransactionBody,
) -> Result<(), TransactionValidationError> {
    if transaction_body.inputs.is_empty() {
        Err(TransactionValidationError::InputSetEmptyUTxO)
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
    tx_size: u64,
    shelley_params: &ShelleyParams,
) -> Result<(), TransactionValidationError> {
    let min_fee = shelley_params.min_fee(tx_size);
    if transaction_body.fee < min_fee {
        Err(TransactionValidationError::FeeTooSmallUTxO {
            supplied: transaction_body.fee,
            required: min_fee,
        })
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
) -> Result<(), TransactionValidationError>
where
    F: Fn(TxOutRef) -> Result<TxIdentifier>,
{
    let bad_inputs = transaction_body
        .inputs
        .iter()
        .filter_map(|input| {
            let tx_ref = TxOutRef::new((*input.transaction_id).into(), input.index as u16);
            lookup_by_hash(tx_ref).is_err().then_some(tx_ref)
        })
        .collect::<Vec<_>>();

    if !bad_inputs.is_empty() {
        Err(TransactionValidationError::BadInputsUTxO { bad_inputs })
    } else {
        Ok(())
    }
}

/// Validate every output address match the network
/// Reference: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Utxo.hs#L481
pub fn validateWrongNetwork(
    transaction_body: &alonzo::TransactionBody,
    network_id: NetworkId,
) -> Result<(), TransactionValidationError> {
    let mut wrong_addresses = Vec::new();
    for output in transaction_body.outputs.iter() {
        let pallas_address = PallasAddress::from_bytes(output.address.as_ref()).map_err(|_| {
            TransactionValidationError::MalformedTransaction {
                era: Era::Shelley,
                reason: "Malformed address".to_string(),
            }
        })?;
        let address = map_address(&pallas_address).map_err(|e| {
            TransactionValidationError::MalformedTransaction {
                era: Era::Shelley,
                reason: format!("Invalid address: {}", e),
            }
        })?;
        let address_network = match address {
            Address::Shelley(sa) => sa.network,
            _ => {
                return Err(TransactionValidationError::MalformedTransaction {
                    era: Era::Shelley,
                    reason: format!(
                        "Not a Shelley Address: {}",
                        address.to_string().unwrap_or("Invalid Address format".to_string())
                    ),
                })
            }
        };
        if address_network != network_id {
            wrong_addresses.push(address);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{test_utils::TestContext, validation_fixture};
    use pallas::{codec as pallas_codec, ledger::primitives::alonzo::MintedTx as AlonzoMintedTx};
    use test_case::test_case;

    #[test_case(validation_fixture!("20ded0bfef32fc5eefba2c1f43bcd99acc0b1c3284617c3cb355ad0eadccaa6e") =>
        matches Ok(());
    )]
    #[test_case(validation_fixture!("20ded0bfef32fc5eefba2c1f43bcd99acc0b1c3284617c3cb355ad0eadccaa6e", "wrong_ttl") =>
        matches Err(TransactionValidationError::ExpiredUTxO { ttl: 7084747, current_slot: 7084748 });
    )]
    fn shelley_test(
        (ctx, raw_tx): (TestContext, Vec<u8>),
    ) -> Result<(), TransactionValidationError> {
        let mtx = pallas_codec::minicbor::decode::<AlonzoMintedTx>(&raw_tx).unwrap();
        validate_shelley_tx(&mtx, &ctx.shelley_params, ctx.current_slot)
    }
}
