use acropolis_common::{
    validation::TransactionValidationError, Era, GenesisDelegates, TxHash, UTXOValue,
    UTxOIdentifier,
};
use anyhow::Result;
use pallas::ledger::traverse::{Era as PallasEra, MultiEraTx};
mod shelley;

pub fn validate_shelley_tx<F>(
    raw_tx: &[u8],
    genesis_delegs: &GenesisDelegates,
    update_quorum: u32,
    lookup_utxo: F,
) -> Result<(), TransactionValidationError>
where
    F: Fn(UTxOIdentifier) -> Result<Option<UTXOValue>>,
{
    let tx = MultiEraTx::decode_for_era(PallasEra::Shelley, raw_tx)
        .map_err(|e| TransactionValidationError::CborDecodeError(e.to_string()))?;
    let tx_hash = TxHash::from(*tx.hash());

    let mtx = match tx {
        MultiEraTx::AlonzoCompatible(mtx, PallasEra::Shelley) => mtx,
        _ => {
            return Err(TransactionValidationError::MalformedTransaction {
                era: Era::Shelley,
                reason: "Not a Shelley transaction".to_string(),
            });
        }
    };

    shelley::utxo::validate(&mtx, &lookup_utxo).map_err(|e| *e)?;
    shelley::utxow::validate(&mtx, tx_hash, genesis_delegs, update_quorum, &lookup_utxo)
        .map_err(|e| *e)?;

    Ok(())
}
