use std::ops::Deref;

#[derive(Default, Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Metadata(pub Vec<(MetadatumLabel, Metadatum)>);

impl AsRef<Vec<(MetadatumLabel, Metadatum)>> for Metadata {
    fn as_ref(&self) -> &Vec<(MetadatumLabel, Metadatum)> {
        &self.0
    }
}

impl AsMut<Vec<(MetadatumLabel, Metadatum)>> for Metadata {
    fn as_mut(&mut self) -> &mut Vec<(MetadatumLabel, Metadatum)> {
        &mut self.0
    }
}

impl Deref for Metadata {
    type Target = Vec<(MetadatumLabel, Metadatum)>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum Metadatum {
    Int(i128),
    Bytes(Vec<u8>),
    Text(String),
    Array(Vec<Metadatum>),
    Map(Vec<(Metadatum, Metadatum)>),
}

pub type MetadatumLabel = u64;
