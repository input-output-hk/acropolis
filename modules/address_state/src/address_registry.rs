use acropolis_common::Address;
use std::collections::HashMap;
use std::sync::Arc; // whatever your Address type is

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AddressId(pub usize);

impl AddressId {
    pub fn new(index: usize) -> Self {
        AddressId(index)
    }

    pub fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AddressKey(pub Arc<Address>);

pub struct AddressRegistry {
    key_to_id: HashMap<AddressKey, AddressId>,
    id_to_key: Vec<AddressKey>,
}

impl AddressRegistry {
    pub fn new() -> Self {
        Self {
            key_to_id: HashMap::new(),
            id_to_key: Vec::new(),
        }
    }

    pub fn get_or_insert(&mut self, address: Address) -> AddressId {
        let key = AddressKey(Arc::new(address));

        if let Some(&id) = self.key_to_id.get(&key) {
            id
        } else {
            let id = AddressId::new(self.id_to_key.len());
            self.id_to_key.push(key.clone());
            self.key_to_id.insert(key, id);
            id
        }
    }

    pub fn lookup_id(&self, address: &Address) -> Option<AddressId> {
        self.key_to_id.get(&AddressKey(Arc::new(address.clone()))).copied()
    }

    pub fn lookup(&self, id: AddressId) -> Option<&AddressKey> {
        self.id_to_key.get(id.index())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use acropolis_common::Address;

    fn dummy_address(index: u8) -> Address {
        match index {
            0 => Address::from_string("addr1qx8ypzkt3m80umslkjnc8qdwcevnclgwkmd46hy5apefmp90qrvwae5c50xvdujen203xg8gmnflrlfqhr29x396ekjqx77clq").unwrap(),
            1 => Address::from_string("addr1q8spsl7vakwfnte0jsztd33rsfw89zvsnu78ejpr6dtnqm4xwfz4f7dmvxc4758jrhr3xp436kmums8py8s2djpks9ss85s7a0").unwrap(),
            _ => Address::from_string("addr1q8l7hny7x96fadvq8cukyqkcfca5xmkrvfrrkt7hp76v3qvssm7fz9ajmtd58ksljgkyvqu6gl23hlcfgv7um5v0rn8qtnzlfk").unwrap(),
        }
    }

    #[test]
    fn only_insert_once() {
        let mut registry = AddressRegistry::new();
        let addr = dummy_address(1);

        let id1 = registry.get_or_insert(addr.clone());
        let id2 = registry.get_or_insert(addr.clone());

        assert_eq!(id1, id2);
    }

    #[test]
    fn different_addresses_get_different_ids() {
        let mut registry = AddressRegistry::new();
        let id1 = registry.get_or_insert(dummy_address(1));
        let id2 = registry.get_or_insert(dummy_address(2));

        assert_ne!(id1, id2);
        assert_eq!(id1.index(), 0);
        assert_eq!(id2.index(), 1);
    }

    #[test]
    fn lookup_id_returns_correct_id() {
        let mut registry = AddressRegistry::new();
        let addr = dummy_address(1);

        let id1 = registry.get_or_insert(addr.clone());
        let id2 = registry.lookup_id(&addr).unwrap();

        assert_eq!(id1, id2);
    }

    #[test]
    fn lookup_id_returns_none_for_missing_key() {
        let registry = AddressRegistry::new();
        let addr = dummy_address(1);

        assert!(registry.lookup_id(&addr).is_none());
    }

    #[test]
    fn lookup_returns_correct_address() {
        let mut registry = AddressRegistry::new();
        let addr = dummy_address(1);

        let id = registry.get_or_insert(addr.clone());
        let key = registry.lookup(id).unwrap();

        assert_eq!(addr, *key.0);
    }
}
