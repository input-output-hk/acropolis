// This file was code-generated using an experimental CDDL to rust tool:
// https://github.com/dcSpark/cddl-codegen

use crate::serialization::{LenEncoding, StringEncoding};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default)]
pub struct DnsNameEncoding {
    pub inner_encoding: StringEncoding,
}

#[derive(Clone, Debug, Default)]
pub struct Ipv4Encoding {
    pub inner_encoding: StringEncoding,
}

#[derive(Clone, Debug, Default)]
pub struct Ipv6Encoding {
    pub inner_encoding: StringEncoding,
}

#[derive(Clone, Debug, Default)]
pub struct MultiHostNameEncoding {
    pub len_encoding: LenEncoding,
    pub index_0_encoding: Option<cbor_event::Sz>,
}

#[derive(Clone, Debug, Default)]
pub struct PoolMetadataEncoding {
    pub len_encoding: LenEncoding,
    pub index_1_encoding: StringEncoding,
}

#[derive(Clone, Debug, Default)]
pub struct PoolParametersEncoding {
    pub len_encoding: LenEncoding,
    pub pledge_encoding: Option<cbor_event::Sz>,
    pub cost_encoding: Option<cbor_event::Sz>,
    pub reward_account_encoding: StringEncoding,
    pub relays_encoding: LenEncoding,
}

#[derive(Clone, Debug, Default)]
pub struct SingleHostAddrEncoding {
    pub len_encoding: LenEncoding,
    pub index_0_encoding: Option<cbor_event::Sz>,
    pub port_encoding: Option<cbor_event::Sz>,
}

#[derive(Clone, Debug, Default)]
pub struct SingleHostNameEncoding {
    pub len_encoding: LenEncoding,
    pub index_0_encoding: Option<cbor_event::Sz>,
    pub port_encoding: Option<cbor_event::Sz>,
}

#[derive(Clone, Debug, Default)]
pub struct SpoStateEncoding {
    pub len_encoding: LenEncoding,
    pub orig_deser_order: Vec<usize>,
    pub pools_encoding: LenEncoding,
    pub pools_key_encoding: StringEncoding,
    pub retiring_encoding: LenEncoding,
    pub retiring_value_encodings: BTreeMap<Hash28, Option<cbor_event::Sz>>,
    pub retiring_key_encoding: StringEncoding,
}
