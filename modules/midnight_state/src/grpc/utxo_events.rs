use std::cmp::Ordering;

use crate::grpc::midnight_state_proto::{utxo_event, CardanoPosition, UtxoEvent};

pub struct TruncatedUtxoEvents {
    pub events: Vec<UtxoEvent>,
    pub num_txs: usize,
}

impl UtxoEvent {
    pub(crate) fn position(&self) -> (u64, u32) {
        match self.kind.as_ref() {
            Some(utxo_event::Kind::AssetCreate(e)) => (e.block_number, e.tx_index),
            Some(utxo_event::Kind::AssetSpend(e)) => (e.block_number, e.tx_index),
            Some(utxo_event::Kind::Registration(e)) => (e.block_number, e.tx_index),
            Some(utxo_event::Kind::Deregistration(e)) => (e.block_number, e.tx_index),
            None => (0, 0),
        }
    }

    fn kind_order(&self) -> u8 {
        // Match the downstream cNIGHT equality contract: creations/registrations sort before
        // spends/deregistrations within the same transaction.
        match self.kind.as_ref() {
            Some(utxo_event::Kind::AssetCreate(_)) | Some(utxo_event::Kind::Registration(_)) => 0,
            Some(utxo_event::Kind::AssetSpend(_))
            | Some(utxo_event::Kind::Deregistration(_))
            | None => 1,
        }
    }

    fn utxo_id(&self) -> (Vec<u8>, u32) {
        match self.kind.as_ref() {
            Some(utxo_event::Kind::AssetCreate(e)) => (e.tx_hash.clone(), e.output_index),
            Some(utxo_event::Kind::AssetSpend(e)) => (e.utxo_tx_hash.clone(), e.utxo_index),
            Some(utxo_event::Kind::Registration(e)) => (e.tx_hash.clone(), e.output_index),
            Some(utxo_event::Kind::Deregistration(e)) => (e.utxo_tx_hash.clone(), e.utxo_index),
            None => (Vec::new(), 0),
        }
    }

    fn block_hash(&self) -> Vec<u8> {
        match self.kind.as_ref() {
            Some(utxo_event::Kind::AssetCreate(e)) => e.block_hash.clone(),
            Some(utxo_event::Kind::AssetSpend(e)) => e.block_hash.clone(),
            Some(utxo_event::Kind::Registration(e)) => e.block_hash.clone(),
            Some(utxo_event::Kind::Deregistration(e)) => e.block_hash.clone(),
            None => Vec::new(),
        }
    }

    fn block_timestamp_unix_millis(&self) -> i64 {
        match self.kind.as_ref() {
            Some(utxo_event::Kind::AssetCreate(e)) => e.block_timestamp_unix_millis,
            Some(utxo_event::Kind::AssetSpend(e)) => e.block_timestamp_unix_millis,
            Some(utxo_event::Kind::Registration(e)) => e.block_timestamp_unix_millis,
            Some(utxo_event::Kind::Deregistration(e)) => e.block_timestamp_unix_millis,
            None => 0,
        }
    }

    pub(crate) fn incremented_position(&self) -> CardanoPosition {
        // The endpoint owns the legacy truncation contract, so it also owns the matching
        // next-position calculation.
        let (block_number, tx_index) = self.position();

        CardanoPosition {
            block_hash: self.block_hash(),
            block_number: block_number as u32,
            tx_index: tx_index.saturating_add(1),
            block_timestamp_unix_millis: self.block_timestamp_unix_millis(),
        }
    }
}

impl Ord for UtxoEvent {
    fn cmp(&self, other: &Self) -> Ordering {
        self.position()
            .cmp(&other.position())
            .then_with(|| self.kind_order().cmp(&other.kind_order()))
            .then_with(|| self.utxo_id().cmp(&other.utxo_id()))
    }
}

impl PartialOrd for UtxoEvent {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Eq for UtxoEvent {}

pub fn truncate_by_legacy_tx_capacity(
    mut events: Vec<UtxoEvent>,
    tx_capacity: usize,
) -> TruncatedUtxoEvents {
    events.sort();

    let mut truncated = Vec::with_capacity(events.len());
    let mut num_txs = 0;
    let mut cur_tx: Option<(u64, u32)> = None;

    for e in events {
        let pos = e.position();

        if cur_tx.is_none_or(|tx| tx < pos) {
            num_txs += 1;
            cur_tx = Some(pos);
        }

        if num_txs == tx_capacity {
            break;
        }

        truncated.push(e);
    }

    TruncatedUtxoEvents {
        events: truncated,
        num_txs,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grpc::midnight_state_proto::{utxo_event, AssetCreate, AssetSpend, UtxoEvent};

    fn create_event(block: u32, tx: u32) -> UtxoEvent {
        UtxoEvent {
            kind: Some(utxo_event::Kind::AssetCreate(AssetCreate {
                address: vec![],
                quantity: 0,
                tx_hash: vec![],
                output_index: 0,
                block_number: block as u64,
                block_hash: vec![],
                tx_index: tx,
                block_timestamp_unix_millis: 0,
            })),
        }
    }

    #[test]
    fn sort_orders_events_by_block_and_tx() {
        let mut events = [
            create_event(2, 0),
            create_event(1, 5),
            create_event(1, 3),
            create_event(1, 4),
        ];

        events.sort();

        let positions: Vec<(u64, u32)> = events.iter().map(|e| e.position()).collect();

        assert_eq!(positions, vec![(1, 3), (1, 4), (1, 5), (2, 0)]);
    }

    #[test]
    fn sort_orders_within_transaction_by_create_then_utxo_id() {
        let mut events = [
            UtxoEvent {
                kind: Some(utxo_event::Kind::AssetSpend(AssetSpend {
                    address: vec![],
                    quantity: 0,
                    spending_tx_hash: vec![9; 32],
                    block_number: 1,
                    block_hash: vec![],
                    tx_index: 0,
                    utxo_tx_hash: vec![2; 32],
                    utxo_index: 0,
                    block_timestamp_unix_millis: 0,
                })),
            },
            UtxoEvent {
                kind: Some(utxo_event::Kind::AssetCreate(AssetCreate {
                    address: vec![],
                    quantity: 0,
                    tx_hash: vec![1; 32],
                    output_index: 2,
                    block_number: 1,
                    block_hash: vec![],
                    tx_index: 0,
                    block_timestamp_unix_millis: 0,
                })),
            },
            UtxoEvent {
                kind: Some(utxo_event::Kind::AssetCreate(AssetCreate {
                    address: vec![],
                    quantity: 0,
                    tx_hash: vec![1; 32],
                    output_index: 0,
                    block_number: 1,
                    block_hash: vec![],
                    tx_index: 0,
                    block_timestamp_unix_millis: 0,
                })),
            },
        ];

        events.sort();

        let kinds_and_indexes: Vec<(&'static str, u32)> = events
            .iter()
            .map(|event| match event.kind.as_ref() {
                Some(utxo_event::Kind::AssetCreate(e)) => ("create", e.output_index),
                Some(utxo_event::Kind::AssetSpend(e)) => ("spend", e.utxo_index),
                _ => ("other", 0),
            })
            .collect();

        assert_eq!(
            kinds_and_indexes,
            vec![("create", 0), ("create", 2), ("spend", 0)]
        );
    }

    #[test]
    fn truncate_uses_legacy_transaction_capacity_semantics() {
        let events = vec![
            create_event(1, 0),
            create_event(1, 0), // same tx
            create_event(1, 1),
            create_event(1, 1), // same tx
            create_event(1, 2),
        ];

        let truncated = truncate_by_legacy_tx_capacity(events, 2);

        let positions: Vec<(u64, u32)> = truncated.events.iter().map(|e| e.position()).collect();

        assert_eq!(positions, vec![(1, 0), (1, 0)]);
        assert_eq!(truncated.num_txs, 2);
    }

    #[test]
    fn truncate_does_not_split_transactions() {
        let events = vec![
            create_event(1, 0),
            create_event(1, 0),
            create_event(1, 1),
            create_event(1, 1),
            create_event(1, 2),
        ];

        let truncated = truncate_by_legacy_tx_capacity(events, 1);

        let positions: Vec<(u64, u32)> = truncated.events.iter().map(|e| e.position()).collect();

        assert!(positions.is_empty());
        assert_eq!(truncated.num_txs, 1);
    }
}
