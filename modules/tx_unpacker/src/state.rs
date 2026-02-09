use crate::validations;
use crate::validations::phase2::{
    validate_transaction_phase2, ExBudget, PlutusVersion, ScriptInput, ScriptPurpose,
};
use acropolis_common::{
    messages::{ProtocolParamsMessage, RawTxsMessage},
    protocol_params::ProtocolParams,
    script::{Redeemer, RedeemerTag, ScriptType},
    validation::{Phase2ValidationError, TransactionValidationError, ValidationError},
    BlockInfo, DRepScriptHash, GenesisDelegates, ScriptHash, Voter,
};
use anyhow::Result;
use pallas::codec::minicbor;
use pallas::ledger::traverse::MultiEraTx;

#[derive(Default, Clone)]
pub struct State {
    pub protocol_params: ProtocolParams,
    /// Whether Phase 2 script validation is enabled (default: false)
    pub phase2_enabled: bool,
}

impl State {
    pub fn new() -> Self {
        Self {
            protocol_params: ProtocolParams::default(),
            phase2_enabled: false,
        }
    }

    /// Create a new State with Phase 2 validation enabled/disabled
    pub fn with_phase2_enabled(phase2_enabled: bool) -> Self {
        Self {
            protocol_params: ProtocolParams::default(),
            phase2_enabled,
        }
    }

    pub fn handle_protocol_params(&mut self, msg: &ProtocolParamsMessage) {
        self.protocol_params = msg.params.clone();
    }

    fn validate_transaction(
        &self,
        block_info: &BlockInfo,
        raw_tx: &[u8],
        genesis_delegs: &GenesisDelegates,
    ) -> Result<(), Box<TransactionValidationError>> {
        validations::validate_tx(
            raw_tx,
            genesis_delegs,
            &self.protocol_params.shelley,
            block_info.slot,
            block_info.era,
        )
    }

    pub fn validate(
        &self,
        block_info: &BlockInfo,
        txs_msg: &RawTxsMessage,
        genesis_delegs: &GenesisDelegates,
    ) -> Result<(), Box<ValidationError>> {
        let mut bad_transactions = Vec::new();
        for (tx_index, raw_tx) in txs_msg.txs.iter().enumerate() {
            let tx_index = tx_index as u16;

            // Phase 1 Validation
            if let Err(e) = self.validate_transaction(block_info, raw_tx, genesis_delegs) {
                bad_transactions.push((tx_index, *e));
                continue; // Don't run Phase 2 if Phase 1 failed
            }

            // Phase 2 Validation (if enabled)
            // Only run if Phase 1 passed (FR-002)
            if self.phase2_enabled {
                if let Err(e) = self.validate_transaction_phase2(raw_tx, block_info) {
                    bad_transactions.push((tx_index, *e));
                }
            }
        }

        if bad_transactions.is_empty() {
            Ok(())
        } else {
            Err(Box::new(ValidationError::BadTransactions {
                bad_transactions,
            }))
        }
    }

    /// Perform Phase 2 validation (Plutus script execution) on a transaction.
    ///
    /// This is called after Phase 1 validation passes. It extracts all Plutus
    /// scripts from the transaction, matches them with their redeemers, and
    /// evaluates each script.
    ///
    /// # Arguments
    ///
    /// * `raw_tx` - Raw CBOR-encoded transaction bytes
    /// * `block_info` - Block context for the transaction
    ///
    /// # Returns
    ///
    /// * `Ok(())` - All scripts executed successfully
    /// * `Err(TransactionValidationError)` - Script validation failed
    fn validate_transaction_phase2(
        &self,
        raw_tx: &[u8],
        block_info: &BlockInfo,
    ) -> Result<(), Box<TransactionValidationError>> {
        // Decode transaction to extract scripts and redeemers
        let tx = MultiEraTx::decode(raw_tx).map_err(|e| {
            TransactionValidationError::CborDecodeError {
                era: block_info.era,
                reason: e.to_string(),
            }
        })?;

        // Extract Plutus scripts from witness set
        let scripts = self.extract_plutus_scripts(&tx);
        if scripts.is_empty() {
            // No Plutus scripts to validate
            return Ok(());
        }

        // Extract redeemers from witness set
        let redeemers = self.extract_redeemers(&tx);
        if redeemers.is_empty() && !scripts.is_empty() {
            // Scripts present but no redeemers - this is a Phase 1 error
            // but we catch it here for safety
            return Ok(());
        }

        // Build script inputs by matching scripts with redeemers
        let script_inputs = self.build_script_inputs(&scripts, &redeemers)?;
        if script_inputs.is_empty() {
            return Ok(());
        }

        // Get cost models from protocol parameters
        let (cost_model_v1, cost_model_v2, cost_model_v3) = self.get_cost_models();

        // Build a minimal script context (placeholder - full implementation needed)
        let script_context = self.build_script_context_bytes(&tx);

        // Convert script inputs to the format expected by validate_transaction_phase2
        let inputs: Vec<ScriptInput<'_>> = script_inputs
            .iter()
            .map(|si| ScriptInput {
                script_hash: si.script_hash,
                script_bytes: &si.script_bytes,
                plutus_version: si.plutus_version,
                purpose: si.purpose.clone(),
                datum: si.datum.as_deref(),
                redeemer: &si.redeemer,
                ex_units: si.ex_units,
            })
            .collect();

        // Execute Phase 2 validation
        validate_transaction_phase2(
            &inputs,
            &cost_model_v1,
            &cost_model_v2,
            &cost_model_v3,
            &script_context,
        )
        .map_err(|e| {
            let phase2_err: Phase2ValidationError = e.into();
            Box::new(TransactionValidationError::Phase2ValidationError(
                phase2_err,
            ))
        })?;

        Ok(())
    }

    /// Extract Plutus scripts from a transaction's witness set.
    fn extract_plutus_scripts(&self, tx: &MultiEraTx) -> Vec<(ScriptHash, Vec<u8>, ScriptType)> {
        let mut scripts = Vec::new();

        // Extract V1 scripts from witness set
        for script in tx.plutus_v1_scripts() {
            let script_bytes: &[u8] = script.as_ref();
            let hash = acropolis_common::crypto::keyhash_224_tagged(1, script_bytes);
            scripts.push((hash, script_bytes.to_vec(), ScriptType::PlutusV1));
        }

        // Extract V2 scripts from witness set
        for script in tx.plutus_v2_scripts() {
            let script_bytes: &[u8] = script.as_ref();
            let hash = acropolis_common::crypto::keyhash_224_tagged(2, script_bytes);
            scripts.push((hash, script_bytes.to_vec(), ScriptType::PlutusV2));
        }

        // Extract V3 scripts from witness set
        for script in tx.plutus_v3_scripts() {
            let script_bytes: &[u8] = script.as_ref();
            let hash = acropolis_common::crypto::keyhash_224_tagged(3, script_bytes);
            scripts.push((hash, script_bytes.to_vec(), ScriptType::PlutusV3));
        }

        scripts
    }

    /// Extract redeemers from a transaction's witness set.
    fn extract_redeemers(&self, tx: &MultiEraTx) -> Vec<Redeemer> {
        tx.redeemers()
            .iter()
            .map(|r| {
                // Encode PlutusData to CBOR bytes
                let mut data_bytes = Vec::new();
                minicbor::encode(r.data(), &mut data_bytes).unwrap_or_default();

                Redeemer {
                    tag: match r.tag() {
                        pallas::ledger::primitives::conway::RedeemerTag::Spend => {
                            RedeemerTag::Spend
                        }
                        pallas::ledger::primitives::conway::RedeemerTag::Mint => RedeemerTag::Mint,
                        pallas::ledger::primitives::conway::RedeemerTag::Cert => RedeemerTag::Cert,
                        pallas::ledger::primitives::conway::RedeemerTag::Reward => {
                            RedeemerTag::Reward
                        }
                        pallas::ledger::primitives::conway::RedeemerTag::Vote => RedeemerTag::Vote,
                        pallas::ledger::primitives::conway::RedeemerTag::Propose => {
                            RedeemerTag::Propose
                        }
                    },
                    index: r.index(),
                    data: data_bytes,
                    ex_units: acropolis_common::ExUnits {
                        mem: r.ex_units().mem,
                        steps: r.ex_units().steps,
                    },
                }
            })
            .collect()
    }

    /// Build script inputs by matching scripts with redeemers.
    ///
    /// This is a simplified implementation that matches scripts to redeemers
    /// by tag and index. A full implementation would need to resolve:
    /// - Which UTxO input corresponds to which spending script
    /// - Which minting policy corresponds to which mint redeemer
    /// - Datum lookup for spending validators
    fn build_script_inputs(
        &self,
        scripts: &[(ScriptHash, Vec<u8>, ScriptType)],
        redeemers: &[Redeemer],
    ) -> Result<Vec<OwnedScriptInput>, Box<TransactionValidationError>> {
        let mut inputs = Vec::new();

        // For now, match scripts to redeemers in order
        // A full implementation would need proper script-to-redeemer resolution
        for (idx, (script_hash, script_bytes, script_type)) in scripts.iter().enumerate() {
            // Find corresponding redeemer
            let redeemer = redeemers.get(idx);

            if let Some(redeemer) = redeemer {
                let plutus_version = match script_type {
                    ScriptType::PlutusV1 => PlutusVersion::V1,
                    ScriptType::PlutusV2 => PlutusVersion::V2,
                    ScriptType::PlutusV3 => PlutusVersion::V3,
                    ScriptType::Native => continue, // Skip native scripts
                };

                let purpose = match redeemer.tag {
                    RedeemerTag::Spend => {
                        // For spending, we'd need the actual UTxO identifier
                        // Using a placeholder for now
                        ScriptPurpose::Spending(acropolis_common::UTxOIdentifier {
                            tx_hash: acropolis_common::TxHash::default(),
                            output_index: redeemer.index as u16,
                        })
                    }
                    RedeemerTag::Mint => ScriptPurpose::Minting(*script_hash),
                    RedeemerTag::Cert => ScriptPurpose::Certifying {
                        index: redeemer.index,
                    },
                    RedeemerTag::Reward => {
                        // For rewards, we'd need the actual stake address
                        ScriptPurpose::Rewarding(acropolis_common::StakeAddress::default())
                    }
                    RedeemerTag::Vote => {
                        // For voting, we use the script hash as a DRepScript voter
                        // A full implementation would resolve the actual voter
                        ScriptPurpose::Voting(Voter::DRepScript(DRepScriptHash::from(*script_hash)))
                    }
                    RedeemerTag::Propose => ScriptPurpose::Proposing {
                        index: redeemer.index,
                    },
                };

                inputs.push(OwnedScriptInput {
                    script_hash: *script_hash,
                    script_bytes: script_bytes.clone(),
                    plutus_version,
                    purpose,
                    datum: None, // TODO: Resolve datum for spending validators
                    redeemer: redeemer.data.clone(),
                    ex_units: ExBudget::new(
                        redeemer.ex_units.steps as i64,
                        redeemer.ex_units.mem as i64,
                    ),
                });
            }
        }

        Ok(inputs)
    }

    /// Get cost models from protocol parameters.
    ///
    /// Returns default cost models if not available in protocol params.
    fn get_cost_models(&self) -> (Vec<i64>, Vec<i64>, Vec<i64>) {
        // TODO: Extract actual cost models from protocol parameters
        // For now, return default cost models for testing
        let v1 = vec![0i64; 166]; // V1 has ~166 params
        let v2 = vec![0i64; 175]; // V2 has ~175 params
        let v3 = vec![0i64; 300]; // V3 has ~300 params
        (v1, v2, v3)
    }

    /// Build script context bytes for script evaluation.
    ///
    /// This is a placeholder that returns a minimal valid ScriptContext.
    /// A full implementation would construct the proper TxInfo and ScriptPurpose.
    fn build_script_context_bytes(&self, _tx: &MultiEraTx) -> Vec<u8> {
        // Return minimal CBOR-encoded ScriptContext
        // Constr 0 with empty fields: d87980
        vec![0xd8, 0x79, 0x80]
    }
}

/// Owned version of ScriptInput for internal use.
#[derive(Debug)]
struct OwnedScriptInput {
    script_hash: ScriptHash,
    script_bytes: Vec<u8>,
    plutus_version: PlutusVersion,
    purpose: ScriptPurpose,
    datum: Option<Vec<u8>>,
    redeemer: Vec<u8>,
    ex_units: ExBudget,
}
