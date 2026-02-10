use std::collections::HashMap;

pub type Metadata = HashMap<MetadatumLabel, Metadatum>;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum Metadatum {
    Int(i128),
    Bytes(Vec<u8>),
    Text(String),
    Array(Vec<Metadatum>),
    Map(Vec<(Metadatum, Metadatum)>),
}

pub type MetadatumLabel = u64;
