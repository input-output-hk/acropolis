// This file was code-generated using an experimental CDDL to rust tool:
// https://github.com/dcSpark/cddl-codegen

use crate::serialization::{LenEncoding, StringEncoding};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default)]
pub struct AssetQuantityU64Encoding {
    pub len_encoding: LenEncoding,
    pub orig_deser_order: Vec<usize>,
    pub asset_id_encoding: StringEncoding,
    pub asset_id_key_encoding: StringEncoding,
    pub quantity_encoding: Option<cbor_event::Sz>,
    pub quantity_key_encoding: StringEncoding,
}
