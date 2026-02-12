//! Validation results for Acropolis consensus

// We don't use these types in the acropolis_common crate itself
#![allow(dead_code)]

use crate::{
    hash::Hash,
    messages::{CardanoMessage::BlockValidation, Message},
    protocol_params::{Nonce, ProtocolVersion},
    rational_number::RationalNumber,
    Address, BlockInfo, CommitteeCredential, DataHash, DatumHash, Era, GenesisKeyhash, GovActionId,
    KeyHash, Lovelace, NetworkId, PoolId, ProposalProcedure, RedeemerPointer, ScriptHash,
    ScriptIntegrityHash, Slot, StakeAddress, UTxOIdentifier, VKeyWitness, Value, Voter, VrfKeyHash,
};
use anyhow::bail;
use caryatid_sdk::Context;
use std::{
    array::TryFromSliceError,
    collections::HashSet,
    fmt::{Debug, Display, Formatter},
    sync::Arc,
};
use thiserror::Error;
use tracing::error;

/// Validation status
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum ValidationStatus {
    /// All good
    Go,

    /// Error
    NoGo(ValidationError),
}

impl ValidationStatus {
    pub fn is_go(&self) -> bool {
        matches!(self, ValidationStatus::Go)
    }

    pub fn compose(&mut self, status: ValidationStatus) {
        if self.is_go() {
            *self = status;
        }
    }
}

/// Validation error
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Error)]
pub enum ValidationError {
    // Either a very peculiar and uncommon error, or a temporary substitution,
    // which is to be later replaced with specific error.
    #[error("Unclassified validation error: {0}")]
    Unclassified(String),

    #[error("VRF failure: {0}")]
    BadVRF(#[from] VrfValidationError),

    #[error("KES failure: {0}")]
    BadKES(#[from] KesValidationError),

    #[error(
        "bad_transactions: {}", 
        bad_transactions
            .iter()
            .map(|(tx_index, error)| format!("tx-index={tx_index}, error={error}"))
            .collect::<Vec<_>>()
            .join("; ")
    )]
    BadTransactions {
        bad_transactions: Vec<(u16, TransactionValidationError)>,
    },

    #[error("Governance failure: {0}")]
    BadGovernance(#[from] GovernanceValidationError),

    #[error("CBOR Decoding error")]
    CborDecodeError {
        era: Era,
        slot: Slot,
        reason: String,
    },
}

/// Transaction Validation Error
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Error, PartialEq, Eq)]
pub enum TransactionValidationError {
    /// **Cause**: Raw Transaction CBOR is invalid
    #[error("CBOR Decoding error: era={era}, reason={reason}")]
    CborDecodeError { era: Era, reason: String },

    /// **Cause**: Phase 1 Validation Error
    #[error("Phase 1 Validation Failed: {0}")]
    Phase1ValidationError(#[from] Phase1ValidationError),

    /// **Cause:** Other errors (e.g. Invalid shelley params)
    #[error("{0}")]
    Other(String),
}

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
    ValueNotConservedUTxO { consumed: Value, produced: Value },

    /// **Cause:** Some of the outputs don't have minimum required lovelace
    #[error(
        "Output too small UTxO: output_index={output_index}, lovelace={lovelace}, required_lovelace={required_lovelace}"
    )]
    OutputTooSmallUTxO {
        output_index: usize,
        lovelace: Lovelace,
        required_lovelace: Lovelace,
    },

    /// **Cause:** Malformed UTxO
    #[error("Malformed UTxO: era={era}, reason={reason}")]
    MalformedUTxO { era: Era, reason: String },
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
    #[error("Unspendable UTxO without datum hash: utxo_identifier={utxo_identifier:?}")]
    UnspendableUTxONoDatumHash { utxo_identifier: UTxOIdentifier },
}

/// Reference
/// https://github.com/IntersectMBO/ouroboros-consensus/blob/e3c52b7c583bdb6708fac4fdaa8bf0b9588f5a88/ouroboros-consensus-protocol/src/ouroboros-consensus-protocol/Ouroboros/Consensus/Protocol/Praos.hs#L342
#[derive(Error, Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq)]
pub enum VrfValidationError {
    /// **Cause:** Block issuer's pool ID is not registered in current stake distribution
    #[error("Unknown Pool: {}", hex::encode(pool_id))]
    UnknownPool { pool_id: PoolId },

    /// **Cause:** The VRF key hash in the block header doesn't match the VRF key
    /// registered with this stake pool in the ledger state for Overlay slot
    #[error("{0}")]
    WrongGenesisLeaderVrfKey(#[from] WrongGenesisLeaderVrfKeyError),

    /// **Cause:** The VRF key hash in the block header doesn't match the VRF key
    /// registered with this stake pool in the ledger state
    #[error("{0}")]
    WrongLeaderVrfKey(#[from] WrongLeaderVrfKeyError),

    /// VRF nonce proof verification failed (TPraos rho - nonce proof)
    /// **Cause:** The (rho - nonce) VRF proof failed verification
    #[error("{0}")]
    TPraosBadNonceVrfProof(#[from] TPraosBadNonceVrfProofError),

    /// VRF leader proof verification failed (TPraos y - leader proof)
    /// **Cause:** The (y - leader) VRF proof failed verification
    #[error("{0}")]
    TPraosBadLeaderVrfProof(#[from] TPraosBadLeaderVrfProofError),

    /// VRF proof cryptographic verification failed (Praos single proof)
    /// **Cause:** The cryptographic VRF proof is invalid
    #[error("{0}")]
    PraosBadVrfProof(#[from] PraosBadVrfProofError),

    /// **Cause:** The VRF output is too large for this pool's stake.
    /// The pool lost the slot lottery
    #[error("{0}")]
    VrfLeaderValueTooBig(#[from] VrfLeaderValueTooBigError),

    /// **Cause:** This slot is in the overlay schedule but marked as non-active.
    /// It's an intentional gap slot where no blocks should be produced.
    #[error("Not Active slot in overlay schedule: {slot}")]
    NotActiveSlotInOverlaySchedule { slot: Slot },

    /// **Cause:** Some data has incorrect bytes
    #[error("TryFromSlice: {0}")]
    TryFromSlice(String),

    /// **Cause:** Other errors (e.g. Invalid shelley params, praos params, missing data)
    #[error("{0}")]
    Other(String),
}

/// Validation function for VRF
pub type VrfValidation<'a> = Box<dyn Fn() -> Result<(), VrfValidationError> + Send + Sync + 'a>;

// ------------------------------------------------------------ WrongGenesisLeaderVrfKeyError

#[derive(Error, Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[error(
    "Wrong Genesis Leader VRF Key: Genesis Key={}, Registered VRF Hash={}, Header VRF Hash={}",
    hex::encode(genesis_key),
    hex::encode(registered_vrf_hash),
    hex::encode(header_vrf_hash)
)]
pub struct WrongGenesisLeaderVrfKeyError {
    pub genesis_key: GenesisKeyhash,
    pub registered_vrf_hash: VrfKeyHash,
    pub header_vrf_hash: VrfKeyHash,
}

// ------------------------------------------------------------ WrongLeaderVrfKeyError

#[derive(Error, Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[error(
    "Wrong Leader VRF Key: Pool ID={}, Registered VRF Key Hash={}, Header VRF Key Hash={}",
    hex::encode(pool_id),
    hex::encode(registered_vrf_key_hash),
    hex::encode(header_vrf_key_hash)
)]
pub struct WrongLeaderVrfKeyError {
    pub pool_id: PoolId,
    pub registered_vrf_key_hash: VrfKeyHash,
    pub header_vrf_key_hash: VrfKeyHash,
}

// ------------------------------------------------------------ TPraosBadNonceVrfProofError

#[derive(Error, Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum TPraosBadNonceVrfProofError {
    #[error("Bad Nonce VRF Proof: Slot={0}, Epoch Nonce={1}, Bad VRF Proof={2}")]
    BadVrfProof(Slot, Nonce, BadVrfProofError),
}

// ------------------------------------------------------------ TPraosBadLeaderVrfProofError

#[derive(Error, Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum TPraosBadLeaderVrfProofError {
    #[error("Bad Leader VRF Proof: Slot={0}, Epoch Nonce={1}, Bad VRF Proof={2}")]
    BadVrfProof(Slot, Nonce, BadVrfProofError),
}

// ------------------------------------------------------------ PraosBadVrfProofError

#[derive(Error, Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum PraosBadVrfProofError {
    #[error("Bad VRF proof: Slot={0}, Epoch Nonce={1}, Bad VRF Proof={2}")]
    BadVrfProof(Slot, Nonce, BadVrfProofError),

    #[error(
        "Mismatch between the declared VRF output in block ({}) and the computed one ({}).",
        hex::encode(declared),
        hex::encode(computed)
    )]
    OutputMismatch {
        declared: Vec<u8>,
        computed: Vec<u8>,
    },
}

impl PartialEq for PraosBadVrfProofError {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::BadVrfProof(l0, l1, l2), Self::BadVrfProof(r0, r1, r2)) => {
                l0 == r0 && l1 == r1 && l2 == r2
            }
            (
                Self::OutputMismatch {
                    declared: l_declared,
                    computed: l_computed,
                },
                Self::OutputMismatch {
                    declared: r_declared,
                    computed: r_computed,
                },
            ) => l_declared == r_declared && l_computed == r_computed,
            _ => false,
        }
    }
}

// ------------------------------------------------------------ VrfLeaderValueTooBigError
#[derive(Error, Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum VrfLeaderValueTooBigError {
    #[error("VRF Leader Value Too Big: pool_id={pool_id}, active_stake={active_stake}, relative_stake={relative_stake}")]
    VrfLeaderValueTooBig {
        pool_id: PoolId,
        active_stake: u64,
        relative_stake: RationalNumber,
    },
}

// ------------------------------------------------------------ BadVrfProofError

#[derive(Error, Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum BadVrfProofError {
    #[error("Malformed VRF proof: {0}")]
    MalformedProof(String),

    #[error("Invalid VRF proof: {0}")]
    /// (error, vrf_input_hash, vrf_public_key_hash)
    InvalidProof(String, Vec<u8>, Vec<u8>),

    #[error("could not convert slice to array")]
    TryFromSliceError,

    #[error(
        "Mismatch between the declared VRF proof hash ({}) and the computed one ({}).",
        hex::encode(declared),
        hex::encode(computed)
    )]
    ProofMismatch {
        // this is Proof Hash (sha512 hash)
        declared: Vec<u8>,
        computed: Vec<u8>,
    },
}

impl From<TryFromSliceError> for BadVrfProofError {
    fn from(_: TryFromSliceError) -> Self {
        Self::TryFromSliceError
    }
}

impl PartialEq for BadVrfProofError {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::MalformedProof(l0), Self::MalformedProof(r0)) => l0 == r0,
            (Self::InvalidProof(l0, l1, l2), Self::InvalidProof(r0, r1, r2)) => {
                l0 == r0 && l1 == r1 && l2 == r2
            }
            (Self::TryFromSliceError, Self::TryFromSliceError) => true,
            (
                Self::ProofMismatch {
                    declared: l_declared,
                    computed: l_computed,
                },
                Self::ProofMismatch {
                    declared: r_declared,
                    computed: r_computed,
                },
            ) => l_declared == r_declared && l_computed == r_computed,
            _ => false,
        }
    }
}

/// Reference
/// https://github.com/IntersectMBO/ouroboros-consensus/blob/e3c52b7c583bdb6708fac4fdaa8bf0b9588f5a88/ouroboros-consensus-protocol/src/ouroboros-consensus-protocol/Ouroboros/Consensus/Protocol/Praos.hs#L342
#[derive(Error, Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq)]
pub enum KesValidationError {
    /// **Cause:** The KES signature on the block header is invalid.
    #[error("KES Signature Error: {0}")]
    KesSignatureError(#[from] KesSignatureError),
    /// **Cause:** The operational certificate is invalid.
    #[error("Operational Certificate Error: {0}")]
    OperationalCertificateError(#[from] OperationalCertificateError),
    /// **Cause:** No OCert counter found for this issuer (not a stake pool or genesis delegate)
    #[error("No OCert Counter For Issuer: Pool ID={}", hex::encode(pool_id))]
    NoOCertCounter { pool_id: PoolId },
    /// **Cause:** Some data has incorrect bytes
    #[error("TryFromSlice: {0}")]
    TryFromSlice(String),
    #[error("Other Kes Validation Error: {0}")]
    Other(String),
}

/// Validation function for Kes
pub type KesValidation<'a> = Box<dyn Fn() -> Result<(), KesValidationError> + Send + Sync + 'a>;

#[derive(Error, Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq)]
pub enum KesSignatureError {
    /// **Cause:** Current KES period is before the operational certificate's
    /// start period.
    #[error(
        "KES Before Start OCert: OCert Start Period={}, Current Period={}",
        ocert_start_period,
        current_period
    )]
    KesBeforeStartOcert {
        ocert_start_period: u64,
        current_period: u64,
    },
    /// **Cause:** Current KES period exceeds the operational certificate's
    /// validity period.
    #[error(
        "KES After End OCert: Current Period={}, OCert Start Period={}, Max KES Evolutions={}",
        current_period,
        ocert_start_period,
        max_kes_evolutions
    )]
    KesAfterEndOcert {
        current_period: u64,
        ocert_start_period: u64,
        max_kes_evolutions: u64,
    },
    /// **Cause:** The KES signature on the block header is cryptographically invalid.
    #[error(
        "Invalid KES Signature OCert: Current Period={}, OCert Start Period={}, Reason={}",
        current_period,
        ocert_start_period,
        reason
    )]
    InvalidKesSignatureOcert {
        current_period: u64,
        ocert_start_period: u64,
        reason: String,
    },
}

#[derive(Error, Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq)]
pub enum OperationalCertificateError {
    /// **Cause:** The operational certificate is malformed.
    #[error("Malformed Signature OCert: Reason={}", reason)]
    MalformedSignatureOcert { reason: String },
    /// **Cause:** The cold key signature on the operational certificate is invalid.
    /// The OCert was not properly signed by the pool's cold key.
    #[error(
        "Invalid Signature OCert: Issuer={}, Pool ID={}",
        hex::encode(issuer),
        hex::encode(pool_id)
    )]
    InvalidSignatureOcert { issuer: Vec<u8>, pool_id: PoolId },
    /// **Cause:** The operational certificate counter in the header is not greater
    /// than the last counter used by this pool.
    #[error(
        "Counter Too Small OCert: Latest Counter={}, Declared Counter={}",
        latest_counter,
        declared_counter
    )]
    CounterTooSmallOcert {
        latest_counter: u64,
        declared_counter: u64,
    },
    /// **Cause:** OCert counter jumped by more than 1. While not strictly invalid,
    /// this is suspicious and may indicate key compromise. (Praos Only)
    #[error(
        "Counter Over Incremented OCert: Latest Counter={}, Declared Counter={}",
        latest_counter,
        declared_counter
    )]
    CounterOverIncrementedOcert {
        latest_counter: u64,
        declared_counter: u64,
    },
    /// **Cause:** No counter found for this key hash (not a stake pool or genesis delegate)
    #[error("No Counter For Key Hash OCert: Pool ID={}", hex::encode(pool_id))]
    NoCounterForKeyHashOcert { pool_id: PoolId },
}

/// Partial formalization of validation outcome errors, relation between entities
/// See Haskell Node, Cardano.Ledger.BaseTypes: Cardano/Src/Ledger/BaseTypes.hs
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub enum MismatchRelation {
    Eq,
    Lt,
    Gt,
    LtEq,
    GtEq,
    Subset,
}

impl Display for MismatchRelation {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let str = match self {
            MismatchRelation::Eq => "=",
            MismatchRelation::Lt => "<",
            MismatchRelation::Gt => ">",
            MismatchRelation::LtEq => "<=",
            MismatchRelation::GtEq => ">=",
            MismatchRelation::Subset => " in ",
        };
        write!(f, "{}", str)
    }
}

/// Partial formalization of validation outcome errors: what's wrong with relation of two entities
/// See Haskell Node, Cardano.Ledger.BaseTypes: Cardano/Src/Ledger/BaseTypes.hs
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct Mismatch<T: Debug + Display> {
    supplied: T,
    expected: T,
    expected_rel: MismatchRelation,
}

impl<T: Debug + Display> Display for Mismatch<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Supplied: {}, expected: {} {}",
            self.supplied, self.expected_rel, self.expected
        )
    }
}

/// See Haskell node, "GOV" rule in Conway epoch, data ConwayGovPredFailure era
/// also, "PPUP" rule in Shelley epoch, data ShelleyPpupPredFailure era
#[derive(Error, Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum GovernanceValidationError {
    #[error("Governance action from protocol {0} is not allowed in current protocol version")]
    WrongProtocolForGovernance(ProtocolVersion),

    /// An update was proposed by a key hash that is not one of the genesis keys.
    /// `mismatchSupplied` ~ key hashes which were a part of the update.
    /// `mismatchExpected` ~ key hashes of the genesis keys.
    #[error("Parameter update from non-genesis key hash")]
    NonGenesisUpdatePPUP(Mismatch<KeyHash>),

    /// | An update was proposed for the wrong epoch.
    /// The first 'EpochNo' is the current epoch.
    /// The second 'EpochNo' is the epoch listed in the update.
    /// The last parameter indicates if the update was intended
    /// for the current (true) or the next epoch (false).
    #[error("Parameter update for wrong epoch: current {0}, requested {1}, requested epoch is current {2}")]
    PPUpdateWrongEpoch(u64, u64, bool),

    /// | An update was proposed which contains an invalid protocol version.
    /// New protocol versions must either increase the major
    /// number by exactly one and set the minor version to zero,
    /// or keep the major version the same and increase the minor
    /// version by exactly one.
    #[error("Protocol update contains impossible new protocol version {0}")]
    PVCannotFollowPPUP(ProtocolVersion),

    #[error("Governance actions {action_id:?} do not exist")]
    GovActionsDoNotExist { action_id: Vec<GovActionId> },

    #[error("Malformed conway proposal {action:?}")]
    MalformedConwayProposal { action: ProposalProcedure }, // TODO: add parameter (GovAction era)

    #[error("Proposal procedure network id mismatch: {reward_account:?} and {network:?}")]
    ProposalProcedureNetworkIdMismatch {
        reward_account: StakeAddress,
        network: NetworkId,
    },

    #[error("Treasury withdrawals network id mismatch: {reward_accounts:?} and {network:?}")]
    TreasuryWithdrawalsNetworkIdMismatch {
        reward_accounts: Vec<StakeAddress>,
        network: NetworkId,
    },

    #[error("Proposal deposit mismatch: {0}")]
    ProposalDepositIncorrect(Mismatch<Lovelace>),

    // Some governance actions are not allowed to be voted on by certain types of
    // Voters. This failure lists all governance action ids with their respective voters
    // that are not allowed to vote on those governance actions.
    #[error("Voters are not allowed for the actions: {0:?}")]
    DisallowedVoters(Vec<(Voter, GovActionId)>),

    // Credentials that are mentioned as members to be both removed and added
    #[error("Committee members both removed and added: {0:?}")]
    ConflictingCommitteeUpdate(Vec<CommitteeCredential>),

    // Members for which the expiration epoch has already been reached
    #[error("Committee members already expired: {0:?}")]
    ExpirationEpochTooSmall(Vec<(CommitteeCredential, u64)>),

    #[error("InvalidPrevGovActionId: {0}")]
    InvalidPrevGovActionId(GovActionId),

    #[error("Voting on expired governance action {0:?}")]
    VotingOnExpiredGovAction(Vec<(Voter, GovActionId)>),

    //The PrevGovActionId of the HardForkInitiation that fails
    // Its protocol version and the protocal version of the previous
    // gov-action pointed to by the proposal
    #[error("Hard fork initiation {purpose:?} mismatches protocol version: {version_mismatch}")]
    ProposalCantFollow {
        purpose: (),
        version_mismatch: Mismatch<ProtocolVersion>,
    },
    //  (StrictMaybe (GovPurposeId 'HardForkPurpose era))
    //  (Mismatch 'RelGT ProtVer)
    #[error("Invalid policy hash: proposed {proposed:?}, current {current:?}")]
    InvalidPolicyHash {
        proposed: Option<ScriptHash>,
        current: Option<ScriptHash>,
    },

    #[error("Conway bootstrap era does not allow proposal {0:?}")]
    DisallowedProposalDuringBootstrap(ProposalProcedure),

    #[error("Conway bootstrap era does not allow votes {0:?}")]
    DisallowedVotesDuringBootstrap(Vec<(Voter, GovActionId)>),

    // Predicate failure for votes by entities that are not present in the ledger state
    #[error("Voters do not present in ledger state: {0:?}")]
    VotersDoNotExist(Vec<Voter>),

    // Treasury withdrawals that sum up to zero are not allowed
    #[error("Zero treausury withdrawals in {0}")]
    ZeroTreasuryWithdrawals(GovActionId),

    // Proposals that have an invalid reward account for returns of the deposit
    #[error("Return account {0} for the proposal does not exist")]
    ProposalReturnAccountDoesNotExist(StakeAddress),

    // Treasury withdrawal proposals to an invalid reward account
    #[error("Treasury withdrawal return account {0} does not exist")]
    TreasuryWithdrawalReturnAccountsDoNotExist(StakeAddress),
}

// Utils for easier validation routines development

#[derive(Default, Clone)]
pub struct ValidationOutcomes {
    outcomes: Vec<ValidationError>,
}

impl ValidationOutcomes {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn merge(&mut self, with: &mut ValidationOutcomes) {
        self.outcomes.append(&mut with.outcomes);
    }

    pub fn push(&mut self, outcome: ValidationError) {
        self.outcomes.push(outcome);
    }

    pub fn push_anyhow(&mut self, error: anyhow::Error) {
        self.outcomes.push(ValidationError::Unclassified(format!("{}", error)));
    }

    pub fn print_errors(&mut self, module: &str, block: Option<&BlockInfo>) {
        if !self.outcomes.is_empty() {
            error!("{module}: block {block:?}, outcomes {:?}", self.outcomes);
            self.outcomes.clear();
        }
    }

    pub async fn publish(
        &mut self,
        context: &Arc<Context<Message>>,
        module: &str,
        topic_field: &str,
        block: &BlockInfo,
    ) -> anyhow::Result<()> {
        if block.intent.do_validation() {
            let status = if let Some(result) = self.outcomes.first() {
                // TODO: add multiple responses / decide that they're not necessary
                ValidationStatus::NoGo(result.clone())
            } else {
                ValidationStatus::Go
            };

            let outcome_msg = Arc::new(Message::Cardano((block.clone(), BlockValidation(status))));

            context.message_bus.publish(topic_field, outcome_msg).await?;
        } else {
            self.print_errors(module, Some(block));
        }
        self.outcomes.clear();
        Ok(())
    }

    #[allow(dead_code)]
    pub fn as_result(&self) -> anyhow::Result<()> {
        if self.outcomes.is_empty() {
            return Ok(());
        }

        let res = self.outcomes.iter().map(|e| format!("{}; ", e)).collect::<String>();

        bail!("Validation failed: {}", res)
    }
}
