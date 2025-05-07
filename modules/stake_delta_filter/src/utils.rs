use std::cmp::max;
use std::sync::Arc;
use std::fs::File;
use std::collections::HashMap;
use std::io::BufReader;
use anyhow::{anyhow, Result};
use acropolis_common::{Address, ShelleyAddressDelegationPart, ShelleyAddressPointer, StakeAddress, StakeAddressDelta};
use acropolis_common::messages::{AddressDeltasMessage, StakeAddressDeltasMessage};
use serde_with::serde_as;
use crate::DeltaPublisher;

#[serde_as]
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct PointerCache {
    #[serde_as(as = "Vec<(_, _)>")]
    pub pointer_map: HashMap<ShelleyAddressPointer, Address>,
    pub max_slot: u64
}

impl PointerCache {
    pub fn new() -> Self {
        Self {
            pointer_map: HashMap::new(),
            max_slot: 0
        }
    }

    pub fn update_max_slot(&mut self, processed_slot: u64) {
        self.max_slot = max(self.max_slot, processed_slot);
    }

    pub fn ensure_up_to_date_ptr(&self, ptr: &ShelleyAddressPointer) -> Result<()> {
        if ptr.slot > self.max_slot {
            return Err(anyhow!("Pointer {:?} is too recent, cache reflects slots up to {}", ptr, self.max_slot));
        }
        Ok(())
    }

    pub fn ensure_up_to_date(&self, addr: &Address) -> Result<()> {
        if let Some(ptr) = addr.get_pointer() {
            self.ensure_up_to_date_ptr(&ptr)?;
        }
        Ok(())
    }

    pub fn decode_pointer(&self, pointer: &ShelleyAddressPointer) -> Result<&Address> {
        match self.pointer_map.get(pointer) {
            Some(address) => Ok(address),
            None => Err(anyhow!("Pointer {:?} missing from cache", pointer)),
        }
    }
/*
    pub fn decode_address(&self, address: &Address) -> Result<Address> {
        self.ensure_up_to_date(address)?;

        if let Address::Shelley(shelley_address) = address {
            if let ShelleyAddressDelegationPart::Pointer(ptr) = &shelley_address.delegation {
                return self.decode_pointer(ptr).cloned();
            }
        }
        Ok(address.clone())
    }
*/
    pub fn decode_stake_address(&self, address: &Address) -> Result<Option<StakeAddress>> {
        match address {
            Address::None => Ok(None),
            Address::Byron(_) => Ok(None),
            Address::Shelley(shelley_address) => {
                self.ensure_up_to_date(address)?;
                if let ShelleyAddressDelegationPart::Pointer(ptr) = &shelley_address.delegation {
                    self.decode_stake_address(self.decode_pointer(ptr)?)
                }
                else {
                    Ok(None)
                }
            },
            Address::Stake(s) => Ok(Some(s.clone()))
        }
    }

    pub fn try_load(file_path: &str) -> Result<Arc<Self>> {
        let file = File::open(file_path)?;
        let reader = BufReader::new(file);
        match serde_json::from_reader::<BufReader<std::fs::File>, PointerCache>(reader) {
            Ok(res) => Ok(Arc::new(res)),
            Err(err) => Err(anyhow!("Error reading json for {}: '{}'", file_path, err))
        }
    }
}

#[derive(Clone, Debug)]
pub enum CacheMode {
    Never, IfAbsent, Always
}

pub async fn process_message(cache: &PointerCache, publisher: &DeltaPublisher, delta: &AddressDeltasMessage) -> Result<()> {
    let mut result = StakeAddressDeltasMessage {
        sequence: delta.sequence,
        block: delta.block.clone(),
        deltas: Vec::new()
    };

    for d in delta.deltas.iter() {
        cache.ensure_up_to_date(&d.address)?;

        match cache.decode_stake_address(&d.address) {
            Ok(Some(stake_address)) => {
                let stake_delta = StakeAddressDelta {
                    address: stake_address,
                    delta: d.delta
                };
                result.deltas.push(stake_delta);
            },
            Ok(None) => (),
            Err(e) => tracing::warn!("Skipping address delta {:?}, error decoding: {e}", d)
        }
    }

    publisher.publish(result).await
}
