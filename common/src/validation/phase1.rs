//! Phase 1 (native script / ledger rules) validation errors

use crate::{
    hash::Hash, Address, DataHash, DatumHash, Era, KeyHash, Lovelace, NetworkId, RedeemerPointer,
    ScriptHash, ScriptIntegrityHash, Slot, StakeAddress, UTxOIdentifier, VKeyWitness,
    ValidityInterval, ValueMap,
};
use std::collections::HashSet;
use thiserror::Error;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Error, PartialEq, Eq)]
pub enum Phase1ValidationError {
    /// **Cause**: Transaction is not in correct form.
    #[error(
        "Malformed Transaction: {}",
         errors.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("; ")
    )]
    MalformedTransaction { errors: Vec<String> },

    /// **Cause:** The UTXO has expired (Shelley only)
    #[error("Expired UTXO: ttl={ttl}, current_slot={current_slot}")]
    ExpiredUTxO { ttl: Slot, current_slot: Slot },

    /// **Cause:** The fee is too small.
    #[error("Fee is too small: supplied={supplied}, required={required}")]
    FeeTooSmallUTxO {
        supplied: Lovelace,
        required: Lovelace,
    },

    /// **Cause:** The transaction size is too big.
    #[error("Max tx size: supplied={supplied}, max={max}")]
    MaxTxSizeUTxO { supplied: u32, max: u32 },

    /// **Cause:** UTxO rules failure
    #[error("UTxOValidationError: {0}")]
    UTxOValidationError(#[from] UTxOValidationError),

    /// **Cause:** UTxOW rules failure
    #[error("UTxOWValidationError: {0}")]
    UTxOWValidationError(#[from] UTxOWValidationError),

    /// **Cause:** Other Phase 1 Validation Errors
    #[error("Other Phase 1 Validation Error: {0}")]
    Other(String),
}

/// UTxO Rules Failure
/// Shelley Era:
/// Reference: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Utxo.hs#L343
///
/// Allegra Era:
/// Reference: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/allegra/impl/src/Cardano/Ledger/Allegra/Rules/Utxo.hs#L160
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Error, PartialEq, Eq)]
pub enum UTxOValidationError {
    /// **Cause:** Malformed output
    #[error("Malformed output at {output_index}: {reason}")]
    MalformedOutput { output_index: usize, reason: String },

    /// **Cause:** Malformed withdrawal
    #[error("Malformed withdrawal at {withdrawal_index}: {reason}")]
    MalformedWithdrawal {
        withdrawal_index: usize,
        reason: String,
    },

    /// **Cause:** The input set is empty. (genesis transactions are exceptions)
    #[error("Input Set Empty UTXO")]
    InputSetEmptyUTxO,

    /// **Cause:** Some of transaction inputs are not in current UTxOs set.
    #[error("Bad inputs: bad_input={bad_input}, bad_input_index={bad_input_index}")]
    BadInputsUTxO {
        bad_input: UTxOIdentifier,
        bad_input_index: usize,
    },

    /// **Cause:** Some of transaction outputs are on a different network than the expected one.
    #[error(
        "Wrong network: expected={expected}, wrong_address={}, output_index={output_index}",
        wrong_address.to_string().unwrap_or("Invalid address".to_string()),
    )]
    WrongNetwork {
        expected: NetworkId,
        wrong_address: Address,
        output_index: usize,
    },

    /// **Cause:** Some of withdrawal accounts are on a different network than the expected one.
    #[error(
        "Wrong network withdrawal: expected={expected}, wrong_account={}, withdrawal_index={withdrawal_index}",
        wrong_account.to_string().unwrap_or("Invalid stake address".to_string()),
    )]
    WrongNetworkWithdrawal {
        expected: NetworkId,
        wrong_account: StakeAddress,
        withdrawal_index: usize,
    },

    /// **Cause:** The value of the UTXO is not conserved.
    /// Consumed = inputs + withdrawals + refunds, Produced = outputs + fees + deposits
    #[error("Value not conserved: consumed={consumed:?}, produced={produced:?}]")]
    ValueNotConservedUTxO {
        consumed: ValueMap,
        produced: ValueMap,
    },

    /// **Cause:** Some of the outputs don't have minimum required lovelace
    #[error(
        "Output too small UTxO: output_index={output_index}, lovelace={lovelace}, required_lovelace={required_lovelace}"
    )]
    OutputTooSmallUTxO {
        output_index: usize,
        lovelace: Lovelace,
        required_lovelace: Lovelace,
    },

    /// **Cause:** Transaction validity interval is not valid
    #[error("Transaction validity interval is not valid: current_slot={current_slot}, validity_interval={validity_interval:?}")]
    OutsideValidityIntervalUTxO {
        current_slot: Slot,
        validity_interval: ValidityInterval,
    },

    /// **Cause:** Transaction output's value size is too big
    #[error("Output too big UTxO: output_index={output_index}, value_size={value_size}, max_value_size={max_value_size}")]
    OutputTooBigUTxO {
        output_index: usize,
        value_size: u64,
        max_value_size: u64,
    },

    /// **Cause:** Malformed UTxO
    #[error("Malformed UTxO: era={era}, reason={reason}")]
    MalformedUTxO { era: Era, reason: String },

    /// **Cause:** Other UTxO Validation Errors
    #[error("Other UTxO Validation Error: {0}")]
    Other(String),
}

/// UTxOW Rules Failure
/// Shelley Era:
/// Reference: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Utxow.hs#L278
/// https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/alonzo/impl/src/Cardano/Ledger/Alonzo/Rules/Utxow.hs#L97
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Error, PartialEq, Eq)]
pub enum UTxOWValidationError {
    /// --------------------------- Shelley Era Errors
    /// ----------------------------------------------
    /// **Cause:** The VKey witness has invalid signature
    #[error("Invalid VKey witness: key_hash={key_hash}, witness={witness}")]
    InvalidWitnessesUTxOW {
        key_hash: KeyHash,
        witness: VKeyWitness,
    },

    /// **Cause:** Required VKey witness missing
    #[error("Missing VKey witness: key_hash={key_hash}")]
    MissingVKeyWitnessesUTxOW { key_hash: KeyHash },

    /// **Cause:** Required script witness missing
    #[error("Missing script witness: script_hash={script_hash}")]
    MissingScriptWitnessesUTxOW { script_hash: ScriptHash },

    /// **Cause:** Native script validation failed
    #[error("Native script validation failed: script_hash={script_hash}")]
    ScriptWitnessNotValidatingUTXOW { script_hash: ScriptHash },

    /// **Cause:** Extraneous script witness is provided
    #[error("Script provided but not used: script_hash={script_hash}")]
    ExtraneousScriptWitnessesUTXOW { script_hash: ScriptHash },

    /// **Cause:** Insufficient genesis signatures for MIR Tx
    #[error(
        "Insufficient Genesis Signatures for MIR: genesis_keys={}, count={}, quorum={}",
        genesis_keys.iter().map(|k| k.to_string()).collect::<Vec<_>>().join(","),
        genesis_keys.len(),
        quorum
    )]
    MIRInsufficientGenesisSigsUTXOW {
        genesis_keys: HashSet<Hash<28>>,
        quorum: u32,
    },

    /// **Cause:** Metadata without metadata hash
    #[error(
        "Metadata without metadata hash: full_hash={}",
        hex::encode(metadata_hash)
    )]
    MissingTxBodyMetadataHash { metadata_hash: DataHash },

    /// **Cause:** Metadata hash mismatch
    #[error(
        "Metadata hash mismatch: expected={}, actual={}",
        hex::encode(expected),
        hex::encode(actual)
    )]
    ConflictingMetadataHash {
        expected: DataHash,
        actual: DataHash,
    },

    /// **Cause:** Invalid metadata
    /// metadata - bytes, text - size (0..64)
    #[error("Invalid metadata: reason={reason}")]
    InvalidMetadata { reason: String },

    /// **Cause:** Invalid metadata hash
    #[error("Invalid metadata hash: reason={reason}")]
    InvalidMetadataHash { reason: String },

    /// **Cause:** Metadata hash without actual metadata
    #[error(
        "Metadata hash without actual metadata: hash={}",
        hex::encode(metadata_hash)
    )]
    MissingTxMetadata {
        // hash of metadata included in tx body
        metadata_hash: DataHash,
    },

    /// --------------------------- Alonzo Era Errors
    /// ----------------------------------------------
    /// **Cause:** Missing Redeemer
    #[error("Missing Redeemers: redeemer_pointer={redeemer_pointer:?}")]
    MissingRedeemers { redeemer_pointer: RedeemerPointer },

    /// **Cause:** Extra Redeemer
    #[error("Extra Redeemers: redeemer_pointer={redeemer_pointer:?}")]
    ExtraRedeemers { redeemer_pointer: RedeemerPointer },

    /// **Cause:** MissingRequiredDatums
    #[error("Missing required datums: datum_hash={datum_hash:?}")]
    MissingRequiredDatums { datum_hash: DatumHash },

    /// **Cause:** Extra Datum
    #[error("Not allowed supplemental datums: datum_hash={datum_hash}")]
    NotAllowedSupplementalDatums { datum_hash: DatumHash },

    /// **Cause:** Script integrity hash mismatch
    #[error(
        "Script integrity hash mismatch: expected={}, actual={}, reason={}",
        hex::encode(expected.unwrap_or_default()),
        hex::encode(actual.unwrap_or_default()),
        reason
    )]
    ScriptIntegrityHashMismatch {
        expected: Option<ScriptIntegrityHash>,
        actual: Option<ScriptIntegrityHash>,
        reason: String,
    },

    /// **Cause:** Unspendable UTxO without datum hash
    /// To spend a UTxO locked at Plutus scripts
    /// datum must be provided
    #[error("Unspendable UTxO without datum hash: utxo_identifier={utxo_identifier:?}, input_index={input_index}")]
    UnspendableUTxONoDatumHash {
        utxo_identifier: UTxOIdentifier,
        input_index: usize,
    },

    /// **Cause:** Malformed Script Witnesses
    #[error("Malformed script witnesses: script_hash={script_hash}, reason={reason}")]
    MalformedScriptWitnesses {
        script_hash: ScriptHash,
        reason: String,
    },

    /// **Cause:** Malformed Reference Script
    #[error("Malformed reference scripts: script_hash={script_hash}, reason={reason}")]
    MalformedReferenceScripts {
        script_hash: ScriptHash,
        reason: String,
    },

    /// **Cause:** Other UTxOW Validation Errors
    #[error("Other UTxOW Validation Error: {0}")]
    Other(String),
}
