use crate::{
    protocol_params::ProtocolParams, validation::Phase1ValidationError,
    AlonzoBabbageUpdateProposal, NativeScript, TxCertificate, TxCertificateWithPos, TxOutput,
    UTxOIdentifier, VKeyWitness, Withdrawal,
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
    pub fn calculate_total_output(&self) -> u128 {
        self.produces.iter().map(|output| output.value.coin() as u128).sum()
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

    pub fn calculate_total_consumed_except_inputs(&self, protocol_params: &ProtocolParams) -> u128 {
        // sum all withdrawals amounts
        let withdrawals_amounts: u128 =
            self.withdrawals.iter().map(|withdrawal| withdrawal.value as u128).sum();

        let refunds_amounts = self.calculate_total_refund(protocol_params) as u128;

        withdrawals_amounts + refunds_amounts
    }

    pub fn calculate_total_produced(&self, protocol_params: &ProtocolParams) -> u128 {
        let total_output = self.calculate_total_output();
        let total_deposits = self.calculate_total_deposits(protocol_params) as u128;

        total_output + (self.fee as u128) + total_deposits
    }
}
