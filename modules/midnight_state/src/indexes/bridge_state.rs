use std::{error::Error, fmt};

use acropolis_common::{BlockNumber, UTxOIdentifier};
use imbl::{HashMap, OrdMap};

use crate::types::IndexedBridgeTransfer;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeCheckpoint {
    Block(BlockNumber),
    Utxo(UTxOIdentifier),
}

type BridgeOrderingKey = (BlockNumber, u32, u16);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeStateError {
    UnknownCheckpointUtxo(UTxOIdentifier),
}

impl fmt::Display for BridgeStateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BridgeStateError::UnknownCheckpointUtxo(utxo) => {
                write!(f, "unknown bridge checkpoint utxo {utxo:?}")
            }
        }
    }
}

impl Error for BridgeStateError {}

#[derive(Clone, Default)]
pub struct BridgeState {
    // Bridge transfers indexed by the block in which they were created.
    created_transfers: OrdMap<BlockNumber, Vec<UTxOIdentifier>>,
    // An index mapping transfer UTxO identifiers to their corresponding transfer metadata.
    transfer_index: HashMap<UTxOIdentifier, IndexedBridgeTransfer>,
    // A lightweight index of all ICS token-bearing outputs used to compute future tokens_in values.
    token_index: HashMap<UTxOIdentifier, u64>,
}

impl BridgeState {
    pub fn add_created_outputs(
        &mut self,
        block: BlockNumber,
        mut transfers: Vec<IndexedBridgeTransfer>,
        token_outputs: Vec<(UTxOIdentifier, u64)>,
    ) -> usize {
        transfers.sort_by_key(|transfer| (transfer.tx_index, transfer.utxo.output_index));

        let mut identifiers = Vec::with_capacity(transfers.len());

        for transfer in transfers {
            identifiers.push(transfer.utxo);
            self.transfer_index.insert(transfer.utxo, transfer);
        }

        for (utxo, token_amount) in token_outputs {
            self.token_index.insert(utxo, token_amount);
        }

        let inserted = identifiers.len();
        if !identifiers.is_empty() {
            self.created_transfers.insert(block, identifiers);
        }
        inserted
    }

    pub fn get_bridge_transfers(
        &self,
        checkpoint: BridgeCheckpoint,
        to_block: BlockNumber,
        transfer_capacity: usize,
    ) -> Result<Vec<IndexedBridgeTransfer>, BridgeStateError> {
        let checkpoint = self.resolve_checkpoint(checkpoint)?;
        let start_block = checkpoint.start_block();
        let mut result = Vec::with_capacity(transfer_capacity);

        for (block_number, transfers) in self.created_transfers.range(start_block..) {
            if *block_number > to_block || result.len() >= transfer_capacity {
                break;
            }

            for utxo_id in transfers {
                let Some(transfer) = self.transfer_index.get(utxo_id) else {
                    continue;
                };

                if checkpoint.includes(transfer.ordering_key()) {
                    result.push(transfer.clone());
                }

                if result.len() >= transfer_capacity {
                    break;
                }
            }
        }

        Ok(result)
    }

    pub fn next_checkpoint(
        transfers: &[IndexedBridgeTransfer],
        to_block: BlockNumber,
        transfer_capacity: usize,
    ) -> BridgeCheckpoint {
        if transfers.len() < transfer_capacity {
            return BridgeCheckpoint::Block(to_block);
        }

        let last = transfers
            .last()
            .expect("non-empty bridge transfer list required when capacity is reached");

        BridgeCheckpoint::Utxo(last.utxo)
    }

    pub fn token_amount_for_utxo(&self, utxo: &UTxOIdentifier) -> Option<u64> {
        self.token_index.get(utxo).copied()
    }

    fn resolve_checkpoint(
        &self,
        checkpoint: BridgeCheckpoint,
    ) -> Result<ResolvedBridgeCheckpoint, BridgeStateError> {
        match checkpoint {
            BridgeCheckpoint::Block(block) => Ok(ResolvedBridgeCheckpoint::Block(block)),
            BridgeCheckpoint::Utxo(utxo) => self
                .transfer_index
                .get(&utxo)
                .map(|transfer| ResolvedBridgeCheckpoint::Utxo(transfer.ordering_key()))
                .ok_or(BridgeStateError::UnknownCheckpointUtxo(utxo)),
        }
    }
}

#[derive(Clone, Copy)]
enum ResolvedBridgeCheckpoint {
    Block(BlockNumber),
    Utxo(BridgeOrderingKey),
}

impl ResolvedBridgeCheckpoint {
    fn start_block(&self) -> BlockNumber {
        match self {
            ResolvedBridgeCheckpoint::Block(block) => *block,
            ResolvedBridgeCheckpoint::Utxo((block, _, _)) => *block,
        }
    }

    fn includes(&self, key: BridgeOrderingKey) -> bool {
        match self {
            ResolvedBridgeCheckpoint::Block(block) => key.0 > *block,
            ResolvedBridgeCheckpoint::Utxo(checkpoint_key) => key > *checkpoint_key,
        }
    }
}

#[cfg(test)]
mod tests {
    use acropolis_common::TxHash;

    use super::*;
    use crate::types::BridgeTransferKind;

    fn id(tx_byte: u8, output_index: u16) -> UTxOIdentifier {
        UTxOIdentifier::new(TxHash::from([tx_byte; 32]), output_index)
    }

    fn transfer(
        block_number: BlockNumber,
        tx_index: u32,
        utxo: UTxOIdentifier,
        token_amount: u64,
    ) -> IndexedBridgeTransfer {
        IndexedBridgeTransfer {
            utxo,
            block_number,
            tx_index,
            kind: BridgeTransferKind::Reserve { token_amount },
        }
    }

    #[test]
    fn bridge_transfers_are_ordered_by_block_tx_and_output() {
        let mut state = BridgeState::default();

        state.add_created_outputs(
            10,
            vec![
                transfer(10, 2, id(3, 0), 3),
                transfer(10, 1, id(2, 1), 2),
                transfer(10, 1, id(1, 0), 1),
            ],
            vec![],
        );

        let result = state
            .get_bridge_transfers(BridgeCheckpoint::Block(9), 10, 10)
            .expect("bridge transfers should be returned");

        let ordered: Vec<(TxHash, u16)> = result
            .iter()
            .map(|transfer| (transfer.utxo.tx_hash, transfer.utxo.output_index))
            .collect();

        assert_eq!(
            ordered,
            vec![
                (id(1, 0).tx_hash, 0),
                (id(2, 1).tx_hash, 1),
                (id(3, 0).tx_hash, 0)
            ]
        );
    }

    #[test]
    fn block_checkpoint_resumes_from_next_block() {
        let mut state = BridgeState::default();

        state.add_created_outputs(10, vec![transfer(10, 0, id(1, 0), 1)], vec![]);
        state.add_created_outputs(11, vec![transfer(11, 0, id(2, 0), 2)], vec![]);

        let result = state
            .get_bridge_transfers(BridgeCheckpoint::Block(10), 11, 10)
            .expect("bridge transfers should be returned");

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].utxo.tx_hash, id(2, 0).tx_hash);
    }

    #[test]
    fn utxo_checkpoint_resumes_after_exact_utxo() {
        let mut state = BridgeState::default();
        let first = id(1, 0);
        let second = id(2, 0);
        let third = id(3, 1);

        state.add_created_outputs(
            10,
            vec![
                transfer(10, 0, first, 1),
                transfer(10, 1, second, 2),
                transfer(10, 1, third, 3),
            ],
            vec![],
        );

        let result = state
            .get_bridge_transfers(BridgeCheckpoint::Utxo(second), 10, 10)
            .expect("bridge transfers should be returned");

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].utxo.tx_hash, third.tx_hash);
        assert_eq!(result[0].utxo.output_index, third.output_index);
    }

    #[test]
    fn unknown_utxo_checkpoint_is_rejected() {
        let state = BridgeState::default();
        let err = state
            .get_bridge_transfers(BridgeCheckpoint::Utxo(id(9, 0)), 10, 10)
            .expect_err("unknown checkpoint should fail");

        assert_eq!(err, BridgeStateError::UnknownCheckpointUtxo(id(9, 0)));
    }

    #[test]
    fn next_checkpoint_is_block_when_under_capacity() {
        let transfers = vec![transfer(10, 0, id(1, 0), 1)];

        assert_eq!(
            BridgeState::next_checkpoint(&transfers, 99, 2),
            BridgeCheckpoint::Block(99)
        );
    }

    #[test]
    fn next_checkpoint_is_last_utxo_when_capacity_is_reached() {
        let transfers = vec![transfer(10, 0, id(1, 0), 1), transfer(10, 1, id(2, 1), 2)];

        assert_eq!(
            BridgeState::next_checkpoint(&transfers, 99, 2),
            BridgeCheckpoint::Utxo(id(2, 1))
        );
    }
}
