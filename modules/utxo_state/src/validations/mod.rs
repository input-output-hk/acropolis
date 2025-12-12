use acropolis_common::{
    validation::{Phase1ValidationError, TransactionValidationError},
    AlonzoBabbageUpdateProposal, GenesisDelegates, NativeScript, TxCertificateWithPos, TxHash,
    UTXOValue, UTxOIdentifier, VKeyWitness, Withdrawal,
};
use anyhow::Result;
mod shelley;

#[allow(clippy::too_many_arguments)]
pub fn validate_shelley_tx<F>(
    tx_hash: TxHash,
    inputs: &[UTxOIdentifier],
    certificates: &[TxCertificateWithPos],
    withdrawals: &[Withdrawal],
    alonzo_babbage_update_proposal: &Option<AlonzoBabbageUpdateProposal>,
    vkey_witnesses: &[VKeyWitness],
    native_scripts: &[NativeScript],
    low_bnd: Option<u64>,
    upp_bnd: Option<u64>,
    genesis_delegs: &GenesisDelegates,
    update_quorum: u32,
    lookup_utxo: F,
) -> Result<(), TransactionValidationError>
where
    F: Fn(&UTxOIdentifier) -> Result<Option<UTXOValue>>,
{
    shelley::utxo::validate(inputs, &lookup_utxo)
        .map_err(|e| Phase1ValidationError::UTxOValidationError(*e))?;
    shelley::utxow::validate(
        tx_hash,
        inputs,
        certificates,
        withdrawals,
        alonzo_babbage_update_proposal,
        vkey_witnesses,
        native_scripts,
        low_bnd,
        upp_bnd,
        genesis_delegs,
        update_quorum,
        &lookup_utxo,
    )
    .map_err(|e| Phase1ValidationError::UTxOWValidationError(*e))?;

    Ok(())
}
