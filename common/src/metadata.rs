use minicbor::data::Int;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Clone)]
pub struct MetadataInt(pub Int);

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum Metadata {
    Int(MetadataInt),
    Bytes(Vec<u8>),
    Text(String),
    Array(Vec<Metadata>),
    Map(Vec<(Metadata, Metadata)>),
}

impl Serialize for MetadataInt {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_i128(self.0.into())
    }
}

impl<'a> Deserialize<'a> for MetadataInt {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'a>,
    {
        // TODO if this is ever used, i64 may not be enough!
        Ok(MetadataInt(Int::from(i64::deserialize(deserializer)?)))
    }
}
