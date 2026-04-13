use acropolis_common::{
    validation::ScriptContextError, Address, Credential, ShelleyAddress,
    ShelleyAddressDelegationPart, ShelleyAddressPaymentPart, StakeAddress,
};
use uplc_turbo::{arena::Arena, data::PlutusData, machine::PlutusVersion};

use super::to_plutus_data::*;

impl ToPlutusData for ShelleyAddressPaymentPart {
    fn to_plutus_data<'a>(
        &self,
        arena: &'a Arena,
        version: PlutusVersion,
    ) -> Result<&'a PlutusData<'a>, ScriptContextError> {
        let cred = match &self {
            ShelleyAddressPaymentPart::PaymentKeyHash(hash) => Credential::AddrKeyHash(*hash),
            ShelleyAddressPaymentPart::ScriptHash(hash) => Credential::ScriptHash(*hash),
        };
        cred.to_plutus_data(arena, version)
    }
}

impl ToPlutusData for ShelleyAddressDelegationPart {
    fn to_plutus_data<'a>(
        &self,
        arena: &'a Arena,
        version: PlutusVersion,
    ) -> Result<&'a PlutusData<'a>, ScriptContextError> {
        match self {
            ShelleyAddressDelegationPart::StakeKeyHash(hash) => {
                let addr_key_cred =
                    Credential::AddrKeyHash(*hash).to_plutus_data(arena, version)?;
                let inline_cred = constr(arena, 0, vec![addr_key_cred]);
                // Some(inline_cred)
                Ok(constr(arena, 0, vec![inline_cred]))
            }
            ShelleyAddressDelegationPart::ScriptHash(hash) => {
                let script_cred = Credential::ScriptHash(*hash).to_plutus_data(arena, version)?;
                let inline_cred = constr(arena, 0, vec![script_cred]);
                // Some(inline_cred)
                Ok(constr(arena, 0, vec![inline_cred]))
            }
            ShelleyAddressDelegationPart::Pointer(ptr) => {
                let staking_ptr = constr(
                    arena,
                    1,
                    vec![
                        ptr.slot.to_plutus_data(arena, version)?,
                        ptr.tx_index.to_plutus_data(arena, version)?,
                        ptr.cert_index.to_plutus_data(arena, version)?,
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
        let pay = self.payment.to_plutus_data(arena, version)?;
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
                let stake_cred = stake.to_plutus_data(arena, version)?;
                match version {
                    PlutusVersion::V1 | PlutusVersion::V2 => Ok(constr(arena, 0, vec![stake_cred])),
                    PlutusVersion::V3 => Ok(stake_cred),
                }
            }
            Address::None => Err(ScriptContextError::UnsupportedAddress(
                "None address not supported in Plutus script context".into(),
            )),
        }
    }
}
