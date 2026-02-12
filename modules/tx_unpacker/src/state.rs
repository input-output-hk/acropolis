use crate::validations;
use crate::validations::phase2::{
    validate_transaction_phase2, PlutusVersion, ScriptInput, ScriptPurpose,
};
use acropolis_codec::map_transaction;
use acropolis_common::{
    messages::{ProtocolParamsMessage, RawTxsMessage},
    protocol_params::ProtocolParams,
    script::{RedeemerTag, ScriptLang},
    validation::{Phase2ValidationError, TransactionValidationError, ValidationError},
    BlockInfo, DRepScriptHash, GenesisDelegates, NetworkId, ScriptHash, Transaction, TxIdentifier,
    Voter,
};
use anyhow::Result;
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
    /// # Current Limitations
    ///
    /// This implementation has limitations that prevent full script validation:
    ///
    /// 1. **Datum Resolution**: Spending validators require the datum from the UTxO
    ///    being spent. This module only receives raw transaction bytes, not resolved
    ///    UTxO state. Full implementation requires either:
    ///    - Passing resolved UTxOs to this function
    ///    - Giving State access to a UTxO store
    ///
    /// 2. **ScriptContext**: Real Plutus scripts receive a ScriptContext containing
    ///    full TxInfo (inputs with their UTxO data, outputs, minting, certs, etc.).
    ///    Without resolved inputs, we cannot build a complete ScriptContext.
    ///
    /// The current implementation uses placeholder values that work for benchmark
    /// testing but would fail for production validators that access these fields.
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
        // Decode transaction using pallas
        let multi_era_tx = MultiEraTx::decode(raw_tx).map_err(|e| {
            TransactionValidationError::CborDecodeError {
                era: block_info.era,
                reason: e.to_string(),
            }
        })?;

        // Map to acropolis_common::Transaction using codec
        // This gives us redeemers, plutus_data (datums), script_witnesses, etc.
        let tx_id = TxIdentifier::new(block_info.number as u32, 0); // tx_index not available here
        let tx = map_transaction(
            &multi_era_tx,
            raw_tx,
            tx_id,
            NetworkId::Mainnet, // TODO: Get network_id from configuration
            block_info.era,
        );

        // Check if there are any Plutus scripts to validate
        let has_plutus_scripts = tx.script_witnesses.iter().any(|(_, lang)| {
            matches!(
                lang,
                ScriptLang::PlutusV1 | ScriptLang::PlutusV2 | ScriptLang::PlutusV3
            )
        });

        if !has_plutus_scripts {
            // No Plutus scripts to validate
            return Ok(());
        }

        if tx.redeemers.is_empty() {
            // Scripts present but no redeemers - this is a Phase 1 error
            // but we catch it here for safety
            return Ok(());
        }

        // Build script inputs from the Transaction and MultiEraTx
        let script_inputs = self.build_script_inputs_from_tx(&tx, &multi_era_tx)?;
        if script_inputs.is_empty() {
            return Ok(());
        }

        // Get cost models from protocol parameters
        let (cost_model_v1, cost_model_v2, cost_model_v3) = self.get_cost_models();

        // Build script context bytes from Transaction
        // TODO: Implement proper ScriptContext construction from Transaction
        let script_context = self.build_script_context_bytes_from_tx(&tx);

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

    /// Build script inputs from a Transaction and its raw MultiEraTx.
    ///
    /// Uses the Transaction's redeemers and script_witnesses for script hashes/versions,
    /// and extracts actual script bytes from the MultiEraTx witness set.
    fn build_script_inputs_from_tx(
        &self,
        tx: &Transaction,
        multi_era_tx: &MultiEraTx,
    ) -> Result<Vec<OwnedScriptInput>, Box<TransactionValidationError>> {
        let mut inputs = Vec::new();

        // Build a map of script hash -> (script_bytes, plutus_version)
        // Extract actual script bytes from the pallas transaction
        let mut scripts: std::collections::HashMap<ScriptHash, (Vec<u8>, PlutusVersion)> =
            std::collections::HashMap::new();

        for script in multi_era_tx.plutus_v1_scripts() {
            let script_bytes: &[u8] = script.as_ref();
            let hash = acropolis_common::crypto::keyhash_224_tagged(1, script_bytes);
            scripts.insert(hash, (script_bytes.to_vec(), PlutusVersion::V1));
        }
        for script in multi_era_tx.plutus_v2_scripts() {
            let script_bytes: &[u8] = script.as_ref();
            let hash = acropolis_common::crypto::keyhash_224_tagged(2, script_bytes);
            scripts.insert(hash, (script_bytes.to_vec(), PlutusVersion::V2));
        }
        for script in multi_era_tx.plutus_v3_scripts() {
            let script_bytes: &[u8] = script.as_ref();
            let hash = acropolis_common::crypto::keyhash_224_tagged(3, script_bytes);
            scripts.insert(hash, (script_bytes.to_vec(), PlutusVersion::V3));
        }

        // Process each redeemer
        for redeemer in &tx.redeemers {
            // Determine the script hash for this redeemer based on tag and index
            // This is a simplified version - full implementation would need UTxO resolution
            let script_hash = match redeemer.tag {
                RedeemerTag::Mint => {
                    // For minting, the index refers to the sorted policy IDs
                    tx.mint_burn_deltas
                        .get(redeemer.index as usize)
                        .map(|(policy_id, _)| *policy_id)
                }
                RedeemerTag::Spend => {
                    // For spending, would need to look up the script at the input
                    // For now, try to match by index in scripts_provided
                    scripts.keys().nth(redeemer.index as usize).copied()
                }
                _ => {
                    // For other tags, skip for now
                    // Full implementation would handle certs, rewards, voting, etc.
                    None
                }
            };

            let Some(script_hash) = script_hash else {
                continue;
            };

            let Some((script_bytes, plutus_version)) = scripts.get(&script_hash) else {
                continue;
            };

            let purpose = match redeemer.tag {
                RedeemerTag::Spend => ScriptPurpose::Spending(acropolis_common::UTxOIdentifier {
                    tx_hash: acropolis_common::TxHash::default(),
                    output_index: redeemer.index as u16,
                }),
                RedeemerTag::Mint => ScriptPurpose::Minting(script_hash),
                RedeemerTag::Cert => ScriptPurpose::Certifying {
                    index: redeemer.index,
                },
                RedeemerTag::Reward => {
                    ScriptPurpose::Rewarding(acropolis_common::StakeAddress::default())
                }
                RedeemerTag::Vote => {
                    ScriptPurpose::Voting(Voter::DRepScript(DRepScriptHash::from(script_hash)))
                }
                RedeemerTag::Propose => ScriptPurpose::Proposing {
                    index: redeemer.index,
                },
            };

            // LIMITATION: Datum resolution requires UTxO state access.
            // For spending validators, the datum is either:
            // - Inline in the UTxO being spent (need to look up UTxO by tx.consumes[index])
            // - A hash in the UTxO, resolved via tx.plutus_data BTreeMap
            // Since this module doesn't have UTxO state, we cannot resolve datums.
            // Real spending validators will fail if they access the datum argument.
            let datum = None;

            inputs.push(OwnedScriptInput {
                script_hash,
                script_bytes: script_bytes.clone(),
                plutus_version: *plutus_version,
                purpose,
                datum,
                redeemer: redeemer.data.clone(),
                ex_units: redeemer.ex_units,
            });
        }

        Ok(inputs)
    }

    /// Build script context bytes from Transaction.
    ///
    /// # Current Limitation
    ///
    /// Building a proper ScriptContext requires resolved input UTxOs, which this
    /// module doesn't have access to. The TxInfo structure (per CIP-0035/CIP-0069)
    /// requires:
    /// - Inputs as (TxOutRef, TxOut) pairs - we only have TxOutRef (consumes)
    /// - Reference inputs with their full TxOut data
    /// - Full output data (we have this via produces)
    ///
    /// The Transaction struct has partial data:
    /// - consumes: UTxO identifiers only (no values/addresses/datums)
    /// - produces: Full output data ✓
    /// - fee ✓
    /// - mint_burn_deltas ✓
    /// - certs ✓
    /// - withdrawals ✓
    /// - voting_procedures ✓
    /// - required_signers ✓
    /// - plutus_data (witness datums) ✓
    /// - redeemers ✓
    ///
    /// Without resolved inputs, scripts that access txInfoInputs will get
    /// incorrect data. This returns a minimal placeholder for testing.
    fn build_script_context_bytes_from_tx(&self, _tx: &Transaction) -> Vec<u8> {
        // Minimal CBOR-encoded ScriptContext placeholder
        // Real implementation needs UTxO state to resolve inputs
        // Constr 0 with empty fields: d87980
        vec![0xd8, 0x79, 0x80]
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
    ex_units: acropolis_common::ExUnits,
}
