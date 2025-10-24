use acropolis_common::TxHash;
use anyhow::Result;
use pallas::ledger::traverse::MultiEraTx;

pub struct Transaction {
    pub id: TxHash,
    pub body: Vec<u8>,
    pub era: u16,
}

impl Transaction {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let parsed = MultiEraTx::decode(bytes)?;
        let id = TxHash::from(*parsed.hash());
        let era = parsed.era().into();
        Ok(Self {
            id,
            body: bytes.to_vec(),
            era,
        })
    }
}
