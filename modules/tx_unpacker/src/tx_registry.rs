use acropolis_common::TxOutRef;
use acropolis_common::{params::SECURITY_PARAMETER_K, TxIdentifier};
use anyhow::Result;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, RwLock};

/// Volatile registry for entries within rollback window
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

    pub fn add(&mut self, block: u32, key: K) {
        let idx = block.wrapping_sub(self.start_block) as usize;

        if idx < self.window.len() {
            self.window[idx].insert(key);
        } else if idx == self.window.len() {
            if self.window.len() == self.capacity {
                self.window.pop_front();
                self.start_block += 1;
            }
            self.window.push_back(HashSet::new());
            self.window.back_mut().unwrap().insert(key);
        } else {
            panic!(
                "unexpected block number: got {}, expected {} or {}",
                block,
                self.start_block,
                self.start_block + self.window.len() as u32
            );
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

/// Registry of live TxHash set and created/spent txs within rollback window
#[derive(Clone)]
pub struct TxRegistry {
    map: Arc<RwLock<HashMap<TxOutRef, TxIdentifier>>>,
    created: Arc<RwLock<VolatileIndex<TxOutRef>>>,
    spent: Arc<RwLock<VolatileIndex<(TxOutRef, TxIdentifier)>>>,
    last_number: Arc<RwLock<u32>>,
}

impl Default for TxRegistry {
    fn default() -> Self {
        Self::new(SECURITY_PARAMETER_K as u32)
    }
}

impl TxRegistry {
    pub fn new(k: u32) -> Self {
        Self {
            map: Arc::new(RwLock::new(HashMap::new())),
            created: Arc::new(RwLock::new(VolatileIndex::new(k))),
            spent: Arc::new(RwLock::new(VolatileIndex::new(k))),
            last_number: Arc::new(RwLock::new(0)),
        }
    }

    pub fn bootstrap_from_genesis_utxos(&self, pairs: &Vec<(TxOutRef, TxIdentifier)>) {
        for (utxo_ref, id) in pairs {
            self.map.write().unwrap().insert(*utxo_ref, *id);
            self.created.write().unwrap().add(id.block_number(), *utxo_ref);
        }
        *self.last_number.write().unwrap() = 0;
    }

    /// Add a new tx output
    pub fn add(&self, block_number: u32, tx_index: u16, tx_ref: TxOutRef) -> Result<()> {
        let id = TxIdentifier::new(block_number, tx_index);
        {
            let mut map = self.map.write().unwrap();

            if let Some(existing) = map.get(&tx_ref) {
                if *existing != id {
                    return Err(anyhow::anyhow!(
                        "conflicting mapping for hash={:?}: old={:?}, new={:?}",
                        tx_ref.hash,
                        existing,
                        id
                    ));
                }
                return Ok(());
            }
            map.insert(tx_ref, id);
        }
        self.created.write().unwrap().add(block_number, tx_ref);
        *self.last_number.write().unwrap() = block_number;
        Ok(())
    }

    /// Spend an existing tx output
    pub fn spend(&self, block_number: u32, tx_ref: &TxOutRef) {
        if let Some(id) = self.map.write().unwrap().remove(tx_ref) {
            self.spent.write().unwrap().add(block_number, (tx_ref.clone(), id));
        }
        *self.last_number.write().unwrap() = block_number;
    }

    /// Lookup unspent tx output
    pub fn lookup_by_hash(&self, tx_ref: TxOutRef) -> Result<TxIdentifier> {
        self.map.read().unwrap().get(&tx_ref).copied().ok_or_else(|| {
            anyhow::anyhow!(
                "TxHash not found or already spent: {:?}",
                hex::encode(tx_ref.hash)
            )
        })
    }

    /// Rollback to specified block
    pub fn rollback_to(&self, block: u32) {
        let mut map = self.map.write().unwrap();
        let mut created = self.created.write().unwrap();
        let mut spent = self.spent.write().unwrap();

        // Remove tx ouputs created at or after rollback block
        for h in created.prune_on_or_after(block) {
            map.remove(&h);
        }

        // Reinsert tx outputs removed at or after rollback block
        for (h, id) in spent.prune_on_or_after(block) {
            map.insert(h, id);
        }

        *self.last_number.write().unwrap() = block;
    }
}

#[cfg(test)]
mod tests {
    use acropolis_common::TxHash;

    use super::*;

    fn make_hash(byte: u8) -> TxHash {
        [byte; 32]
    }

    #[test]
    fn add_and_lookup() {
        let registry = TxRegistry::new(SECURITY_PARAMETER_K as u32);
        let tx_ref = TxOutRef {
            hash: make_hash(1),
            index: 0,
        };

        registry.add(10, 0, tx_ref).unwrap();

        let id = registry.lookup_by_hash(tx_ref).unwrap();
        assert_eq!(id.block_number(), 10);
        assert_eq!(id.tx_index(), 0);
    }

    #[test]
    fn spend_and_fail_lookup() {
        let registry = TxRegistry::new(SECURITY_PARAMETER_K as u32);
        let tx_ref = TxOutRef {
            hash: make_hash(2),
            index: 0,
        };

        registry.add(10, 0, tx_ref).unwrap();
        registry.spend(11, &tx_ref);

        assert!(registry.lookup_by_hash(tx_ref).is_err());
    }

    #[test]
    fn rollback_restores_spent() {
        let registry = TxRegistry::new(SECURITY_PARAMETER_K as u32);
        let tx_ref = TxOutRef {
            hash: make_hash(3),
            index: 0,
        };

        // Add at block 10
        registry.add(10, 0, tx_ref).unwrap();
        assert!(registry.lookup_by_hash(tx_ref).is_ok());

        // Spend at block 11
        registry.spend(11, &tx_ref);

        assert!(registry.lookup_by_hash(tx_ref).is_err());

        // Rollback to block 10 (undoes spend)
        registry.rollback_to(10);

        // Hash should be back in map
        let id = registry.lookup_by_hash(tx_ref).unwrap();
        assert_eq!(id.block_number(), 10);
        assert_eq!(id.tx_index(), 0);
    }

    #[test]
    fn rollback_discards_created() {
        let registry = TxRegistry::new(SECURITY_PARAMETER_K as u32);
        let tx_ref = TxOutRef {
            hash: make_hash(4),
            index: 0,
        };

        // Created at block 15
        registry.add(15, 1, tx_ref).unwrap();
        assert!(registry.lookup_by_hash(tx_ref).is_ok());

        // Roll back to before creation
        registry.rollback_to(14);

        // Entry should be gone
        assert!(registry.lookup_by_hash(tx_ref).is_err());
    }
}
