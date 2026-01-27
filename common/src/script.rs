use std::collections::{HashMap, HashSet};

use crate::{
    crypto::{keyhash_224, keyhash_224_tagged},
    hash::Hash,
    AddrKeyhash, ExUnits, KeyHash, NativeAssetsDelta, ProposalProcedure, ScriptHash,
    ShelleyAddressPaymentPart, StakeCredential, TxCertificateWithPos, UTXOValue, UTxOIdentifier,
    VotingProcedures, Withdrawal,
};

pub type ScriptIntegrityHash = Hash<32>;
pub type DatumHash = Hash<32>;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum ScriptType {
    Native,
    PlutusV1,
    PlutusV2,
    PlutusV3,
}

// The full CBOR bytes of a reference script
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub enum ReferenceScript {
    Native(NativeScript),
    PlutusV1(Vec<u8>),
    PlutusV2(Vec<u8>),
    PlutusV3(Vec<u8>),
}

impl ReferenceScript {
    pub fn compute_hash(&self) -> Option<ScriptHash> {
        match self {
            ReferenceScript::Native(_) => None,
            ReferenceScript::PlutusV1(script) => Some(keyhash_224_tagged(1, script)),
            ReferenceScript::PlutusV2(script) => Some(keyhash_224_tagged(2, script)),
            ReferenceScript::PlutusV3(script) => Some(keyhash_224_tagged(3, script)),
        }
    }

    pub fn get_script_type(&self) -> ScriptType {
        match self {
            ReferenceScript::Native(_) => ScriptType::Native,
            ReferenceScript::PlutusV1(_) => ScriptType::PlutusV1,
            ReferenceScript::PlutusV2(_) => ScriptType::PlutusV2,
            ReferenceScript::PlutusV3(_) => ScriptType::PlutusV3,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub enum NativeScript {
    ScriptPubkey(AddrKeyhash),
    ScriptAll(Vec<NativeScript>),
    ScriptAny(Vec<NativeScript>),
    ScriptNOfK(u32, Vec<NativeScript>),
    InvalidBefore(u64),
    InvalidHereafter(u64),
}

impl<'b, C> minicbor::decode::Decode<'b, C> for NativeScript {
    fn decode(d: &mut minicbor::Decoder<'b>, ctx: &mut C) -> Result<Self, minicbor::decode::Error> {
        let size = d.array()?;

        let assert_size = |expected| {
            // NOTE: unwrap_or allows for indefinite arrays.
            if expected != size.unwrap_or(expected) {
                return Err(minicbor::decode::Error::message(
                    "unexpected array size in NativeScript",
                ));
            }
            Ok(())
        };

        let variant = d.u32()?;

        let script = match variant {
            0 => {
                assert_size(2)?;
                Ok(NativeScript::ScriptPubkey(d.decode_with(ctx)?))
            }
            1 => {
                assert_size(2)?;
                Ok(NativeScript::ScriptAll(d.decode_with(ctx)?))
            }
            2 => {
                assert_size(2)?;
                Ok(NativeScript::ScriptAny(d.decode_with(ctx)?))
            }
            3 => {
                assert_size(3)?;
                Ok(NativeScript::ScriptNOfK(
                    d.decode_with(ctx)?,
                    d.decode_with(ctx)?,
                ))
            }
            4 => {
                assert_size(2)?;
                Ok(NativeScript::InvalidBefore(d.decode_with(ctx)?))
            }
            5 => {
                assert_size(2)?;
                Ok(NativeScript::InvalidHereafter(d.decode_with(ctx)?))
            }
            _ => Err(minicbor::decode::Error::message(
                "unknown variant id for native script",
            )),
        }?;

        if size.is_none() {
            let next = d.datatype()?;
            if next != minicbor::data::Type::Break {
                return Err(minicbor::decode::Error::type_mismatch(next));
            }
        }

        Ok(script)
    }
}

impl<C> minicbor::encode::Encode<C> for NativeScript {
    fn encode<W: minicbor::encode::Write>(
        &self,
        e: &mut minicbor::Encoder<W>,
        ctx: &mut C,
    ) -> Result<(), minicbor::encode::Error<W::Error>> {
        match self {
            NativeScript::ScriptPubkey(v) => {
                e.array(2)?;
                e.encode_with(0, ctx)?;
                e.encode_with(v, ctx)?;
            }
            NativeScript::ScriptAll(v) => {
                e.array(2)?;
                e.encode_with(1, ctx)?;
                e.encode_with(v, ctx)?;
            }
            NativeScript::ScriptAny(v) => {
                e.array(2)?;
                e.encode_with(2, ctx)?;
                e.encode_with(v, ctx)?;
            }
            NativeScript::ScriptNOfK(a, b) => {
                e.array(3)?;
                e.encode_with(3, ctx)?;
                e.encode_with(a, ctx)?;
                e.encode_with(b, ctx)?;
            }
            NativeScript::InvalidBefore(v) => {
                e.array(2)?;
                e.encode_with(4, ctx)?;
                e.encode_with(v, ctx)?;
            }
            NativeScript::InvalidHereafter(v) => {
                e.array(2)?;
                e.encode_with(5, ctx)?;
                e.encode_with(v, ctx)?;
            }
        }

        Ok(())
    }
}

impl NativeScript {
    pub fn compute_hash(&self) -> ScriptHash {
        let mut data = vec![0u8];
        let raw_bytes = minicbor::to_vec(self).expect("Failed to encode NativeScript to CBOR");
        data.extend_from_slice(raw_bytes.as_slice());
        ScriptHash::from(keyhash_224(&data))
    }

    pub fn eval(
        &self,
        vkey_hashes_provided: &HashSet<KeyHash>,
        low_bnd: Option<u64>,
        upp_bnd: Option<u64>,
    ) -> bool {
        match self {
            Self::ScriptAll(scripts) => {
                scripts.iter().all(|script| script.eval(vkey_hashes_provided, low_bnd, upp_bnd))
            }
            Self::ScriptAny(scripts) => {
                scripts.iter().any(|script| script.eval(vkey_hashes_provided, low_bnd, upp_bnd))
            }
            Self::ScriptPubkey(hash) => vkey_hashes_provided.contains(hash),
            Self::ScriptNOfK(val, scripts) => {
                let count = scripts
                    .iter()
                    .map(|script| script.eval(vkey_hashes_provided, low_bnd, upp_bnd))
                    .fold(0, |x, y| x + y as u32);
                count >= *val
            }
            Self::InvalidBefore(val) => {
                match low_bnd {
                    Some(time) => *val <= time,
                    None => false, // as per mary-ledger.pdf, p.20
                }
            }
            Self::InvalidHereafter(val) => {
                match upp_bnd {
                    Some(time) => *val >= time,
                    None => false, // as per mary-ledger.pdf, p.20
                }
            }
        }
    }
}

/// Datum (inline or hash)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum Datum {
    Hash(DatumHash),
    Inline(Vec<u8>),
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
pub enum RedeemerTag {
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
pub struct Redeemer {
    #[n(0)]
    pub tag: RedeemerTag,
    #[n(1)]
    pub index: u32,
    #[n(2)]
    pub data: Vec<u8>,
    #[n(3)]
    pub ex_units: ExUnits,
}

impl Redeemer {
    pub fn redeemer_pointer(&self) -> RedeemerPointer {
        RedeemerPointer {
            tag: self.tag.clone(),
            index: self.index,
        }
    }
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
pub struct RedeemerPointer {
    #[n(0)]
    pub tag: RedeemerTag,
    #[n(1)]
    pub index: u32,
}

/// Get Scripts needed from UTxOs being spend
/// Return a list of (RedeemerPointer, ScriptHash) pairs
/// NOTE:
/// Inputs must be sorted lexicographically by UTxO identifier
pub fn get_scripts_needed_from_inputs(
    sorted_inputs: &[UTxOIdentifier],
    utxos: &HashMap<UTxOIdentifier, UTXOValue>,
) -> Vec<(RedeemerPointer, ScriptHash)> {
    let mut scripts_needed = Vec::new();
    for (index, input) in sorted_inputs.iter().enumerate() {
        if let Some(utxo) = utxos.get(input) {
            if let Some(ShelleyAddressPaymentPart::ScriptHash(script_hash)) =
                utxo.address.get_payment_part()
            {
                scripts_needed.push((
                    RedeemerPointer {
                        tag: RedeemerTag::Spend,
                        index: index as u32,
                    },
                    script_hash,
                ));
            }
        }
    }

    scripts_needed
}

/// Get Scripts needed from Withdrawals
/// Return a list of (RedeemerPointer, ScriptHash) pairs
/// NOTE:
/// Withdrawals must be sorted lexicographically by address
pub fn get_scripts_needed_from_withdrawals(
    sorted_withdrawals: &[Withdrawal],
) -> Vec<(RedeemerPointer, ScriptHash)> {
    let mut scripts_needed = Vec::new();
    for (index, withdrawal) in sorted_withdrawals.iter().enumerate() {
        if let StakeCredential::ScriptHash(script_hash) = withdrawal.address.credential {
            scripts_needed.push((
                RedeemerPointer {
                    tag: RedeemerTag::Reward,
                    index: index as u32,
                },
                script_hash,
            ));
        }
    }
    scripts_needed
}

/// Get Scripts needed from certificates
/// Return a list of (RedeemerPointer, ScriptHash) pairs
pub fn get_scripts_needed_from_certificates(
    certificates: &[TxCertificateWithPos],
) -> Vec<(RedeemerPointer, ScriptHash)> {
    let mut scripts_needed = Vec::new();
    for (index, certificate) in certificates.iter().enumerate() {
        if let Some(script_hash) = certificate.cert.get_script_cert_author() {
            scripts_needed.push((
                RedeemerPointer {
                    tag: RedeemerTag::Cert,
                    index: index as u32,
                },
                script_hash,
            ));
        }
    }

    scripts_needed
}

/// Get Scripts needed from mint-burn
/// Return a list of (RedeemerPointer, ScriptHash) pairs
/// NOTE:
/// Mint-burn must be sorted lexicographically by policy id
pub fn get_scripts_needed_from_mint_burn(
    sorted_mint_burn: &NativeAssetsDelta,
) -> Vec<(RedeemerPointer, ScriptHash)> {
    let mut scripts_needed = Vec::new();
    for (index, (policy_id, _)) in sorted_mint_burn.iter().enumerate() {
        scripts_needed.push((
            RedeemerPointer {
                tag: RedeemerTag::Mint,
                index: index as u32,
            },
            *policy_id,
        ));
    }

    scripts_needed
}

/// Get Scripts needed from voting procedures
/// Return a list of (RedeemerPointer, ScriptHash) pairs
/// NOTE:
/// Voting procedures must be sorted lexicographically by voter
pub fn get_scripts_needed_from_voting(
    voting_procedures: &VotingProcedures,
) -> Vec<(RedeemerPointer, ScriptHash)> {
    let mut scripts_needed = Vec::new();
    let mut voters = voting_procedures.votes.keys().cloned().collect::<Vec<_>>();
    voters.sort_by_key(|voter| voter.to_owned());

    for (index, voter) in voters.iter().enumerate() {
        if let Some(script_hash) = voter.get_voter_script_hash() {
            scripts_needed.push((
                RedeemerPointer {
                    tag: RedeemerTag::Vote,
                    index: index as u32,
                },
                script_hash,
            ));
        }
    }
    scripts_needed
}

/// Get Scripts needed from proposal procedures
/// Return a list of (RedeemerPointer, ScriptHash) pairs
/// NOTE:
/// Proposal procedures are sorted by its insertion order
pub fn get_scripts_needed_from_proposal(
    proposal_procedures: &[ProposalProcedure],
) -> Vec<(RedeemerPointer, ScriptHash)> {
    let mut scripts_needed = Vec::new();
    for (index, proposal_procedure) in proposal_procedures.iter().enumerate() {
        if let Some(script_hash) = proposal_procedure.get_proposal_script_hash() {
            scripts_needed.push((
                RedeemerPointer {
                    tag: RedeemerTag::Propose,
                    index: index as u32,
                },
                script_hash,
            ));
        }
    }
    scripts_needed
}

#[cfg(test)]
mod tests {
    use crate::*;
    use std::str::FromStr;

    #[test]
    fn resolve_hash_correctly() {
        let native_script = NativeScript::ScriptPubkey(
            AddrKeyhash::from_str("976ec349c3a14f58959088e13e98f6cd5a1e8f27f6f3160b25e415ca")
                .unwrap(),
        );
        let script_hash = native_script.compute_hash();
        assert_eq!(
            script_hash,
            ScriptHash::from_str("c3a33acb8903cf42611e26b15c7731f537867c6469f5bf69c837e4a3")
                .unwrap()
        );
    }
}
