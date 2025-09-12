use acropolis_common::{AssetName, PolicyId};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AssetId(pub u32);

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
            let id = AssetId(self.id_to_key.len() as u32);
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
        self.id_to_key.get(id.0 as usize)
    }
}
