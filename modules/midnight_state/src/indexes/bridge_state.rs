use std::{error::Error, fmt};

use acropolis_common::{BlockNumber, UTxOIdentifier};
use imbl::{HashMap, OrdMap};

use crate::types::{BridgeAssetUtxo, BridgeCreation, BridgeUtxoMeta};

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
    // Bridge UTxOs indexed by the block in which they were created.
    created_utxos: OrdMap<BlockNumber, Vec<UTxOIdentifier>>,
    // An index mapping UTxO identifiers to their corresponding bridge metadata.
    pub utxo_index: HashMap<UTxOIdentifier, BridgeUtxoMeta>,
}

impl BridgeState {
    pub fn add_created_utxos(
        &mut self,
        block: BlockNumber,
        mut utxos: Vec<BridgeCreation>,
    ) -> usize {
        utxos.sort_by_key(|creation| (creation.tx_index, creation.utxo.output_index));

        let mut identifiers = Vec::with_capacity(utxos.len());

        for creation in utxos {
            identifiers.push(creation.utxo);
            self.utxo_index.insert(creation.utxo, BridgeUtxoMeta { creation });
        }

        let inserted = identifiers.len();
        self.created_utxos.insert(block, identifiers);
        inserted
    }

    pub fn get_bridge_utxos(
        &self,
        checkpoint: BridgeCheckpoint,
        to_block: BlockNumber,
        utxo_capacity: usize,
    ) -> Result<Vec<BridgeAssetUtxo>, BridgeStateError> {
        let checkpoint = self.resolve_checkpoint(checkpoint)?;
        let start_block = checkpoint.start_block();
        let mut result = Vec::with_capacity(utxo_capacity);

        for (block_number, utxos) in self.created_utxos.range(start_block..) {
            if *block_number > to_block || result.len() >= utxo_capacity {
                break;
            }

            for utxo_id in utxos {
                let Some(meta) = self.utxo_index.get(utxo_id) else {
                    continue;
                };

                if checkpoint.includes(meta.ordering_key()) {
                    result.push(BridgeAssetUtxo::from(meta));
                }

                if result.len() >= utxo_capacity {
                    break;
                }
            }
        }

        Ok(result)
    }

    pub fn next_checkpoint(
        utxos: &[BridgeAssetUtxo],
        to_block: BlockNumber,
        utxo_capacity: usize,
    ) -> BridgeCheckpoint {
        if utxos.len() < utxo_capacity {
            return BridgeCheckpoint::Block(to_block);
        }

        let last =
            utxos.last().expect("non-empty bridge utxo list required when capacity is reached");

        BridgeCheckpoint::Utxo(UTxOIdentifier::new(last.tx_hash, last.output_index))
    }

    fn resolve_checkpoint(
        &self,
        checkpoint: BridgeCheckpoint,
    ) -> Result<ResolvedBridgeCheckpoint, BridgeStateError> {
        match checkpoint {
            BridgeCheckpoint::Block(block) => Ok(ResolvedBridgeCheckpoint::Block(block)),
            BridgeCheckpoint::Utxo(utxo) => self
                .utxo_index
                .get(&utxo)
                .map(|meta| ResolvedBridgeCheckpoint::Utxo(meta.ordering_key()))
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

    fn id(tx_byte: u8, output_index: u16) -> UTxOIdentifier {
        UTxOIdentifier::new(TxHash::from([tx_byte; 32]), output_index)
    }

    fn creation(
        block_number: BlockNumber,
        tx_index: u32,
        utxo: UTxOIdentifier,
        tokens_out: u64,
    ) -> BridgeCreation {
        BridgeCreation {
            utxo,
            block_number,
            tx_index,
            tokens_out,
            tokens_in: 0,
            datum: Some(vec![0xAA]),
        }
    }

    #[test]
    fn bridge_utxos_are_ordered_by_block_tx_and_output() {
        let mut state = BridgeState::default();

        state.add_created_utxos(
            10,
            vec![
                creation(10, 2, id(3, 0), 3),
                creation(10, 1, id(2, 1), 2),
                creation(10, 1, id(1, 0), 1),
            ],
        );

        let result = state
            .get_bridge_utxos(BridgeCheckpoint::Block(9), 10, 10)
            .expect("bridge utxos should be returned");

        let ordered: Vec<(TxHash, u16)> =
            result.iter().map(|utxo| (utxo.tx_hash, utxo.output_index)).collect();

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

        state.add_created_utxos(10, vec![creation(10, 0, id(1, 0), 1)]);
        state.add_created_utxos(11, vec![creation(11, 0, id(2, 0), 2)]);

        let result = state
            .get_bridge_utxos(BridgeCheckpoint::Block(10), 11, 10)
            .expect("bridge utxos should be returned");

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].tx_hash, id(2, 0).tx_hash);
    }

    #[test]
    fn utxo_checkpoint_resumes_after_exact_utxo() {
        let mut state = BridgeState::default();
        let first = id(1, 0);
        let second = id(2, 0);
        let third = id(3, 1);

        state.add_created_utxos(
            10,
            vec![
                creation(10, 0, first, 1),
                creation(10, 1, second, 2),
                creation(10, 1, third, 3),
            ],
        );

        let result = state
            .get_bridge_utxos(BridgeCheckpoint::Utxo(second), 10, 10)
            .expect("bridge utxos should be returned");

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].tx_hash, third.tx_hash);
        assert_eq!(result[0].output_index, third.output_index);
    }

    #[test]
    fn unknown_utxo_checkpoint_is_rejected() {
        let state = BridgeState::default();
        let err = state
            .get_bridge_utxos(BridgeCheckpoint::Utxo(id(9, 0)), 10, 10)
            .expect_err("unknown checkpoint should fail");

        assert_eq!(err, BridgeStateError::UnknownCheckpointUtxo(id(9, 0)));
    }

    #[test]
    fn next_checkpoint_is_block_when_under_capacity() {
        let utxos = vec![BridgeAssetUtxo {
            tx_hash: id(1, 0).tx_hash,
            output_index: 0,
            tokens_out: 1,
            tokens_in: 0,
            datum: None,
        }];

        assert_eq!(
            BridgeState::next_checkpoint(&utxos, 99, 2),
            BridgeCheckpoint::Block(99)
        );
    }

    #[test]
    fn next_checkpoint_is_last_utxo_when_capacity_is_reached() {
        let utxos = vec![
            BridgeAssetUtxo {
                tx_hash: id(1, 0).tx_hash,
                output_index: 0,
                tokens_out: 1,
                tokens_in: 0,
                datum: None,
            },
            BridgeAssetUtxo {
                tx_hash: id(2, 1).tx_hash,
                output_index: 1,
                tokens_out: 2,
                tokens_in: 0,
                datum: None,
            },
        ];

        assert_eq!(
            BridgeState::next_checkpoint(&utxos, 99, 2),
            BridgeCheckpoint::Utxo(id(2, 1))
        );
    }
}
