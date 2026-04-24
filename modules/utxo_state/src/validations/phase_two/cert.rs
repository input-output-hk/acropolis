use acropolis_common::{
    validation::ScriptContextError, Deregistration, Registration, TxCertificate,
};
use amaru_uplc::{arena::Arena, data::PlutusData, machine::PlutusVersion};

use super::to_plutus_data::*;

impl ToPlutusData for TxCertificate {
    fn to_plutus_data<'a>(
        &self,
        arena: &'a Arena,
        version: PlutusVersion,
    ) -> Result<&'a PlutusData<'a>, ScriptContextError> {
        match version {
            PlutusVersion::V1 | PlutusVersion::V2 => encode_dcert(self, arena, version),
            PlutusVersion::V3 => encode_tx_cert(self, arena, version),
        }
    }
}

// ============================================================================
// DCert encoding (V1/V2)
// ============================================================================

/// Reference: https://github.com/IntersectMBO/plutus/blob/master/plutus-ledger-api/src/PlutusLedgerApi/V1/DCert.hs
fn encode_dcert<'a>(
    cert: &TxCertificate,
    arena: &'a Arena,
    version: PlutusVersion,
) -> Result<&'a PlutusData<'a>, ScriptContextError> {
    match cert {
        TxCertificate::StakeRegistration(addr)
        | TxCertificate::Registration(Registration {
            stake_address: addr,
            ..
        }) => {
            let cred = addr.credential.to_plutus_data(arena, version)?;
            // Staking Hash Wapper
            let staking = constr(arena, 0, vec![cred]);
            Ok(constr(arena, 0, vec![staking]))
        }
        TxCertificate::StakeDeregistration(addr)
        | TxCertificate::Deregistration(Deregistration {
            stake_address: addr,
            ..
        }) => {
            let cred = addr.credential.to_plutus_data(arena, version)?;
            // Staking Hash Wrapper
            let staking = constr(arena, 0, vec![cred]);
            Ok(constr(arena, 1, vec![staking]))
        }
        TxCertificate::StakeDelegation(deleg) => {
            let cred = deleg.stake_address.credential.to_plutus_data(arena, version)?;
            // Staking Hash Wrapper
            let staking = constr(arena, 0, vec![cred]);
            let pool = deleg.operator.to_plutus_data(arena, version)?;
            Ok(constr(arena, 2, vec![staking, pool]))
        }
        TxCertificate::PoolRegistration(pool_reg) => {
            let op = pool_reg.operator.to_plutus_data(arena, version)?;
            let vrf = pool_reg.vrf_key_hash.to_plutus_data(arena, version)?;
            Ok(constr(arena, 3, vec![op, vrf]))
        }
        TxCertificate::PoolRetirement(ret) => {
            let op = ret.operator.to_plutus_data(arena, version)?;
            let epoch = ret.epoch.to_plutus_data(arena, version)?;
            Ok(constr(arena, 4, vec![op, epoch]))
        }
        TxCertificate::GenesisKeyDelegation(_) => Ok(constr(arena, 5, vec![])),
        TxCertificate::MoveInstantaneousReward(_) => Ok(constr(arena, 6, vec![])),
        _ => Err(ScriptContextError::UnsupportedCertificate),
    }
}

// ============================================================================
// TxCert encoding (V3)
// ============================================================================

/// Reference: https://github.com/IntersectMBO/plutus/blob/4b90cc267ac620739723236ecd8c0bf3361c558d/plutus-ledger-api/src/PlutusLedgerApi/V3/Contexts.hs#L178
fn encode_tx_cert<'a>(
    cert: &TxCertificate,
    arena: &'a Arena,
    version: PlutusVersion,
) -> Result<&'a PlutusData<'a>, ScriptContextError> {
    match cert {
        TxCertificate::StakeRegistration(addr) => {
            let cred = addr.credential.to_plutus_data(arena, version)?;
            // Option<deposit>
            let nothing = constr(arena, 1, vec![]);
            Ok(constr(arena, 0, vec![cred, nothing]))
        }
        TxCertificate::Registration(reg) => {
            let cred = reg.stake_address.credential.to_plutus_data(arena, version)?;
            let deposit = reg.deposit.to_plutus_data(arena, version)?;
            let just = constr(arena, 0, vec![deposit]);
            Ok(constr(arena, 0, vec![cred, just]))
        }
        TxCertificate::StakeDeregistration(addr) => {
            let cred = addr.credential.to_plutus_data(arena, version)?;
            // Option<refund>
            let nothing = constr(arena, 1, vec![]);
            Ok(constr(arena, 1, vec![cred, nothing]))
        }
        TxCertificate::Deregistration(dereg) => {
            let cred = dereg.stake_address.credential.to_plutus_data(arena, version)?;
            let refund = dereg.refund.to_plutus_data(arena, version)?;
            let just = constr(arena, 0, vec![refund]);
            Ok(constr(arena, 1, vec![cred, just]))
        }
        TxCertificate::StakeDelegation(deleg) => {
            let cred = deleg.stake_address.credential.to_plutus_data(arena, version)?;
            let pool = deleg.operator.to_plutus_data(arena, version)?;
            let delegatee = constr(arena, 0, vec![pool]);
            Ok(constr(arena, 2, vec![cred, delegatee]))
        }
        TxCertificate::VoteDelegation(vd) => {
            let cred = vd.stake_address.credential.to_plutus_data(arena, version)?;
            let drep = vd.drep.to_plutus_data(arena, version)?;
            let delegatee = constr(arena, 1, vec![drep]);
            Ok(constr(arena, 2, vec![cred, delegatee]))
        }
        TxCertificate::StakeAndVoteDelegation(svd) => {
            let cred = svd.stake_address.credential.to_plutus_data(arena, version)?;
            let pool = svd.operator.to_plutus_data(arena, version)?;
            let drep = svd.drep.to_plutus_data(arena, version)?;
            let delegatee = constr(arena, 2, vec![pool, drep]);
            Ok(constr(arena, 2, vec![cred, delegatee]))
        }
        TxCertificate::StakeRegistrationAndDelegation(srd) => {
            let cred = srd.stake_address.credential.to_plutus_data(arena, version)?;
            let pool = srd.operator.to_plutus_data(arena, version)?;
            let delegatee = constr(arena, 0, vec![pool]);
            let deposit = srd.deposit.to_plutus_data(arena, version)?;
            Ok(constr(arena, 3, vec![cred, delegatee, deposit]))
        }
        TxCertificate::StakeRegistrationAndVoteDelegation(svrd) => {
            let cred = svrd.stake_address.credential.to_plutus_data(arena, version)?;
            let drep = svrd.drep.to_plutus_data(arena, version)?;
            let delegatee = constr(arena, 1, vec![drep]);
            let deposit = svrd.deposit.to_plutus_data(arena, version)?;
            Ok(constr(arena, 3, vec![cred, delegatee, deposit]))
        }
        TxCertificate::StakeRegistrationAndStakeAndVoteDelegation(ssvrd) => {
            let cred = ssvrd.stake_address.credential.to_plutus_data(arena, version)?;
            let pool = ssvrd.operator.to_plutus_data(arena, version)?;
            let drep = ssvrd.drep.to_plutus_data(arena, version)?;
            let delegatee = constr(arena, 2, vec![pool, drep]);
            let deposit = ssvrd.deposit.to_plutus_data(arena, version)?;
            Ok(constr(arena, 3, vec![cred, delegatee, deposit]))
        }
        TxCertificate::DRepRegistration(drep_reg) => {
            let cred = drep_reg.credential.to_plutus_data(arena, version)?;
            let deposit = drep_reg.deposit.to_plutus_data(arena, version)?;
            Ok(constr(arena, 4, vec![cred, deposit]))
        }
        TxCertificate::DRepUpdate(drep_update) => {
            let cred = drep_update.credential.to_plutus_data(arena, version)?;
            Ok(constr(arena, 5, vec![cred]))
        }
        TxCertificate::DRepDeregistration(drep_dereg) => {
            let cred = drep_dereg.credential.to_plutus_data(arena, version)?;
            let refund = drep_dereg.refund.to_plutus_data(arena, version)?;
            Ok(constr(arena, 6, vec![cred, refund]))
        }
        TxCertificate::PoolRegistration(pool_reg) => {
            let op = pool_reg.operator.to_plutus_data(arena, version)?;
            let vrf = pool_reg.vrf_key_hash.to_plutus_data(arena, version)?;
            Ok(constr(arena, 7, vec![op, vrf]))
        }
        TxCertificate::PoolRetirement(ret) => {
            let op = ret.operator.to_plutus_data(arena, version)?;
            let epoch = ret.epoch.to_plutus_data(arena, version)?;
            Ok(constr(arena, 8, vec![op, epoch]))
        }
        TxCertificate::AuthCommitteeHot(auth) => {
            let cold = auth.cold_credential.to_plutus_data(arena, version)?;
            let hot = auth.hot_credential.to_plutus_data(arena, version)?;
            Ok(constr(arena, 9, vec![cold, hot]))
        }
        TxCertificate::ResignCommitteeCold(resign) => {
            let cold = resign.cold_credential.to_plutus_data(arena, version)?;
            Ok(constr(arena, 10, vec![cold]))
        }
        _ => Err(ScriptContextError::UnsupportedCertificate),
    }
}
