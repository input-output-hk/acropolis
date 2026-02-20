use acropolis_common::{
    AssetName, BlockInfo, CreatedUTxOExtended, Era, ExtendedAddressDelta, PolicyId,
};

/// Epoch summary emitted by midnight-state logging runtime.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EpochSummary {
    pub epoch: u64,
    pub era: Era,
    pub indexed_night_utxos: usize,
}

trait EpochTotalsObserver {
    fn start_block(&mut self, block: &BlockInfo);
    fn observe_deltas(&mut self, deltas: &[ExtendedAddressDelta]);
    fn finalise_block(&mut self, block: &BlockInfo);
}

#[derive(Clone, Default)]
pub struct EpochTotals {
    cnight_policy_id: PolicyId,
    cnight_asset_name: AssetName,
    indexed_night_utxos: usize,
    last_checkpoint: Option<EpochCheckpoint>,
}

#[derive(Clone)]
struct EpochCheckpoint {
    epoch: u64,
    era: Era,
}

impl EpochCheckpoint {
    fn from_block(block: &BlockInfo) -> Self {
        Self {
            epoch: block.epoch,
            era: block.era,
        }
    }
}

impl EpochTotals {
    pub fn new(cnight_policy_id: PolicyId, cnight_asset_name: AssetName) -> Self {
        Self {
            cnight_policy_id,
            cnight_asset_name,
            ..Self::default()
        }
    }

    pub fn start_block(&mut self, block: &BlockInfo) {
        <Self as EpochTotalsObserver>::start_block(self, block);
    }

    pub fn observe_deltas(&mut self, deltas: &[ExtendedAddressDelta]) {
        <Self as EpochTotalsObserver>::observe_deltas(self, deltas);
    }

    pub fn finalise_block(&mut self, block: &BlockInfo) {
        <Self as EpochTotalsObserver>::finalise_block(self, block);
    }

    pub fn summarise_completed_epoch(&self, boundary_block: &BlockInfo) -> EpochSummary {
        let (epoch, era) = if let Some(checkpoint) = self.last_checkpoint.as_ref() {
            (checkpoint.epoch, checkpoint.era)
        } else {
            (boundary_block.epoch.saturating_sub(1), boundary_block.era)
        };

        EpochSummary {
            epoch,
            era,
            indexed_night_utxos: self.indexed_night_utxos,
        }
    }

    pub fn reset_epoch(&mut self) {
        self.indexed_night_utxos = 0;
        self.last_checkpoint = None;
    }

    fn is_indexed_night_utxo(&self, created: &CreatedUTxOExtended) -> bool {
        created.value.assets.iter().any(|(policy, assets)| {
            *policy == self.cnight_policy_id
                && assets
                    .iter()
                    .any(|asset| asset.name == self.cnight_asset_name && asset.amount > 0)
        })
    }
}

impl EpochTotalsObserver for EpochTotals {
    fn start_block(&mut self, _block: &BlockInfo) {}

    fn observe_deltas(&mut self, deltas: &[ExtendedAddressDelta]) {
        self.indexed_night_utxos += deltas
            .iter()
            .flat_map(|delta| delta.created_utxos.iter())
            .filter(|created| self.is_indexed_night_utxo(created))
            .count();
    }

    fn finalise_block(&mut self, block: &BlockInfo) {
        self.last_checkpoint = Some(EpochCheckpoint::from_block(block));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use acropolis_common::{
        Address, BlockHash, BlockIntent, BlockStatus, Datum, NativeAsset, TxHash, TxIdentifier,
        UTxOIdentifier, Value,
    };

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

    fn mk_value(policy: PolicyId, name: AssetName, amount: u64) -> Value {
        Value::new(0, vec![(policy, vec![NativeAsset { name, amount }])])
    }

    fn mk_created(index: u16, value: Value) -> CreatedUTxOExtended {
        CreatedUTxOExtended {
            utxo: UTxOIdentifier::new(TxHash::from([index as u8; 32]), index),
            value,
            datum: None::<Datum>,
        }
    }

    fn mk_delta(created_utxos: Vec<CreatedUTxOExtended>) -> ExtendedAddressDelta {
        ExtendedAddressDelta {
            address: Address::None,
            tx_identifier: TxIdentifier::new(1, 0),
            spent_utxos: Vec::new(),
            created_utxos,
            sent: Value::default(),
            received: Value::default(),
        }
    }

    #[test]
    fn observe_deltas_counts_only_matching_cnight_creations() {
        let cnight_policy = PolicyId::from([1u8; 28]);
        let other_policy = PolicyId::from([2u8; 28]);
        let cnight_asset = AssetName::new(b"CNIGHT").expect("valid asset name");
        let other_asset = AssetName::new(b"OTHER").expect("valid asset name");

        let mut totals = EpochTotals::new(cnight_policy, cnight_asset);
        let block = mk_block(10, 100, Era::Conway);

        totals.start_block(&block);
        totals.observe_deltas(&[mk_delta(vec![
            mk_created(0, mk_value(cnight_policy, cnight_asset, 1)),
            mk_created(1, mk_value(cnight_policy, cnight_asset, 0)),
            mk_created(2, mk_value(cnight_policy, other_asset, 1)),
            mk_created(3, mk_value(other_policy, cnight_asset, 1)),
        ])]);
        totals.finalise_block(&block);

        let boundary = mk_block(11, 101, Era::Conway);
        let summary = totals.summarise_completed_epoch(&boundary);
        assert_eq!(summary.epoch, 100);
        assert_eq!(summary.era, Era::Conway);
        assert_eq!(summary.indexed_night_utxos, 1);
    }
}
