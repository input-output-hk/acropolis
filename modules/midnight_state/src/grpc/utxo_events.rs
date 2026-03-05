use std::cmp::Ordering;

use crate::grpc::midnight_state_proto::{utxo_event, UtxoEvent};

impl UtxoEvent {
    fn position(&self) -> (u64, u32) {
        match self.kind.as_ref() {
            Some(utxo_event::Kind::AssetCreate(e)) => (e.block_number, e.tx_index),
            Some(utxo_event::Kind::AssetSpend(e)) => (e.block_number, e.tx_index),
            Some(utxo_event::Kind::Registration(e)) => (e.block_number, e.tx_index),
            Some(utxo_event::Kind::Deregistration(e)) => (e.block_number, e.tx_index),
            None => (0, 0),
        }
    }
}

impl Ord for UtxoEvent {
    fn cmp(&self, other: &Self) -> Ordering {
        self.position().cmp(&other.position())
    }
}

impl PartialOrd for UtxoEvent {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Eq for UtxoEvent {}

pub fn truncate_by_tx_capacity(mut events: Vec<UtxoEvent>, tx_capacity: usize) -> Vec<UtxoEvent> {
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

        if num_txs > tx_capacity {
            break;
        }

        truncated.push(e);
    }

    truncated
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grpc::midnight_state_proto::AssetCreate;

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
                block_timestamp_unix: 0,
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
    fn truncate_respects_transaction_capacity() {
        let events = vec![
            create_event(1, 0),
            create_event(1, 0), // same tx
            create_event(1, 1),
            create_event(1, 1), // same tx
            create_event(1, 2),
        ];

        let truncated = truncate_by_tx_capacity(events, 2);

        let positions: Vec<(u64, u32)> = truncated.iter().map(|e| e.position()).collect();

        assert_eq!(positions, vec![(1, 0), (1, 0), (1, 1), (1, 1)]);
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

        let truncated = truncate_by_tx_capacity(events, 1);

        let positions: Vec<(u64, u32)> = truncated.iter().map(|e| e.position()).collect();

        assert_eq!(positions, vec![(1, 0), (1, 0)]);
    }
}
