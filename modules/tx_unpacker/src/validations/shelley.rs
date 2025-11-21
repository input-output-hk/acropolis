//! Shelley era transaction validation
//! Reference: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Utxo.hs#L343

use acropolis_common::{validation::TransactionValidationError, Era};
use pallas::ledger::{
    primitives::alonzo,
    traverse::{Era as PallasEra, MultiEraTx},
};

pub fn validate_shelley_tx(
    tx: &MultiEraTx,
    current_slot: u64,
) -> Result<(), TransactionValidationError> {
    let tx = match tx {
        MultiEraTx::AlonzoCompatible(tx, PallasEra::Shelley) => tx,
        _ => {
            return Err(TransactionValidationError::MalformedTransaction {
                era: Era::Shelley,
                reason: "Transaction is not Shelley compatible".to_string(),
            })
        }
    };

    validate_time_to_live(tx, current_slot)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{test_utils::TestContext, validation_fixture};
    use pallas::{codec, ledger::primitives::alonzo::MintedTx as AlonzoMintedTx};
    use test_case::test_case;

    #[test_case(validation_fixture!("20ded0bfef32fc5eefba2c1f43bcd99acc0b1c3284617c3cb355ad0eadccaa6e") =>
        matches Ok(());
    )]
    #[test_case(validation_fixture!("20ded0bfef32fc5eefba2c1f43bcd99acc0b1c3284617c3cb355ad0eadccaa6e", "invalid-ttl") =>
        matches Err(TransactionValidationError::ExpiredUTxO { ttl: 7084747, current_slot: 7084748 });
    )]
    fn shelley_test(
        (ctx, raw_tx): (TestContext, Vec<u8>),
    ) -> Result<(), TransactionValidationError> {
        let mtx = codec::minicbor::decode::<AlonzoMintedTx>(&raw_tx).unwrap();
        let metx = MultiEraTx::from_alonzo_compatible(&mtx, PallasEra::Shelley);
        validate_shelley_tx(&metx, ctx.current_slot)
    }
}
