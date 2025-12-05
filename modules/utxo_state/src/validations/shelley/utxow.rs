//! Shelley era UTxOW Rules
//! Reference: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Utxow.hs#L278

use acropolis_common::{validation::UTxOWValidationError, UTXOValue, UTxOIdentifier};
use anyhow::Result;
use pallas::ledger::primitives::alonzo;

pub fn validate_withnesses<F>(
    tx: &alonzo::MintedTx,
    lookup_utxo: F,
) -> Result<(), Box<UTxOWValidationError>>
where
    F: Fn(UTxOIdentifier) -> Result<Option<UTXOValue>>,
{
    for (input_index, input) in tx.transaction_body.inputs.iter().enumerate() {
        let utxo_identifier = UTxOIdentifier::new(input.
        match lookup_utxo(input) {
            Ok(Some(utxo)) => {
                if let Some(alonzo_comp_output) = MultiEraOutput::as_alonzo(multi_era_output) {
                    match get_payment_part(&alonzo_comp_output.address)
                        .ok_or(ShelleyMA(AddressDecoding))?
                    {
                        ShelleyPaymentPart::Key(payment_key_hash) => {
                            check_vk_wit(&payment_key_hash, tx_hash, vk_wits)?
                        }
                        ShelleyPaymentPart::Script(script_hash) => check_native_script_witness(
                            &script_hash,
                            &tx_wits
                                .native_script
                                .as_ref()
                                .map(|x| x.iter().map(|y| y.deref().clone()).collect()),
                        )?,
                    }
                }
            }
            None => return Err(ShelleyMA(InputNotInUTxO)),
        }
    }
}
