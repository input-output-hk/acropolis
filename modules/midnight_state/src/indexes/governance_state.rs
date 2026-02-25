use acropolis_common::{BlockNumber, Datum};
use imbl::OrdMap;

#[derive(Clone, Default)]
pub struct GovernanceState {
    /// Technical Committee datum mapped to the block number it was created
    pub technical_committee: OrdMap<BlockNumber, Datum>,
    /// Council datum mapped to the block number it was created
    pub council: OrdMap<BlockNumber, Datum>,
}

impl GovernanceState {
    /// Insert a new technical committee datum
    pub fn insert_technical_committee_datum(
        &mut self,
        block_number: BlockNumber,
        datum: Datum,
    ) -> bool {
        self.technical_committee.insert(block_number, datum).is_none()
    }

    /// Insert a new council datum
    pub fn insert_council_datum(&mut self, block_number: BlockNumber, datum: Datum) -> bool {
        self.council.insert(block_number, datum).is_none()
    }

    /// Get the latest technical committee datum entry at a specific block number
    pub fn get_technical_committee_datum_with_block(
        &self,
        block_number: BlockNumber,
    ) -> Option<(BlockNumber, Datum)> {
        self.technical_committee
            .range(..=block_number)
            .next_back()
            .map(|(number, datum)| (*number, datum.clone()))
    }

    #[allow(dead_code)]
    /// Get the latest technical committee datum at a specific block number
    pub fn get_technical_committee_datum(&self, block_number: BlockNumber) -> Option<Datum> {
        self.get_technical_committee_datum_with_block(block_number).map(|(_, datum)| datum)
    }

    /// Get the latest council datum entry at a specific block number
    pub fn get_council_datum_with_block(
        &self,
        block_number: BlockNumber,
    ) -> Option<(BlockNumber, Datum)> {
        self.council
            .range(..=block_number)
            .next_back()
            .map(|(number, datum)| (*number, datum.clone()))
    }

    #[allow(dead_code)]
    /// Get the latest council datum at a specific block number
    pub fn get_council_datum(&self, block_number: BlockNumber) -> Option<Datum> {
        self.get_council_datum_with_block(block_number).map(|(_, datum)| datum)
    }
}
