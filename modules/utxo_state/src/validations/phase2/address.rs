use acropolis_common::{
    Address, Credential, ShelleyAddress, ShelleyAddressDelegationPart, ShelleyAddressPaymentPart,
    StakeAddress,
};
use uplc_turbo::{arena::Arena, data::PlutusData, machine::PlutusVersion};

use super::to_plutus_data::*;
use acropolis_common::validation::ScriptContextError;

impl ToPlutusData for ShelleyAddressDelegationPart {
    fn to_plutus_data<'a>(
        &self,
        arena: &'a Arena,
        version: PlutusVersion,
    ) -> Result<&'a PlutusData<'a>, ScriptContextError> {
        match self {
            // Just
            ShelleyAddressDelegationPart::StakeKeyHash(hash) => {
                let addr_hash = Credential::AddrKeyHash(*hash).to_plutus_data(arena, version)?;
                let cred = constr(arena, 0, vec![addr_hash]);
                Ok(constr(arena, 0, vec![cred]))
            }
            ShelleyAddressDelegationPart::ScriptHash(hash) => {
                let script_hash = Credential::ScriptHash(*hash).to_plutus_data(arena, version)?;
                let cred = constr(arena, 0, vec![script_hash]);
                Ok(constr(arena, 0, vec![cred]))
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
            // Nothing
            ShelleyAddressDelegationPart::None => Ok(constr(arena, 1, vec![])),
        }
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
        let stake = self.delegation.to_plutus_data(arena, version)?;
        Ok(constr(arena, 0, vec![pay, stake]))
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
                let state_cred = stake.to_plutus_data(arena, version)?;
                match version {
                    PlutusVersion::V1 | PlutusVersion::V2 => Ok(constr(arena, 0, vec![state_cred])),
                    PlutusVersion::V3 => Ok(state_cred),
                }
            }
            Address::None => Err(ScriptContextError::UnsupportedAddress(
                "None address not supported in Plutus script context".into(),
            )),
        }
    }
}
