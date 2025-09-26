use acropolis_common::{AssetName, PolicyId};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AssetId(pub usize);

impl AssetId {
    pub fn new(index: usize) -> Self {
        AssetId(index)
    }

    pub fn index(self) -> usize {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AssetKey {
    pub policy: Arc<PolicyId>,
    pub name: Arc<AssetName>,
}

pub struct AssetRegistry {
    key_to_id: HashMap<AssetKey, AssetId>,
    id_to_key: Vec<AssetKey>,
}

impl AssetRegistry {
    pub fn new() -> Self {
        Self {
            key_to_id: HashMap::new(),
            id_to_key: Vec::new(),
        }
    }

    pub fn get_or_insert(&mut self, policy: PolicyId, name: AssetName) -> AssetId {
        let key = AssetKey {
            policy: Arc::new(policy),
            name: Arc::new(name),
        };

        if let Some(&id) = self.key_to_id.get(&key) {
            id
        } else {
            let id = AssetId::new(self.id_to_key.len());
            self.id_to_key.push(key.clone());
            self.key_to_id.insert(key, id);
            id
        }
    }

    pub fn lookup_id(&self, policy: &PolicyId, name: &AssetName) -> Option<AssetId> {
        let key = AssetKey {
            policy: Arc::new(policy.clone()),
            name: Arc::new(name.clone()),
        };
        self.key_to_id.get(&key).copied()
    }

    pub fn lookup(&self, id: AssetId) -> Option<&AssetKey> {
        self.id_to_key.get(id.index())
    }
}

#[cfg(test)]
mod tests {
    use crate::asset_registry::{AssetId, AssetRegistry};
    use acropolis_common::{AssetName, PolicyId};

    fn dummy_policy(byte: u8) -> PolicyId {
        [byte; 28]
    }

    fn asset_name_from_str(s: &str) -> AssetName {
        AssetName::new(s.as_bytes()).unwrap()
    }

    #[test]
    fn only_insert_once() {
        let mut registry = AssetRegistry::new();
        let policy = dummy_policy(1);
        let name = asset_name_from_str("tokenA");

        let id1 = registry.get_or_insert(policy.clone(), name.clone());
        let id2 = registry.get_or_insert(policy.clone(), name.clone());

        assert_eq!(id1, id2);
    }

    #[test]
    fn different_assets_get_different_ids() {
        let mut registry = AssetRegistry::new();
        let policy1 = dummy_policy(1);
        let policy2 = dummy_policy(2);
        let name1 = asset_name_from_str("tokenA");
        let name2 = asset_name_from_str("tokenB");

        let id1 = registry.get_or_insert(policy1.clone(), name1.clone());
        let id2 = registry.get_or_insert(policy2.clone(), name2.clone());

        assert_ne!(id1, id2);
        assert_eq!(id1.index(), 0);
        assert_eq!(id2.index(), 1);
    }

    #[test]
    fn lookup_id_returns_correct_id() {
        let mut registry = AssetRegistry::new();
        let policy = dummy_policy(1);
        let name = asset_name_from_str("tokenA");

        let id1 = registry.get_or_insert(policy.clone(), name.clone());
        let id2 = registry.lookup_id(&policy, &name).unwrap();

        assert_eq!(id1, id2);
    }

    #[test]
    fn lookup_id_returns_none_for_missing_key() {
        let registry = AssetRegistry::new();
        let policy = dummy_policy(9);
        let name = asset_name_from_str("missing");

        assert!(registry.lookup_id(&policy, &name).is_none());
    }

    #[test]
    fn lookup_returns_correct_asset() {
        let mut registry = AssetRegistry::new();
        let policy = dummy_policy(1);
        let name = asset_name_from_str("tokenA");

        let id = registry.get_or_insert(policy.clone(), name.clone());
        let key = registry.lookup(id).unwrap();

        assert_eq!(policy, *key.policy);
        assert_eq!(name, *key.name);
    }

    #[test]
    fn lookup_returns_none_for_invalid_id() {
        let mut registry = AssetRegistry::new();
        let policy = dummy_policy(1);
        let name = asset_name_from_str("tokenA");
        registry.get_or_insert(policy, name);

        let invalid_id = AssetId::new(999);
        assert!(registry.lookup(invalid_id).is_none());
    }
}
