// This file was code-generated using an experimental CDDL to rust tool:
// https://github.com/dcSpark/cddl-codegen

pub mod serialization;

use crate::common::{Address, Coin, Credential, GovActionId, Hash28, Hash32, Keyhash};
use crate::error::*;
use crate::{AssetQuantityU64, Int};
use std::collections::BTreeMap;
use std::convert::TryFrom;

#[derive(Clone, Debug)]
pub struct AssetValue {
    pub lovelace: Coin,
    pub multi_asset: Multiasset,
}

impl AssetValue {
    pub fn new(lovelace: Coin, multi_asset: Multiasset) -> Self {
        Self {
            lovelace,
            multi_asset,
        }
    }
}

#[derive(Clone, Debug)]
pub struct BabbageTxOut {
    pub key_0: Address,
    pub key_1: Value,
    pub key_2: Option<DatumOption>,
    pub key_3: Option<ScriptRef>,
}

impl BabbageTxOut {
    pub fn new(key_0: Address, key_1: Value) -> Self {
        Self {
            key_0,
            key_1,
            key_2: None,
            key_3: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum BigInt {
    Int(Int),
    BigUint(BigUint),
    BigNint(BigNint),
}

impl BigInt {
    pub fn new_int(int: Int) -> Self {
        Self::Int(int)
    }

    pub fn new_big_uint(big_uint: BigUint) -> Self {
        Self::BigUint(big_uint)
    }

    pub fn new_big_nint(big_nint: BigNint) -> Self {
        Self::BigNint(big_nint)
    }
}

pub type BigNint = BoundedBytes;

pub type BigUint = BoundedBytes;

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct BoundedBytes(Vec<u8>);

impl BoundedBytes {
    pub fn new(inner: Vec<u8>) -> Result<Self, DeserializeError> {
        if inner.len() > 64 {
            return Err(DeserializeError::new(
                "BoundedBytes",
                DeserializeFailure::RangeCheck {
                    found: inner.len() as isize,
                    min: Some(0),
                    max: Some(64),
                },
            ));
        }
        Ok(Self(inner))
    }
}

impl TryFrom<Vec<u8>> for BoundedBytes {
    type Error = DeserializeError;

    fn try_from(inner: Vec<u8>) -> Result<Self, Self::Error> {
        BoundedBytes::new(inner)
    }
}

impl From<BoundedBytes> for Vec<u8> {
    fn from(wrapper: BoundedBytes) -> Self {
        wrapper.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum Constr {
    ArrPlutusData(Vec<PlutusData>),
    ArrPlutusData2(Vec<PlutusData>),
    ArrPlutusData3(Vec<PlutusData>),
    ArrPlutusData4(Vec<PlutusData>),
    ArrPlutusData5(Vec<PlutusData>),
    ArrPlutusData6(Vec<PlutusData>),
    ArrPlutusData7(Vec<PlutusData>),
}

impl Constr {
    pub fn new_arr_plutus_data(arr_plutus_data: Vec<PlutusData>) -> Self {
        Self::ArrPlutusData(arr_plutus_data)
    }

    pub fn new_arr_plutus_data2(arr_plutus_data2: Vec<PlutusData>) -> Self {
        Self::ArrPlutusData2(arr_plutus_data2)
    }

    pub fn new_arr_plutus_data3(arr_plutus_data3: Vec<PlutusData>) -> Self {
        Self::ArrPlutusData3(arr_plutus_data3)
    }

    pub fn new_arr_plutus_data4(arr_plutus_data4: Vec<PlutusData>) -> Self {
        Self::ArrPlutusData4(arr_plutus_data4)
    }

    pub fn new_arr_plutus_data5(arr_plutus_data5: Vec<PlutusData>) -> Self {
        Self::ArrPlutusData5(arr_plutus_data5)
    }

    pub fn new_arr_plutus_data6(arr_plutus_data6: Vec<PlutusData>) -> Self {
        Self::ArrPlutusData6(arr_plutus_data6)
    }

    pub fn new_arr_plutus_data7(arr_plutus_data7: Vec<PlutusData>) -> Self {
        Self::ArrPlutusData7(arr_plutus_data7)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct CredentialDeposit {
    pub credential: Credential,
}

impl CredentialDeposit {
    pub fn new(credential: Credential) -> Self {
        Self { credential }
    }
}

pub type Data = PlutusData;

#[derive(Clone, Debug)]
pub enum DatumOption {
    I0,
    I1,
}

impl DatumOption {
    pub fn new_i0() -> Self {
        Self::I0
    }

    pub fn new_i1() -> Self {
        Self::I1
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum Deposit {
    CredentialDeposit(CredentialDeposit),
    PoolDeposit(PoolDeposit),
    DrepDeposit(DrepDeposit),
    GovActionDeposit(GovActionDeposit),
}

impl Deposit {
    pub fn new_credential_deposit(credential: Credential) -> Self {
        Self::CredentialDeposit(CredentialDeposit::new(credential))
    }

    pub fn new_pool_deposit(keyhash: Keyhash) -> Self {
        Self::PoolDeposit(PoolDeposit::new(keyhash))
    }

    pub fn new_drep_deposit(credential: Credential) -> Self {
        Self::DrepDeposit(DrepDeposit::new(credential))
    }

    pub fn new_gov_action_deposit(gov_action_id: GovActionId) -> Self {
        Self::GovActionDeposit(GovActionDeposit::new(gov_action_id))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct DrepDeposit {
    pub credential: Credential,
}

impl DrepDeposit {
    pub fn new(credential: Credential) -> Self {
        Self { credential }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct GovActionDeposit {
    pub gov_action_id: GovActionId,
}

impl GovActionDeposit {
    pub fn new(gov_action_id: GovActionId) -> Self {
        Self { gov_action_id }
    }
}

pub type Int64 = i64;

#[derive(Clone, Debug)]
pub struct InvalidBefore {
    pub slot_no: SlotNo,
}

impl InvalidBefore {
    pub fn new(slot_no: SlotNo) -> Self {
        Self { slot_no }
    }
}

#[derive(Clone, Debug)]
pub struct InvalidHereafter {
    pub slot_no: SlotNo,
}

impl InvalidHereafter {
    pub fn new(slot_no: SlotNo) -> Self {
        Self { slot_no }
    }
}

#[derive(Clone, Debug)]
pub struct Multiasset {
    pub policy_id: Hash28,
    pub asset_bundle: AssetQuantityU64,
}

impl Multiasset {
    pub fn new(policy_id: Hash28, asset_bundle: AssetQuantityU64) -> Self {
        Self {
            policy_id,
            asset_bundle,
        }
    }
}

#[derive(Clone, Debug)]
pub enum NativeScript {
    ScriptPubkey(ScriptPubkey),
    ScriptAll(ScriptAll),
    ScriptAny(ScriptAny),
    ScriptNOfK(ScriptNOfK),
    InvalidBefore(InvalidBefore),
    InvalidHereafter(InvalidHereafter),
}

impl NativeScript {
    pub fn new_script_pubkey(hash_28: Hash28) -> Self {
        Self::ScriptPubkey(ScriptPubkey::new(hash_28))
    }

    pub fn new_script_all(native_scripts: Vec<NativeScript>) -> Self {
        Self::ScriptAll(ScriptAll::new(native_scripts))
    }

    pub fn new_script_any(native_scripts: Vec<NativeScript>) -> Self {
        Self::ScriptAny(ScriptAny::new(native_scripts))
    }

    pub fn new_script_n_of_k(n: Int64, native_scripts: Vec<NativeScript>) -> Self {
        Self::ScriptNOfK(ScriptNOfK::new(n, native_scripts))
    }

    pub fn new_invalid_before(slot_no: SlotNo) -> Self {
        Self::InvalidBefore(InvalidBefore::new(slot_no))
    }

    pub fn new_invalid_hereafter(slot_no: SlotNo) -> Self {
        Self::InvalidHereafter(InvalidHereafter::new(slot_no))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum PlutusData {
    Constr(Constr),
    MapPlutusDataToPlutusData(BTreeMap<PlutusData, PlutusData>),
    ArrPlutusData(Vec<PlutusData>),
    BigInt(BigInt),
    BoundedBytes(BoundedBytes),
}

impl PlutusData {
    pub fn new_constr(constr: Constr) -> Self {
        Self::Constr(constr)
    }

    pub fn new_map_plutus_data_to_plutus_data(
        map_plutus_data_to_plutus_data: BTreeMap<PlutusData, PlutusData>,
    ) -> Self {
        Self::MapPlutusDataToPlutusData(map_plutus_data_to_plutus_data)
    }

    pub fn new_arr_plutus_data(arr_plutus_data: Vec<PlutusData>) -> Self {
        Self::ArrPlutusData(arr_plutus_data)
    }

    pub fn new_big_int(big_int: BigInt) -> Self {
        Self::BigInt(big_int)
    }

    pub fn new_bounded_bytes(bounded_bytes: BoundedBytes) -> Self {
        Self::BoundedBytes(bounded_bytes)
    }
}

pub type PlutusV1Script = Vec<u8>;

pub type PlutusV2Script = Vec<u8>;

pub type PlutusV3Script = Vec<u8>;

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct PoolDeposit {
    pub keyhash: Keyhash,
}

impl PoolDeposit {
    pub fn new(keyhash: Keyhash) -> Self {
        Self { keyhash }
    }
}

#[derive(Clone, Debug)]
pub enum Script {
    Naitve,
    PlutusV1,
    PlutusV2,
    PlutusV3,
}

impl Script {
    pub fn new_naitve() -> Self {
        Self::Naitve
    }

    pub fn new_plutus_v1() -> Self {
        Self::PlutusV1
    }

    pub fn new_plutus_v2() -> Self {
        Self::PlutusV2
    }

    pub fn new_plutus_v3() -> Self {
        Self::PlutusV3
    }
}

#[derive(Clone, Debug)]
pub struct ScriptAll {
    pub native_scripts: Vec<NativeScript>,
}

impl ScriptAll {
    pub fn new(native_scripts: Vec<NativeScript>) -> Self {
        Self { native_scripts }
    }
}

#[derive(Clone, Debug)]
pub struct ScriptAny {
    pub native_scripts: Vec<NativeScript>,
}

impl ScriptAny {
    pub fn new(native_scripts: Vec<NativeScript>) -> Self {
        Self { native_scripts }
    }
}

#[derive(Clone, Debug)]
pub struct ScriptNOfK {
    pub n: Int64,
    pub native_scripts: Vec<NativeScript>,
}

impl ScriptNOfK {
    pub fn new(n: Int64, native_scripts: Vec<NativeScript>) -> Self {
        Self { n, native_scripts }
    }
}

#[derive(Clone, Debug)]
pub struct ScriptPubkey {
    pub hash_28: Hash28,
}

impl ScriptPubkey {
    pub fn new(hash_28: Hash28) -> Self {
        Self { hash_28 }
    }
}

pub type ScriptRef = Script;

#[derive(Clone, Debug)]
pub struct ShelleyTxOut {
    pub address: Address,
    pub value: Value,
    pub hash_32: Option<Hash32>,
}

impl ShelleyTxOut {
    pub fn new(address: Address, value: Value) -> Self {
        Self {
            address,
            value,
            hash_32: None,
        }
    }
}

pub type SlotNo = u64;

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct TxIn {
    pub hash_32: Hash32,
    pub uint: u16,
}

impl TxIn {
    pub fn new(hash_32: Hash32, uint: u16) -> Self {
        Self { hash_32, uint }
    }
}

#[derive(Clone, Debug)]
pub enum TxOut {
    ShelleyTxOut(ShelleyTxOut),
    BabbageTxOut(BabbageTxOut),
}

impl TxOut {
    pub fn new_shelley_tx_out(shelley_tx_out: ShelleyTxOut) -> Self {
        Self::ShelleyTxOut(shelley_tx_out)
    }

    pub fn new_babbage_tx_out(babbage_tx_out: BabbageTxOut) -> Self {
        Self::BabbageTxOut(babbage_tx_out)
    }
}

#[derive(Clone, Debug)]
pub struct UtxoState {
    pub utxos: BTreeMap<TxIn, TxOut>,
    pub fees: Coin,
    pub deposits: BTreeMap<Deposit, Coin>,
    pub donations: Coin,
}

impl UtxoState {
    pub fn new(
        utxos: BTreeMap<TxIn, TxOut>,
        fees: Coin,
        deposits: BTreeMap<Deposit, Coin>,
        donations: Coin,
    ) -> Self {
        Self {
            utxos,
            fees,
            deposits,
            donations,
        }
    }
}

#[derive(Clone, Debug)]
pub enum Value {
    Coin(Coin),
    AssetValue(AssetValue),
}

impl Value {
    pub fn new_coin(coin: Coin) -> Self {
        Self::Coin(coin)
    }

    pub fn new_asset_value(asset_value: AssetValue) -> Self {
        Self::AssetValue(asset_value)
    }
}
