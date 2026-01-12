use crate::{hash::Hash, ExUnits};

pub type ScriptIntegrityHash = Hash<32>;

#[derive(
    serde::Serialize,
    serde::Deserialize,
    minicbor::Encode,
    minicbor::Decode,
    Debug,
    PartialEq,
    Eq,
    Clone,
)]
pub enum ScriptRedeemerTag {
    #[n(0)]
    Spend,
    #[n(1)]
    Mint,
    #[n(2)]
    Cert,
    #[n(3)]
    Reward,
    #[n(4)]
    Vote,
    #[n(5)]
    Propose,
}

#[derive(
    serde::Serialize,
    serde::Deserialize,
    minicbor::Encode,
    minicbor::Decode,
    Debug,
    PartialEq,
    Eq,
    Clone,
)]
pub struct ScriptRedeemer {
    #[n(0)]
    pub tag: ScriptRedeemerTag,
    #[n(1)]
    pub index: u32,
    #[n(2)]
    pub data: Vec<u8>,
    #[n(3)]
    pub ex_units: ExUnits,
}
