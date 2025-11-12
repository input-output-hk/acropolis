//! Acropolis AssetsState: State storage

use crate::asset_registry::{AssetId, AssetRegistry};
use acropolis_common::{
    math::update_value_with_delta,
    queries::assets::{AssetHistory, PolicyAssets},
    Address, AddressDelta, AssetAddressEntry, AssetInfoRecord, AssetMetadataStandard,
    AssetMintRecord, AssetName, Datum, Lovelace, NativeAssetsDelta, PolicyAsset, PolicyId,
    ShelleyAddress, TxIdentifier, TxUTxODeltas,
};
use anyhow::Result;
use imbl::{HashMap, Vector};
use tracing::{error, info};

const CIP67_LABEL_222: [u8; 4] = [0x00, 0x0d, 0xe1, 0x40];
const CIP67_LABEL_333: [u8; 4] = [0x00, 0x14, 0xdf, 0x10];
const CIP67_LABEL_444: [u8; 4] = [0x00, 0x1b, 0x4e, 0x20];
const CIP68_LABEL_100: [u8; 4] = [0x00, 0x06, 0x43, 0xb0];

#[derive(Debug, Default, Clone, Copy)]
pub struct AssetsStorageConfig {
    pub store_assets: bool,
    pub store_info: bool,
    pub store_history: bool,
    pub store_transactions: StoreTransactions,
    pub store_addresses: bool,
    pub index_by_policy: bool,
}

#[derive(Debug, Default, Clone, Copy)]
pub enum StoreTransactions {
    #[default]
    None,
    All,
    Last(u64),
}

impl StoreTransactions {
    pub fn is_enabled(&self) -> bool {
        !matches!(self, StoreTransactions::None)
    }
}

#[derive(Debug, Default, Clone)]
pub struct State {
    pub config: AssetsStorageConfig,

    /// Assets mapped to supply
    pub supply: Option<HashMap<AssetId, Lovelace>>,

    /// Assets mapped to mint/burn history
    pub history: Option<HashMap<AssetId, Vector<AssetMintRecord>>>,

    /// Assets mapped to extended info
    pub info: Option<HashMap<AssetId, AssetInfoRecord>>,

    /// Assets mapped to addresses
    pub addresses: Option<HashMap<AssetId, std::collections::HashMap<ShelleyAddress, u64>>>,

    /// Assets mapped to transactions
    pub transactions: Option<HashMap<AssetId, Vector<TxIdentifier>>>,

    // PolicyId mapped associated AssetIds
    pub policy_index: Option<HashMap<PolicyId, Vector<AssetId>>>,
}

impl State {
    pub fn new(config: AssetsStorageConfig) -> Self {
        let store_assets = config.store_assets;
        let store_history = config.store_history;
        let store_info = config.store_info;
        let store_addresses = config.store_addresses;
        let store_transactions = config.store_transactions;
        let index_by_policy = config.index_by_policy;

        Self {
            config,
            supply: if store_assets {
                Some(HashMap::new())
            } else {
                None
            },
            history: if store_history {
                Some(HashMap::new())
            } else {
                None
            },
            info: if store_info {
                Some(HashMap::new())
            } else {
                None
            },
            addresses: if store_addresses {
                Some(HashMap::new())
            } else {
                None
            },
            transactions: match store_transactions {
                StoreTransactions::None => None,
                _ => Some(HashMap::new()),
            },
            policy_index: if index_by_policy {
                Some(HashMap::new())
            } else {
                None
            },
        }
    }

    pub fn get_assets_list(&self, registry: &AssetRegistry) -> Result<Vec<PolicyAsset>> {
        let supply = self
            .supply
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("asset storage is disabled in config"))?;

        let mut out = Vec::with_capacity(supply.len());
        for (id, amount) in supply {
            if let Some(key) = registry.lookup(*id) {
                out.push(PolicyAsset {
                    policy: *key.policy,
                    name: *key.name.as_ref(),
                    quantity: *amount,
                });
            }
        }

        Ok(out)
    }

    pub fn get_asset_info(
        &self,
        asset_id: &AssetId,
        registry: &AssetRegistry,
    ) -> Result<Option<(u64, AssetInfoRecord)>> {
        if !self.config.store_info || !self.config.store_assets {
            return Err(anyhow::anyhow!("asset info storage disabled in config"));
        }

        let supply = self.supply.as_ref().and_then(|supply_map| supply_map.get(asset_id));
        let mut info = self.info.as_ref().and_then(|info_map| info_map.get(asset_id)).cloned();

        // Overwrite asset metadata if an associated CIP68 reference token is found
        if let Some(ref_info) = self.resolve_cip68_metadata(asset_id, registry) {
            if let Some(info_mut) = info.as_mut() {
                info_mut.onchain_metadata = ref_info.onchain_metadata;
                info_mut.metadata_standard = ref_info.metadata_standard;
            } else {
                info = Some(ref_info);
            }
        }

        Ok(match (supply, info) {
            (Some(supply), Some(info)) => Some((*supply, info)),
            _ => None,
        })
    }

    pub fn get_asset_history(&self, asset_id: &AssetId) -> Result<Option<AssetHistory>> {
        if !self.config.store_history {
            return Err(anyhow::anyhow!("asset history storage disabled in config"));
        }

        Ok(self
            .history
            .as_ref()
            .and_then(|hist_map| hist_map.get(asset_id))
            .map(|v| v.iter().cloned().collect()))
    }

    pub fn get_asset_addresses(
        &self,
        asset_id: &AssetId,
    ) -> Result<Option<Vec<AssetAddressEntry>>> {
        if !self.config.store_addresses {
            return Err(anyhow::anyhow!(
                "asset addresses storage disabled in config"
            ));
        }

        Ok(
            self.addresses.as_ref().and_then(|addr_map| addr_map.get(asset_id)).map(|inner_map| {
                inner_map
                    .iter()
                    .map(|(addr, qty)| AssetAddressEntry {
                        address: addr.clone(),
                        quantity: *qty,
                    })
                    .collect()
            }),
        )
    }

    pub fn get_asset_transactions(&self, asset_id: &AssetId) -> Result<Option<Vec<TxIdentifier>>> {
        if !self.config.store_transactions.is_enabled() {
            return Err(anyhow::anyhow!(
                "asset transactions storage disabled in config"
            ));
        }

        Ok(self
            .transactions
            .as_ref()
            .and_then(|tx_map| tx_map.get(asset_id))
            .map(|v| v.iter().cloned().collect()))
    }

    pub fn get_policy_assets(
        &self,
        policy_id: &PolicyId,
        registry: &AssetRegistry,
    ) -> Result<Option<PolicyAssets>> {
        if !self.config.store_assets {
            return Err(anyhow::anyhow!("asset storage is disabled in config"));
        }
        if !self.config.index_by_policy {
            return Err(anyhow::anyhow!("policy index disabled in config"));
        }

        let ids = match self.policy_index.as_ref().and_then(|idx| idx.get(policy_id)) {
            Some(ids) => ids,
            None => return Ok(None),
        };

        let supply_map = self.supply.as_ref();

        let result: Vec<PolicyAsset> = ids
            .iter()
            .filter_map(|asset_id| {
                let supply = supply_map.and_then(|s| s.get(asset_id))?;
                let key = registry.lookup(*asset_id)?;
                Some(PolicyAsset {
                    policy: *policy_id,
                    name: *key.name,
                    quantity: *supply,
                })
            })
            .collect();

        Ok(Some(result))
    }

    pub fn tick(&self) -> Result<()> {
        if let Some(supply) = &self.supply {
            self.log_assets(supply.len());
        } else if let Some(history) = &self.history {
            self.log_assets(history.len());
        } else if let Some(info_map) = &self.info {
            self.log_assets(info_map.len());
        } else if let Some(addresses) = &self.addresses {
            self.log_assets(addresses.len());
        } else if let Some(transactions) = &self.transactions {
            self.log_assets(transactions.len());
        } else {
            info!("asset_state storage disabled in config");
        }

        Ok(())
    }

    fn log_assets(&self, asset_count: usize) {
        if let Some(policy_index) = &self.policy_index {
            let policy_count = policy_index.len();
            info!("Tracking {asset_count} assets across {policy_count} policies");
        } else {
            info!("Tracking {asset_count} assets");
        }
    }

    pub fn handle_mint_deltas(
        &self,
        deltas: &[(TxIdentifier, NativeAssetsDelta)],
        registry: &mut AssetRegistry,
    ) -> Result<Self> {
        let mut new_supply = self.supply.clone();
        let mut new_info = self.info.clone();
        let mut new_history = self.history.clone();
        let mut new_index = self.policy_index.clone();
        let mut new_addresses = self.addresses.clone();
        let mut new_transactions = self.transactions.clone();

        for (tx_identifier, tx_deltas) in deltas {
            for (policy_id, asset_deltas) in tx_deltas {
                for delta in asset_deltas {
                    let asset_id = registry.get_or_insert(*policy_id, delta.name);

                    if let Some(supply) = new_supply.as_mut() {
                        let delta_amount = delta.amount;

                        let new_amt = match supply.get(&asset_id) {
                            Some(&current) => {
                                let sum = (current as i128) + (delta_amount as i128);
                                u64::try_from(sum).map_err(|_| {
                                    anyhow::anyhow!("Burn amount is greater than asset supply")
                                })?
                            }
                            None => {
                                if delta_amount < 0 {
                                    return Err(anyhow::anyhow!("First detected tx is a burn"));
                                }
                                delta_amount as u64
                            }
                        };

                        supply.insert(asset_id, new_amt);
                    }

                    if let Some(info_map) = new_info.as_mut() {
                        info_map
                            .entry(asset_id)
                            .and_modify(|rec| rec.mint_or_burn_count += 1)
                            .or_insert(AssetInfoRecord {
                                initial_mint_tx: *tx_identifier,
                                mint_or_burn_count: 1,
                                onchain_metadata: None,
                                metadata_standard: None,
                            });
                    }

                    if let Some(hist_map) = new_history.as_mut() {
                        hist_map.entry(asset_id).or_insert_with(Vector::new).push_back(
                            AssetMintRecord {
                                tx: *tx_identifier,
                                amount: delta.amount.unsigned_abs(),
                                burn: delta.amount < 0,
                            },
                        );
                    }

                    if let Some(index) = new_index.as_mut() {
                        let ids = index.entry(*policy_id).or_insert_with(Vector::new);
                        if !ids.contains(&asset_id) {
                            ids.push_back(asset_id);
                        }
                    }
                    if let Some(addr_map) = new_addresses.as_mut() {
                        addr_map.entry(asset_id).or_insert_with(std::collections::HashMap::new);
                    }
                    if let Some(tx_map) = new_transactions.as_mut() {
                        tx_map.entry(asset_id).or_insert_with(Vector::new);
                    }
                }
            }
        }

        Ok(Self {
            config: self.config,
            supply: new_supply,
            history: new_history,
            info: new_info,
            addresses: new_addresses,
            transactions: new_transactions,
            policy_index: new_index,
        })
    }

    pub fn handle_transactions(
        &self,
        deltas: &[TxUTxODeltas],
        registry: &AssetRegistry,
    ) -> Result<Self> {
        let mut new_txs = self.transactions.clone();

        let Some(txs_map) = new_txs.as_mut() else {
            return Ok(Self {
                transactions: new_txs,
                ..self.clone()
            });
        };

        let store_cfg = self.config.store_transactions;

        for tx in deltas {
            for output in &tx.outputs {
                let tx_identifier = TxIdentifier::from(output.utxo_identifier);
                for (policy_id, assets) in &output.value.assets {
                    for asset in assets {
                        if let Some(asset_id) = registry.lookup_id(policy_id, &asset.name) {
                            let entry = txs_map.entry(asset_id).or_default();

                            let should_push = entry.back() != Some(&tx_identifier);

                            if should_push {
                                entry.push_back(tx_identifier);

                                if let StoreTransactions::Last(max) = store_cfg {
                                    if entry.len() as u64 > max {
                                        entry.pop_front();
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(Self {
            transactions: new_txs,
            ..self.clone()
        })
    }

    pub fn handle_address_deltas(
        &self,
        deltas: &[AddressDelta],
        registry: &AssetRegistry,
    ) -> Result<Self> {
        let mut new_addresses = self.addresses.clone();

        let Some(addr_map) = new_addresses.as_mut() else {
            return Ok(Self {
                addresses: new_addresses,
                ..self.clone()
            });
        };

        for address_delta in deltas {
            if let Address::Shelley(shelley_addr) = &address_delta.address {
                for (policy_id, assets) in &address_delta.sent.assets {
                    for asset in assets {
                        if let Some(asset_id) = registry.lookup_id(policy_id, &asset.name) {
                            if let Some(holders) = addr_map.get_mut(&asset_id) {
                                let current = holders.entry(shelley_addr.clone()).or_insert(0);
                                if let Err(e) = update_value_with_delta(current, asset.amount) {
                                    error!(
                                        "Address balance update error for {:?}: {e}",
                                        address_delta.address
                                    );
                                }

                                if *current == 0 {
                                    holders.remove(shelley_addr);
                                }
                            } else {
                                error!("Sent delta for unknown asset_id: {:?}", asset_id);
                            }
                        }
                    }
                }

                for (policy_id, assets) in &address_delta.received.assets {
                    for asset in assets {
                        if let Some(asset_id) = registry.lookup_id(policy_id, &asset.name) {
                            if let Some(holders) = addr_map.get_mut(&asset_id) {
                                let current = holders.entry(shelley_addr.clone()).or_insert(0);
                                *current = current.saturating_add(asset.amount);
                            } else {
                                error!("Received delta for unknown asset_id: {:?}", asset_id);
                            }
                        }
                    }
                }
            }
        }

        Ok(Self {
            addresses: new_addresses,
            ..self.clone()
        })
    }

    pub fn handle_cip25_metadata(
        &self,
        registry: &mut AssetRegistry,
        metadata_bytes: &[Vec<u8>],
    ) -> Result<Self> {
        let mut new_info = self.info.clone();
        let Some(info_map) = new_info.as_mut() else {
            return Ok(Self {
                info: new_info,
                ..self.clone()
            });
        };

        for bytes in metadata_bytes {
            let Ok(decoded) = serde_cbor::from_slice::<serde_cbor::Value>(bytes) else {
                continue;
            };

            let policy_map = match decoded {
                serde_cbor::Value::Map(m) => m,
                _ => continue,
            };

            // Retrieve CIP25 version from map and default to v1 if missing
            let version_key = serde_cbor::Value::Text("version".to_string());
            let mut standard = AssetMetadataStandard::CIP25v1;
            if let Some(serde_cbor::Value::Text(ver)) = policy_map.get(&version_key) {
                if ver == "2.0" {
                    standard = AssetMetadataStandard::CIP25v2;
                }
            }

            for (policy_key, assets_val) in policy_map {
                let asset_map = match assets_val {
                    serde_cbor::Value::Map(m) => m,
                    _ => continue,
                };

                let policy_id: Option<PolicyId> = match policy_key {
                    serde_cbor::Value::Text(hex_str) => {
                        hex::decode(&hex_str).ok().and_then(|b| b.try_into().ok())
                    }
                    serde_cbor::Value::Bytes(bytes) => bytes.try_into().ok(),
                    _ => None,
                };

                let Some(policy_id) = policy_id else {
                    continue;
                };

                for (asset_key, metadata_val) in asset_map {
                    let asset_bytes: Option<Vec<u8>> = match asset_key {
                        serde_cbor::Value::Text(hex_str) => {
                            hex::decode(&hex_str).ok().or_else(|| Some(hex_str.into_bytes()))
                        }
                        serde_cbor::Value::Bytes(bytes) => Some(bytes),
                        _ => None,
                    };

                    let Some(asset_bytes) = asset_bytes else {
                        continue;
                    };

                    if let Some(asset_name) = AssetName::new(&asset_bytes) {
                        if let Some(asset_id) = registry.lookup_id(&policy_id, &asset_name) {
                            if let Ok(metadata_raw) = serde_cbor::to_vec(&metadata_val) {
                                if let Some(record) = info_map.get_mut(&asset_id) {
                                    record.onchain_metadata = Some(metadata_raw);
                                    record.metadata_standard = Some(standard);
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(Self {
            info: new_info,
            ..self.clone()
        })
    }

    pub fn handle_cip68_metadata(
        &self,
        deltas: &[TxUTxODeltas],
        registry: &AssetRegistry,
    ) -> Result<Self> {
        let mut new_info = self.info.clone();

        for tx in deltas {
            for output in &tx.outputs {
                let Some(Datum::Inline(blob)) = &output.datum else {
                    continue;
                };

                for (policy_id, native_assets) in &output.value.assets {
                    for asset in native_assets {
                        let name = &asset.name;

                        if !name.as_slice().starts_with(&CIP68_LABEL_100) {
                            continue;
                        }

                        // NOTE: CIP68 metadata version is included in the blob and is decoded in REST handler
                        match registry.lookup_id(policy_id, name) {
                            Some(asset_id) => {
                                if let Some(record) =
                                    new_info.as_mut().and_then(|m| m.get_mut(&asset_id))
                                {
                                    record.onchain_metadata = Some(blob.clone());
                                }
                            }
                            None => {
                                error!(
                                    "Got CIP-68 datum for unknown asset: {}.{}",
                                    hex::encode(policy_id),
                                    hex::encode(name.as_slice())
                                );
                            }
                        }
                    }
                }
            }
        }

        Ok(Self {
            info: new_info,
            ..self.clone()
        })
    }

    fn resolve_cip68_metadata(
        &self,
        asset_id: &AssetId,
        registry: &AssetRegistry,
    ) -> Option<AssetInfoRecord> {
        let key = registry.lookup(*asset_id)?;
        let name_bytes = key.name.as_slice();
        if name_bytes.len() < 4 {
            return None;
        }

        let mut label = [0u8; 4];
        label.copy_from_slice(&name_bytes[0..4]);

        match label {
            CIP68_LABEL_100 => self.info.as_ref()?.get(asset_id).cloned().map(|mut rec| {
                // Hide metadata on the reference itself (Per Blockfrost spec)
                rec.onchain_metadata = None;
                rec.metadata_standard = None;
                rec
            }),

            CIP67_LABEL_222 | CIP67_LABEL_333 | CIP67_LABEL_444 => {
                let mut ref_bytes = name_bytes.to_vec();
                ref_bytes[0..4].copy_from_slice(&[0x00, 0x06, 0x43, 0xb0]);
                let ref_name = AssetName::new(&ref_bytes)?;
                let ref_id = registry.lookup_id(&key.policy, &ref_name)?;
                self.info.as_ref()?.get(&ref_id).cloned()
            }

            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::{
        asset_registry::{AssetId, AssetRegistry},
        state::{AssetsStorageConfig, State, StoreTransactions},
    };
    use acropolis_common::{
        Address, AddressDelta, AssetInfoRecord, AssetMetadataStandard, AssetName, Datum,
        NativeAsset, NativeAssetDelta, PolicyId, ShelleyAddress, TxIdentifier, TxOutput,
        TxUTxODeltas, UTxOIdentifier, Value, ValueDelta,
    };

    fn dummy_policy(byte: u8) -> PolicyId {
        [byte; 28]
    }

    fn asset_name_from_str(s: &str) -> AssetName {
        AssetName::new(s.as_bytes()).unwrap()
    }

    fn dummy_tx_identifier(byte: u8) -> TxIdentifier {
        TxIdentifier::new(byte as u32, byte as u16)
    }

    fn full_config() -> AssetsStorageConfig {
        AssetsStorageConfig {
            store_assets: true,
            store_info: true,
            store_history: true,
            store_transactions: StoreTransactions::All,
            store_addresses: true,
            index_by_policy: true,
        }
    }

    fn setup_state_with_asset(
        registry: &mut AssetRegistry,
        policy_id: PolicyId,
        asset_name_bytes: &[u8],
        seed_info: bool,
        seed_addresses: bool,
        seed_transactions: StoreTransactions,
    ) -> (State, AssetId, AssetName) {
        let asset_name = AssetName::new(asset_name_bytes).unwrap();
        let asset_id = registry.get_or_insert(policy_id, asset_name);

        let mut state = State::new(AssetsStorageConfig {
            store_info: true,
            store_assets: true,
            store_transactions: seed_transactions,
            store_addresses: true,
            ..Default::default()
        });

        if seed_info {
            state
                .info
                .get_or_insert_with(Default::default)
                .insert(asset_id, AssetInfoRecord::default());
        }

        if seed_addresses {
            state
                .addresses
                .get_or_insert_with(Default::default)
                .insert(asset_id, std::collections::HashMap::new());
        }

        if seed_transactions.is_enabled() {
            state
                .transactions
                .get_or_insert_with(Default::default)
                .insert(asset_id, imbl::Vector::new());
        }

        (state, asset_id, asset_name)
    }

    fn build_cip25_metadata(
        policy_id: PolicyId,
        asset_name: &AssetName,
        value: &str,
        version: Option<&str>,
    ) -> Vec<u8> {
        let policy_hex = hex::encode(policy_id);
        let asset_hex = hex::encode(asset_name.as_slice());
        let metadata_value = serde_cbor::Value::Text(value.to_string());

        let mut asset_map = BTreeMap::new();
        asset_map.insert(serde_cbor::Value::Text(asset_hex), metadata_value);

        let mut policy_map = BTreeMap::new();
        policy_map.insert(
            serde_cbor::Value::Text(policy_hex),
            serde_cbor::Value::Map(asset_map),
        );

        if let Some(ver) = version {
            policy_map.insert(
                serde_cbor::Value::Text("version".to_string()),
                serde_cbor::Value::Text(ver.to_string()),
            );
        }

        serde_cbor::to_vec(&serde_cbor::Value::Map(policy_map)).unwrap()
    }

    fn make_address_delta(policy_id: PolicyId, name: AssetName, amount: i64) -> AddressDelta {
        AddressDelta {
            address: dummy_address(),
            tx_identifier: TxIdentifier::new(0, 0),
            spent_utxos: Vec::new(),
            created_utxos: Vec::new(),
            sent: ValueDelta {
                lovelace: 0,
                assets: vec![(policy_id, vec![NativeAssetDelta { name, amount }])]
                    .into_iter()
                    .collect(),
            },
            received: Value::default(),
        }
    }

    fn dummy_address() -> acropolis_common::Address {
        acropolis_common::Address::Shelley(
            ShelleyAddress::from_string(
                "addr1q9g0u0aeuyvrn8ptc6yesgj6dtfgw2gadnc9y2p9cs8pneejrkwtdvk97yp2zayhvmm3wu0v672psdg2xn0temkz83ds7qfxdt",
            )
            .unwrap(),
        )
    }

    fn make_output(policy_id: PolicyId, asset_name: AssetName, datum: Option<Vec<u8>>) -> TxOutput {
        TxOutput {
            utxo_identifier: UTxOIdentifier::new(0, 0, 0),
            address: dummy_address(),
            value: Value {
                lovelace: 0,
                assets: vec![(
                    policy_id,
                    vec![NativeAsset {
                        name: asset_name,
                        amount: 1,
                    }],
                )],
            },
            datum: datum.map(Datum::Inline),
            reference_script: None,
        }
    }

    #[test]
    fn mint_creates_new_asset_and_updates_all_fields() {
        let mut registry = AssetRegistry::new();
        let state = State::new(full_config());

        let policy = dummy_policy(1);
        let name = asset_name_from_str("tokenA");
        let tx = dummy_tx_identifier(9);

        let deltas = vec![(
            tx,
            vec![(policy, vec![NativeAssetDelta { name, amount: 100 }])],
        )];

        let new_state = state.handle_mint_deltas(&deltas, &mut registry).unwrap();

        // supply updated
        let asset_id = registry.lookup_id(&policy, &name).unwrap();
        assert_eq!(
            new_state.supply.as_ref().unwrap().get(&asset_id),
            Some(&100)
        );

        // info initialized
        let info = new_state.info.as_ref().unwrap().get(&asset_id).unwrap();
        assert_eq!(info.initial_mint_tx, tx);
        assert_eq!(info.mint_or_burn_count, 1);

        // history contains mint record
        let hist = new_state.get_asset_history(&asset_id).unwrap().unwrap();
        assert_eq!(hist[0].amount, 100);
        assert!(!hist[0].burn);

        // policy index updated
        let pol_assets = new_state.get_policy_assets(&policy, &registry).unwrap().unwrap();
        assert_eq!(pol_assets[0].quantity, 100);
    }

    #[test]
    fn second_mint_increments_supply_and_records_mint() {
        let mut registry = AssetRegistry::new();
        let state = State::new(full_config());

        let policy = dummy_policy(1);
        let name = asset_name_from_str("tokenA");
        let tx1 = dummy_tx_identifier(1);
        let tx2 = dummy_tx_identifier(2);

        let deltas1 = vec![(
            tx1,
            vec![(policy, vec![NativeAssetDelta { name, amount: 50 }])],
        )];
        let deltas2 = vec![(
            tx2,
            vec![(policy, vec![NativeAssetDelta { name, amount: 25 }])],
        )];

        let state = state.handle_mint_deltas(&deltas1, &mut registry).unwrap();
        let state = state.handle_mint_deltas(&deltas2, &mut registry).unwrap();

        let asset_id = registry.lookup_id(&policy, &name).unwrap();

        // supply updated
        assert_eq!(state.supply.as_ref().unwrap().get(&asset_id), Some(&75));

        // mint/burn count incremented
        assert_eq!(
            state.info.as_ref().unwrap().get(&asset_id).unwrap().mint_or_burn_count,
            2
        );

        // history contains both mint records
        let hist = state.get_asset_history(&asset_id).unwrap().unwrap();
        assert_eq!(hist.len(), 2);
    }

    #[test]
    fn burn_reduces_supply_and_records_burn() {
        let mut registry = AssetRegistry::new();
        let state = State::new(full_config());

        let policy = dummy_policy(1);
        let name = asset_name_from_str("tokenA");
        let tx1 = dummy_tx_identifier(1);
        let tx2 = dummy_tx_identifier(2);

        let mint = vec![(
            tx1,
            vec![(policy, vec![NativeAssetDelta { name, amount: 100 }])],
        )];
        let burn = vec![(
            tx2,
            vec![(policy, vec![NativeAssetDelta { name, amount: -40 }])],
        )];

        let state = state.handle_mint_deltas(&mint, &mut registry).unwrap();
        let state = state.handle_mint_deltas(&burn, &mut registry).unwrap();

        let asset_id = registry.lookup_id(&policy, &name).unwrap();

        // supply reduced by burn amount
        assert_eq!(state.supply.as_ref().unwrap().get(&asset_id), Some(&60));

        let hist = state.get_asset_history(&asset_id).unwrap().unwrap();

        // history contains both mint and burn records
        assert_eq!(hist.len(), 2);

        // latest entry in history is the burn record
        assert!(hist[1].burn);

        // correct amount stored for burn record
        assert_eq!(hist[1].amount, 40);
    }

    #[test]
    fn first_tx_as_burn_fails() {
        let mut registry = AssetRegistry::new();
        let state = State::new(full_config());

        let policy = dummy_policy(1);
        let name = asset_name_from_str("tokenA");
        let tx = dummy_tx_identifier(1);

        let deltas = vec![(
            tx,
            vec![(policy, vec![NativeAssetDelta { name, amount: -50 }])],
        )];

        let result = state.handle_mint_deltas(&deltas, &mut registry);
        // Error on first tx being a burn
        assert!(result.is_err());
    }

    #[test]
    fn burn_more_than_supply_fails() {
        let mut registry = AssetRegistry::new();
        let state = State::new(full_config());

        let policy = dummy_policy(1);
        let name = asset_name_from_str("tokenA");
        let tx = dummy_tx_identifier(1);

        let deltas = vec![(
            tx,
            vec![(policy, vec![NativeAssetDelta { name, amount: -10 }])],
        )];

        let result = state.handle_mint_deltas(&deltas, &mut registry);

        // Error on negative supply
        assert!(result.is_err());
    }

    #[test]
    fn getters_return_error_when_disabled() {
        let config = AssetsStorageConfig::default();
        let reg = AssetRegistry::new();
        let state = State::new(config);
        let fake_id = AssetId::new(0);

        // Error when storage is disabled by config
        assert!(state.get_assets_list(&AssetRegistry::new()).is_err());
        assert!(state.get_asset_info(&fake_id, &reg).is_err());
        assert!(state.get_asset_history(&fake_id).is_err());
        assert!(state.get_asset_addresses(&fake_id).is_err());
        assert!(state.get_asset_transactions(&fake_id).is_err());
        assert!(state.get_policy_assets(&dummy_policy(1), &AssetRegistry::new()).is_err());
    }

    // CIP-25 tests
    #[test]
    fn handle_cip25_metadata_updates_correct_asset() {
        let mut registry = AssetRegistry::new();
        let policy_id: PolicyId = [0u8; 28];

        let (state, asset_id, asset_name) = setup_state_with_asset(
            &mut registry,
            policy_id,
            b"TestAsset",
            true,
            false,
            StoreTransactions::None,
        );

        let metadata_cbor = build_cip25_metadata(policy_id, &asset_name, "hello world", None);

        let new_state = state.handle_cip25_metadata(&mut registry, &[metadata_cbor]).unwrap();
        let info = new_state.info.expect("info should be Some");
        let record = info.get(&asset_id).unwrap();

        // Onchain metadata has been set
        assert!(record.onchain_metadata.is_some());
        // Metadata standard defaults to v1 if not present in map
        assert_eq!(
            record.metadata_standard,
            Some(AssetMetadataStandard::CIP25v1)
        );
    }

    #[test]
    fn handle_cip25_metadata_version_field_sets_v2() {
        let mut registry = AssetRegistry::new();
        let policy_id: PolicyId = [1u8; 28];

        let (state, asset_id, asset_name) = setup_state_with_asset(
            &mut registry,
            policy_id,
            b"VersionedAsset",
            true,
            false,
            StoreTransactions::None,
        );

        let metadata_cbor =
            build_cip25_metadata(policy_id, &asset_name, "metadata for v2", Some("2.0"));

        let new_state = state.handle_cip25_metadata(&mut registry, &[metadata_cbor]).unwrap();
        let info = new_state.info.expect("info should be Some");
        let record = info.get(&asset_id).unwrap();

        // Onchain metadata has been set
        assert!(record.onchain_metadata.is_some());
        // Metadata standard set to v2 when present in map
        assert_eq!(
            record.metadata_standard,
            Some(AssetMetadataStandard::CIP25v2)
        );
    }

    #[test]
    fn handle_cip25_metadata_unknown_asset_is_ignored() {
        let mut registry = AssetRegistry::new();
        let policy_id: PolicyId = [2u8; 28];
        let (state, asset_id, _) = setup_state_with_asset(
            &mut registry,
            policy_id,
            b"KnownAsset",
            true,
            false,
            StoreTransactions::None,
        );

        let other_asset_name = AssetName::new(b"UnknownAsset").unwrap();
        let metadata_cbor =
            build_cip25_metadata(policy_id, &other_asset_name, "ignored metadata", None);

        let new_state = state.handle_cip25_metadata(&mut registry, &[metadata_cbor]).unwrap();
        let info = new_state.info.expect("info should be Some");
        let record = info.get(&asset_id).unwrap();

        // Metadata for known asset unchanged by unknown asset
        assert!(
            record.onchain_metadata.is_none(),
            "unknown asset should not update records"
        );
    }

    #[test]
    fn handle_cip25_metadata_invalid_cbor_is_skipped() {
        let mut registry = AssetRegistry::new();
        let policy_id: PolicyId = [3u8; 28];
        let (state, asset_id, _) = setup_state_with_asset(
            &mut registry,
            policy_id,
            b"BadAsset",
            true,
            false,
            StoreTransactions::None,
        );

        let metadata_cbor = vec![0xff, 0x00, 0x13, 0x37];

        let new_state = state.handle_cip25_metadata(&mut registry, &[metadata_cbor]).unwrap();
        let info = new_state.info.expect("info should be Some");
        let record = info.get(&asset_id).unwrap();

        // Metadata not set when CBOR is invalid
        assert!(
            record.onchain_metadata.is_none(),
            "invalid CBOR should be ignored"
        );
        // Metadata standard not set when CBOR is invalid
        assert!(
            record.metadata_standard.is_none(),
            "invalid CBOR should not set a standard"
        );
    }

    // CIP-68 tests
    #[test]
    fn handle_cip68_metadata_updates_onchain_metadata() {
        let mut registry = AssetRegistry::new();
        let policy_id: PolicyId = [9u8; 28];

        let (state, reference_id, reference_name) = setup_state_with_asset(
            &mut registry,
            policy_id,
            &[0x00, 0x06, 0x43, 0xb0, 0x01],
            true,
            false,
            StoreTransactions::None,
        );

        let datum_blob = vec![1, 2, 3, 4];
        let output = make_output(policy_id, reference_name, Some(datum_blob.clone()));

        let tx_deltas = TxUTxODeltas {
            tx_identifier: TxIdentifier::new(0, 0),
            inputs: Vec::new(),
            outputs: vec![output],
        };

        let new_state = state.handle_cip68_metadata(&[tx_deltas], &registry).unwrap();
        let info = new_state.info.expect("info should be Some");
        let record = info.get(&reference_id).expect("record should exist");

        // Onchain metadata set when asset already exists and TxOutput with inline datum is processed
        assert_eq!(record.onchain_metadata, Some(datum_blob));
    }

    #[test]
    fn handle_cip68_metadata_ignores_non_reference_assets() {
        let mut registry = AssetRegistry::new();
        let policy_id: PolicyId = [9u8; 28];

        let (state, normal_id, normal_name) = setup_state_with_asset(
            &mut registry,
            policy_id,
            &[0xAA, 0xBB, 0xCC],
            true,
            false,
            StoreTransactions::None,
        );

        let datum_blob = vec![1, 2, 3, 4];
        let output = make_output(policy_id, normal_name, Some(datum_blob.clone()));

        let tx_deltas = TxUTxODeltas {
            tx_identifier: TxIdentifier::new(0, 0),
            inputs: Vec::new(),
            outputs: vec![output],
        };

        let new_state = state.handle_cip68_metadata(&[tx_deltas], &registry).unwrap();

        let info = new_state.info.expect("info should be Some");
        let record = info.get(&normal_id).expect("non reference asset should exist");

        // Onchain metadata not updated for non reference asset
        assert_eq!(record.onchain_metadata, None);
    }

    #[test]
    fn handle_cip68_metadata_ignores_unknown_reference_assets() {
        let mut registry = AssetRegistry::new();
        let policy_id: PolicyId = [9u8; 28];

        let (state, asset_id, name) = setup_state_with_asset(
            &mut registry,
            policy_id,
            &[0x00, 0x06, 0x43, 0xb0, 0x02],
            false,
            false,
            StoreTransactions::None,
        );

        let datum_blob = vec![1, 2, 3, 4];
        let output = make_output(policy_id, name, Some(datum_blob));

        let tx_deltas = TxUTxODeltas {
            tx_identifier: TxIdentifier::new(0, 0),
            inputs: Vec::new(),
            outputs: vec![output],
        };

        let new_state = state.handle_cip68_metadata(&[tx_deltas], &registry).unwrap();

        let info = new_state.info.expect("info should be Some");

        // Metadata not populated if asset does not exist
        assert!(
            info.get(&asset_id).is_none(),
            "unknown reference assets should be ignored"
        );
    }

    #[test]
    fn handle_cip68_metadata_ignores_inputs_and_outputs_without_datum() {
        let mut registry = AssetRegistry::new();
        let policy_id: PolicyId = [7u8; 28];

        let (state, asset_id, name) = setup_state_with_asset(
            &mut registry,
            policy_id,
            &[0x00, 0x06, 0x43, 0xb0, 0x02],
            true,
            false,
            StoreTransactions::None,
        );

        let input = UTxOIdentifier::new(0, 0, 0);
        let output = make_output(policy_id, name, None);

        let tx_deltas = TxUTxODeltas {
            tx_identifier: TxIdentifier::new(0, 0),
            inputs: vec![input],
            outputs: vec![output],
        };

        let new_state = state.handle_cip68_metadata(&[tx_deltas], &registry).unwrap();

        let info = new_state.info.expect("info should be Some");
        let record = info.get(&asset_id).unwrap();

        // Metadata not populated for inputs or outputs without inline datum
        assert!(
            record.onchain_metadata.is_none(),
            "inputs and outputs without datums should both be ignored"
        );
    }

    #[test]
    fn get_asset_info_reference_nft_strips_metadata() {
        let mut registry = AssetRegistry::new();
        let policy_id: PolicyId = [9u8; 28];

        let (mut state, ref_id, _) = setup_state_with_asset(
            &mut registry,
            policy_id,
            &[0x00, 0x06, 0x43, 0xb0],
            true,
            false,
            StoreTransactions::None,
        );

        let mut info = state.info.take().unwrap();
        let rec = info.get_mut(&ref_id).unwrap();
        rec.onchain_metadata = Some(vec![1, 2, 3]);
        rec.metadata_standard = Some(AssetMetadataStandard::CIP68v1);
        state.info = Some(info);

        state.supply = Some(imbl::HashMap::new());
        state.supply.as_mut().unwrap().insert(ref_id, 42);

        let result = state.get_asset_info(&ref_id, &registry).unwrap().unwrap();
        let (supply, rec) = result;

        // Supply unchanged
        assert_eq!(supply, 42);
        // Metadata removed for reference asset
        assert!(rec.onchain_metadata.is_none());
        // Metadata standard removed for reference asset
        assert!(rec.metadata_standard.is_none());
    }

    #[test]
    fn resolve_cip68_metadata_overwrites_cip25_user_token_metadata() {
        let mut registry = AssetRegistry::new();
        let policy_id: PolicyId = [10u8; 28];

        let user_name = AssetName::new(&[0x00, 0x0d, 0xe1, 0x40, 0xaa]).unwrap();
        let user_id = registry.get_or_insert(policy_id, user_name);

        let mut ref_bytes = user_name.as_slice().to_vec();
        ref_bytes[0..4].copy_from_slice(&[0x00, 0x06, 0x43, 0xb0]);
        let ref_name = AssetName::new(&ref_bytes).unwrap();
        let ref_id = registry.get_or_insert(policy_id, ref_name);

        let mut state = State::new(AssetsStorageConfig {
            store_info: true,
            store_assets: true,
            ..Default::default()
        });
        let mut info_map = imbl::HashMap::new();

        let user_record = AssetInfoRecord {
            onchain_metadata: Some(vec![1, 2, 3]),
            metadata_standard: Some(AssetMetadataStandard::CIP25v1),
            ..Default::default()
        };
        info_map.insert(user_id, user_record);

        let ref_record = AssetInfoRecord {
            onchain_metadata: Some(vec![9, 9, 9]),
            metadata_standard: Some(AssetMetadataStandard::CIP68v2),
            ..Default::default()
        };
        info_map.insert(ref_id, ref_record);

        state.info = Some(info_map);

        state.supply = Some(imbl::HashMap::new());
        state.supply.as_mut().unwrap().insert(user_id, 100);

        let result = state.get_asset_info(&user_id, &registry).unwrap().unwrap();
        let (supply, rec) = result;

        // User asset supply unchanged
        assert_eq!(supply, 100);
        // User asset metadata overwritten with reference token metadata
        assert_eq!(rec.onchain_metadata, Some(vec![9, 9, 9]));
        // User asset metadata standard overwritten with reference token metadata standard
        assert_eq!(rec.metadata_standard, Some(AssetMetadataStandard::CIP68v2));
    }

    #[test]
    fn handle_transactions_duplicate_tx_ignored() {
        let mut registry = AssetRegistry::new();
        let policy_id: PolicyId = [1u8; 28];

        let (state, asset_id, asset_name) = setup_state_with_asset(
            &mut registry,
            policy_id,
            b"TKN",
            false,
            false,
            StoreTransactions::All,
        );

        let output = make_output(policy_id, asset_name, None);

        let tx_identifier = TxIdentifier::new(0, 0);

        let tx1 = TxUTxODeltas {
            tx_identifier,
            inputs: Vec::new(),
            outputs: vec![TxOutput {
                utxo_identifier: UTxOIdentifier::new(0, 0, 0),
                ..output.clone()
            }],
        };
        let tx2 = TxUTxODeltas {
            tx_identifier,
            inputs: Vec::new(),
            outputs: vec![TxOutput {
                utxo_identifier: UTxOIdentifier::new(0, 0, 1),
                ..output
            }],
        };

        let new_state = state.handle_transactions(&[tx1, tx2], &registry).unwrap();
        let txs = new_state.transactions.expect("transactions should exist");
        let entry = txs.get(&asset_id).expect("asset_id should be present");

        // Only one entry is added for a duplicate tx_hash
        assert_eq!(entry.len(), 1, "duplicate tx_hash should be ignored");
    }

    #[test]
    fn handle_transactions_updates_asset_transactions() {
        let mut registry = AssetRegistry::new();
        let policy_id: PolicyId = [2u8; 28];

        let (state, asset_id, asset_name) = setup_state_with_asset(
            &mut registry,
            policy_id,
            b"TKN",
            false,
            false,
            StoreTransactions::All,
        );

        let out1 = make_output(policy_id, asset_name, None);
        let out2 = make_output(policy_id, asset_name, None);

        let tx1 = TxUTxODeltas {
            tx_identifier: TxIdentifier::new(9, 0),
            inputs: Vec::new(),
            outputs: vec![TxOutput {
                utxo_identifier: UTxOIdentifier::new(9, 0, 0),
                ..out1
            }],
        };
        let tx2 = TxUTxODeltas {
            tx_identifier: TxIdentifier::new(10, 0),
            inputs: Vec::new(),
            outputs: vec![TxOutput {
                utxo_identifier: UTxOIdentifier::new(10, 0, 0),
                ..out2
            }],
        };

        let new_state = state.handle_transactions(&[tx1, tx2], &registry).unwrap();
        let txs = new_state.transactions.expect("transactions should exist");
        let entry = txs.get(&asset_id).expect("asset_id should be present");

        // Both transactions were added to the Vec
        assert_eq!(entry.len(), 2);
        // Transactions are in order oldest to newest
        assert_eq!(entry[0], TxIdentifier::new(9, 0));
        assert_eq!(entry[1], TxIdentifier::new(10, 0));
    }

    #[test]
    fn handle_transactions_prunes_on_store_transactions_config() {
        let mut registry = AssetRegistry::new();
        let policy_id: PolicyId = [3u8; 28];

        let (state, asset_id, asset_name) = setup_state_with_asset(
            &mut registry,
            policy_id,
            b"TKN",
            false,
            false,
            StoreTransactions::Last(2),
        );

        let base_output = make_output(policy_id, asset_name, None);
        let tx1 = TxUTxODeltas {
            tx_identifier: TxIdentifier::new(9, 0),
            inputs: Vec::new(),
            outputs: vec![TxOutput {
                utxo_identifier: UTxOIdentifier::new(9, 0, 0),
                ..base_output.clone()
            }],
        };
        let tx2 = TxUTxODeltas {
            tx_identifier: TxIdentifier::new(8, 0),
            inputs: Vec::new(),
            outputs: vec![TxOutput {
                utxo_identifier: UTxOIdentifier::new(8, 0, 0),
                ..base_output.clone()
            }],
        };
        let tx3 = TxUTxODeltas {
            tx_identifier: TxIdentifier::new(7, 0),
            inputs: Vec::new(),
            outputs: vec![TxOutput {
                utxo_identifier: UTxOIdentifier::new(7, 0, 0),
                ..base_output
            }],
        };

        let new_state = state.handle_transactions(&[tx1, tx2, tx3], &registry).unwrap();
        let txs = new_state.transactions.expect("transactions should exist");
        let entry = txs.get(&asset_id).expect("asset_id should be present");

        // Transactions are pruned at the prune config
        assert_eq!(entry.len(), 2);
        // Transactions are in order with newest last
        assert_eq!(entry[0], TxIdentifier::new(8, 0));
        assert_eq!(entry[1], TxIdentifier::new(7, 0));
    }

    #[test]
    fn handle_address_deltas_accumulates_address_balance() {
        let mut registry = AssetRegistry::new();
        let policy_id: PolicyId = [4u8; 28];
        let (state, asset_id, asset_name) = setup_state_with_asset(
            &mut registry,
            policy_id,
            b"TKN",
            false,
            true,
            StoreTransactions::None,
        );

        let delta1 = make_address_delta(policy_id, asset_name, 10);
        let delta2 = make_address_delta(policy_id, asset_name, 15);

        let new_state = state.handle_address_deltas(&[delta1, delta2], &registry).unwrap();
        let addr_map = new_state.addresses.unwrap();
        let holders = addr_map.get(&asset_id).unwrap();

        // Sum of both deltas applied to address balance
        assert_eq!(
            *holders
                .get(match &dummy_address() {
                    Address::Shelley(s) => s,
                    _ => panic!(),
                })
                .unwrap(),
            25
        );
    }

    #[test]
    fn handle_address_deltas_removes_zero_balance_addresses() {
        let mut registry = AssetRegistry::new();
        let policy_id: PolicyId = [5u8; 28];

        let (state, asset_id, asset_name) = setup_state_with_asset(
            &mut registry,
            policy_id,
            b"TKN",
            false,
            true,
            StoreTransactions::None,
        );

        let add_delta = make_address_delta(policy_id, asset_name, 10);
        let state_after_add = state.handle_address_deltas(&[add_delta], &registry).unwrap();
        let addr_map = state_after_add.addresses.as_ref().unwrap();
        let holders = addr_map.get(&asset_id).unwrap();

        // Address added to asset map with correct balance
        assert_eq!(
            *holders
                .get(match &dummy_address() {
                    Address::Shelley(s) => s,
                    _ => panic!(),
                })
                .unwrap(),
            10
        );

        let remove_delta = make_address_delta(policy_id, asset_name, -10);
        let state_after_remove =
            state_after_add.handle_address_deltas(&[remove_delta], &registry).unwrap();
        let addr_map = state_after_remove.addresses.as_ref().unwrap();
        let holders = addr_map.get(&asset_id).unwrap();

        // Address removed from asset map after balance zeroed
        assert!(!holders.contains_key(match &dummy_address() {
            Address::Shelley(s) => s,
            _ => panic!(),
        }));
    }
}
