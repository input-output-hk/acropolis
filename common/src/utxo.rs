use std::collections::HashSet;

use crate::{
    certificate::TxCertificateIdentifier, KeyHash, Lovelace, PoolRegistrationUpdate,
    ReferenceScript, ScriptHash, StakeRegistrationUpdate, TxIdentifier, TxOutput, UTxOIdentifier,
    Value,
};

use blake2::digest::{Update, VariableOutput};
use blake2::Blake2bVar;

// Individual transaction UTxO deltas
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TxUTxODeltas {
    // Transaction in which delta occured
    pub tx_identifier: TxIdentifier,

    // Created and spent UTxOs
    pub consumes: Vec<UTxOIdentifier>,
    pub produces: Vec<TxOutput>,

    // Transaction fee
    pub fee: u64,

    // Tx validity flag
    pub is_valid: bool,

    // State needed for validation
    pub total_withdrawals: Option<Lovelace>,
    // NOTE:
    // These certificates will be resolved against
    // StakeRegistrationUpdates and PoolRegistrationUpdates message
    // in utxo_state while validating the transaction
    pub certs_identifiers: Option<Vec<TxCertificateIdentifier>>,
    pub value_minted: Option<Value>,
    pub value_burnt: Option<Value>,

    // NOTE:
    // VKey and Script Hashes here are
    // missing UTxO (inputs) Authors
    pub vkey_hashes_needed: Option<HashSet<KeyHash>>,
    pub script_hashes_needed: Option<HashSet<ScriptHash>>,
    // From witnesses
    pub vkey_hashes_provided: Option<Vec<KeyHash>>,
    // NOTE:
    // This includes only native scripts
    // missing Plutus Scripts
    pub script_hashes_provided: Option<Vec<ScriptHash>>,
}

impl TxUTxODeltas {
    pub fn calculate_total_refund(
        &self,
        stake_registration_updates: &[StakeRegistrationUpdate],
    ) -> Lovelace {
        let mut total_refund: Lovelace = 0;
        let Some(certs_identifiers) = self.certs_identifiers.as_ref() else {
            return 0;
        };

        for cert_identifier in certs_identifiers.iter() {
            total_refund += stake_registration_updates
                .iter()
                .find(|delta| delta.cert_identifier.eq(cert_identifier))
                .map(|delta| delta.outcome.refund())
                .unwrap_or(0);
        }
        total_refund
    }

    pub fn calculate_total_deposit(
        &self,
        pool_registration_updates: &[PoolRegistrationUpdate],
        stake_registration_updates: &[StakeRegistrationUpdate],
    ) -> Lovelace {
        let mut total_deposit: Lovelace = 0;
        let Some(certs_identifiers) = self.certs_identifiers.as_ref() else {
            return 0;
        };

        for cert_identifier in certs_identifiers.iter() {
            total_deposit += pool_registration_updates
                .iter()
                .find(|delta| delta.cert_identifier.eq(cert_identifier))
                .map(|delta| delta.outcome.deposit())
                .unwrap_or(0);
            total_deposit += stake_registration_updates
                .iter()
                .find(|delta| delta.cert_identifier.eq(cert_identifier))
                .map(|delta| delta.outcome.deposit())
                .unwrap_or(0);
        }
        total_deposit
    }

    pub fn calculate_total_produced(
        &self,
        pool_registration_updates: &[PoolRegistrationUpdate],
        stake_registration_updates: &[StakeRegistrationUpdate],
    ) -> Value {
        let mut total_produced = Value::default();
        total_produced += &Value::new(
            self.calculate_total_deposit(pool_registration_updates, stake_registration_updates),
            vec![],
        );
        total_produced += &Value::new(self.fee, vec![]);

        for output in &self.produces {
            total_produced += &output.value;
        }
        total_produced
    }
}

pub fn hash_script_ref(script_opt: Option<ReferenceScript>) -> Option<ScriptHash> {
    match script_opt {
        Some(
            ReferenceScript::PlutusV1(b)
            | ReferenceScript::PlutusV2(b)
            | ReferenceScript::PlutusV3(b),
        ) => {
            let mut hasher = Blake2bVar::new(28).ok()?;
            hasher.update(&b);

            let mut out = [0u8; 28];
            hasher.finalize_variable(&mut out).ok()?;

            Some(ScriptHash::new(out))
        }
        _ => None,
    }
}
