//! Index of volatile UTXOs
//! Maps volatile blocks to UTXOs created or spent in that block
use std::collections::VecDeque;
use tracing::error;

use crate::state::UTXOKey;

pub struct VolatileIndex {
    /// First block number represented in the index VecDeque
    first_block: u64,

    /// List of UTXOs for each block number
    blocks: VecDeque<Vec<UTXOKey>>,
}

impl VolatileIndex {
    /// Create a new empty index
    pub fn new() -> Self {
        Self {
            first_block: 0,
            blocks: VecDeque::new(),
        }
    }

    /// Add a new block entry
    pub fn add_block(&mut self, number: u64) {
        // Capture the first volatile block we get
        if self.first_block == 0 {
            self.first_block = number;
        }

        if number == self.first_block + self.blocks.len() as u64 {
            // Add empty UTXO set
            self.blocks.push_back(Vec::new());
        }
        else {
            error!("Block {number} added to volatile index out of order")
        }
    }

    /// Add a UTXO to the current last block
    pub fn add_utxo(&mut self, utxo: &UTXOKey) {
        if let Some(last) = self.blocks.back_mut() {
            last.push(utxo.clone());
        }  
    }

    /// Prune all blocks before the given boundary, calling the provided
    /// lambda on each UTXO
    pub fn prune_before<F>(&mut self, boundary: u64, mut callback: F)
        where F: FnMut(&UTXOKey),
    {
        // Remove blocks before boundary, calling back for all UTXOs in them
        while self.first_block < boundary {
            if let Some(block) = self.blocks.pop_front() {
                for utxo in block {
                    callback(&utxo);
                }
            }
            else { break; }

            self.first_block += 1;
        }
    }

    /// Prune all blocks at or after the given boundary, calling the provided
    /// lambda on each UTXO
    pub fn prune_on_or_after<F>(&mut self, boundary: u64, mut callback: F)
        where F: FnMut(&UTXOKey),
    {
        let mut last_block = self.first_block + self.blocks.len() as u64 - 1;

        // Remove blocks before boundary, calling back for all UTXOs in them
        while last_block >= boundary {
            if let Some(block) = self.blocks.pop_back() {
                for utxo in block {
                    callback(&utxo);
                }
            }
            else { break; }

            last_block -= 1;
        }
    }

    
}


// -- Tests --
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_index_is_empty() {
        let index = VolatileIndex::new();
        assert_eq!(0, index.first_block);
        assert_eq!(0, index.blocks.len());
    }

    #[test]
    fn add_block_sequential_captures_number_and_adds_block() {
        let mut index = VolatileIndex::new();
        index.add_block(42);
        assert_eq!(42, index.first_block);
        assert_eq!(1, index.blocks.len());
        assert!(index.blocks[0].is_empty());

        index.add_block(43);
        assert_eq!(42, index.first_block);
        assert_eq!(2, index.blocks.len());
        assert!(index.blocks[1].is_empty());

    }

    #[test]
    fn add_block_non_sequential_ignores_it() {
        let mut index = VolatileIndex::new();
        index.add_block(42);
        assert_eq!(42, index.first_block);
        assert_eq!(1, index.blocks.len());

        index.add_block(99);
        assert_eq!(42, index.first_block);
        assert_eq!(1, index.blocks.len());
    }

    #[test]
    fn add_utxo_adds_to_last_block() {
        let mut index = VolatileIndex::new();
        index.add_block(1);
        index.add_block(2);
        assert_eq!(1, index.first_block);
        assert_eq!(2, index.blocks.len());

        let utxo = UTXOKey::new(&[42], 42);
        index.add_utxo(&utxo);

        assert!(index.blocks[0].is_empty());
        assert!(!index.blocks[1].is_empty());
        assert_eq!(42, index.blocks[1][0].index);
    }

    #[test]
    fn prune_before_deletes_and_calls_back_with_utxos() {
        let mut index = VolatileIndex::new();
        index.add_block(1);
        index.add_utxo(&UTXOKey::new(&[1], 1));
        index.add_utxo(&UTXOKey::new(&[2], 2));
        index.add_block(2);
        index.add_utxo(&UTXOKey::new(&[3], 3));
        let mut pruned = Vec::<UTXOKey>::new();
        index.prune_before(2, |utxo| { pruned.push(utxo.clone())});
        assert_eq!(2, index.first_block);
        assert_eq!(1, index.blocks.len());
        assert_eq!(2, pruned.len());
        assert_eq!(1, pruned[0].index);
        assert_eq!(2, pruned[1].index);
    }
 
    #[test]
    fn prune_on_or_after_deletes_and_calls_back_with_utxos() {
        let mut index = VolatileIndex::new();
        index.add_block(1);
        index.add_utxo(&UTXOKey::new(&[1], 1));
        index.add_utxo(&UTXOKey::new(&[2], 2));
        index.add_block(2);
        index.add_utxo(&UTXOKey::new(&[3], 3));
        let mut pruned = Vec::<UTXOKey>::new();
        index.prune_on_or_after(1, |utxo| { pruned.push(utxo.clone())});
        assert_eq!(1, index.first_block);
        assert_eq!(0, index.blocks.len());
        assert_eq!(3, pruned.len());

        // Note reverse order of blocks
        assert_eq!(3, pruned[0].index);
        assert_eq!(1, pruned[1].index);
        assert_eq!(2, pruned[2].index);
    }
}