//! Validation results for Acropolis consensus

// We don't use these types in the acropolis_common crate itself
#![allow(dead_code)]

use std::array::TryFromSliceError;
use std::fmt::{Debug, Display, Formatter};
use thiserror::Error;

use crate::{
    protocol_params::{Nonce, ProtocolVersion},
    rational_number::RationalNumber, 
    Address, Era, CommitteeCredential, GenesisKeyhash, GovActionId, 
    Lovelace, NetworkId, PoolId, ProposalProcedure, Slot, StakeAddress, 
    TxOutRef, Value, Voter, VrfKeyHash,
};

/// Transaction Validation Error
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Error, PartialEq, Eq)]
pub enum TransactionValidationError {
    /// **Cause**: Raw Transaction CBOR is invalid
    #[error("CBOR Decoding error: {0}")]
    CborDecodeError(String),

    /// **Cause**: Transaction is not in correct form.
    #[error("Malformed Transaction: era={era}, reason={reason}")]
    MalformedTransaction { era: Era, reason: String },

    /// **Cause**: UTxO rules failure
    #[error("{0}")]
    UTxOValidationError(#[from] UTxOValidationError),

    /// **Cause:** Other errors (e.g. Invalid shelley params)
    #[error("{0}")]
    Other(String),
}

/// UTxO rules failure
/// Shelley Era Errors:
/// Reference: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Utxo.hs#L343
///
/// Allegra Era Errors:
/// Reference: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/allegra/impl/src/Cardano/Ledger/Allegra/Rules/Utxo.hs#L160
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Error, PartialEq, Eq)]
pub enum UTxOValidationError {
    /// ------------ Shelley Era Errors ------------
    /// **Cause:** The UTXO has expired
    #[error("Expired UTXO: ttl={ttl}, current_slot={current_slot}")]
    ExpiredUTxO { ttl: Slot, current_slot: Slot },

    /// **Cause:** The input set is empty. (genesis transactions are exceptions)
    #[error("Input Set Empty UTXO")]
    InputSetEmptyUTxO,

    /// **Cause:** The fee is too small.
    #[error("Fee is too small: supplied={supplied}, required={required}")]
    FeeTooSmallUTxO {
        supplied: Lovelace,
        required: Lovelace,
    },

    /// **Cause:** Some of transaction inputs are not in current UTxOs set.
    #[error("Bad inputs: bad_input={bad_input}, bad_input_index={bad_input_index}")]
    BadInputsUTxO {
        bad_input: TxOutRef,
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

    /// **Cause:** The transaction size is too big.
    #[error("Max tx size: supplied={supplied}, max={max}")]
    MaxTxSizeUTxO { supplied: u32, max: u32 },

    /// **Cause:** Malformed UTxO
    #[error("Malformed UTxO: era={era}, reason={reason}")]
    MalformedUTxO { era: Era, reason: String },
}

/// Validation error
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Error)]
pub enum ValidationError {
    #[error("Uncategorized validation error: {0}")]
    Unclassified(String),

    #[error("VRF failure: {0}")]
    BadVRF(#[from] VrfValidationError),

    #[error("KES failure: {0}")]
    BadKES(#[from] KesValidationError),

    #[error("Governance failure: {0}")]
    BadGovernance(#[from] GovernanceValidationError),

    #[error("Invalid Transaction: tx-index={tx_index}, error={error}")]
    BadTransaction {
        tx_index: u16,
        error: TransactionValidationError,
    },

    #[error("CBOR Decoding error")]
    CborDecodeError(usize, String),

    #[error("Malformed transaction")]
    MalformedTransaction(u16, String),

    #[error("Doubly spent UTXO: {0}")]
    DoubleSpendUTXO(String),
}

/// Validation status
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
    RelEq,
    RelLt,
    RelGt,
    RelLtEq,
    RelGtEq,
    RelSubset
}

impl Display for MismatchRelation {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let str = match self {
            MismatchRelation::RelEq => "=",
            MismatchRelation::RelLt => "<",
            MismatchRelation::RelGt => ">",
            MismatchRelation::RelLtEq => "<=",
            MismatchRelation::RelGtEq => ">=",
            MismatchRelation::RelSubset => " in "
        };
        write!(f, "{}", str)
    }
}

/// Partial formalization of validation outcome errors, what's wrong with relation of two entities
/// See Haskell Node, Cardano.Ledger.BaseTypes: Cardano/Src/Ledger/BaseTypes.hs
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub enum Mismatch<T: Debug + Display> {
    Supplied(T, MismatchRelation),
    Expected(T, MismatchRelation),
}

impl <T: Debug + Display> Display for Mismatch<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Supplied(val, relation) => write!(f, "{relation} {val}"),
            Self::Expected(val, relation) => write!(f, "not {relation} {val}"),
        }
    }
}

/// See Haskell node, "GOV" rule in Conway epoch, data ConwayGovPredFailure era
/// also, "PPUP" rule in Shelley epoch, data ShelleyPpupPredFailure era
#[derive(Error, Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq)]
pub enum GovernanceValidationError {
    #[error("Governance action from protocol {0} is not allowed in current protocol version")]
    WrongProtocolForGovernance(ProtocolVersion),
    
    /// An update was proposed by a key hash that is not one of the genesis keys.
    /// `mismatchSupplied` ~ key hashes which were a part of the update.
    /// `mismatchExpected` ~ key hashes of the genesis keys.
    #[error("Parameter update from non-genesis key hash")]
    NonGenesisUpdatePPUP, //(Mismatch 'RelSubset (Set (KeyHash 'Genesis)))

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
    ProposalProcedureNetworkIdMismatch { reward_account: StakeAddress, network: NetworkId },

    #[error("Treasury withdrawals network id mismatch: {reward_accounts:?} and {network:?}")]
    TreasuryWithdrawalsNetworkIdMismatch { reward_accounts: Vec<StakeAddress>, network: NetworkId },

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
    InvalidPrevGovActionId (GovActionId),

    #[error("Voting on expired governance action {0:?}")]
    VotingOnExpiredGovAction (Vec<(Voter, GovActionId)>)
/*
  | ProposalCantFollow
      -- | The PrevGovActionId of the HardForkInitiation that fails
      (StrictMaybe (GovPurposeId 'HardForkPurpose era))
      -- | Its protocol version and the protocal version of the previous gov-action pointed to by the proposal
      (Mismatch 'RelGT ProtVer)
  | InvalidPolicyHash
      -- | The policy script hash in the proposal
      (StrictMaybe ScriptHash)
      -- | The policy script hash of the current constitution
      (StrictMaybe ScriptHash)
  | DisallowedProposalDuringBootstrap (ProposalProcedure era)
  | DisallowedVotesDuringBootstrap
      (NonEmpty (Voter, GovActionId))
  | -- | Predicate failure for votes by entities that are not present in the ledger state
    VotersDoNotExist (NonEmpty Voter)
  | -- | Treasury withdrawals that sum up to zero are not allowed
    ZeroTreasuryWithdrawals (GovAction era)
  | -- | Proposals that have an invalid reward account for returns of the deposit
    ProposalReturnAccountDoesNotExist RewardAccount
  | -- | Treasury withdrawal proposals to an invalid reward account
    TreasuryWithdrawalReturnAccountsDoNotExist (NonEmpty RewardAccount)
*/
}
