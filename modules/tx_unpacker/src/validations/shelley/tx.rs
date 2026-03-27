//! Shelley era transaction validation
//! Reference: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Utxo.hs#L343

use acropolis_common::{protocol_params::ProtocolParams, validation::Phase1ValidationError, Era};
use anyhow::Result;
use pallas::ledger::traverse::MultiEraTx;
pub type Phase1ValidationResult = Result<(), Box<Phase1ValidationError>>;

pub fn validate(
    tx: &MultiEraTx,
    protocol_params: &ProtocolParams,
    current_slot: u64,
    era: Era,
) -> Phase1ValidationResult {
    // This check is only for shelley
    validate_time_to_live(tx.ttl(), current_slot, era)?;

    let tx_fee = tx.fee().unwrap_or(0);
    let tx_size = tx.size() as u32;
    validate_fee_too_small_utxo(tx_fee, tx_size, protocol_params)?;
    validate_max_tx_size_utxo(tx_size, protocol_params)?;
    Ok(())
}

/// Validate transaction's TTL field
/// pass if ttl >= current_slot
/// Reference: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Utxo.hs#L421
pub fn validate_time_to_live(
    ttl: Option<u64>,
    current_slot: u64,
    era: Era,
) -> Phase1ValidationResult {
    if let Some(ttl) = ttl {
        if ttl >= current_slot {
            Ok(())
        } else {
            Err(Box::new(Phase1ValidationError::ExpiredUTxO {
                ttl,
                current_slot,
            }))
        }
    } else if era == Era::Shelley {
        Err(Box::new(Phase1ValidationError::MalformedTransaction {
            errors: vec!["TTL is missing for Shelley Tx".to_string()],
        }))
    } else {
        Ok(())
    }
}

/// Validate every transaction has minimum fee required
/// Fee calculation:
/// minFee = (tx_size_in_bytes * min_a) + min_b + ref_script_fee (this is after Alonzo Era)
/// Reference: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Utxo.hs#L447
pub fn validate_fee_too_small_utxo(
    fee: u64,
    tx_size: u32,
    protocol_params: &ProtocolParams,
) -> Phase1ValidationResult {
    let Some(shelley_params) = protocol_params.shelley.as_ref() else {
        return Err(Box::new(Phase1ValidationError::Other(
            "Shelley params are not set".to_string(),
        )));
    };
    let min_fee = shelley_params.min_fee(tx_size);
    if fee < min_fee {
        Err(Box::new(Phase1ValidationError::FeeTooSmallUTxO {
            supplied: fee,
            required: min_fee,
        }))
    } else {
        Ok(())
    }
}

/// Validate transaction size is under the limit
/// Reference: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Utxo.hs#L575
pub fn validate_max_tx_size_utxo(
    tx_size: u32,
    protocol_params: &ProtocolParams,
) -> Phase1ValidationResult {
    let Some(shelley_params) = protocol_params.shelley.as_ref() else {
        return Err(Box::new(Phase1ValidationError::Other(
            "Shelley params are not set".to_string(),
        )));
    };
    let max_tx_size = shelley_params.protocol_params.max_tx_size;
    if tx_size > max_tx_size {
        Err(Box::new(Phase1ValidationError::MaxTxSizeUTxO {
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
    use crate::{
        test_utils::{to_era, to_pallas_era, TestContext},
        validation_fixture,
    };
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
        "expired_utxo"
    ) =>
        matches Err(Phase1ValidationError::ExpiredUTxO { ttl: 7084747, current_slot: 7084748 });
        "expired_utxo"
    )]
    #[test_case(validation_fixture!(
        "shelley",
        "20ded0bfef32fc5eefba2c1f43bcd99acc0b1c3284617c3cb355ad0eadccaa6e",
        "fee_too_small_utxo"
    ) =>
        matches Err(Phase1ValidationError::FeeTooSmallUTxO { supplied: 22541, required: 172277 });
        "fee_too_small_utxo"
    )]
    #[test_case(validation_fixture!(
        "shelley",
        "20ded0bfef32fc5eefba2c1f43bcd99acc0b1c3284617c3cb355ad0eadccaa6e",
        "max_tx_size_utxo"
    ) =>
        matches Err(Phase1ValidationError::MaxTxSizeUTxO { supplied: 17983, max: 16384 });
        "max_tx_size_utxo"
    )]
    #[allow(clippy::result_large_err)]
    fn shelley_tx_test(
        (ctx, raw_tx, era): (TestContext, Vec<u8>, &str),
    ) -> Result<(), Phase1ValidationError> {
        let tx = MultiEraTx::decode_for_era(to_pallas_era(era), &raw_tx).unwrap();
        let era = to_era(era);
        validate(&tx, &ctx.protocol_params, ctx.current_slot, era).map_err(|e| *e)
    }
}
