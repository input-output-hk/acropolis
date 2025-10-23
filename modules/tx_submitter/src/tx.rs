use anyhow::Result;
use pallas::ledger::traverse::MultiEraTx;

pub struct Transaction {
    pub id: Vec<u8>,
    pub body: Vec<u8>,
    pub era: u16,
}

impl Transaction {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let parsed = MultiEraTx::decode(bytes)?;
        let id = parsed.hash().to_vec();
        let era = parsed.era().into();
        Ok(Self {
            id,
            body: bytes.to_vec(),
            era,
        })
    }
}
