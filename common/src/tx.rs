use crate::{
    protocol_params::ProtocolParams, validation::Phase1ValidationError,
    AlonzoBabbageUpdateProposal, NativeScript, TxCertificate, TxCertificateWithPos, TxOutput,
    UTxOIdentifier, VKeyWitness, Value, Withdrawal,
};

const DEFAULT_KEY_DEPOSIT: u64 = 2_000_000;
const DEFAULT_POOL_DEPOSIT: u64 = 500_000_000;

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

    pub fn calculate_total_deposits(&self, protocol_params: &ProtocolParams) -> u64 {
        self.certs
            .iter()
            .map(|TxCertificateWithPos { cert, .. }| match &cert {
                // TODO:
                // check stake_address is already registered
                TxCertificate::StakeRegistration(_stake_address) => protocol_params
                    .shelley
                    .as_ref()
                    .map(|shelley| shelley.protocol_params.key_deposit)
                    .unwrap_or(DEFAULT_KEY_DEPOSIT),
                TxCertificate::Registration(registration) => registration.deposit,
                TxCertificate::StakeRegistrationAndDelegation(delegation) => delegation.deposit,
                TxCertificate::StakeRegistrationAndVoteDelegation(delegation) => delegation.deposit,
                TxCertificate::StakeRegistrationAndStakeAndVoteDelegation(delegation) => {
                    delegation.deposit
                }
                TxCertificate::PoolRegistration(_) => protocol_params
                    .shelley
                    .as_ref()
                    .map(|shelley| shelley.protocol_params.pool_deposit)
                    .unwrap_or(DEFAULT_POOL_DEPOSIT),
                _ => 0,
            })
            .sum()
    }

    pub fn calculate_total_refund(&self, protocol_params: &ProtocolParams) -> u64 {
        self.certs
            .iter()
            .map(|TxCertificateWithPos { cert, .. }| match &cert {
                // TODO:
                // check stake_address is already deregistered
                TxCertificate::StakeDeregistration(_stake_address) => protocol_params
                    .shelley
                    .as_ref()
                    .map(|shelley| shelley.protocol_params.key_deposit)
                    .unwrap_or(DEFAULT_KEY_DEPOSIT),
                TxCertificate::Deregistration(deregistration) => deregistration.refund,
                TxCertificate::PoolRetirement(_) => protocol_params
                    .shelley
                    .as_ref()
                    .map(|shelley| shelley.protocol_params.pool_deposit)
                    .unwrap_or(DEFAULT_POOL_DEPOSIT),
                _ => 0,
            })
            .sum()
    }

    pub fn calculate_total_consumed_except_inputs(
        &self,
        protocol_params: &ProtocolParams,
    ) -> Value {
        // sum all withdrawals amounts
        let withdrawals = self.withdrawals.iter().map(|withdrawal| withdrawal.value).sum::<u64>();
        let refunds = self.calculate_total_refund(protocol_params);

        Value::new(withdrawals + refunds, vec![])
    }

    pub fn calculate_total_produced(&self, protocol_params: &ProtocolParams) -> Value {
        let total_output = self.calculate_total_output();
        let total_deposits = self.calculate_total_deposits(protocol_params);

        total_output + Value::new(self.fee + total_deposits, vec![])
    }
}
