use acropolis_common::TxCertificate;
use uplc_turbo::{arena::Arena, data::PlutusData, machine::PlutusVersion};

use super::governance::encode_drep_choice;
use super::to_plutus_data::*;
use acropolis_common::validation::ScriptContextError;

pub fn encode_certificate<'a>(
    cert: &TxCertificate,
    arena: &'a Arena,
    version: PlutusVersion,
) -> Result<&'a PlutusData<'a>, ScriptContextError> {
    match version {
        PlutusVersion::V1 | PlutusVersion::V2 => encode_dcert(cert, arena, version),
        PlutusVersion::V3 => encode_tx_cert(cert, arena, version),
    }
}

// ============================================================================
// DCert encoding (V1/V2)
// ============================================================================

fn encode_dcert<'a>(
    cert: &TxCertificate,
    arena: &'a Arena,
    version: PlutusVersion,
) -> Result<&'a PlutusData<'a>, ScriptContextError> {
    match cert {
        TxCertificate::StakeRegistration(addr)
        | TxCertificate::Registration(acropolis_common::Registration {
            stake_address: addr,
            ..
        }) => {
            let cred = addr.credential.to_plutus_data(arena, version)?;
            let staking = constr(arena, 0, vec![cred]);
            Ok(constr(arena, 0, vec![staking]))
        }
        TxCertificate::StakeDeregistration(addr)
        | TxCertificate::Deregistration(acropolis_common::Deregistration {
            stake_address: addr,
            ..
        }) => {
            let cred = addr.credential.to_plutus_data(arena, version)?;
            let staking = constr(arena, 0, vec![cred]);
            Ok(constr(arena, 1, vec![staking]))
        }
        TxCertificate::StakeDelegation(deleg) => {
            let cred = deleg.stake_address.credential.to_plutus_data(arena, version)?;
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
            let epoch = integer(arena, ret.epoch as i128);
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

fn encode_tx_cert<'a>(
    cert: &TxCertificate,
    arena: &'a Arena,
    version: PlutusVersion,
) -> Result<&'a PlutusData<'a>, ScriptContextError> {
    match cert {
        TxCertificate::StakeRegistration(addr) => {
            let cred = addr.credential.to_plutus_data(arena, version)?;
            let nothing = constr(arena, 1, vec![]);
            Ok(constr(arena, 0, vec![cred, nothing]))
        }
        TxCertificate::Registration(reg) => {
            let cred = reg.stake_address.credential.to_plutus_data(arena, version)?;
            let deposit = integer(arena, reg.deposit as i128);
            let just = constr(arena, 0, vec![deposit]);
            Ok(constr(arena, 0, vec![cred, just]))
        }
        TxCertificate::StakeDeregistration(addr) => {
            let cred = addr.credential.to_plutus_data(arena, version)?;
            let nothing = constr(arena, 1, vec![]);
            Ok(constr(arena, 1, vec![cred, nothing]))
        }
        TxCertificate::Deregistration(dereg) => {
            let cred = dereg.stake_address.credential.to_plutus_data(arena, version)?;
            let refund = integer(arena, dereg.refund as i128);
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
            let drep = encode_drep_choice(&vd.drep, arena, version)?;
            let delegatee = constr(arena, 1, vec![drep]);
            Ok(constr(arena, 2, vec![cred, delegatee]))
        }
        TxCertificate::StakeAndVoteDelegation(svd) => {
            let cred = svd.stake_address.credential.to_plutus_data(arena, version)?;
            let pool = svd.operator.to_plutus_data(arena, version)?;
            let drep = encode_drep_choice(&svd.drep, arena, version)?;
            let delegatee = constr(arena, 2, vec![pool, drep]);
            Ok(constr(arena, 2, vec![cred, delegatee]))
        }
        TxCertificate::StakeRegistrationAndDelegation(srd) => {
            let cred = srd.stake_address.credential.to_plutus_data(arena, version)?;
            let pool = srd.operator.to_plutus_data(arena, version)?;
            let delegatee = constr(arena, 0, vec![pool]);
            let deposit = integer(arena, srd.deposit as i128);
            Ok(constr(arena, 3, vec![cred, delegatee, deposit]))
        }
        TxCertificate::StakeRegistrationAndVoteDelegation(svrd) => {
            let cred = svrd.stake_address.credential.to_plutus_data(arena, version)?;
            let drep = encode_drep_choice(&svrd.drep, arena, version)?;
            let delegatee = constr(arena, 1, vec![drep]);
            let deposit = integer(arena, svrd.deposit as i128);
            Ok(constr(arena, 3, vec![cred, delegatee, deposit]))
        }
        TxCertificate::StakeRegistrationAndStakeAndVoteDelegation(ssvrd) => {
            let cred = ssvrd.stake_address.credential.to_plutus_data(arena, version)?;
            let pool = ssvrd.operator.to_plutus_data(arena, version)?;
            let drep = encode_drep_choice(&ssvrd.drep, arena, version)?;
            let delegatee = constr(arena, 2, vec![pool, drep]);
            let deposit = integer(arena, ssvrd.deposit as i128);
            Ok(constr(arena, 3, vec![cred, delegatee, deposit]))
        }
        TxCertificate::AuthCommitteeHot(auth) => {
            let cold = auth.cold_credential.to_plutus_data(arena, version)?;
            let hot = auth.hot_credential.to_plutus_data(arena, version)?;
            Ok(constr(arena, 4, vec![cold, hot]))
        }
        TxCertificate::ResignCommitteeCold(resign) => {
            let cold = resign.cold_credential.to_plutus_data(arena, version)?;
            let nothing = constr(arena, 1, vec![]);
            Ok(constr(arena, 5, vec![cold, nothing]))
        }
        TxCertificate::DRepRegistration(drep_reg) => {
            let cred = drep_reg.credential.to_plutus_data(arena, version)?;
            let deposit = integer(arena, drep_reg.deposit as i128);
            let nothing = constr(arena, 1, vec![]);
            Ok(constr(arena, 6, vec![cred, deposit, nothing]))
        }
        TxCertificate::DRepDeregistration(drep_dereg) => {
            let cred = drep_dereg.credential.to_plutus_data(arena, version)?;
            let refund = integer(arena, drep_dereg.refund as i128);
            Ok(constr(arena, 7, vec![cred, refund]))
        }
        TxCertificate::DRepUpdate(drep_update) => {
            let cred = drep_update.credential.to_plutus_data(arena, version)?;
            let nothing = constr(arena, 1, vec![]);
            Ok(constr(arena, 8, vec![cred, nothing]))
        }
        TxCertificate::PoolRegistration(pool_reg) => {
            let op = pool_reg.operator.to_plutus_data(arena, version)?;
            let vrf = pool_reg.vrf_key_hash.to_plutus_data(arena, version)?;
            Ok(constr(arena, 9, vec![op, vrf]))
        }
        TxCertificate::PoolRetirement(ret) => {
            let op = ret.operator.to_plutus_data(arena, version)?;
            let epoch = integer(arena, ret.epoch as i128);
            Ok(constr(arena, 10, vec![op, epoch]))
        }
        _ => Err(ScriptContextError::UnsupportedCertificate),
    }
}
