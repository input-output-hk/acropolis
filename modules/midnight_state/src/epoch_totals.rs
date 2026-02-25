use acropolis_common::BlockInfo;

/// Epoch summary emitted by midnight-state logging runtime.
#[derive(Clone, Default, Debug, PartialEq, Eq)]
pub struct StatsSummary {
    pub indexed_night_utxo_creations: usize,
    pub indexed_night_utxo_spends: usize,
    pub indexed_candidate_registrations: usize,
    pub indexed_candidate_deregistrations: usize,
    pub indexed_parameter_datums: usize,
    pub indexed_governance_technical_committee_datums: usize,
    pub indexed_governance_council_datums: usize,
}

impl StatsSummary {
    fn accumulate(&mut self, other: &StatsSummary) {
        self.indexed_night_utxo_creations += other.indexed_night_utxo_creations;
        self.indexed_night_utxo_spends += other.indexed_night_utxo_spends;
        self.indexed_candidate_registrations += other.indexed_candidate_registrations;
        self.indexed_candidate_deregistrations += other.indexed_candidate_deregistrations;
        self.indexed_parameter_datums += other.indexed_parameter_datums;
        self.indexed_governance_technical_committee_datums +=
            other.indexed_governance_technical_committee_datums;
        self.indexed_governance_council_datums += other.indexed_governance_council_datums;
    }
}

#[derive(Clone, Default)]
pub struct EpochTotals {
    // Contains the cumulative summary
    cumulative: StatsSummary,
    // Contains the current epoch summary
    current: StatsSummary,
}

impl EpochTotals {
    pub fn add_indexed_night_utxos(&mut self, creations: usize, spends: usize) {
        self.current.indexed_night_utxo_creations += creations;
        self.current.indexed_night_utxo_spends += spends;
    }

    pub fn add_indexed_candidates(&mut self, registrations: usize, deregistrations: usize) {
        self.current.indexed_candidate_registrations += registrations;
        self.current.indexed_candidate_deregistrations += deregistrations;
    }

    pub fn add_indexed_parameter_datums(&mut self, indexed: usize) {
        self.current.indexed_parameter_datums += indexed;
    }

    pub fn add_indexed_governance_datums(&mut self, technical_committee: usize, council: usize) {
        self.current.indexed_governance_technical_committee_datums += technical_committee;
        self.current.indexed_governance_council_datums += council;
    }

    pub fn summarise_completed_epoch(&mut self, boundary_block: &BlockInfo) {
        let epoch = boundary_block.epoch.saturating_sub(1);

        self.cumulative.accumulate(&self.current);

        tracing::info!(
            "epoch={} | creations=+{}/{} spends=+{}/{} regs=+{}/{} deregs=+{}/{} council=+{}/{} committee=+{}/{} params=+{}/{}",
            epoch,
            self.current.indexed_night_utxo_creations,
            self.cumulative.indexed_night_utxo_creations,
            self.current.indexed_night_utxo_spends,
            self.cumulative.indexed_night_utxo_spends,
            self.current.indexed_candidate_registrations,
            self.cumulative.indexed_candidate_registrations,
            self.current.indexed_candidate_deregistrations,
            self.cumulative.indexed_candidate_deregistrations,
            self.current.indexed_governance_council_datums,
            self.cumulative.indexed_governance_council_datums,
            self.current.indexed_governance_technical_committee_datums,
            self.cumulative.indexed_governance_technical_committee_datums,
            self.current.indexed_parameter_datums,
            self.cumulative.indexed_parameter_datums,
        );

        self.current = StatsSummary::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use acropolis_common::{BlockHash, BlockIntent, BlockStatus, Era};

    fn mk_block(number: u64, epoch: u64, era: Era) -> BlockInfo {
        BlockInfo {
            status: BlockStatus::Immutable,
            intent: BlockIntent::Apply,
            slot: number,
            number,
            hash: BlockHash::default(),
            epoch,
            epoch_slot: number,
            new_epoch: false,
            is_new_era: false,
            tip_slot: None,
            timestamp: 0,
            era,
        }
    }

    #[test]
    fn tracks_indexed_night_utxos_for_epoch() {
        let mut totals = EpochTotals::default();

        totals.add_indexed_night_utxos(2, 0);
        totals.add_indexed_night_utxos(1, 4);
        totals.add_indexed_parameter_datums(5);
        totals.add_indexed_governance_datums(2, 6);

        let boundary = mk_block(11, 101, Era::Conway);
        totals.summarise_completed_epoch(&boundary);
        assert_eq!(totals.cumulative.indexed_night_utxo_creations, 3);
        assert_eq!(totals.cumulative.indexed_night_utxo_spends, 4);
        assert_eq!(
            totals.cumulative.indexed_governance_technical_committee_datums,
            2
        );
        assert_eq!(totals.cumulative.indexed_governance_council_datums, 6);
        assert_eq!(totals.cumulative.indexed_parameter_datums, 5);
    }
}
