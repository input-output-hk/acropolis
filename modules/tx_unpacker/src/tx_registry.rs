use acropolis_common::TxOutRef;
use acropolis_common::{params::SECURITY_PARAMETER_K, TxIdentifier};
use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};
use tracing::info;

/// Minimal volatile index: block_number -> set of keys
pub struct VolatileIndex<K> {
    by_block: HashMap<u32, HashSet<K>>,
}

impl<K> Default for VolatileIndex<K> {
    fn default() -> Self {
        Self {
            by_block: HashMap::new(),
        }
    }
}

impl<K: Eq + std::hash::Hash + Clone> VolatileIndex<K> {
    fn add(&mut self, block: u32, key: K) {
        self.by_block.entry(block).or_default().insert(key);
    }

    fn prune_on_or_after(&mut self, block: u32) -> Vec<K> {
        let doomed: Vec<u32> = self.by_block.keys().cloned().filter(|b| *b >= block).collect();
        let mut out = Vec::new();
        for b in doomed {
            if let Some(keys) = self.by_block.remove(&b) {
                out.extend(keys);
            }
        }
        out
    }

    fn prune_before(&mut self, boundary: u32) -> Vec<K> {
        let doomed: Vec<u32> = self.by_block.keys().cloned().filter(|b| *b < boundary).collect();
        let mut out = Vec::new();
        for b in doomed {
            if let Some(keys) = self.by_block.remove(&b) {
                out.extend(keys);
            }
        }
        out
    }
}

/// Registry of live + recently spent txs, rollback-safe like UTxOState
#[derive(Clone)]
pub struct TxRegistry {
    map: Arc<RwLock<HashMap<TxOutRef, TxIdentifier>>>,
    created: Arc<RwLock<VolatileIndex<TxOutRef>>>,
    spent: Arc<RwLock<VolatileIndex<(TxOutRef, TxIdentifier)>>>,
    last_number: Arc<RwLock<u32>>,
}

impl Default for TxRegistry {
    fn default() -> Self {
        Self {
            map: Arc::new(RwLock::new(HashMap::new())),
            created: Arc::new(RwLock::new(VolatileIndex::default())),
            spent: Arc::new(RwLock::new(VolatileIndex::default())),
            last_number: Arc::new(RwLock::new(0)),
        }
    }
}

impl TxRegistry {
    pub fn bootstrap_from_genesis_utxos(&self, pairs: &Vec<(TxOutRef, TxIdentifier)>) {
        for (utxo_ref, id) in pairs {
            self.map.write().unwrap().insert(*utxo_ref, *id);
            self.created.write().unwrap().add(id.block_number(), *utxo_ref);
        }
        *self.last_number.write().unwrap() = 0;
    }

    /// Add a new tx output (unspent)
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

    /// Spend an output (remove from map, but track in `spent` so rollbacks can restore it)
    pub fn spend(&self, block_number: u32, tx_ref: &TxOutRef) {
        if let Some(id) = self.map.write().unwrap().remove(tx_ref) {
            self.spent.write().unwrap().add(block_number, (tx_ref.clone(), id));
        }
        *self.last_number.write().unwrap() = block_number;
    }

    /// Lookup only returns unspent entries
    pub fn lookup_by_hash(&self, tx_ref: TxOutRef) -> Result<TxIdentifier> {
        self.map.read().unwrap().get(&tx_ref).copied().ok_or_else(|| {
            anyhow::anyhow!(
                "TxHash not found or already spent: {:?}",
                hex::encode(tx_ref.hash)
            )
        })
    }

    /// Rollback to safe_block
    pub fn rollback_to(&self, safe_block: u32) {
        let mut map = self.map.write().unwrap();
        let mut created = self.created.write().unwrap();
        let mut spent = self.spent.write().unwrap();

        // Remove creations >= safe_block
        for h in created.prune_on_or_after(safe_block) {
            map.remove(&h);
        }

        // Undo spends >= safe_block (restore them to live)
        for (h, id) in spent.prune_on_or_after(safe_block) {
            map.insert(h, id);
        }

        *self.last_number.write().unwrap() = safe_block;
    }

    /// Periodic prune: drop history older than k
    pub fn tick(&self) {
        let n = *self.last_number.read().unwrap();
        let boundary = if n >= SECURITY_PARAMETER_K as u32 {
            n - SECURITY_PARAMETER_K as u32
        } else {
            0
        };

        let mut map = self.map.write().unwrap();
        let mut created = self.created.write().unwrap();
        let mut spent = self.spent.write().unwrap();

        // Remove permanently spent txs before boundary
        for (h, _) in spent.prune_before(boundary) {
            map.remove(&h);
        }

        // Clean created index for entries older than boundary
        created.prune_before(boundary);

        // Free up unused memory
        map.shrink_to_fit();

        info!("TxRegistry prune complete at boundary {}", boundary);
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
        let registry = TxRegistry::default();
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
        let registry = TxRegistry::default();
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
        let registry = TxRegistry::default();
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
        let registry = TxRegistry::default();
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
