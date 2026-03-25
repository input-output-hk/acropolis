use acropolis_common::{
    Address, Credential, ShelleyAddress, ShelleyAddressDelegationPart,
    ShelleyAddressPaymentPart, StakeAddress,
};
use uplc_turbo::{arena::Arena, data::PlutusData, machine::PlutusVersion};

use acropolis_common::validation::ScriptContextError;
use super::to_plutus_data::*;

fn encode_staking_credential<'a>(
    delegation: &ShelleyAddressDelegationPart,
    arena: &'a Arena,
    version: PlutusVersion,
) -> Result<&'a PlutusData<'a>, ScriptContextError> {
    match delegation {
        ShelleyAddressDelegationPart::StakeKeyHash(hash) => {
            let cred = Credential::AddrKeyHash(*hash).to_plutus_data(arena, version)?;
            let staking_hash = constr(arena, 0, vec![cred]);
            Ok(constr(arena, 0, vec![staking_hash])) // Just
        }
        ShelleyAddressDelegationPart::ScriptHash(hash) => {
            let cred = Credential::ScriptHash(*hash).to_plutus_data(arena, version)?;
            let staking_hash = constr(arena, 0, vec![cred]);
            Ok(constr(arena, 0, vec![staking_hash]))
        }
        ShelleyAddressDelegationPart::Pointer(ptr) => {
            let staking_ptr = constr(
                arena,
                1,
                vec![
                    integer(arena, ptr.slot as i128),
                    integer(arena, ptr.tx_index as i128),
                    integer(arena, ptr.cert_index as i128),
                ],
            );
            Ok(constr(arena, 0, vec![staking_ptr]))
        }
        ShelleyAddressDelegationPart::None => Ok(constr(arena, 1, vec![])), // Nothing
    }
}

impl ToPlutusData for ShelleyAddress {
    fn to_plutus_data<'a>(
        &self,
        arena: &'a Arena,
        version: PlutusVersion,
    ) -> Result<&'a PlutusData<'a>, ScriptContextError> {
        let payment_cred = match &self.payment {
            ShelleyAddressPaymentPart::PaymentKeyHash(hash) => Credential::AddrKeyHash(*hash),
            ShelleyAddressPaymentPart::ScriptHash(hash) => Credential::ScriptHash(*hash),
        };
        let pay = payment_cred.to_plutus_data(arena, version)?;
        let stake = encode_staking_credential(&self.delegation, arena, version)?;
        Ok(constr(arena, 0, vec![pay, stake]))
    }
}

impl ToPlutusData for Address {
    fn to_plutus_data<'a>(
        &self,
        arena: &'a Arena,
        version: PlutusVersion,
    ) -> Result<&'a PlutusData<'a>, ScriptContextError> {
        match self {
            Address::Shelley(shelley) => shelley.to_plutus_data(arena, version),
            Address::Byron(_) => Err(ScriptContextError::UnsupportedAddress(
                "Byron addresses not supported in Plutus script context".into(),
            )),
            Address::Stake(stake) => {
                let pay = stake.credential.to_plutus_data(arena, version)?;
                // Just (StakingHash credential)
                let staking_hash = constr(arena, 0, vec![pay]);
                let just = constr(arena, 0, vec![staking_hash]);
                Ok(constr(arena, 0, vec![pay, just]))
            }
            Address::None => Err(ScriptContextError::UnsupportedAddress(
                "None address not supported in Plutus script context".into(),
            )),
        }
    }
}

impl ToPlutusData for StakeAddress {
    fn to_plutus_data<'a>(
        &self,
        arena: &'a Arena,
        version: PlutusVersion,
    ) -> Result<&'a PlutusData<'a>, ScriptContextError> {
        self.credential.to_plutus_data(arena, version)
    }
}
