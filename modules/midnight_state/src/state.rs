use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, NaiveDateTime, Utc};
use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};
use tracing::{debug, info, warn};

use acropolis_common::{
    messages::AddressDeltasMessage, AssetName, BlockInfo, BlockNumber, Datum, Epoch, PolicyId,
    UTxOIdentifier, Value,
};

use crate::types::{
    AssetCreate, AssetSpend, CandidateUTxO, Deregistration, DeregistrationEvent, Registration,
    RegistrationEvent, UTxOMeta,
};

#[derive(Clone)]
pub struct State {
    // cNight asset identity
    cnight_policy_id: PolicyId,
    cnight_asset_name: AssetName,

    // CNight UTxO spends and creations indexed by block
    asset_utxos: AssetUTxOState,
    // Candidate (Node operator) registrations/deregistrations indexed by block
    candidate_registrations: CandidateRegistrationState,
    // Candidate (Node operator) sets indexed by the last block of each epoch
    _candidate_utxos: CandidateUTxOState,
    // Governance indexed by block
    _governance: GovernanceState,
    // Parameters indexed by epoch
    _parameters: ParametersState,
}

impl Default for State {
    fn default() -> Self {
        Self::new(
            PolicyId::default(),
            AssetName::new(&[]).expect("empty asset name should be valid"),
        )
    }
}

#[derive(Clone, Default)]
pub struct AssetUTxOState {
    pub created_utxos: BTreeMap<BlockNumber, Vec<UTxOIdentifier>>,
    pub spent_utxos: BTreeMap<BlockNumber, Vec<UTxOIdentifier>>,
    pub utxo_index: HashMap<UTxOIdentifier, UTxOMeta>,
}

#[derive(Clone, Default)]
pub struct CandidateRegistrationState {
    pub registrations: BTreeMap<BlockNumber, Vec<Arc<RegistrationEvent>>>,
    pub deregistrations: BTreeMap<BlockNumber, Vec<Arc<DeregistrationEvent>>>,
}

#[derive(Clone, Default)]
pub struct CandidateUTxOState {
    pub _current: BTreeMap<UTxOIdentifier, CandidateUTxO>,
    pub _history: BTreeMap<BlockNumber, Vec<CandidateUTxO>>,
}

#[derive(Clone, Default)]
pub struct GovernanceState {
    pub _technical_committee: HashMap<BlockNumber, Datum>,
    pub _council: HashMap<BlockNumber, Datum>,
}

#[derive(Clone, Default)]
pub struct ParametersState {
    pub _permissioned_candidates: BTreeMap<Epoch, Option<Datum>>,
}

impl State {
    pub fn new(cnight_policy_id: PolicyId, cnight_asset_name: AssetName) -> Self {
        Self {
            cnight_policy_id,
            cnight_asset_name,
            asset_utxos: AssetUTxOState::default(),
            candidate_registrations: CandidateRegistrationState::default(),
            _candidate_utxos: CandidateUTxOState::default(),
            _governance: GovernanceState::default(),
            _parameters: ParametersState::default(),
        }
    }

    pub fn handle_address_deltas(
        &mut self,
        block: &BlockInfo,
        address_deltas: &AddressDeltasMessage,
    ) -> Result<()> {
        let Some(extended_deltas) = address_deltas.as_extended_deltas() else {
            warn!(
                block_number = block.number,
                block_hash = %block.hash,
                "midnight-state received compact deltas; extended deltas are required"
            );
            return Err(anyhow!(
                "midnight-state requires AddressDeltasMessage::ExtendedDeltas"
            ));
        };

        debug!(
            block_number = block.number,
            block_hash = %block.hash,
            delta_count = extended_deltas.len(),
            "midnight-state processing address deltas"
        );

        let block_timestamp = Self::to_block_timestamp(block.timestamp)?;
        let mut cnight_creates = 0usize;
        let mut cnight_spends = 0usize;

        for delta in extended_deltas {
            let tx_index_in_block = u32::from(delta.tx_identifier.tx_index());
            debug!(
                block_number = block.number,
                tx_identifier = %delta.tx_identifier,
                address = ?delta.address,
                created_utxos = delta.created_utxos.len(),
                spent_utxos = delta.spent_utxos.len(),
                "midnight-state scanning address delta"
            );

            for created_utxo in &delta.created_utxos {
                let Some(quantity) = self.get_cnight_quantity(&created_utxo.value)? else {
                    continue;
                };
                cnight_creates += 1;

                self.asset_utxos
                    .created_utxos
                    .entry(block.number)
                    .or_default()
                    .push(created_utxo.utxo);

                self.asset_utxos.utxo_index.insert(
                    created_utxo.utxo,
                    UTxOMeta {
                        holder_address: delta.address.clone(),
                        asset_quantity: quantity,
                        created_in: block.number,
                        created_block_hash: block.hash,
                        created_tx: created_utxo.utxo.tx_hash,
                        created_tx_index: tx_index_in_block,
                        created_utxo_index: created_utxo.utxo.output_index,
                        created_block_timestamp: block_timestamp,
                        spent_in: None,
                        spent_block_hash: None,
                        spend_tx: None,
                        spent_tx_index: None,
                        spent_block_timestamp: None,
                    },
                );

                debug!(
                    block_number = block.number,
                    address = ?delta.address,
                    utxo = %created_utxo.utxo,
                    quantity,
                    "midnight-state indexed cNight create"
                );
            }

            for spent_utxo in &delta.spent_utxos {
                let Some(quantity) = self.get_cnight_quantity(&spent_utxo.value)? else {
                    continue;
                };
                cnight_spends += 1;

                self.asset_utxos.spent_utxos.entry(block.number).or_default().push(spent_utxo.utxo);

                let entry =
                    self.asset_utxos.utxo_index.entry(spent_utxo.utxo).or_insert_with(|| {
                        UTxOMeta {
                            holder_address: delta.address.clone(),
                            asset_quantity: quantity,
                            created_in: 0,
                            created_block_hash: Default::default(),
                            created_tx: spent_utxo.utxo.tx_hash,
                            created_tx_index: 0,
                            created_utxo_index: spent_utxo.utxo.output_index,
                            created_block_timestamp: block_timestamp,
                            spent_in: None,
                            spent_block_hash: None,
                            spend_tx: None,
                            spent_tx_index: None,
                            spent_block_timestamp: None,
                        }
                    });

                entry.holder_address = delta.address.clone();
                entry.asset_quantity = quantity;
                entry.spent_in = Some(block.number);
                entry.spent_block_hash = Some(block.hash);
                entry.spend_tx = Some(spent_utxo.spent_by);
                entry.spent_tx_index = Some(tx_index_in_block);
                entry.spent_block_timestamp = Some(block_timestamp);

                debug!(
                    block_number = block.number,
                    address = ?delta.address,
                    utxo = %spent_utxo.utxo,
                    quantity,
                    spending_tx_hash = %spent_utxo.spent_by,
                    "midnight-state indexed cNight spend"
                );
            }
        }

        if cnight_creates > 0 || cnight_spends > 0 {
            info!(
                block_number = block.number,
                block_hash = %block.hash,
                cnight_creates,
                cnight_spends,
                tracked_utxos = self.asset_utxos.utxo_index.len(),
                "midnight-state indexed cNight activity"
            );
        } else {
            debug!(
                block_number = block.number,
                block_hash = %block.hash,
                "midnight-state found no cNight activity in block"
            );
        }

        Ok(())
    }

    pub fn handle_new_epoch(&mut self) -> Result<()> {
        Ok(())
    }

    fn get_cnight_quantity(&self, value: &Value) -> Result<Option<i64>> {
        let Some((_, assets)) =
            value.assets.iter().find(|(policy_id, _)| policy_id == &self.cnight_policy_id)
        else {
            return Ok(None);
        };

        let Some(asset) = assets.iter().find(|asset| asset.name == self.cnight_asset_name) else {
            return Ok(None);
        };

        let quantity =
            i64::try_from(asset.amount).context("cNight asset quantity overflowed i64")?;
        Ok(Some(quantity))
    }

    fn to_block_timestamp(timestamp: u64) -> Result<NaiveDateTime> {
        let timestamp = i64::try_from(timestamp).context("block timestamp is out of i64 range")?;
        DateTime::<Utc>::from_timestamp(timestamp, 0)
            .map(|value| value.naive_utc())
            .ok_or_else(|| anyhow!("invalid block timestamp: {timestamp}"))
    }

    #[allow(dead_code)]
    /// Get the CNight UTxO creations within a specified block range
    pub fn get_asset_creates(&self, start: BlockNumber, end: BlockNumber) -> Vec<AssetCreate> {
        self.asset_utxos
            .created_utxos
            .range(start..=end)
            .flat_map(|(_, utxos)| {
                utxos.iter().map(|utxo_id| {
                    let meta = self
                        .asset_utxos
                        .utxo_index
                        .get(utxo_id)
                        .expect("UTxO index out of sync with created_utxos");

                    AssetCreate {
                        block_number: meta.created_in,
                        block_hash: meta.created_block_hash,
                        block_timestamp: meta.created_block_timestamp,
                        tx_index_in_block: meta.created_tx_index,
                        quantity: meta.asset_quantity,
                        holder_address: meta.holder_address.clone(),
                        tx_hash: meta.created_tx,
                        utxo_index: meta.created_utxo_index,
                    }
                })
            })
            .collect()
    }

    #[allow(dead_code)]
    /// Get the CNight UTxO spends within a specified block range
    pub fn get_asset_spends(&self, start: BlockNumber, end: BlockNumber) -> Vec<AssetSpend> {
        self.asset_utxos
            .spent_utxos
            .range(start..=end)
            .flat_map(|(_, utxos)| {
                utxos.iter().map(|utxo_id| {
                    let meta = self
                        .asset_utxos
                        .utxo_index
                        .get(utxo_id)
                        .expect("UTxO index out of sync with spent_utxos");

                    AssetSpend {
                        block_number: meta
                            .spent_in
                            .expect("UTxO index out of sync with spent_utxos"),
                        block_hash: meta
                            .spent_block_hash
                            .expect("UTxO index out of sync with spent_utxos"),
                        block_timestamp: meta
                            .spent_block_timestamp
                            .expect("UTxO index out of sync with spent_utxos"),
                        tx_index_in_block: meta
                            .spent_tx_index
                            .expect("UTxO index out of sync with spent_utxos"),
                        quantity: meta.asset_quantity,
                        holder_address: meta.holder_address.clone(),
                        utxo_tx_hash: meta.created_tx,
                        utxo_index: meta.created_utxo_index,
                        spending_tx_hash: meta
                            .spend_tx
                            .expect("UTxO index out of sync with spent_utxos"),
                    }
                })
            })
            .collect()
    }

    #[allow(dead_code)]
    pub fn get_registrations(&self, start: BlockNumber, end: BlockNumber) -> Vec<Registration> {
        self.candidate_registrations
            .registrations
            .range(start..=end)
            .flat_map(|(block_number, events)| {
                events.iter().map(|event| Registration {
                    full_datum: event.datum.clone(),
                    block_number: *block_number,
                    block_hash: event.header.block_hash,
                    block_timestamp: event.header.block_timestamp,
                    tx_index_in_block: event.header.tx_index,
                    tx_hash: event.header.tx_hash,
                    utxo_index: event.header.utxo_index,
                })
            })
            .collect()
    }

    #[allow(dead_code)]
    pub fn get_deregistrations(&self, start: BlockNumber, end: BlockNumber) -> Vec<Deregistration> {
        self.candidate_registrations
            .deregistrations
            .range(start..=end)
            .flat_map(|(block_number, events)| {
                events.iter().map(|event| Deregistration {
                    full_datum: event.datum.clone(),
                    block_number: *block_number,
                    block_hash: event.spent_block_hash,
                    block_timestamp: event.spent_block_timestamp,
                    tx_index_in_block: event.spent_tx_index,
                    tx_hash: event.spent_tx_hash,
                    utxo_tx_hash: event.header.tx_hash,
                    utxo_index: event.header.utxo_index,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use acropolis_common::{
        messages::AddressDeltasMessage, Address, BlockHash, BlockIntent, BlockStatus,
        CreatedUTxOExtended, Era, ExtendedAddressDelta, NativeAsset, SpentUTxOExtended, TxHash,
        TxIdentifier, UTxOIdentifier, Value,
    };

    fn cnight_policy() -> PolicyId {
        PolicyId::from([7u8; 28])
    }

    fn cnight_asset() -> AssetName {
        AssetName::new(b"cNight").unwrap()
    }

    fn block(number: u64, timestamp: u64) -> BlockInfo {
        BlockInfo {
            status: BlockStatus::Immutable,
            intent: BlockIntent::Apply,
            slot: number,
            number,
            hash: BlockHash::from([number as u8; 32]),
            epoch: 1,
            epoch_slot: number,
            new_epoch: false,
            is_new_era: false,
            timestamp,
            tip_slot: None,
            era: Era::Conway,
        }
    }

    fn cnight_value(amount: u64) -> Value {
        Value::new(
            0,
            vec![(
                cnight_policy(),
                vec![NativeAsset {
                    name: cnight_asset(),
                    amount,
                }],
            )],
        )
    }

    #[test]
    fn rejects_compact_address_deltas() {
        let mut state = State::new(cnight_policy(), cnight_asset());
        let err = state
            .handle_address_deltas(&block(1, 1), &AddressDeltasMessage::Deltas(vec![]))
            .expect_err("compact deltas should be rejected");

        assert!(err.to_string().contains("AddressDeltasMessage::ExtendedDeltas"));
    }

    #[test]
    fn indexes_cnight_creates_and_spends_with_block_and_tx_hashes() {
        let mut state = State::new(cnight_policy(), cnight_asset());

        let created_utxo = UTxOIdentifier::new(TxHash::from([1u8; 32]), 2);
        let create_delta = ExtendedAddressDelta {
            address: Address::None,
            tx_identifier: TxIdentifier::new(1, 3),
            spent_utxos: Vec::new(),
            created_utxos: vec![CreatedUTxOExtended {
                utxo: created_utxo,
                value: cnight_value(10),
                datum: None,
            }],
            sent: Value::default(),
            received: cnight_value(10),
        };
        let create_block = block(1, 1_700_000_000);
        state
            .handle_address_deltas(
                &create_block,
                &AddressDeltasMessage::ExtendedDeltas(vec![create_delta]),
            )
            .unwrap();

        let creates = state.get_asset_creates(1, 1);
        assert_eq!(creates.len(), 1);
        assert_eq!(creates[0].block_hash, create_block.hash);
        assert_eq!(creates[0].tx_hash, created_utxo.tx_hash);
        assert_eq!(creates[0].quantity, 10);
        assert_eq!(creates[0].utxo_index, created_utxo.output_index);

        let spend_tx_hash = TxHash::from([2u8; 32]);
        let spend_delta = ExtendedAddressDelta {
            address: Address::None,
            tx_identifier: TxIdentifier::new(2, 4),
            spent_utxos: vec![SpentUTxOExtended {
                utxo: created_utxo,
                value: cnight_value(10),
                spent_by: spend_tx_hash,
                datum: None,
            }],
            created_utxos: Vec::new(),
            sent: cnight_value(10),
            received: Value::default(),
        };
        let spend_block = block(2, 1_700_000_010);
        state
            .handle_address_deltas(
                &spend_block,
                &AddressDeltasMessage::ExtendedDeltas(vec![spend_delta]),
            )
            .unwrap();

        let spends = state.get_asset_spends(2, 2);
        assert_eq!(spends.len(), 1);
        assert_eq!(spends[0].block_hash, spend_block.hash);
        assert_eq!(spends[0].spending_tx_hash, spend_tx_hash);
        assert_eq!(spends[0].utxo_tx_hash, created_utxo.tx_hash);
        assert_eq!(spends[0].quantity, 10);
    }

    #[test]
    fn ignores_non_cnight_assets() {
        let mut state = State::new(cnight_policy(), cnight_asset());

        let other_policy = PolicyId::from([3u8; 28]);
        let other_value = Value::new(
            0,
            vec![(
                other_policy,
                vec![NativeAsset {
                    name: AssetName::new(b"other").unwrap(),
                    amount: 1,
                }],
            )],
        );

        let delta = ExtendedAddressDelta {
            address: Address::None,
            tx_identifier: TxIdentifier::new(1, 0),
            spent_utxos: Vec::new(),
            created_utxos: vec![CreatedUTxOExtended {
                utxo: UTxOIdentifier::new(TxHash::from([9u8; 32]), 0),
                value: other_value.clone(),
                datum: None,
            }],
            sent: Value::default(),
            received: other_value,
        };

        state
            .handle_address_deltas(
                &block(1, 1_700_000_000),
                &AddressDeltasMessage::ExtendedDeltas(vec![delta]),
            )
            .unwrap();

        assert!(state.get_asset_creates(1, 1).is_empty());
        assert!(state.get_asset_spends(1, 1).is_empty());
    }
}
