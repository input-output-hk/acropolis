use acropolis_common::{
    messages::{AddressDeltasMessage, StakeAddressDeltasMessage},
    Address, AddressDelta, BlockInfo, ShelleyAddressDelegationPart, ShelleyAddressPointer,
    StakeAddress, StakeAddressDelta, StakeAddressPayload,
};
use anyhow::{anyhow, Result};
use serde_with::serde_as;
use std::{
    cmp::max,
    collections::{HashMap, HashSet},
    fs::File,
    io::BufReader,
    io::Write,
    sync::Arc,
};
use tracing::error;

#[serde_as]
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct PointerCache {
    #[serde_as(as = "Vec<(_, _)>")]
    pub pointer_map: HashMap<ShelleyAddressPointer, Option<StakeAddress>>,
    pub max_slot: u64,
}

impl PointerCache {
    pub fn new() -> Self {
        Self {
            pointer_map: HashMap::new(),
            max_slot: 0,
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
            return Err(anyhow!(
                "Pointer {:?} is too recent, cache reflects slots up to {}",
                ptr,
                self.max_slot
            ));
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
            Err(err) => Err(anyhow!("Error reading json for {}: '{}'", file_path, err)),
        }
    }

    pub fn try_load_predefined(name: &str) -> Result<Arc<Self>> {
        let value = crate::predefined::POINTER_CACHE
            .iter()
            .fold(None, |prev, (id, val)| {
                prev.or_else(|| if *id == name { Some(val) } else { None })
            })
            .ok_or_else(|| anyhow!("Error finding predefined pointer cache for {name}"))?;

        match serde_json::from_str::<PointerCache>(value) {
            Ok(res) => Ok(Arc::new(res.clone())),
            Err(err) => Err(anyhow!(
                "Error reading predefined cache JSON for {name}: '{err}'"
            )),
        }
    }

    pub fn try_save(&self, file_path: &str) -> Result<()> {
        let mut file = File::create(file_path)?;
        file.write_all(serde_json::to_string_pretty(&self)?.as_bytes())?;
        Ok(())
    }

    pub fn try_save_filtered(
        &self,
        file_path: &str,
        used_pointers: &Vec<ShelleyAddressPointer>,
    ) -> Result<()> {
        let mut clean_pointer_cache = PointerCache {
            max_slot: self.max_slot,
            pointer_map: HashMap::new(),
        };

        for ptr in used_pointers.iter() {
            clean_pointer_cache.pointer_map.insert(
                ptr.clone(),
                self.pointer_map.get(ptr).unwrap_or(&None).clone(),
            );
        }

        clean_pointer_cache.try_save(file_path)
    }
}

#[derive(Clone, Debug, serde::Deserialize, PartialEq)]
pub enum CacheMode {
    /// Built-in cache (see builit-in.rs, Address::network is taken as cache name), fails if none.
    #[serde(rename = "predefined")]
    Predefined,
    /// Read cache, fail if it is not found on disk.
    #[serde(rename = "read")]
    Read,
    /// Create and write cache, ignoring anything pre-existing cache on disk.
    #[serde(rename = "write")]
    Write,
    /// Create and write cache only if it is absent, otherwise use existing one.
    #[serde(rename = "write-if-absent")]
    WriteIfAbsent,
}

#[derive(Debug)]
pub struct OccurrenceInfo {
    block: BlockInfo,
    address_delta: AddressDelta,
    stake_address: Option<StakeAddress>,
}

#[derive(Debug)]
enum OccurrenceInfoKind {
    Valid,
    Invalid,
    Mixed,
}

#[derive(Debug)]
pub struct Tracker {
    occurrence: HashMap<ShelleyAddressPointer, Vec<OccurrenceInfo>>,
}

impl Tracker {
    pub fn new() -> Self {
        Self {
            occurrence: HashMap::new(),
        }
    }

    pub fn get_used_pointers(&self) -> Vec<ShelleyAddressPointer> {
        self.occurrence
            .keys()
            .cloned()
            .collect::<Vec<ShelleyAddressPointer>>()
    }

    pub fn track(
        &mut self,
        p: &ShelleyAddressPointer,
        b: &BlockInfo,
        d: &AddressDelta,
        sa: Option<&StakeAddress>,
    ) {
        self.occurrence
            .entry(p.clone())
            .or_insert(vec![])
            .push(OccurrenceInfo {
                block: b.clone(),
                address_delta: d.clone(),
                stake_address: sa.cloned(),
            });
    }

    fn get_kind(v: &Vec<OccurrenceInfo>) -> Option<OccurrenceInfoKind> {
        let mut is_valid = false;
        let mut is_invalid = false;
        for event in v.iter() {
            is_valid |= event.stake_address.is_some();
            is_invalid |= !event.stake_address.is_some();
        }
        match (is_valid, is_invalid) {
            (true, false) => Some(OccurrenceInfoKind::Valid),
            (false, true) => Some(OccurrenceInfoKind::Invalid),
            (true, true) => Some(OccurrenceInfoKind::Mixed),
            _ => None,
        }
    }

    pub fn info(&self) {
        let mut valid_ptrs = 0;
        let mut invalid_ptrs = 0;
        let mut mixed_ptrs = 0;
        for (_k, v) in self.occurrence.iter() {
            if let Some(kind) = Self::get_kind(&v) {
                match kind {
                    OccurrenceInfoKind::Valid => valid_ptrs += 1,
                    OccurrenceInfoKind::Invalid => invalid_ptrs += 1,
                    OccurrenceInfoKind::Mixed => mixed_ptrs += 1,
                }
            }
        }
        tracing::info!(
            "Pointers dereferencing stats: valid {}, invalid {}, mixed {}",
            valid_ptrs,
            invalid_ptrs,
            mixed_ptrs
        )
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
                let src_addr = event
                    .address_delta
                    .address
                    .to_string()
                    .unwrap_or("(???)".to_owned());
                let dst_addr = event
                    .stake_address
                    .as_ref()
                    .map(|a| a.to_string())
                    .unwrap_or(Ok("(none)".to_owned()))
                    .unwrap_or("(???)".to_owned());
                delta += event.address_delta.delta;

                chunk.push(format!(
                    "   blk {}, {}: {} ({:?}) => {} ({:?})",
                    event.block.number,
                    src_addr,
                    event.address_delta.delta,
                    event.address_delta.address,
                    dst_addr,
                    event.stake_address
                ));

                src_addr_set.insert(src_addr);
                dst_addr_set.insert(dst_addr);
            }
            let src_addr = Self::join_hash_set(src_addr_set, ":");
            let dst_addr = Self::join_hash_set(dst_addr_set, ":");
            chunk.insert(
                0,
                format!("{kind} {src_addr} => {dst_addr}, pointer {ptr:?}, total delta {delta}"),
            );
            chunk.push("".to_owned());

            let flattened = chunk.join("\n");
            if is_valid {
                valid.push(flattened);
            } else {
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
pub fn process_message(
    cache: &PointerCache,
    delta: &AddressDeltasMessage,
    block: &BlockInfo,
    mut tracker: Option<&mut Tracker>,
) -> StakeAddressDeltasMessage {
    let mut result = StakeAddressDeltasMessage { deltas: Vec::new() };

    for d in delta.deltas.iter() {
        // Variants to be processed:
        // 1. Shelley Address delegation is a stake
        // 2. Shelley Address delegation is a pointer + target address is a stake
        // 3. Stake Address (that is, Base Address)
        // Normal, but not processed:
        // 1. Shelley Address delegation is a pointer + pointer known, but cannot be resolved
        // 2. Shelley Address delegation is not a pointer and not a stake
        // Errors:
        // 1. Shelley Address delegation is a pointer + pointer not known

        cache
            .ensure_up_to_date(&d.address)
            .unwrap_or_else(|e| error!("{e}"));

        let stake_address = match &d.address {
            // Not good for staking
            Address::None | Address::Byron(_) => continue,

            Address::Shelley(shelley) => {
                match &shelley.delegation {
                    // Base addresses (stake delegated to itself)
                    ShelleyAddressDelegationPart::StakeKeyHash(keyhash) => StakeAddress {
                        network: shelley.network.clone(),
                        payload: StakeAddressPayload::StakeKeyHash(keyhash.clone()),
                    },

                    ShelleyAddressDelegationPart::ScriptHash(scripthash) => StakeAddress {
                        network: shelley.network.clone(),
                        payload: StakeAddressPayload::ScriptHash(scripthash.clone()),
                    },

                    // Shelley addresses (stake delegated to some different address)
                    ShelleyAddressDelegationPart::Pointer(ref ptr) => {
                        match cache.decode_pointer(ptr) {
                            None => {
                                tracing::warn!("Pointer {ptr:?} is not registered in cache");
                                tracker.as_mut().map(|t| t.track(ptr, block, &d, None));
                                continue;
                            }

                            Some(None) => {
                                tracker.as_mut().map(|t| t.track(ptr, block, &d, None));
                                continue;
                            }

                            Some(Some(ref stake_address)) => {
                                tracker
                                    .as_mut()
                                    .map(|t| t.track(ptr, block, &d, Some(stake_address)));
                                stake_address.clone()
                            }
                        }
                    }

                    // Enterprise addresses, does not delegate stake
                    ShelleyAddressDelegationPart::None => continue,
                }
            }

            Address::Stake(stake_address) => stake_address.clone(),
        };

        let stake_delta = StakeAddressDelta {
            address: stake_address,
            delta: d.delta,
        };
        result.deltas.push(stake_delta);
    }

    result
}

#[cfg(test)]
mod test {
    use crate::*;
    use acropolis_common::{
        messages::AddressDeltasMessage, Address, AddressDelta, BlockInfo, BlockStatus,
        ByronAddress, Era, ShelleyAddress, ShelleyAddressDelegationPart, ShelleyAddressPaymentPart,
        ShelleyAddressPointer, StakeAddress, StakeAddressPayload,
    };
    use bech32::{Bech32, Hrp};

    fn parse_addr(s: &str) -> Result<AddressDelta> {
        let a = pallas_addresses::Address::from_bech32(s)?;
        Ok(AddressDelta {
            address: map_address(&a)?,
            delta: 1,
        })
    }

    /// Map Pallas Network to our AddressNetwork
    fn map_network(network: pallas_addresses::Network) -> Result<AddressNetwork> {
        match network {
            pallas_addresses::Network::Mainnet => Ok(AddressNetwork::Main),
            pallas_addresses::Network::Testnet => Ok(AddressNetwork::Test),
            _ => return Err(anyhow!("Unknown network in address")),
        }
    }

    /// Derive our Address from a Pallas address
    // This is essentially a 1:1 mapping but makes the Message definitions independent
    // of Pallas
    fn map_address(address: &pallas_addresses::Address) -> Result<Address> {
        match address {
            pallas_addresses::Address::Byron(byron_address) => Ok(Address::Byron(ByronAddress {
                payload: byron_address.payload.to_vec(),
            })),

            pallas_addresses::Address::Shelley(shelley_address) => {
                Ok(Address::Shelley(ShelleyAddress {
                    network: map_network(shelley_address.network())?,

                    payment: match shelley_address.payment() {
                        pallas_addresses::ShelleyPaymentPart::Key(hash) => {
                            ShelleyAddressPaymentPart::PaymentKeyHash(hash.to_vec())
                        }
                        pallas_addresses::ShelleyPaymentPart::Script(hash) => {
                            ShelleyAddressPaymentPart::ScriptHash(hash.to_vec())
                        }
                    },

                    delegation: match shelley_address.delegation() {
                        pallas_addresses::ShelleyDelegationPart::Null => {
                            ShelleyAddressDelegationPart::None
                        }
                        pallas_addresses::ShelleyDelegationPart::Key(hash) => {
                            ShelleyAddressDelegationPart::StakeKeyHash(hash.to_vec())
                        }
                        pallas_addresses::ShelleyDelegationPart::Script(hash) => {
                            ShelleyAddressDelegationPart::ScriptHash(hash.to_vec())
                        }
                        pallas_addresses::ShelleyDelegationPart::Pointer(pointer) => {
                            ShelleyAddressDelegationPart::Pointer(ShelleyAddressPointer {
                                slot: pointer.slot(),
                                tx_index: pointer.tx_idx(),
                                cert_index: pointer.cert_idx(),
                            })
                        }
                    },
                }))
            }

            pallas_addresses::Address::Stake(stake_address) => Ok(Address::Stake(StakeAddress {
                network: map_network(stake_address.network())?,
                payload: match stake_address.payload() {
                    pallas_addresses::StakePayload::Stake(hash) => {
                        StakeAddressPayload::StakeKeyHash(hash.to_vec())
                    }
                    pallas_addresses::StakePayload::Script(hash) => {
                        StakeAddressPayload::ScriptHash(hash.to_vec())
                    }
                },
            })),
        }
    }

    fn key_to_keyhash(prefix: &str, key: &str) -> String {
        let (_hrp, key_vec) = bech32::decode(key).unwrap();
        let hash_vec = pallas_crypto::hash::Hasher::<224>::hash(&key_vec);
        let prefix_hrp: Hrp = Hrp::parse(prefix).unwrap();
        bech32::encode::<Bech32>(prefix_hrp, &hash_vec.to_vec()).unwrap()
    }

    // The test is based on CIP-19 standard examples.
    #[tokio::test]
    async fn test_process_message_cip19() -> Result<()> {
        let mut cache = PointerCache::default();

        let stake_addr = "stake1uyehkck0lajq8gr28t9uxnuvgcqrc6070x3k9r8048z8y5gh6ffgw";
        let stake_key = "stake_vk1px4j0r2fk7ux5p23shz8f3y5y2qam7s954rgf3lg5merqcj6aetsft99wu";
        let stake_key_hash = key_to_keyhash("stake_vkh", stake_key);
        let script_addr = "stake178phkx6acpnf78fuvxn0mkew3l0fd058hzquvz7w36x4gtcccycj5";
        let script_hash = "script1cda3khwqv60360rp5m7akt50m6ttapacs8rqhn5w342z7r35m37";

        // Custom address, not related to cip19 examples
        let pointed_addr = "stake1u8jxcva0489xpnlt8d699fq4cfchwgpqk06h4jgvf94xzfcfcnezg";

        let pointed = match parse_addr(pointed_addr)?.address {
            Address::Stake(stake) => stake.clone(),
            _ => panic!("Not a stake address"),
        };

        cache.set_pointer(
            ShelleyAddressPointer {
                slot: 2498243,
                tx_index: 27,
                cert_index: 3,
            },
            pointed,
            2498243,
        );

        let delta = AddressDeltasMessage {
            deltas: vec![
                parse_addr("addr1qx2fxv2umyhttkxyxp8x0dlpdt3k6cwng5pxj3jhsydzer3n0d3vllmyqwsx5wktcd8cc3sq835lu7drv2xwl2wywfgse35a3x")?,
                parse_addr("addr1z8phkx6acpnf78fuvxn0mkew3l0fd058hzquvz7w36x4gten0d3vllmyqwsx5wktcd8cc3sq835lu7drv2xwl2wywfgs9yc0hh")?,
                parse_addr("addr1yx2fxv2umyhttkxyxp8x0dlpdt3k6cwng5pxj3jhsydzerkr0vd4msrxnuwnccdxlhdjar77j6lg0wypcc9uar5d2shs2z78ve")?,
                parse_addr("addr1x8phkx6acpnf78fuvxn0mkew3l0fd058hzquvz7w36x4gt7r0vd4msrxnuwnccdxlhdjar77j6lg0wypcc9uar5d2shskhj42g")?,
                // type 4
                parse_addr("addr1gx2fxv2umyhttkxyxp8x0dlpdt3k6cwng5pxj3jhsydzer5pnz75xxcrzqf96k")?,
                // types 6 and 7, should be ignored as enterprise (no-stake) addresses;
                // placed between pointers to delimit positions of the ignored deltas.
                parse_addr("addr1vx2fxv2umyhttkxyxp8x0dlpdt3k6cwng5pxj3jhsydzers66hrl8")?,
                parse_addr("addr1w8phkx6acpnf78fuvxn0mkew3l0fd058hzquvz7w36x4gtcyjy7wx")?,
                // type 5
                parse_addr("addr128phkx6acpnf78fuvxn0mkew3l0fd058hzquvz7w36x4gtupnz75xxcrtw79hu")?,
                parse_addr(stake_addr)?,
                parse_addr(script_addr)?,
            ]
        };

        let block = BlockInfo {
            status: BlockStatus::Immutable,
            slot: 2498243,
            number: 1,
            hash: vec![],
            epoch: 1,
            new_epoch: true,
            era: Era::Conway,
        };

        let stake_delta = process_message(&cache, &delta, &block, None);

        assert_eq!(
            stake_delta
                .deltas
                .get(0)
                .unwrap()
                .address
                .to_string()
                .unwrap(),
            stake_addr
        );
        assert_eq!(
            stake_delta
                .deltas
                .get(1)
                .unwrap()
                .address
                .to_string()
                .unwrap(),
            stake_addr
        );
        assert_eq!(
            stake_delta
                .deltas
                .get(2)
                .unwrap()
                .address
                .to_string()
                .unwrap(),
            script_addr
        );
        assert_eq!(
            stake_delta
                .deltas
                .get(3)
                .unwrap()
                .address
                .to_string()
                .unwrap(),
            script_addr
        );
        assert_eq!(
            stake_delta
                .deltas
                .get(4)
                .unwrap()
                .address
                .to_string()
                .unwrap(),
            pointed_addr
        );
        assert_eq!(
            stake_delta
                .deltas
                .get(5)
                .unwrap()
                .address
                .to_string()
                .unwrap(),
            pointed_addr
        );
        assert_eq!(
            stake_delta
                .deltas
                .get(6)
                .unwrap()
                .address
                .to_string()
                .unwrap(),
            stake_addr
        );
        assert_eq!(
            stake_delta
                .deltas
                .get(7)
                .unwrap()
                .address
                .to_string()
                .unwrap(),
            script_addr
        );

        // additional check: payload conversion correctness
        assert_eq!(
            stake_delta
                .deltas
                .get(0)
                .unwrap()
                .address
                .payload
                .to_string()
                .unwrap(),
            stake_key_hash
        );
        assert_eq!(
            stake_delta
                .deltas
                .get(2)
                .unwrap()
                .address
                .payload
                .to_string()
                .unwrap(),
            script_hash
        );

        assert_eq!(stake_delta.deltas.len(), 8);

        Ok(())
    }
}
