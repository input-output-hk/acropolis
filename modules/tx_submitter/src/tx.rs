use acropolis_common::TxHash;
use anyhow::{Result, bail};
use pallas::ledger::traverse::{Era, MultiEraTx};

pub struct Transaction {
    pub id: TxHash,
    pub body: Vec<u8>,
    pub era: u16,
}

impl Transaction {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let parsed = MultiEraTx::decode(bytes)?;
        let id = TxHash::from(*parsed.hash());
        let era = match parsed.era() {
            Era::Conway => 6,
            other => bail!("cannot submit {other} era transactions"),
        };
        Ok(Self {
            id,
            body: bytes.to_vec(),
            era,
        })
    }
}
