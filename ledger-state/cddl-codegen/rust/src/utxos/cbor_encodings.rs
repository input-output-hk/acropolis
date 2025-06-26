// This file was code-generated using an experimental CDDL to rust tool:
// https://github.com/dcSpark/cddl-codegen

use crate::serialization::{LenEncoding, StringEncoding};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default)]
pub struct AssetValueEncoding {
    pub len_encoding: LenEncoding,
    pub lovelace_encoding: Option<cbor_event::Sz>,
}

#[derive(Clone, Debug, Default)]
pub struct BabbageTxOutEncoding {
    pub len_encoding: LenEncoding,
    pub orig_deser_order: Vec<usize>,
    pub key_0_encoding: StringEncoding,
    pub key_0_key_encoding: Option<cbor_event::Sz>,
    pub key_1_key_encoding: Option<cbor_event::Sz>,
    pub key_2_key_encoding: Option<cbor_event::Sz>,
    pub key_3_tag_encoding: Option<cbor_event::Sz>,
    pub key_3_bytes_encoding: StringEncoding,
    pub key_3_key_encoding: Option<cbor_event::Sz>,
}

#[derive(Clone, Debug, Default)]
pub struct BoundedBytesEncoding {
    pub inner_encoding: StringEncoding,
}

#[derive(Clone, Debug, Default)]
pub struct CredentialDepositEncoding {
    pub len_encoding: LenEncoding,
    pub index_0_encoding: Option<cbor_event::Sz>,
}

#[derive(Clone, Debug, Default)]
pub struct DrepDepositEncoding {
    pub len_encoding: LenEncoding,
    pub index_0_encoding: Option<cbor_event::Sz>,
}

#[derive(Clone, Debug, Default)]
pub struct GovActionDepositEncoding {
    pub len_encoding: LenEncoding,
    pub index_0_encoding: Option<cbor_event::Sz>,
}

#[derive(Clone, Debug, Default)]
pub struct MultiassetEncoding {
    pub len_encoding: LenEncoding,
    pub orig_deser_order: Vec<usize>,
    pub policy_id_key_encoding: StringEncoding,
    pub asset_bundle_key_encoding: StringEncoding,
}

#[derive(Clone, Debug, Default)]
pub struct PoolDepositEncoding {
    pub len_encoding: LenEncoding,
    pub index_0_encoding: Option<cbor_event::Sz>,
}

#[derive(Clone, Debug, Default)]
pub struct ScriptEncoding {
    pub len_encoding: LenEncoding,
    pub index_0_encoding: Option<cbor_event::Sz>,
}

#[derive(Clone, Debug, Default)]
pub struct ShelleyTxOutEncoding {
    pub len_encoding: LenEncoding,
    pub address_encoding: StringEncoding,
}

#[derive(Clone, Debug, Default)]
pub struct TxInEncoding {
    pub len_encoding: LenEncoding,
    pub uint_encoding: Option<cbor_event::Sz>,
}

#[derive(Clone, Debug, Default)]
pub struct UtxoStateEncoding {
    pub len_encoding: LenEncoding,
    pub orig_deser_order: Vec<usize>,
    pub utxos_encoding: LenEncoding,
    pub utxos_key_encoding: StringEncoding,
    pub fees_encoding: Option<cbor_event::Sz>,
    pub fees_key_encoding: StringEncoding,
    pub deposits_encoding: LenEncoding,
    pub deposits_value_encodings: BTreeMap<Deposit, Option<cbor_event::Sz>>,
    pub deposits_key_encoding: StringEncoding,
    pub donations_encoding: Option<cbor_event::Sz>,
    pub donations_key_encoding: StringEncoding,
}
