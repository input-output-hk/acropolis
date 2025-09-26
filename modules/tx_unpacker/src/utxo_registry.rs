use acropolis_common::{params::SECURITY_PARAMETER_K, TxIdentifier};
use acropolis_common::{TxOutRef, UTxOIdentifier};
use anyhow::Result;
use std::collections::{HashMap, HashSet, VecDeque};

/// Volatile registry for entries within rollback window
#[derive(Clone)]
pub struct VolatileIndex<K> {
    window: VecDeque<HashSet<K>>,
    start_block: u32,
    capacity: usize,
}

impl<K: Eq + std::hash::Hash> VolatileIndex<K> {
    pub fn new(k: u32) -> Self {
        let capacity = (k + 1) as usize;
        VolatileIndex {
            window: VecDeque::with_capacity(capacity),
            capacity,
            start_block: 0,
        }
    }

    pub fn next_block(&mut self) {
        if self.window.len() == self.capacity {
            self.window.pop_front();
            self.start_block += 1;
        }
        self.window.push_back(HashSet::new());
    }

    pub fn add(&mut self, key: K) {
        if let Some(current) = self.window.back_mut() {
            current.insert(key);
        } else {
            panic!("Called add() before any block was initialized with next_block()");
        }
    }

    pub fn prune_on_or_after(&mut self, block: u32) -> Vec<K> {
        let mut out = Vec::new();
        while let Some(last_block) = self.start_block.checked_add(self.window.len() as u32 - 1) {
            if last_block < block {
                break;
            }
            if let Some(set) = self.window.pop_back() {
                out.extend(set);
            }
        }
        out
    }
}

/// Registry of the live TxOutRef (utxo) set and creations/spends within the rollback window.
/// Provides lookup of compact UTxOIdentifiers by TxOutRef, derived from Pallas input.output_ref().
#[derive(Clone)]
pub struct UTxORegistry {
    live_map: HashMap<TxOutRef, TxIdentifier>,
    created: VolatileIndex<TxOutRef>,
    spent: VolatileIndex<(TxOutRef, TxIdentifier)>,
    last_number: u32,
}

impl Default for UTxORegistry {
    fn default() -> Self {
        Self::new(SECURITY_PARAMETER_K as u32)
    }
}

impl UTxORegistry {
    pub fn new(k: u32) -> Self {
        Self {
            live_map: HashMap::new(),
            created: VolatileIndex::new(k),
            spent: VolatileIndex::new(k),
            last_number: 0,
        }
    }

    pub fn bootstrap_from_genesis_utxos(&mut self, pairs: &Vec<(TxOutRef, TxIdentifier)>) {
        self.created.next_block();
        self.spent.next_block();

        for (utxo_ref, id) in pairs {
            self.live_map.insert(*utxo_ref, *id);
            self.created.add(*utxo_ref);
        }
        self.last_number = 0;
    }

    pub fn next_block(&mut self) {
        self.created.next_block();
        self.spent.next_block();
        self.last_number += 1;
    }

    /// Add a new TxOutRef and return its UTxOIdentifier
    pub fn add(
        &mut self,
        block_number: u32,
        tx_index: u16,
        tx_ref: TxOutRef,
    ) -> Result<UTxOIdentifier> {
        let id = TxIdentifier::new(block_number, tx_index);

        if let Some(existing) = self.live_map.get(&tx_ref) {
            return Err(anyhow::anyhow!(
                "duplicate UTxO insertion for {:?}: old={:?}, new={:?}",
                tx_ref,
                existing,
                id
            ));
        }

        self.live_map.insert(tx_ref, id);
        self.created.add(tx_ref);
        self.last_number = block_number;

        Ok(UTxOIdentifier::new(
            block_number,
            tx_index,
            tx_ref.output_index,
        ))
    }

    /// Consume an existing TxOutRef and return its UTxOIdentifier
    pub fn consume(&mut self, block_number: u32, tx_ref: &TxOutRef) -> Result<TxIdentifier> {
        match self.live_map.remove(tx_ref) {
            Some(id) => {
                self.spent.add((tx_ref.clone(), id));
                self.last_number = block_number;
                Ok(id)
            }
            None => Err(anyhow::anyhow!(
                "attempted to consume non-existent or already spent UTxO: {:?}",
                tx_ref
            )),
        }
    }

    /// Rollback to block N-1
    pub fn rollback_before(&mut self, block: u32) {
        // Remove tx ouputs created at or after rollback block
        for h in self.created.prune_on_or_after(block) {
            self.live_map.remove(&h);
        }

        // Reinsert tx outputs removed at or after rollback block
        for (h, id) in self.spent.prune_on_or_after(block) {
            self.live_map.insert(h, id);
        }

        self.last_number = block;
    }
}

#[cfg(test)]
mod tests {
    use crate::utxo_registry::UTxORegistry;
    use acropolis_common::{params::SECURITY_PARAMETER_K, TxHash, TxIdentifier, TxOutRef};
    use anyhow::Result;

    fn make_hash(byte: u8) -> TxHash {
        [byte; 32]
    }
    impl UTxORegistry {
        /// Lookup unspent tx output
        pub fn lookup_by_hash(&self, tx_ref: TxOutRef) -> Result<TxIdentifier> {
            self.live_map.get(&tx_ref).copied().ok_or_else(|| {
                anyhow::anyhow!(
                    "TxHash not found or already spent: {:?}",
                    hex::encode(tx_ref.tx_hash)
                )
            })
        }
    }

    #[test]
    fn add_and_lookup() {
        let mut registry = UTxORegistry::new(SECURITY_PARAMETER_K as u32);
        let tx_ref = TxOutRef {
            tx_hash: make_hash(1),
            output_index: 0,
        };

        for _ in 0..=10 {
            registry.next_block();
        }

        registry.add(10, 0, tx_ref).unwrap();
        let id = registry.lookup_by_hash(tx_ref).unwrap();

        // TxOutRef lookup returns correct TxIdentifier
        assert_eq!(id.block_number(), 10);
        assert_eq!(id.tx_index(), 0);
    }

    #[test]
    fn spend_removes_entry() {
        let mut registry = UTxORegistry::new(SECURITY_PARAMETER_K as u32);
        let tx_ref = TxOutRef {
            tx_hash: make_hash(2),
            output_index: 0,
        };

        for _ in 0..=10 {
            registry.next_block();
        }

        registry.add(10, 0, tx_ref).unwrap();

        // Entry was added to the registry
        assert!(registry.lookup_by_hash(tx_ref).is_ok());

        registry.next_block();
        registry.consume(11, &tx_ref).unwrap();

        // Entry was removed from the regsitry
        assert!(registry.lookup_by_hash(tx_ref).is_err());
    }

    #[test]
    fn rollback_restores_spent() {
        let mut registry = UTxORegistry::new(SECURITY_PARAMETER_K as u32);
        let tx_ref = TxOutRef {
            tx_hash: make_hash(3),
            output_index: 0,
        };

        for _ in 0..=10 {
            registry.next_block();
        }

        registry.add(10, 0, tx_ref).unwrap();

        // Entry was added to the registry
        assert!(registry.lookup_by_hash(tx_ref).is_ok());

        registry.next_block();
        registry.consume(11, &tx_ref).unwrap();

        // Entry was removed from the registry
        assert!(registry.lookup_by_hash(tx_ref).is_err());

        registry.rollback_before(10);
        let id = registry.lookup_by_hash(tx_ref).unwrap();

        // Entry was restored on rollback
        assert_eq!(id.block_number(), 10);
        assert_eq!(id.tx_index(), 0);
    }

    #[test]
    fn rollback_discards_created() {
        let mut registry = UTxORegistry::new(SECURITY_PARAMETER_K as u32);
        let tx_ref = TxOutRef {
            tx_hash: make_hash(4),
            output_index: 0,
        };

        for _ in 0..=15 {
            registry.next_block();
        }

        registry.add(15, 1, tx_ref).unwrap();

        // Entry was added to the registry
        assert!(registry.lookup_by_hash(tx_ref).is_ok());

        registry.rollback_before(14);

        // Entry was removed on rollback
        assert!(registry.lookup_by_hash(tx_ref).is_err());
    }
}
