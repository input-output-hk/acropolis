//! DRep (Delegated Representative) types and structures

use crate::rational_number::RationalNumber;
use crate::types::{Credential, Lovelace};
use serde_with::{hex::Hex, serde_as};

pub type DRepCredential = Credential;

/// Anchor - verifiable link on-chain identifiers with off-chain content,
/// typically metadata that describes a DRep's identity, platform, or governance
/// philosophy.
#[serde_as]
#[derive(Default, Debug, Clone, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Anchor {
    /// Metadata URL
    pub url: String,

    /// Metadata hash
    #[serde_as(as = "Hex")]
    pub data_hash: Vec<u8>,
}

impl<'b, C> minicbor::Decode<'b, C> for Anchor {
    fn decode(
        d: &mut minicbor::Decoder<'b>,
        _ctx: &mut C,
    ) -> Result<Self, minicbor::decode::Error> {
        d.array()?;

        // URL can be either bytes or text string (snapshot format uses bytes)
        let url = match d.datatype()? {
            minicbor::data::Type::Bytes => {
                let url_bytes = d.bytes()?;
                String::from_utf8_lossy(url_bytes).to_string()
            }
            minicbor::data::Type::String => d.str()?.to_string(),
            _ => {
                return Err(minicbor::decode::Error::message(
                    "Expected bytes or string for URL",
                ))
            }
        };

        // data_hash is encoded as direct bytes, not an array
        let data_hash = d.bytes()?.to_vec();

        Ok(Self { url, data_hash })
    }
}

/// DRep Record - represents the current state of a DRep in the ledger
///
/// TODO: The Haskell ledger's DRepState has additional fields we don't track:
/// - `drepExpiry: EpochNo` - computed as (currentEpoch + ppDRepActivity - numDormantEpochs)
/// - `drepDelegs: Set Credential` - reverse index of who delegated TO this DRep
/// See: cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/Governance/DRepPulser.hs
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DRepRecord {
    /// Deposit amount in lovelace
    pub deposit: Lovelace,
    /// Optional anchor (metadata reference)
    pub anchor: Option<Anchor>,
}

impl DRepRecord {
    pub fn new(deposit: Lovelace, anchor: Option<Anchor>) -> Self {
        Self { deposit, anchor }
    }
}

/// DRepChoice (=CDDL drep, badly named)
#[derive(Debug, Clone, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum DRepChoice {
    /// Address key
    Key(crate::KeyHash),

    /// Script key
    Script(crate::KeyHash),

    /// Abstain
    Abstain,

    /// No confidence
    NoConfidence,
}

/// DRep Registration = reg_drep_cert
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DRepRegistration {
    /// DRep credential
    pub credential: DRepCredential,

    /// Deposit paid
    pub deposit: Lovelace,

    /// Optional anchor
    pub anchor: Option<Anchor>,
}

/// DRep Deregistration = unreg_drep_cert
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DRepDeregistration {
    /// DRep credential
    pub credential: DRepCredential,

    /// Deposit to refund
    pub refund: Lovelace,
}

/// DRep Update = update_drep_cert
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DRepUpdate {
    /// DRep credential
    pub credential: DRepCredential,

    /// Optional anchor
    pub anchor: Option<Anchor>,
}

/// DRep voting thresholds for governance actions
#[derive(
    Default, serde::Serialize, serde::Deserialize, Debug, PartialEq, Eq, Clone, minicbor::Decode,
)]
pub struct DRepVotingThresholds {
    #[n(0)]
    pub motion_no_confidence: RationalNumber,
    #[n(1)]
    pub committee_normal: RationalNumber,
    #[n(2)]
    pub committee_no_confidence: RationalNumber,
    #[n(3)]
    pub update_constitution: RationalNumber,
    #[n(4)]
    pub hard_fork_initiation: RationalNumber,
    #[n(5)]
    pub pp_network_group: RationalNumber,
    #[n(6)]
    pub pp_economic_group: RationalNumber,
    #[n(7)]
    pub pp_technical_group: RationalNumber,
    #[n(8)]
    pub pp_governance_group: RationalNumber,
    #[n(9)]
    pub treasury_withdrawal: RationalNumber,
}
