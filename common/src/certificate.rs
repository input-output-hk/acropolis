//! Certificate type definitions for Acropolis

use crate::address::StakeAddress;
use crate::drep::{Anchor, DRepChoice, DRepDeregistration, DRepRegistration, DRepUpdate};
use crate::hash::Hash;
use crate::types::{
    Credential, KeyHash, Lovelace, PoolId, PoolMetadata, Ratio, Relay, ScriptHash, StakeCredential,
    TxIdentifier, VrfKeyHash,
};
use serde_with::{hex::Hex, serde_as};
use std::collections::HashSet;
use std::fmt;

// === Pool certificate types ===

/// Pool registration data
#[serde_as]
#[derive(
    Debug,
    Default,
    Clone,
    serde::Serialize,
    serde::Deserialize,
    minicbor::Decode,
    minicbor::Encode,
    PartialEq,
    Eq,
)]
pub struct PoolRegistration {
    /// Operator pool key hash - used as ID
    #[serde_as(as = "Hex")]
    #[n(0)]
    pub operator: PoolId,

    /// VRF key hash
    #[serde_as(as = "Hex")]
    #[n(1)]
    pub vrf_key_hash: VrfKeyHash,

    /// Pledged Ada
    #[n(2)]
    pub pledge: Lovelace,

    /// Fixed cost
    #[n(3)]
    pub cost: Lovelace,

    /// Marginal cost (fraction)
    #[n(4)]
    pub margin: Ratio,

    /// Reward account
    #[n(5)]
    pub reward_account: StakeAddress,

    /// Pool owners by their key hash
    #[n(6)]
    pub pool_owners: Vec<StakeAddress>,

    // Relays
    #[n(7)]
    pub relays: Vec<Relay>,

    // Metadata
    #[n(8)]
    pub pool_metadata: Option<PoolMetadata>,
}

/// Pool retirement data
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Eq, PartialEq)]
pub struct PoolRetirement {
    /// Operator pool key hash - used as ID
    pub operator: PoolId,

    /// Epoch it will retire at the end of
    pub epoch: u64,
}

// === Stake delegation types ===

/// Stake delegation data
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Eq, PartialEq)]
pub struct StakeDelegation {
    /// Stake address
    pub stake_address: StakeAddress,

    /// Pool ID to delegate to
    pub operator: PoolId,
}

// === Genesis delegation ===

/// Genesis key delegation
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Eq, PartialEq)]
pub struct GenesisKeyDelegation {
    /// Genesis hash
    pub genesis_hash: Hash<28>,

    /// Genesis delegate hash
    pub genesis_delegate_hash: PoolId,

    /// VRF key hash
    pub vrf_key_hash: VrfKeyHash,
}

// === Move Instantaneous Rewards (MIR) types ===

/// Source of a Move Instantaneous Reward (MIR)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Eq, PartialEq)]
pub enum InstantaneousRewardSource {
    Reserves,
    Treasury,
}

impl fmt::Display for InstantaneousRewardSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InstantaneousRewardSource::Reserves => write!(f, "reserves"),
            InstantaneousRewardSource::Treasury => write!(f, "treasury"),
        }
    }
}

/// Target of a Move Instantaneous Reward (MIR)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Eq, PartialEq)]
pub enum InstantaneousRewardTarget {
    StakeAddresses(Vec<(StakeAddress, i64)>),
    OtherAccountingPot(u64),
}

/// Move instantaneous reward
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Eq, PartialEq)]
pub struct MoveInstantaneousReward {
    /// Source
    pub source: InstantaneousRewardSource,

    /// Target
    pub target: InstantaneousRewardTarget,
}

// === Conway stake registration/deregistration ===

/// Register stake (Conway version) = 'reg_cert'
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Eq, PartialEq)]
pub struct Registration {
    /// Stake address
    pub stake_address: StakeAddress,

    /// Deposit paid
    pub deposit: Lovelace,
}

/// Deregister stake (Conway version) = 'unreg_cert'
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Eq, PartialEq)]
pub struct Deregistration {
    /// Stake address
    pub stake_address: StakeAddress,

    /// Deposit to be refunded
    pub refund: Lovelace,
}

// === Vote delegation types ===

/// Vote delegation (simple, existing registration) = vote_deleg_cert
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Eq, PartialEq)]
pub struct VoteDelegation {
    /// Stake address
    pub stake_address: StakeAddress,

    // DRep choice
    pub drep: DRepChoice,
}

/// Stake+vote delegation (to SPO and DRep) = stake_vote_deleg_cert
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Eq, PartialEq)]
pub struct StakeAndVoteDelegation {
    /// Stake address
    pub stake_address: StakeAddress,

    /// Pool
    pub operator: PoolId,

    // DRep vote
    pub drep: DRepChoice,
}

/// Stake delegation to SPO + registration = stake_reg_deleg_cert
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Eq, PartialEq)]
pub struct StakeRegistrationAndDelegation {
    /// Stake address
    pub stake_address: StakeAddress,

    /// Pool
    pub operator: PoolId,

    // Deposit paid
    pub deposit: Lovelace,
}

/// Vote delegation to DRep + registration = vote_reg_deleg_cert
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Eq, PartialEq)]
pub struct StakeRegistrationAndVoteDelegation {
    /// Stake address
    pub stake_address: StakeAddress,

    /// DRep choice
    pub drep: DRepChoice,

    // Deposit paid
    pub deposit: Lovelace,
}

/// All the trimmings:
/// Vote delegation to DRep + Stake delegation to SPO + registration
/// = stake_vote_reg_deleg_cert
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Eq, PartialEq)]
pub struct StakeRegistrationAndStakeAndVoteDelegation {
    /// Stake credential
    pub stake_address: StakeAddress,

    /// Pool
    pub operator: PoolId,

    /// DRep choice
    pub drep: DRepChoice,

    // Deposit paid
    pub deposit: Lovelace,
}

// === Committee types ===

pub type CommitteeCredential = Credential;

/// Authorise a committee hot credential
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Eq, PartialEq)]
pub struct AuthCommitteeHot {
    /// Cold credential
    pub cold_credential: CommitteeCredential,

    /// Hot credential
    pub hot_credential: CommitteeCredential,
}

/// Resign a committee cold credential
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Eq, PartialEq)]
pub struct ResignCommitteeCold {
    /// Cold credential
    pub cold_credential: CommitteeCredential,

    /// Associated anchor (reasoning?)
    pub anchor: Option<Anchor>,
}

// === TxCertificate enum ===

/// Certificate in a transaction
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Eq, PartialEq)]
pub enum TxCertificate {
    /// Default
    None(()),

    /// Stake registration
    StakeRegistration(StakeAddress),

    /// Stake de-registration
    StakeDeregistration(StakeAddress),

    /// Stake Delegation to a pool
    StakeDelegation(StakeDelegation),

    /// Pool registration
    PoolRegistration(PoolRegistration),

    /// Pool retirement
    PoolRetirement(PoolRetirement),

    /// Genesis key delegation
    GenesisKeyDelegation(GenesisKeyDelegation),

    /// Move instantaneous rewards
    MoveInstantaneousReward(MoveInstantaneousReward),

    /// New stake registration
    Registration(Registration),

    /// Stake deregistration
    Deregistration(Deregistration),

    /// Vote delegation
    VoteDelegation(VoteDelegation),

    /// Combined stake and vote delegation
    StakeAndVoteDelegation(StakeAndVoteDelegation),

    /// Stake registration and SPO delegation
    StakeRegistrationAndDelegation(StakeRegistrationAndDelegation),

    /// Stake registration and vote delegation
    StakeRegistrationAndVoteDelegation(StakeRegistrationAndVoteDelegation),

    /// Stake registration and combined SPO and vote delegation
    StakeRegistrationAndStakeAndVoteDelegation(StakeRegistrationAndStakeAndVoteDelegation),

    /// Authorise a committee hot credential
    AuthCommitteeHot(AuthCommitteeHot),

    /// Resign a committee cold credential
    ResignCommitteeCold(ResignCommitteeCold),

    /// DRep registration
    DRepRegistration(DRepRegistration),

    /// DRep deregistration
    DRepDeregistration(DRepDeregistration),

    /// DRep update
    DRepUpdate(DRepUpdate),
}

impl TxCertificate {
    /// This function extracts required VKey Hashes from TxCertificate
    /// Reference: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/shelley/impl/src/Cardano/Ledger/Shelley/TxCert.hs#L583
    ///
    /// returns (vkey_hashes, script_hashes)
    pub fn get_cert_authors(
        &self,
        vkey_hashes: &mut HashSet<KeyHash>,
        script_hashes: &mut HashSet<ScriptHash>,
    ) {
        let mut parse_cred = |cred: &StakeCredential| match cred {
            StakeCredential::AddrKeyHash(vkey_hash) => {
                vkey_hashes.insert(*vkey_hash);
            }
            StakeCredential::ScriptHash(script_hash) => {
                script_hashes.insert(*script_hash);
            }
        };

        match self {
            // Deregistration requires witness from stake credential
            Self::StakeDeregistration(addr) => {
                parse_cred(&addr.credential);
            }
            // Delegation requires witness from delegator
            Self::StakeDelegation(deleg) => {
                parse_cred(&deleg.stake_address.credential);
            }
            // Pool registration requires witness from pool cold key and owners
            Self::PoolRegistration(pool_reg) => {
                vkey_hashes.insert(*pool_reg.operator);
                vkey_hashes.extend(
                    pool_reg.pool_owners.iter().map(|o| o.get_hash()).collect::<HashSet<_>>(),
                );
            }
            // Pool retirement requires witness from pool cold key
            Self::PoolRetirement(retirement) => {
                vkey_hashes.insert(*retirement.operator);
            }
            // Genesis delegation requires witness from genesis key
            Self::GenesisKeyDelegation(gen_deleg) => {
                vkey_hashes.insert(*gen_deleg.genesis_delegate_hash);
            }
            _ => {}
        }
    }
}

/// Certificate with position information in a transaction
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Eq, PartialEq)]
pub struct TxCertificateWithPos {
    pub cert: TxCertificate,
    pub tx_identifier: TxIdentifier,
    pub cert_index: u64,
}

impl TxCertificateWithPos {
    pub fn tx_certificate_identifier(&self) -> TxCertificateIdentifier {
        TxCertificateIdentifier {
            tx_identifier: self.tx_identifier,
            cert_index: self.cert_index,
        }
    }
}

/// Certificate position
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Eq, PartialEq)]
pub struct TxCertificateIdentifier {
    pub tx_identifier: TxIdentifier,
    pub cert_index: u64,
}
