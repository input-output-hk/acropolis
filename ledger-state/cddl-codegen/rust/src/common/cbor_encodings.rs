// This file was code-generated using an experimental CDDL to rust tool:
// https://github.com/dcSpark/cddl-codegen

use crate::serialization::{LenEncoding, StringEncoding};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default)]
pub struct DenominatorEncoding {
    pub inner_encoding: Option<cbor_event::Sz>,
}

#[derive(Clone, Debug, Default)]
pub struct GovActionIdEncoding {
    pub len_encoding: LenEncoding,
    pub uint_encoding: Option<cbor_event::Sz>,
}

#[derive(Clone, Debug, Default)]
pub struct Hash28Encoding {
    pub inner_encoding: StringEncoding,
}

#[derive(Clone, Debug, Default)]
pub struct Hash32Encoding {
    pub inner_encoding: StringEncoding,
}

#[derive(Clone, Debug, Default)]
pub struct UnitIntervalEncoding {
    pub len_encoding: LenEncoding,
    pub tag_encoding: Option<cbor_event::Sz>,
    pub index_0_encoding: Option<cbor_event::Sz>,
    pub index_1_encoding: Option<cbor_event::Sz>,
}

#[derive(Clone, Debug, Default)]
pub struct UrlEncoding {
    pub inner_encoding: StringEncoding,
}
