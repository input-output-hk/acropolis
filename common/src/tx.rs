use crate::{
    validation::Phase1ValidationError, AlonzoBabbageUpdateProposal, NativeScript,
    TxCertificateWithPos, TxOutput, UTxOIdentifier, VKeyWitness, Withdrawal,
};

pub struct Transaction {
    pub inputs: Vec<UTxOIdentifier>,
    pub outputs: Vec<TxOutput>,
    pub total_output: u128,
    pub certs: Vec<TxCertificateWithPos>,
    pub withdrawals: Vec<Withdrawal>,
    pub proposal_update: Option<AlonzoBabbageUpdateProposal>,
    pub vkey_witnesses: Vec<VKeyWitness>,
    pub native_scripts: Vec<NativeScript>,
    pub error: Option<Phase1ValidationError>,
}
