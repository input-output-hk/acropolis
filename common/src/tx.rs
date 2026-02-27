use std::collections::{HashMap, HashSet};

use crate::{
    validation::Phase1ValidationError, Address, AlonzoBabbageUpdateProposal, Datum, DatumHash,
    KeyHash, Lovelace, NativeAsset, NativeAssetsDelta, PoolRegistrationUpdate, ProposalProcedure,
    Redeemer, ReferenceScript, ScriptHash, ScriptLang, ScriptRef, StakeRegistrationUpdate,
    TxCertificate, TxCertificateWithPos, TxIdentifier, UTXOValue, UTxOIdentifier, VKeyWitness,
    Value, ValueMap, VotingProcedures, Withdrawal,
};

/// Transaction output (UTXO)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TxOutput {
    /// Identifier for this UTxO
    pub utxo_identifier: UTxOIdentifier,

    /// Address data
    pub address: Address,

    /// Output value (Lovelace + native assets)
    pub value: Value,

    /// Datum (Inline or Hash)
    pub datum: Option<Datum>,

    /// Reference Script hash and type
    pub script_ref: Option<ScriptRef>,
}

impl TxOutput {
    pub fn utxo_value(&self) -> UTXOValue {
        UTXOValue {
            address: self.address.clone(),
            value: self.value.clone(),
            datum: self.datum.clone(),
            script_ref: self.script_ref.clone(),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Transaction {
    pub id: TxIdentifier,
    pub consumes: Vec<UTxOIdentifier>,
    pub produces: Vec<TxOutput>,
    pub reference_inputs: Vec<UTxOIdentifier>,
    pub fee: u64,
    pub reference_scripts: Vec<(ScriptHash, ReferenceScript)>,
    // Transaction total collateral that is moved to fee pot
    // only added since Babbage era
    pub stated_total_collateral: Option<u64>,
    pub is_valid: bool,
    pub certs: Vec<TxCertificateWithPos>,
    pub withdrawals: Vec<Withdrawal>,
    pub mint_burn_deltas: NativeAssetsDelta,
    pub required_signers: Vec<KeyHash>,
    pub proposal_update: Option<AlonzoBabbageUpdateProposal>,
    pub voting_procedures: Option<VotingProcedures>,
    pub proposal_procedures: Option<Vec<ProposalProcedure>>,
    pub vkey_witnesses: Vec<VKeyWitness>,
    pub script_witnesses: Vec<(ScriptHash, ScriptLang)>,
    pub redeemers: Vec<Redeemer>,
    pub plutus_data: Vec<(DatumHash, Vec<u8>)>,
    pub error: Option<Phase1ValidationError>,
}

impl Transaction {
    pub fn calculate_tx_output(&self) -> Value {
        let mut total_output = Value::default();
        for output in &self.produces {
            total_output += &output.value;
        }
        total_output
    }

    pub fn convert_to_utxo_deltas(self, do_validation: bool) -> TxUTxODeltas {
        let Self {
            id,
            consumes,
            produces,
            reference_inputs,
            fee,
            reference_scripts,
            stated_total_collateral,
            is_valid,
            certs,
            withdrawals,
            mint_burn_deltas,
            required_signers,
            proposal_update,
            voting_procedures,
            proposal_procedures,
            vkey_witnesses,
            script_witnesses,
            redeemers,
            plutus_data,
            ..
        } = self;
        let mut utxo_deltas = TxUTxODeltas {
            tx_identifier: id,
            consumes,
            produces,
            reference_inputs,
            fee,
            reference_scripts: None,
            stated_total_collateral,
            is_valid,
            withdrawals: None,
            certs: None,
            mint_burn_deltas: None,
            required_signers: None,
            proposal_update: None,
            voting_procedures: None,
            proposal_procedures: None,
            vkey_witnesses: None,
            script_witnesses: None,
            redeemers: None,
            plutus_data: None,
        };

        if do_validation {
            utxo_deltas.reference_scripts = Some(reference_scripts);
            utxo_deltas.certs = Some(certs);
            utxo_deltas.withdrawals = Some(withdrawals);
            utxo_deltas.mint_burn_deltas = Some(mint_burn_deltas);
            utxo_deltas.required_signers = Some(required_signers);
            utxo_deltas.proposal_update = proposal_update;
            utxo_deltas.voting_procedures = voting_procedures;
            utxo_deltas.proposal_procedures = proposal_procedures;
            utxo_deltas.vkey_witnesses = Some(vkey_witnesses);
            utxo_deltas.script_witnesses = Some(script_witnesses);
            utxo_deltas.redeemers = Some(redeemers);
            utxo_deltas.plutus_data = Some(plutus_data);
        }

        utxo_deltas
    }
}

// Individual transaction info
// Some of the fields are optional
// when validation is not required
#[derive(Default, Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TxUTxODeltas {
    // Transaction identifier
    pub tx_identifier: TxIdentifier,

    // Spent and Created UTxOs
    pub consumes: Vec<UTxOIdentifier>,
    pub produces: Vec<TxOutput>,

    // Reference inputs (introduced in Alonzo)
    pub reference_inputs: Vec<UTxOIdentifier>,

    // Transaction fee
    pub fee: u64,

    // Transaction total collateral
    pub stated_total_collateral: Option<u64>,

    // Tx validity flag
    pub is_valid: bool,

    // State needed for validation

    // Reference scripts (needed for phase 2 validation)
    pub reference_scripts: Option<Vec<(ScriptHash, ReferenceScript)>>,

    // Certificates
    // NOTE:
    // These certificates will be resolved against
    // StakeRegistrationUpdates and PoolRegistrationUpdates message
    // from accounts_state and spo_state
    // in utxo_state while validating the transaction
    pub certs: Option<Vec<TxCertificateWithPos>>,

    // Withdrawls
    pub withdrawals: Option<Vec<Withdrawal>>,

    // Value Minted and Burnt
    pub mint_burn_deltas: Option<NativeAssetsDelta>,

    // Required signers
    pub required_signers: Option<Vec<KeyHash>>,

    // Proposal update (for Alonzo and Babbage)
    pub proposal_update: Option<AlonzoBabbageUpdateProposal>,

    // Voting procedures (for Conway)
    pub voting_procedures: Option<VotingProcedures>,

    // Proposal procedures (for Conway)
    pub proposal_procedures: Option<Vec<ProposalProcedure>>,

    // VKey Witnesses
    pub vkey_witnesses: Option<Vec<VKeyWitness>>,

    // Scripts Witnesses Provided
    pub script_witnesses: Option<Vec<(ScriptHash, ScriptLang)>>,

    // Redeemers
    pub redeemers: Option<Vec<Redeemer>>,

    // Plutus data
    pub plutus_data: Option<Vec<(DatumHash, Vec<u8>)>>,
}

impl TxUTxODeltas {
    /// This function returns VKey hashes provided
    /// from Vkey witnesses
    pub fn get_vkey_witness_hashes(&self) -> HashSet<KeyHash> {
        let Some(vkey_witnesses) = self.vkey_witnesses.as_ref() else {
            return HashSet::new();
        };
        vkey_witnesses.iter().map(|w| w.key_hash()).collect::<HashSet<_>>()
    }

    /// This function returns script hashes provided
    /// from scripts witnesses provided
    pub fn get_script_witness_hashes(&self) -> HashSet<ScriptHash> {
        let Some(script_witnesses) = self.script_witnesses.as_ref() else {
            return HashSet::new();
        };
        script_witnesses.iter().map(|(hash, _)| *hash).collect::<HashSet<_>>()
    }

    /// This functions returns the total consumed value of the transaction
    /// Consumed = Inputs + Refund + Withrawals + Value Minted
    /// When transaction is failed
    /// Consumed = Collateral Inputs
    pub fn calculate_total_consumed(
        &self,
        stake_registration_updates: &[StakeRegistrationUpdate],
        utxos: &HashMap<UTxOIdentifier, UTXOValue>,
    ) -> ValueMap {
        let mut total_consumed = ValueMap::default();

        // Add Inputs UTxO values
        for input in self.consumes.iter() {
            if let Some(utxo) = utxos.get(input) {
                total_consumed.add_value(&utxo.value);
            }
        }

        if !self.is_valid {
            // If the transaction is invalid, it only consumes
            // collateral inputs
            return total_consumed;
        }

        let total_refund = self.calculate_total_refund(stake_registration_updates);
        let total_withdrawals = self.calculate_total_withdrawals();
        total_consumed.add_value(&Value::new(total_refund + total_withdrawals, vec![]));

        // Add Value Minted
        total_consumed.add_value(&self.get_minted_value());

        total_consumed.remove_zero_amounts();

        total_consumed
    }

    /// This functions returns the total produced value of the transaction
    /// Produced = Outputs + Fee + Deposits + Value Burnt
    /// When transaction is failed
    /// Produced =
    /// - Before Babbage: Collater Inputs (this is just moved to fee pot)
    /// - Since Babbage: Either Collateral Outputs + Total Collateral or just Collateral Inputs (this is just moved to fee pot)
    pub fn calculate_total_produced(
        &self,
        pool_registration_updates: &[PoolRegistrationUpdate],
        stake_registration_updates: &[StakeRegistrationUpdate],
        utxos: &HashMap<UTxOIdentifier, UTXOValue>,
    ) -> ValueMap {
        let mut total_produced = ValueMap::default();

        // Add Outputs UTxO values
        for output in &self.produces {
            total_produced.add_value(&output.value);
        }

        if !self.is_valid {
            // total_collateral is only set since Babbage era.
            match self.stated_total_collateral {
                Some(stated_total_collateral) => {
                    total_produced.add_value(&Value::new(stated_total_collateral, vec![]));
                    return total_produced;
                }
                None => {
                    // if there is no total_collateral set, then collateral inputs are just moved to fee pot
                    let mut total_collateral = ValueMap::default();

                    // Add Inputs UTxO values
                    for input in self.consumes.iter() {
                        if let Some(utxo) = utxos.get(input) {
                            total_collateral.add_value(&utxo.value);
                        }
                    }
                    return total_collateral;
                }
            }
        }

        let total_deposit =
            self.calculate_total_deposit(pool_registration_updates, stake_registration_updates);
        total_produced.add_value(&Value::new(total_deposit + self.fee, vec![]));

        // Add Value Burnt
        total_produced.add_value(&self.get_burnt_value());

        total_produced.remove_zero_amounts();

        total_produced
    }

    pub fn calculate_total_withdrawals(&self) -> Lovelace {
        let mut total_withdrawals: Lovelace = 0;
        let Some(withdrawals) = self.withdrawals.as_ref() else {
            return 0;
        };
        for withdrawal in withdrawals.iter() {
            total_withdrawals += withdrawal.value;
        }
        total_withdrawals
    }

    pub fn calculate_total_refund(
        &self,
        stake_registration_updates: &[StakeRegistrationUpdate],
    ) -> Lovelace {
        let mut total_refund: Lovelace = 0;
        let Some(certs) = self.certs.as_ref() else {
            return 0;
        };

        for cert in certs.iter() {
            let cert_identifier = cert.tx_certificate_identifier();

            // Stake Deregistration Cert
            total_refund += stake_registration_updates
                .iter()
                .find(|delta| delta.cert_identifier == cert_identifier)
                .map(|delta| delta.outcome.refund())
                .unwrap_or(0);

            // DRep Deregistration Cert
            if let TxCertificate::DRepDeregistration(dereg) = &cert.cert {
                total_refund += dereg.refund;
            }
        }
        total_refund
    }

    pub fn calculate_total_deposit(
        &self,
        pool_registration_updates: &[PoolRegistrationUpdate],
        stake_registration_updates: &[StakeRegistrationUpdate],
    ) -> Lovelace {
        let mut total_deposit: Lovelace = 0;
        let Some(certs) = self.certs.as_ref() else {
            return 0;
        };

        // Check certificates
        for cert in certs.iter() {
            let cert_identifier = cert.tx_certificate_identifier();

            // Pool Registration Cert
            total_deposit += pool_registration_updates
                .iter()
                .find(|delta| delta.cert_identifier == cert_identifier)
                .map(|delta| delta.outcome.deposit())
                .unwrap_or(0);

            // Stake Registration Cert
            total_deposit += stake_registration_updates
                .iter()
                .find(|delta| delta.cert_identifier == cert_identifier)
                .map(|delta| delta.outcome.deposit())
                .unwrap_or(0);

            // DRep Registration Cert
            if let TxCertificate::DRepRegistration(reg) = &cert.cert {
                total_deposit += reg.deposit;
            }
        }

        // Check Governance Proposals
        if let Some(proposals) = self.proposal_procedures.as_ref() {
            for proposal in proposals.iter() {
                total_deposit += proposal.deposit;
            }
        }

        total_deposit
    }

    pub fn get_minted_value(&self) -> Value {
        let mut value_minted = Value::default();
        let Some(deltas) = self.mint_burn_deltas.as_ref() else {
            return value_minted;
        };

        for (policy_id, asset_deltas) in deltas.iter() {
            for asset_delta in asset_deltas.iter() {
                if asset_delta.amount > 0 {
                    value_minted += &Value::new(
                        0,
                        vec![(
                            *policy_id,
                            vec![NativeAsset {
                                name: asset_delta.name,
                                amount: asset_delta.amount.unsigned_abs(),
                            }],
                        )],
                    );
                }
            }
        }

        value_minted
    }

    pub fn get_burnt_value(&self) -> Value {
        let mut value_burnt = Value::default();
        let Some(deltas) = self.mint_burn_deltas.as_ref() else {
            return value_burnt;
        };

        for (policy_id, asset_deltas) in deltas.iter() {
            for asset_delta in asset_deltas.iter() {
                if asset_delta.amount < 0 {
                    value_burnt += &Value::new(
                        0,
                        vec![(
                            *policy_id,
                            vec![NativeAsset {
                                name: asset_delta.name,
                                amount: asset_delta.amount.unsigned_abs(),
                            }],
                        )],
                    );
                }
            }
        }

        value_burnt
    }
}
