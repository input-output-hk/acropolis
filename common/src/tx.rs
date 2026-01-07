use crate::{
    validation::Phase1ValidationError, AlonzoBabbageUpdateProposal, NativeScript,
    TxCertificateWithPos, TxOutput, UTxOIdentifier, VKeyWitness, Value, Withdrawal,
};

pub struct Transaction {
    pub consumes: Vec<UTxOIdentifier>,
    pub produces: Vec<TxOutput>,
    pub fee: u64,
    pub certs: Vec<TxCertificateWithPos>,
    pub withdrawals: Vec<Withdrawal>,
    pub proposal_update: Option<AlonzoBabbageUpdateProposal>,
    pub vkey_witnesses: Vec<VKeyWitness>,
    pub native_scripts: Vec<NativeScript>,
    pub error: Option<Phase1ValidationError>,
}

impl Transaction {
    pub fn calculate_total_output(&self) -> Value {
        let mut total_output = Value::default();
        for output in &self.produces {
            total_output += &output.value;
        }
        total_output
    }
}
