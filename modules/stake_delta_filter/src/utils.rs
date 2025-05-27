use anyhow::{anyhow, Result};
use serde_with::serde_as;
use std::{cmp::max, collections::{HashMap, HashSet}, fs::File, io::BufReader, io::Write, sync::Arc};
use acropolis_common::{
    Address, AddressDelta, BlockInfo, ShelleyAddressPointer, StakeAddress, StakeAddressDelta,
    messages::{AddressDeltasMessage, StakeAddressDeltasMessage}
};

#[serde_as]
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct PointerCache {
    #[serde_as(as = "Vec<(_, _)>")]
    pub pointer_map: HashMap<ShelleyAddressPointer, Option<StakeAddress>>,
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

    pub fn set_pointer(&mut self, ptr: ShelleyAddressPointer, addr: StakeAddress, slot: u64) {
        self.update_max_slot(slot);
        self.pointer_map.insert(ptr, Some(addr));
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

    pub fn decode_pointer(&self, pointer: &ShelleyAddressPointer) -> Option<&Option<StakeAddress>> {
        self.pointer_map.get(pointer)
    }

    #[allow(dead_code)]
    pub fn add_empty_pointer(&mut self, ptr: &ShelleyAddressPointer) {
        self.pointer_map.entry(ptr.clone()).or_insert(None);
    }

    pub fn try_load(file_path: &str) -> Result<Arc<Self>> {
        let file = File::open(file_path)?;
        let reader = BufReader::new(file);
        match serde_json::from_reader::<BufReader<std::fs::File>, PointerCache>(reader) {
            Ok(res) => Ok(Arc::new(res)),
            Err(err) => Err(anyhow!("Error reading json for {}: '{}'", file_path, err))
        }
    }

    pub fn try_load_predefined(name: &str) -> Result<Arc<Self>> {
        let value = crate::predefined::POINTER_CACHE.iter()
            .fold(None, |prev, (id,val)| prev.or_else(|| if *id==name { Some(val) } else { None }))
            .ok_or_else(|| anyhow!("Error finding predefined pointer cache for {name}"))?;

        match serde_json::from_str::<PointerCache>(value) {
            Ok(res) => Ok(Arc::new(res.clone())),
            Err(err) => Err(anyhow!("Error reading predefined cache JSON for {name}: '{err}'"))
        }
    }

    pub fn try_save(&self, file_path: &str) -> Result<()> {
        let mut file = File::create(file_path)?;
        file.write_all(serde_json::to_string_pretty(&self)?.as_bytes())?;
        Ok(())
    }

    pub fn try_save_filtered(&self, file_path: &str, used_pointers: &Vec<ShelleyAddressPointer>) -> Result<()> {
        let mut clean_pointer_cache = PointerCache {
            max_slot: self.max_slot,
            pointer_map: HashMap::new()
        };

        for ptr in used_pointers.iter() {
            clean_pointer_cache.pointer_map.insert(ptr.clone(), self.pointer_map.get(ptr).unwrap_or(&None).clone());
        }

        clean_pointer_cache.try_save(file_path)
    }
}

#[derive(Clone, Debug, serde::Deserialize)]
pub enum CacheMode {
    /// Built-in cache (see builit-in.rs, Address::network is taken as cache name), fails if none.
    #[serde(rename="predefined")]
    Predefined,
    /// Read cache, fail if it is not found on disk.
    #[serde(rename="read")]
    Read, 
    /// Create and write cache, ignoring anything pre-existing cache on disk.
    #[serde(rename="write")]
    Write, 
    /// Create and write cache only if it is absent, otherwise use existing one.
    #[serde(rename="write-if-absent")]
    WriteIfAbsent
}

#[derive(Debug)]
pub struct OccurrenceInfo {
    block: BlockInfo,
    address_delta: AddressDelta, 
    stake_address_delta: Option<StakeAddressDelta>
}

#[derive(Debug)]
enum OccurrenceInfoKind {
    Valid,
    Invalid,
    Mixed
}

#[derive(Debug)]
pub struct Tracker {
    occurrence: HashMap<ShelleyAddressPointer, Vec<OccurrenceInfo>>
}

impl Tracker {
    pub fn new() -> Self {
        Self { occurrence: HashMap::new() }
    }

    pub fn get_used_pointers(&self) -> Vec<ShelleyAddressPointer> {
        self.occurrence.keys().cloned().collect::<Vec<ShelleyAddressPointer>>()
    }

    pub fn track(&mut self, p: &ShelleyAddressPointer, b: &BlockInfo, d: &AddressDelta, sd: Option<&StakeAddressDelta>) {
        self.occurrence.entry(p.clone()).or_insert(vec![]).push(OccurrenceInfo {
            block: b.clone(),
            address_delta: d.clone(),
            stake_address_delta: sd.cloned()
        });
    }

    fn get_kind(v: &Vec<OccurrenceInfo>) -> Option<OccurrenceInfoKind> {
        let mut is_valid = false;
        let mut is_invalid = false;
        for event in v.iter() {
            is_valid |= event.stake_address_delta.is_some();
            is_invalid |= !event.stake_address_delta.is_some();
        }
        match (is_valid, is_invalid) {
            (true, false) => Some(OccurrenceInfoKind::Valid),
            (false, true) => Some(OccurrenceInfoKind::Invalid),
            (true, true) => Some(OccurrenceInfoKind::Mixed),
            _ => None
        }
    }

    pub fn info(&self) {
        let mut valid_ptrs = 0;
        let mut invalid_ptrs = 0;
        let mut mixed_ptrs = 0;
        for (_k,v) in self.occurrence.iter() {
            if let Some(kind) = Self::get_kind(&v) {
                match kind {
                    OccurrenceInfoKind::Valid => valid_ptrs += 1,
                    OccurrenceInfoKind::Invalid => invalid_ptrs += 1,
                    OccurrenceInfoKind::Mixed => mixed_ptrs += 1
                }
            }
        }
        tracing::info!("Pointers dereferencing stats: valid {}, invalid {}, mixed {}", valid_ptrs, invalid_ptrs, mixed_ptrs)
    }

    fn join_hash_set(hs: HashSet<String>, mid: &str) -> String {
        let v = Vec::from_iter(hs.into_iter());
        v.join(mid)
    }

    /// Tracker report: writes information about actual pointers used in blockchain,
    /// trying to print all possible details that are known.
    pub fn report(&self) -> String {
        let mut valid = Vec::new();
        let mut invalid = Vec::new();

        for (ptr, stats) in self.occurrence.iter() {
            let mut chunk = Vec::new();

            let (kind, is_valid) = match Self::get_kind(stats) {
                None => {
                    invalid.push(format!("Empty {:?}", ptr));
                    continue;
                }
                Some(OccurrenceInfoKind::Valid) => ("Valid".to_owned(), true),
                Some(k) => (format!("{:?}", k), false),
            };

            let mut delta = 0;
            let mut src_addr_set = HashSet::new();
            let mut dst_addr_set = HashSet::new();
            for event in stats.iter() {
                let src_addr = event.address_delta.address.to_string();
                let dst_addr = event.stake_address_delta.as_ref()
                    .map(|sa| sa.address.to_string())
                    .unwrap_or("(none)".to_owned());
                delta += event.address_delta.delta;

                chunk.push(format!("   blk {}, hash {}, {}: {} ({:?}) => {} ({:?})", 
                    event.block.number, hex::encode(&event.address_delta.tx_hash), src_addr, event.address_delta.delta, 
                    event.address_delta.address, dst_addr, event.stake_address_delta
                ));

                src_addr_set.insert(src_addr);
                dst_addr_set.insert(dst_addr);
            }
            let src_addr = Self::join_hash_set(src_addr_set, ":");
            let dst_addr = Self::join_hash_set(dst_addr_set, ":");
            chunk.insert(0, format!("{kind} {src_addr} => {dst_addr}, pointer {ptr:?}, total delta {delta}"));
            chunk.push("".to_owned());

            let flattened = chunk.join("\n");
            if is_valid {
                valid.push(flattened);
            }
            else {
                invalid.push(flattened);
            }
        }

        valid.append(&mut invalid);
        valid.into_iter().collect::<String>()
    }
}

/// Iterates through all address deltas in `delta`, leaves only stake addresses 
/// (and removes all others). If the address is a pointer, tries to resolve it.
/// If the pointer is incorrect, then filters it out too (incorrect pointers cannot
/// be used for staking). Updates info about pointer occurrences, if tracker provided.
pub async fn process_message(
   cache: &PointerCache, 
   delta: &AddressDeltasMessage, 
   block: &BlockInfo,
   mut tracker: Option<&mut Tracker>
) -> Result<StakeAddressDeltasMessage> {
    let mut result = StakeAddressDeltasMessage {
        deltas: Vec::new()
    };

    for d in delta.deltas.iter() {
        // Variants to be processed:
        // 1. Address is not a pointer         --- address is stake
        // 2. Address is a pointer             --- target address is a stake
        // Normal, but not processed:
        // 1. Address is a pointer             --- pointer known, but cannot be resolved
        // 2. Address is not a pointer         --- address is not a stake
        // Errors:
        // 1. Address is a pointer             --- pointer not known
        // 2. Address is a pointer             --- target address is not a stake

        cache.ensure_up_to_date(&d.address)?;
        match d.address.get_pointer() {
            None => if let Address::Stake(ref stake_address) = d.address {
                let stake_delta = StakeAddressDelta { address: stake_address.clone(), delta: d.delta };
                result.deltas.push (stake_delta);
            },

            Some(ptr) => match cache.decode_pointer(&ptr) {
                None => {
                    tracing::warn!("Pointer {ptr:?} is not registered in cache");
                    tracker.as_mut().map(|t| t.track(&ptr, block, &d, None));
                },

                Some(None) => {
                    tracker.as_mut().map(|t| t.track(&ptr, block, &d, None));
                },

                Some(Some(stake_address)) => {
                    let stake_delta = StakeAddressDelta { address: stake_address.clone(), delta: d.delta };
                    tracker.as_mut().map(|t| t.track(&ptr, block, &d, Some(&stake_delta)));
                    result.deltas.push (stake_delta);
                }
            }
        }
    }

    Ok(result)
}
