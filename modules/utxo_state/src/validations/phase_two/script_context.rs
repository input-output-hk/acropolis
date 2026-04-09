use acropolis_common::{
    genesis_values::GenesisValues, validation::ScriptContextError, Address, Datum, DatumHash,
    KeyHash, NativeAssetDelta, NativeAssetsDelta, PolicyId, ProposalProcedure, Redeemer,
    RedeemerPointer, RedeemerTag, ScriptHash, ScriptLang, ScriptPurpose, ScriptRef,
    TxCertificateWithPos, TxHash, TxUTxODeltas, UTXOValue, UTxOIdentifier, Value, Voter,
    VotingProcedures, Withdrawal,
};
use std::collections::HashMap;
use uplc_turbo::{arena::Arena, data::PlutusData, machine::PlutusVersion};

use super::governance::*;
use super::input::*;
use super::time_range::*;
use super::to_plutus_data::*;
use super::value::encode_mint_value;

/// Complete transaction information record needed for Plutus script evaluation.
/// Constructed from `TxUTxODeltas` with resolved UTXO inputs.
/// Shared across all `ScriptContext`s for the same transaction.
#[derive(Debug)]
pub struct TxInfo {
    pub inputs: Vec<ResolvedInput>,
    pub reference_inputs: Vec<ResolvedInput>,
    pub outputs: Vec<(Address, Value, Option<Datum>, Option<ScriptRef>)>,
    pub fee: u64,
    pub mint: NativeAssetsDelta,
    pub certificates: Vec<TxCertificateWithPos>,
    pub withdrawals: Vec<Withdrawal>,
    pub valid_range: TimeRange,
    pub signatories: Vec<KeyHash>,
    pub datums: Vec<(DatumHash, Vec<u8>)>,
    pub tx_id: TxHash,
    pub voting_procedures: Option<VotingProcedures>,
    pub proposal_procedures: Vec<ProposalProcedure>,
    pub current_treasury_amount: Option<u64>,
    pub treasury_donation: Option<u64>,
    pub redeemers: Vec<Redeemer>,
}

impl TxInfo {
    /// Build a `TxInfo` from transaction deltas and resolved UTXOs.
    pub fn new(
        tx_deltas: &TxUTxODeltas,
        utxos: &HashMap<UTxOIdentifier, UTXOValue>,
        genesis_values: &GenesisValues,
    ) -> Result<Self, ScriptContextError> {
        let mut sorted_consumes = tx_deltas.consumes.clone();
        sorted_consumes.sort();

        let inputs = sorted_consumes
            .iter()
            .map(|utxo_id| {
                let utxo_value =
                    utxos.get(utxo_id).ok_or(ScriptContextError::MissingInput(*utxo_id))?;
                Ok(ResolvedInput {
                    utxo_id: *utxo_id,
                    utxo_value: utxo_value.clone(),
                })
            })
            .collect::<Result<Vec<_>, ScriptContextError>>()?;

        let mut sorted_ref_inputs = tx_deltas.reference_inputs.clone();
        sorted_ref_inputs.sort();

        let reference_inputs = sorted_ref_inputs
            .iter()
            .map(|utxo_id| {
                let utxo_value =
                    utxos.get(utxo_id).ok_or(ScriptContextError::MissingInput(*utxo_id))?;
                Ok(ResolvedInput {
                    utxo_id: *utxo_id,
                    utxo_value: utxo_value.clone(),
                })
            })
            .collect::<Result<Vec<_>, ScriptContextError>>()?;

        let outputs = tx_deltas
            .produces
            .iter()
            .map(|out| {
                (
                    out.address.clone(),
                    out.value.clone(),
                    out.datum.clone(),
                    out.script_ref.clone(),
                )
            })
            .collect();

        let validity = tx_deltas.validity_interval.as_ref().ok_or(
            ScriptContextError::MissingValidationData("validity_interval".into()),
        )?;
        let valid_range = TimeRange::new(
            validity.invalid_before,
            validity.invalid_hereafter,
            genesis_values,
        );

        let certificates = tx_deltas.certs.clone().unwrap_or_default();
        let withdrawals = tx_deltas.withdrawals.clone().unwrap_or_default();
        let mint = tx_deltas.mint_burn_deltas.clone().unwrap_or_default();
        let signatories = tx_deltas.required_signers.clone().unwrap_or_default();
        let redeemers = tx_deltas.redeemers.clone().unwrap_or_default();

        // In Babbage/Conway era, txInfoData includes both explicit datum witnesses AND
        // inline datums from all inputs and reference inputs (keyed by their hash).
        let datums = tx_deltas.plutus_data.clone().unwrap_or_default();
        // for utxo_id in tx_deltas.consumes.iter().chain(tx_deltas.reference_inputs.iter()) {
        //     if let Some(utxo) = utxos.get(utxo_id) {
        //         if let Some(Datum::Inline(bytes)) = &utxo.datum {
        //             let hash: DatumHash = keyhash_256(bytes);
        //             if !datums.iter().any(|(h, _)| h == &hash) {
        //                 datums.push((hash, bytes.clone()));
        //             }
        //         }
        //     }
        // }

        let tx_hash =
            tx_deltas.produces.first().map(|out| out.utxo_identifier.tx_hash).unwrap_or_default();

        Ok(TxInfo {
            inputs,
            reference_inputs,
            outputs,
            fee: tx_deltas.fee,
            mint,
            certificates,
            withdrawals,
            valid_range,
            signatories,
            datums,
            tx_id: tx_hash,
            voting_procedures: tx_deltas.voting_procedures.clone(),
            proposal_procedures: tx_deltas.proposal_procedures.clone().unwrap_or_default(),
            current_treasury_amount: None,
            treasury_donation: tx_deltas.donation,
            redeemers,
        })
    }
}

/// Per-execution script context for Plutus script validation.
///
/// Each `ScriptContext` represents one script that needs to be evaluated,
/// holding a reference to the shared `TxInfo` plus the execution-specific
/// data (redeemer, purpose, datum, script identity).
#[derive(Debug)]
pub struct ScriptContext<'a> {
    pub tx_info: &'a TxInfo,
    pub script_hash: ScriptHash,
    pub script_lang: ScriptLang,
    pub redeemer: Redeemer,
    pub purpose: ScriptPurpose,
    pub datum: Option<Vec<u8>>,
}

/// Build all `ScriptContext`s for a transaction.
///
/// Returns one `ScriptContext` per Plutus script execution (native scripts are skipped).
/// All contexts share a reference to the same `TxInfo`.
///
/// `scripts_needed` and `scripts_provided` are already computed during phase 1 validation,
/// so they are passed in to avoid redundant work.
pub fn build_script_contexts<'a>(
    tx_info: &'a TxInfo,
    scripts_needed: &HashMap<RedeemerPointer, ScriptHash>,
    scripts_provided: &HashMap<ScriptHash, ScriptLang>,
) -> Result<Vec<ScriptContext<'a>>, ScriptContextError> {
    let sorted_inputs: Vec<UTxOIdentifier> = tx_info.inputs.iter().map(|ri| ri.utxo_id).collect();

    let mut contexts = Vec::new();
    for redeemer in &tx_info.redeemers {
        let pointer = redeemer.redeemer_pointer();
        if let Some(script_hash) = scripts_needed.get(&pointer) {
            if let Some(script_lang) = scripts_provided.get(script_hash) {
                if script_lang.is_native() {
                    continue;
                }
                let purpose = build_script_purpose(
                    &redeemer.tag,
                    redeemer.index,
                    &sorted_inputs,
                    &tx_info.mint,
                    &tx_info.withdrawals,
                    &tx_info.certificates,
                    tx_info.voting_procedures.as_ref(),
                    if tx_info.proposal_procedures.is_empty() {
                        None
                    } else {
                        Some(tx_info.proposal_procedures.as_slice())
                    },
                )?;

                let datum = if redeemer.tag == RedeemerTag::Spend {
                    tx_info.inputs.get(redeemer.index as usize).and_then(|ri| {
                        match &ri.utxo_value.datum {
                            Some(Datum::Hash(hash)) => tx_info
                                .datums
                                .iter()
                                .find(|(h, _)| h == hash)
                                .map(|(_, b)| b.clone()),
                            Some(Datum::Inline(b)) => Some(b.clone()),
                            None => None,
                        }
                    })
                } else {
                    None
                };

                contexts.push(ScriptContext {
                    tx_info,
                    script_hash: *script_hash,
                    script_lang: script_lang.clone(),
                    redeemer: redeemer.clone(),
                    purpose,
                    datum,
                });
            }
        }
    }

    Ok(contexts)
}

impl ScriptContext<'_> {
    /// Produce arena-allocated PlutusData arguments for this script execution.
    ///
    /// V1/V2: `[datum?, redeemer, script_context]`
    /// V3: `[script_context]`
    pub fn to_script_args<'a>(
        &self,
        arena: &'a Arena,
        version: PlutusVersion,
    ) -> Result<Vec<&'a PlutusData<'a>>, ScriptContextError> {
        let tx_info_pd = encode_tx_info(self.tx_info, arena, version)?;

        match version {
            PlutusVersion::V1 | PlutusVersion::V2 => {
                let purpose_pd = encode_script_purpose(&self.purpose, arena, version)?;
                let context_pd = constr(arena, 0, vec![tx_info_pd, purpose_pd]);

                let mut args = Vec::new();
                if let Some(datum_bytes) = &self.datum {
                    args.push(from_cbor(arena, datum_bytes)?);
                }
                args.push(from_cbor(arena, &self.redeemer.data)?);
                args.push(context_pd);
                Ok(args)
            }
            PlutusVersion::V3 => {
                let redeemer_pd = from_cbor(arena, &self.redeemer.data)?;
                let script_info_pd =
                    encode_script_info(&self.purpose, &self.datum, arena, version)?;
                let context_pd = constr(arena, 0, vec![tx_info_pd, redeemer_pd, script_info_pd]);
                Ok(vec![context_pd])
            }
        }
    }
}

// ============================================================================
// TxInfo encoding
// ============================================================================

fn encode_tx_info<'a>(
    tx_info: &TxInfo,
    arena: &'a Arena,
    version: PlutusVersion,
) -> Result<&'a PlutusData<'a>, ScriptContextError> {
    let inputs = {
        let items: Vec<_> = tx_info
            .inputs
            .iter()
            .map(|ri| encode_tx_in_info(&ri.utxo_id, &ri.utxo_value, arena, version))
            .collect::<Result<_, _>>()?;
        list(arena, items)
    };

    let outputs = {
        let items: Vec<_> = tx_info
            .outputs
            .iter()
            .map(|(addr, val, datum, script_ref)| {
                encode_tx_out(addr, val, datum, script_ref, arena, version)
            })
            .collect::<Result<_, _>>()?;
        list(arena, items)
    };

    let fee = match version {
        PlutusVersion::V3 => tx_info.fee.to_plutus_data(arena, version)?,
        _ => {
            let fee_value = Value::new(tx_info.fee, vec![]);
            fee_value.to_plutus_data(arena, version)?
        }
    };

    let mint_pd = encode_mint_value(&tx_info.mint, arena, version)?;

    let certs = {
        let items: Vec<_> = tx_info
            .certificates
            .iter()
            .map(|c| c.cert.to_plutus_data(arena, version))
            .collect::<Result<_, _>>()?;
        list(arena, items)
    };

    let wdrls = encode_withdrawals(&tx_info.withdrawals, arena, version)?;
    let valid_range = tx_info.valid_range.to_plutus_data(arena, version)?;

    let sigs = {
        let mut sorted_sigs = tx_info.signatories.clone();
        sorted_sigs.sort_by(|a, b| a.as_ref().cmp(b.as_ref()));
        let items: Vec<_> = sorted_sigs
            .iter()
            .map(|k| k.to_plutus_data(arena, version))
            .collect::<Result<_, _>>()?;
        list(arena, items)
    };

    let datums_pd = encode_datums(&tx_info.datums, arena, version)?;

    let tx_id = tx_info.tx_id.to_plutus_data(arena, version)?;
    let tx_id_with_wrapper = constr(arena, 0, vec![tx_id]);

    match version {
        PlutusVersion::V1 => Ok(constr(
            arena,
            0,
            vec![
                inputs,
                outputs,
                fee,
                mint_pd,
                certs,
                wdrls,
                valid_range,
                sigs,
                datums_pd,
                tx_id_with_wrapper,
            ],
        )),
        PlutusVersion::V2 => {
            let ref_inputs = {
                let items: Vec<_> = tx_info
                    .reference_inputs
                    .iter()
                    .map(|ri| encode_tx_in_info(&ri.utxo_id, &ri.utxo_value, arena, version))
                    .collect::<Result<_, _>>()?;
                list(arena, items)
            };
            let redeemers_pd = encode_redeemers_map(tx_info, arena, version)?;
            Ok(constr(
                arena,
                0,
                vec![
                    inputs,
                    ref_inputs,
                    outputs,
                    fee,
                    mint_pd,
                    certs,
                    wdrls,
                    valid_range,
                    sigs,
                    redeemers_pd,
                    datums_pd,
                    tx_id_with_wrapper,
                ],
            ))
        }
        PlutusVersion::V3 => {
            let ref_inputs = {
                let items: Vec<_> = tx_info
                    .reference_inputs
                    .iter()
                    .map(|ri| encode_tx_in_info(&ri.utxo_id, &ri.utxo_value, arena, version))
                    .collect::<Result<_, _>>()?;
                list(arena, items)
            };
            let redeemers_pd = encode_redeemers_map(tx_info, arena, version)?;
            let votes = match &tx_info.voting_procedures {
                Some(vp) => vp.to_plutus_data(arena, version)?,
                None => map(arena, vec![]),
            };
            let proposals = {
                let items: Vec<_> = tx_info
                    .proposal_procedures
                    .iter()
                    .map(|p| p.to_plutus_data(arena, version))
                    .collect::<Result<_, _>>()?;
                list(arena, items)
            };
            let treasury = encode_maybe_lovelace(tx_info.current_treasury_amount, arena, version)?;
            let donation = encode_maybe_lovelace(tx_info.treasury_donation, arena, version)?;

            Ok(constr(
                arena,
                0,
                vec![
                    inputs,
                    ref_inputs,
                    outputs,
                    fee,
                    mint_pd,
                    certs,
                    wdrls,
                    valid_range,
                    sigs,
                    redeemers_pd,
                    datums_pd,
                    tx_id,
                    votes,
                    proposals,
                    treasury,
                    donation,
                ],
            ))
        }
    }
}

fn encode_maybe_lovelace<'a>(
    amount: Option<u64>,
    arena: &'a Arena,
    version: PlutusVersion,
) -> Result<&'a PlutusData<'a>, ScriptContextError> {
    match amount {
        Some(a) => Ok(constr(arena, 0, vec![a.to_plutus_data(arena, version)?])),
        None => Ok(constr(arena, 1, vec![])),
    }
}

// ============================================================================
// ScriptPurpose (V1/V2)
// ============================================================================

fn encode_script_purpose<'a>(
    purpose: &ScriptPurpose,
    arena: &'a Arena,
    version: PlutusVersion,
) -> Result<&'a PlutusData<'a>, ScriptContextError> {
    match purpose {
        ScriptPurpose::Mint(policy_id) => {
            let p = policy_id.to_plutus_data(arena, version)?;
            Ok(constr(arena, 0, vec![p]))
        }
        ScriptPurpose::Spend(utxo_id) => {
            let u = utxo_id.to_plutus_data(arena, version)?;
            Ok(constr(arena, 1, vec![u]))
        }
        ScriptPurpose::Reward(cred) => {
            let c = cred.to_plutus_data(arena, version)?;
            let staking = constr(arena, 0, vec![c]);
            Ok(constr(arena, 2, vec![staking]))
        }
        ScriptPurpose::Certify(cert_with_pos) => {
            let c = cert_with_pos.cert.to_plutus_data(arena, version)?;
            Ok(constr(arena, 3, vec![c]))
        }
        _ => Err(ScriptContextError::UnsupportedScriptPurpose),
    }
}

// ============================================================================
// ScriptInfo (V3)
// ============================================================================

fn encode_script_info<'a>(
    purpose: &ScriptPurpose,
    datum: &Option<Vec<u8>>,
    arena: &'a Arena,
    version: PlutusVersion,
) -> Result<&'a PlutusData<'a>, ScriptContextError> {
    match purpose {
        ScriptPurpose::Mint(policy_id) => {
            let p = policy_id.to_plutus_data(arena, version)?;
            Ok(constr(arena, 0, vec![p]))
        }
        ScriptPurpose::Spend(utxo_id) => {
            let u = utxo_id.to_plutus_data(arena, version)?;
            let maybe_datum = match datum {
                Some(cbor_bytes) => {
                    let data = from_cbor(arena, cbor_bytes)?;
                    constr(arena, 0, vec![data])
                }
                None => constr(arena, 1, vec![]),
            };
            Ok(constr(arena, 1, vec![u, maybe_datum]))
        }
        ScriptPurpose::Reward(cred) => {
            let c = cred.to_plutus_data(arena, version)?;
            Ok(constr(arena, 2, vec![c]))
        }
        ScriptPurpose::Certify(cert_with_pos) => {
            let idx = cert_with_pos.cert_index.to_plutus_data(arena, version)?;
            let c = cert_with_pos.cert.to_plutus_data(arena, version)?;
            Ok(constr(arena, 3, vec![idx, c]))
        }
        ScriptPurpose::Vote(voter) => {
            let v = voter.to_plutus_data(arena, version)?;
            Ok(constr(arena, 4, vec![v]))
        }
        ScriptPurpose::Propose(idx, proposal) => {
            let idx = idx.to_plutus_data(arena, version)?;
            let p = proposal.to_plutus_data(arena, version)?;
            Ok(constr(arena, 5, vec![idx, p]))
        }
    }
}

// ============================================================================
// Redeemers map (V2/V3)
// ============================================================================

fn encode_redeemers_map<'a>(
    tx_info: &TxInfo,
    arena: &'a Arena,
    version: PlutusVersion,
) -> Result<&'a PlutusData<'a>, ScriptContextError> {
    let sorted_inputs: Vec<UTxOIdentifier> = tx_info.inputs.iter().map(|ri| ri.utxo_id).collect();

    let mut entries: Vec<_> = tx_info
        .redeemers
        .iter()
        .map(|redeemer| {
            let purpose = build_script_purpose(
                &redeemer.tag,
                redeemer.index,
                &sorted_inputs,
                &tx_info.mint,
                &tx_info.withdrawals,
                &tx_info.certificates,
                tx_info.voting_procedures.as_ref(),
                if tx_info.proposal_procedures.is_empty() {
                    None
                } else {
                    Some(tx_info.proposal_procedures.as_slice())
                },
            )?;
            let key = encode_redeemer_key(&purpose, arena, version)?;
            let value = from_cbor(arena, &redeemer.data)?;
            let sort_key = (&redeemer.tag, redeemer.index);
            Ok((sort_key, key, value))
        })
        .collect::<Result<_, ScriptContextError>>()?;

    // Sort by ScriptPurpose (ledger order)
    entries.sort_by_key(|(sort_key, _, _)| *sort_key);

    let pairs = entries.into_iter().map(|(_, k, v)| (k, v)).collect();
    Ok(map(arena, pairs))
}

/// Encode a `ScriptPurpose` as a redeemer map key.
///
/// V2 and V3 differ in `Rewarding` (StakingCredential vs Credential) and
/// `Certifying` (cert only vs index + cert). V3 also adds `Voting`/`Proposing`.
fn encode_redeemer_key<'a>(
    purpose: &ScriptPurpose,
    arena: &'a Arena,
    version: PlutusVersion,
) -> Result<&'a PlutusData<'a>, ScriptContextError> {
    match purpose {
        // This is because Plutus Data Constructor Order
        // is different from Ledger ScriptPurpose Order.
        ScriptPurpose::Mint(policy_id) => {
            let p = policy_id.to_plutus_data(arena, version)?;
            Ok(constr(arena, 0, vec![p]))
        }
        ScriptPurpose::Spend(utxo_id) => {
            let u = utxo_id.to_plutus_data(arena, version)?;
            Ok(constr(arena, 1, vec![u]))
        }
        ScriptPurpose::Reward(cred) => match version {
            PlutusVersion::V1 | PlutusVersion::V2 => {
                // V1/V2: StakingCredential.StakingHash(cred)
                let c = cred.to_plutus_data(arena, version)?;
                let staking = constr(arena, 0, vec![c]);
                Ok(constr(arena, 2, vec![staking]))
            }
            PlutusVersion::V3 => {
                // V3: Credential directly, no StakingHash wrapper
                let c = cred.to_plutus_data(arena, version)?;
                Ok(constr(arena, 2, vec![c]))
            }
        },
        ScriptPurpose::Certify(cert_with_pos) => match version {
            PlutusVersion::V1 | PlutusVersion::V2 => {
                let c = cert_with_pos.cert.to_plutus_data(arena, version)?;
                Ok(constr(arena, 3, vec![c]))
            }
            PlutusVersion::V3 => {
                // V3: includes the certificate index
                let idx = cert_with_pos.cert_index.to_plutus_data(arena, version)?;
                let c = cert_with_pos.cert.to_plutus_data(arena, version)?;
                Ok(constr(arena, 3, vec![idx, c]))
            }
        },
        ScriptPurpose::Vote(voter) => {
            let v = voter.to_plutus_data(arena, version)?;
            Ok(constr(arena, 4, vec![v]))
        }
        ScriptPurpose::Propose(idx, proposal) => {
            let i = idx.to_plutus_data(arena, version)?;
            let p = proposal.to_plutus_data(arena, version)?;
            Ok(constr(arena, 5, vec![i, p]))
        }
    }
}

// ============================================================================
// Build ScriptPurpose from redeemer tag + index
// ============================================================================

#[allow(clippy::too_many_arguments)]
fn build_script_purpose(
    tag: &RedeemerTag,
    index: u32,
    sorted_inputs: &[UTxOIdentifier],
    mint: &[(PolicyId, Vec<NativeAssetDelta>)],
    withdrawals: &[Withdrawal],
    certificates: &[TxCertificateWithPos],
    voting_procedures: Option<&VotingProcedures>,
    proposal_procedures: Option<&[ProposalProcedure]>,
) -> Result<ScriptPurpose, ScriptContextError> {
    let idx = index as usize;
    match tag {
        RedeemerTag::Spend => {
            let utxo_id = sorted_inputs.get(idx).ok_or(ScriptContextError::MissingScript(
                RedeemerPointer {
                    tag: tag.clone(),
                    index,
                },
            ))?;
            Ok(ScriptPurpose::Spend(*utxo_id))
        }
        RedeemerTag::Mint => {
            let (policy_id, _) =
                mint.get(idx).ok_or(ScriptContextError::MissingScript(RedeemerPointer {
                    tag: tag.clone(),
                    index,
                }))?;
            Ok(ScriptPurpose::Mint(*policy_id))
        }
        RedeemerTag::Reward => {
            let withdrawal =
                withdrawals.get(idx).ok_or(ScriptContextError::MissingScript(RedeemerPointer {
                    tag: tag.clone(),
                    index,
                }))?;
            Ok(ScriptPurpose::Reward(withdrawal.address.credential.clone()))
        }
        RedeemerTag::Cert => {
            let cert = certificates.get(idx).ok_or(ScriptContextError::MissingScript(
                RedeemerPointer {
                    tag: tag.clone(),
                    index,
                },
            ))?;
            Ok(ScriptPurpose::Certify(cert.clone()))
        }
        RedeemerTag::Vote => {
            let vp = voting_procedures.ok_or(ScriptContextError::MissingValidationData(
                "voting_procedures".into(),
            ))?;
            let mut voters: Vec<&Voter> = vp.votes.keys().collect();
            voters.sort();
            let voter =
                voters.get(idx).ok_or(ScriptContextError::MissingScript(RedeemerPointer {
                    tag: tag.clone(),
                    index,
                }))?;
            Ok(ScriptPurpose::Vote((*voter).clone()))
        }
        RedeemerTag::Propose => {
            let proposals = proposal_procedures.ok_or(
                ScriptContextError::MissingValidationData("proposal_procedures".into()),
            )?;
            let proposal =
                proposals.get(idx).ok_or(ScriptContextError::MissingScript(RedeemerPointer {
                    tag: tag.clone(),
                    index,
                }))?;
            Ok(ScriptPurpose::Propose(idx, proposal.clone()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{to_era, to_pallas_era, TestContext};
    use crate::validation_fixture;
    use acropolis_common::{NetworkId, TxIdentifier};
    use pallas::ledger::traverse::MultiEraTx;

    fn build_test_deltas(_ctx: &TestContext, raw_tx: &[u8], era: &str) -> TxUTxODeltas {
        let tx = MultiEraTx::decode_for_era(to_pallas_era(era), raw_tx).unwrap();
        let raw_tx = tx.encode();
        let tx_identifier = TxIdentifier::new(4533644, 1);
        let mapped_tx = acropolis_codec::map_transaction(
            &tx,
            &raw_tx,
            tx_identifier,
            NetworkId::Mainnet,
            to_era(era),
        );
        mapped_tx.convert_to_utxo_deltas(true)
    }

    #[test]
    fn tx_info_new_populates_fields() {
        let (ctx, raw_tx, era) = validation_fixture!(
            "alonzo",
            "a95d16e891e51f98a3b1d3fe862ed355ebc8abffb7a7269d86f775553d9e653f"
        );
        let tx_deltas = build_test_deltas(&ctx, &raw_tx, era);
        let genesis_values = GenesisValues::mainnet();

        let tx_info = TxInfo::new(&tx_deltas, &ctx.utxos, &genesis_values).unwrap();

        assert_eq!(tx_info.inputs.len(), 1, "should have 1 input");
        assert_eq!(tx_info.outputs.len(), 1, "should have 1 output");
        assert!(tx_info.fee > 0, "fee should be non-zero");
        assert!(tx_info.mint.is_empty(), "no minting in this tx");
        assert!(tx_info.certificates.is_empty(), "no certificates");
        assert!(tx_info.withdrawals.is_empty(), "no withdrawals");
        assert!(!tx_info.datums.is_empty(), "should have datum witnesses");
        assert!(!tx_info.redeemers.is_empty(), "should have redeemers");
    }

    #[test]
    fn build_script_contexts_produces_one_per_plutus_redeemer() {
        let (ctx, raw_tx, era) = validation_fixture!(
            "alonzo",
            "a95d16e891e51f98a3b1d3fe862ed355ebc8abffb7a7269d86f775553d9e653f"
        );
        let tx_deltas = build_test_deltas(&ctx, &raw_tx, era);
        let genesis_values = GenesisValues::mainnet();

        let tx_info = TxInfo::new(&tx_deltas, &ctx.utxos, &genesis_values).unwrap();
        let scripts_needed = crate::utils::get_scripts_needed(&tx_deltas, &ctx.utxos);
        let scripts_provided = crate::utils::get_scripts_provided(&tx_deltas, &ctx.utxos);
        let contexts = build_script_contexts(&tx_info, &scripts_needed, &scripts_provided).unwrap();

        assert_eq!(contexts.len(), 1, "should have 1 script context");
        let sc = &contexts[0];
        assert_eq!(sc.redeemer.tag, RedeemerTag::Spend);
        assert!(sc.datum.is_some(), "spending script should have datum");
    }

    #[test]
    fn to_script_args_v1_produces_three_args_for_spending() {
        let (ctx, raw_tx, era) = validation_fixture!(
            "alonzo",
            "a95d16e891e51f98a3b1d3fe862ed355ebc8abffb7a7269d86f775553d9e653f"
        );
        let tx_deltas = build_test_deltas(&ctx, &raw_tx, era);
        let genesis_values = GenesisValues::mainnet();

        let tx_info = TxInfo::new(&tx_deltas, &ctx.utxos, &genesis_values).unwrap();
        let scripts_needed = crate::utils::get_scripts_needed(&tx_deltas, &ctx.utxos);
        let scripts_provided = crate::utils::get_scripts_provided(&tx_deltas, &ctx.utxos);
        let contexts = build_script_contexts(&tx_info, &scripts_needed, &scripts_provided).unwrap();
        let sc = &contexts[0];

        let arena = Arena::new();
        let args = sc.to_script_args(&arena, PlutusVersion::V1).unwrap();

        // V1 spending: [datum, redeemer, context]
        assert_eq!(args.len(), 3, "V1 spending should produce 3 args");

        // arg[0] = datum (ByteString)
        assert!(
            matches!(args[0], PlutusData::ByteString(_)),
            "datum should be ByteString"
        );

        // arg[1] = redeemer (ByteString)
        assert!(
            matches!(args[1], PlutusData::ByteString(_)),
            "redeemer should be ByteString"
        );

        // arg[2] = context = Constr(0, [tx_info, purpose])
        if let PlutusData::Constr { tag, fields } = args[2] {
            assert_eq!(*tag, 0);
            assert_eq!(fields.len(), 2, "context should have [tx_info, purpose]");

            // tx_info = Constr(0, 10 fields) for V1
            if let PlutusData::Constr { tag, fields } = fields[0] {
                assert_eq!(*tag, 0);
                assert_eq!(fields.len(), 10, "V1 TxInfo should have 10 fields");
            } else {
                panic!("tx_info should be Constr");
            }

            // purpose = Constr(1, [TxOutRef]) for Spending
            if let PlutusData::Constr { tag, .. } = fields[1] {
                assert_eq!(*tag, 1, "Spending purpose should be Constr tag 1");
            } else {
                panic!("purpose should be Constr");
            }
        } else {
            panic!("context should be Constr");
        }
    }
}
