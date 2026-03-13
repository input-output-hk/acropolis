use std::collections::{HashMap as StdHashMap, HashSet};

use anyhow::Result;

use acropolis_common::{
    messages::AddressDeltasMessage, protocol_params::Nonce, BlockInfo, BlockNumber, Datum, Epoch,
    ExtendedAddressDelta, UTxOIdentifier,
};
use imbl::HashMap;
use tracing::warn;

use crate::{
    configuration::MidnightConfig,
    epoch_totals::EpochTotals,
    grpc::midnight_state_proto::EpochCandidate,
    indexes::{
        bridge_state::BridgeState, cnight_utxo_state::CNightUTxOState,
        committee_candidate_state::CommitteeCandidateState, governance_state::GovernanceState,
        mapping_candidate_state::MappingCandidateState, parameters_state::ParametersState,
    },
    types::{BridgeCreation, CNightCreation, CNightSpend, DeregistrationEvent, RegistrationEvent},
};

#[derive(Clone, Default)]
pub struct State {
    // Epoch aggregate emitted as telemetry when crossing an epoch boundary.
    epoch_totals: EpochTotals,

    // CNight UTxO spends and creations indexed by block
    pub utxos: CNightUTxOState,
    // Mapping-validator registrations and deregistrations consumed by cNIGHT observation.
    pub mapping_candidates: MappingCandidateState,
    // Committee candidate set snapshotted by epoch for authority selection.
    committee_candidates: CommitteeCandidateState,
    // Bridge UTxOs indexed by block
    pub bridge: BridgeState,
    // Governance indexed by block
    governance: GovernanceState,
    // Parameters indexed by epoch
    parameters: ParametersState,
    // Nonces indexed by epoch
    nonces: HashMap<Epoch, Nonce>,
    // Midnight configuration
    config: MidnightConfig,
}

impl State {
    pub fn new(config: MidnightConfig) -> Self {
        Self {
            config,
            ..Self::default()
        }
    }

    pub fn handle_new_epoch(&mut self, block_info: &BlockInfo, nonce_opt: Option<Nonce>) {
        if let Some(nonce) = nonce_opt {
            self.nonces.insert(block_info.epoch, nonce);
        }
        self.committee_candidates.snapshot_epoch(block_info.epoch);
        self.epoch_totals.summarise_completed_epoch(block_info);
    }

    pub fn get_ariadne_parameters_with_epoch(&self, epoch: Epoch) -> Option<(Epoch, Datum)> {
        self.parameters.get_ariadne_parameters_with_epoch(epoch)
    }

    pub fn get_epoch_nonce(&self, epoch: Epoch) -> Option<Vec<u8>> {
        self.nonces.get(&epoch).and_then(|n| n.hash.map(|h| h.to_vec()))
    }

    pub fn get_epoch_candidates(&self, epoch: Epoch) -> Vec<EpochCandidate> {
        self.committee_candidates.get_epoch_candidates(epoch)
    }

    pub fn handle_address_deltas(
        &mut self,
        block_info: &BlockInfo,
        address_deltas: &AddressDeltasMessage,
    ) -> Result<()> {
        let deltas = address_deltas.as_extended_deltas()?;

        let mut cnight_creations = Vec::new();
        let mut block_created_utxos = HashSet::new();
        let mut cnight_spends = Vec::new();

        let mut bridge_creations = Vec::new();
        let mut block_created_bridge_utxos = StdHashMap::new();

        let mut candidate_registrations = Vec::new();
        let mut block_created_registrations = HashSet::new();
        let mut candidate_deregistrations = Vec::new();
        let mut committee_candidate_registrations = Vec::new();
        let mut block_created_committee_registrations = HashSet::new();
        let mut committee_candidate_deregistrations = Vec::new();

        let mut indexed_parameter_datums = 0usize;
        let mut indexed_governance_technical_committee_datums = 0usize;
        let mut indexed_governance_council_datums = 0usize;

        for delta in deltas {
            // Collect CNight UTxO creations and spends for the block
            self.collect_cnight_creations(
                delta,
                block_info,
                &mut cnight_creations,
                &mut block_created_utxos,
            )?;
            cnight_spends.extend(self.collect_cnight_spends(
                delta,
                block_info,
                &block_created_utxos,
            )?);

            self.collect_bridge_creations(
                delta,
                block_info,
                &mut bridge_creations,
                &mut block_created_bridge_utxos,
            )?;

            // Collect candidate registrations and deregistrations
            self.collect_candidate_registrations(
                delta,
                block_info,
                &mut candidate_registrations,
                &mut block_created_registrations,
            )?;
            candidate_deregistrations.extend(self.collect_candidate_deregistrations(
                delta,
                block_info,
                &block_created_registrations,
            )?);
            self.collect_committee_candidate_registrations(
                delta,
                block_info,
                &mut committee_candidate_registrations,
                &mut block_created_committee_registrations,
            )?;
            committee_candidate_deregistrations.extend(
                self.collect_committee_candidate_deregistrations(
                    delta,
                    block_info,
                    &block_created_committee_registrations,
                )?,
            );

            indexed_parameter_datums += self.collect_parameter_datums(delta, block_info.epoch);

            let (indexed_technical_committee, indexed_council) =
                self.collect_governance_datums(delta, block_info.number);
            indexed_governance_technical_committee_datums += indexed_technical_committee;
            indexed_governance_council_datums += indexed_council;
        }

        // Add created and spent CNight utxos to state
        self.epoch_totals.add_indexed_night_utxos(cnight_creations.len(), cnight_spends.len());
        if !cnight_creations.is_empty() {
            self.utxos.add_created_utxos(block_info.number, cnight_creations);
        }
        if !cnight_spends.is_empty() {
            self.utxos.add_spent_utxos(block_info.number, cnight_spends)?;
        }

        self.epoch_totals.add_indexed_bridge_utxos(bridge_creations.len());
        if !bridge_creations.is_empty() {
            self.bridge.add_created_utxos(block_info.number, bridge_creations);
        }

        // Add registered and deregistered candidates to state
        self.epoch_totals.add_indexed_candidates(
            candidate_registrations.len(),
            candidate_deregistrations.len(),
        );
        if !candidate_registrations.is_empty() {
            self.mapping_candidates.register_candidates(block_info.number, candidate_registrations);
        }
        if !candidate_deregistrations.is_empty() {
            self.mapping_candidates
                .deregister_candidates(block_info.number, candidate_deregistrations);
        }
        if !committee_candidate_registrations.is_empty() {
            self.committee_candidates.register_candidates(committee_candidate_registrations);
        }
        if !committee_candidate_deregistrations.is_empty() {
            self.committee_candidates.deregister_candidates(committee_candidate_deregistrations);
        }

        self.epoch_totals.add_indexed_parameter_datums(indexed_parameter_datums);
        self.epoch_totals.add_indexed_governance_datums(
            indexed_governance_technical_committee_datums,
            indexed_governance_council_datums,
        );
        Ok(())
    }

    pub fn get_technical_committee_datum(
        &self,
        block_number: BlockNumber,
    ) -> Option<(BlockNumber, Datum)> {
        self.governance.get_technical_committee_datum(block_number)
    }

    pub fn get_council_datum(&self, block_number: BlockNumber) -> Option<(BlockNumber, Datum)> {
        self.governance.get_council_datum(block_number)
    }

    fn collect_cnight_creations(
        &self,
        delta: &ExtendedAddressDelta,
        block_info: &BlockInfo,
        cnight_creations: &mut Vec<CNightCreation>,
        block_created_utxos: &mut HashSet<UTxOIdentifier>,
    ) -> Result<()> {
        if !delta.received.assets.contains_key(&self.config.cnight_policy_id) {
            return Ok(());
        }

        for created in &delta.created_utxos {
            let token_amount = created
                .value
                .assets
                .get(&self.config.cnight_policy_id)
                .and_then(|policy_assets| policy_assets.get(&self.config.cnight_asset_name))
                .copied()
                .unwrap_or(0);

            if token_amount == 0 {
                continue;
            }

            let holder_address = match delta.address.to_stake_address() {
                Some(owner_address) => owner_address,
                None => {
                    warn!(
                        block_number = block_info.number,
                        tx_identifier = %delta.tx_identifier,
                        utxo = %created.utxo,
                        address_kind = delta.address.kind(),
                        "skipping cNIGHT creation with unsupported owner address"
                    );
                    continue;
                }
            };

            let creation = CNightCreation {
                holder_address,
                quantity: token_amount,
                utxo: created.utxo,
                block_number: block_info.number,
                block_hash: block_info.hash,
                tx_index: delta.tx_identifier.tx_index().into(),
                block_timestamp: i64::try_from(block_info.timestamp)?,
            };

            block_created_utxos.insert(created.utxo);
            cnight_creations.push(creation);
        }

        Ok(())
    }

    fn collect_cnight_spends(
        &self,
        delta: &ExtendedAddressDelta,
        block_info: &BlockInfo,
        cnight_creations: &HashSet<UTxOIdentifier>,
    ) -> Result<Vec<(UTxOIdentifier, CNightSpend)>> {
        let timestamp = i64::try_from(block_info.timestamp)?;

        Ok(delta
            .spent_utxos
            .iter()
            .filter_map(|spent| {
                if self.utxos.utxo_index.contains_key(&spent.utxo)
                    || cnight_creations.contains(&spent.utxo)
                {
                    Some((
                        spent.utxo,
                        CNightSpend {
                            block_number: block_info.number,
                            block_hash: block_info.hash,
                            tx_hash: spent.spent_by,
                            tx_index: delta.tx_identifier.tx_index().into(),
                            block_timestamp: timestamp,
                        },
                    ))
                } else {
                    None
                }
            })
            .collect())
    }

    fn collect_bridge_creations(
        &self,
        delta: &ExtendedAddressDelta,
        block_info: &BlockInfo,
        bridge_creations: &mut Vec<BridgeCreation>,
        block_created_bridge_utxos: &mut StdHashMap<UTxOIdentifier, u64>,
    ) -> Result<()> {
        if delta.address != self.config.illiquid_circulation_supply_validator_address {
            return Ok(());
        }

        let tokens_in = delta
            .spent_utxos
            .iter()
            .map(|spent| {
                block_created_bridge_utxos
                    .get(&spent.utxo)
                    .copied()
                    .or_else(|| {
                        self.bridge.utxo_index.get(&spent.utxo).map(|meta| meta.creation.tokens_out)
                    })
                    .unwrap_or(0)
            })
            .sum();

        for created in &delta.created_utxos {
            let tokens_out = created
                .value
                .assets
                .get(&self.config.bridge_token_policy_id)
                .and_then(|policy_assets| policy_assets.get(&self.config.bridge_token_asset_name))
                .copied()
                .unwrap_or(0);

            if tokens_out == 0 {
                continue;
            }

            let datum = created.datum.as_ref().and_then(|datum| datum.to_bytes());
            bridge_creations.push(BridgeCreation {
                utxo: created.utxo,
                block_number: block_info.number,
                block_hash: block_info.hash,
                tx_index: delta.tx_identifier.tx_index().into(),
                block_timestamp: i64::try_from(block_info.timestamp)?,
                tokens_out,
                tokens_in,
                datum,
            });
            block_created_bridge_utxos.insert(created.utxo, tokens_out);
        }

        Ok(())
    }

    fn collect_candidate_registrations(
        &self,
        delta: &ExtendedAddressDelta,
        block_info: &BlockInfo,
        registrations: &mut Vec<RegistrationEvent>,
        block_created_registrations: &mut HashSet<UTxOIdentifier>,
    ) -> Result<()> {
        if delta.address != self.config.mapping_validator_address {
            return Ok(());
        }

        for created in &delta.created_utxos {
            let has_auth_token = created
                .value
                .assets
                .get(&self.config.auth_token_policy_id)
                .and_then(|policy_assets| policy_assets.get(&self.config.auth_token_asset_name))
                .copied()
                .unwrap_or(0)
                > 0;

            if !has_auth_token {
                continue;
            }
            if let Some(datum) = &created.datum {
                block_created_registrations.insert(created.utxo);

                registrations.push(RegistrationEvent {
                    block_number: block_info.number,
                    block_hash: block_info.hash,
                    block_timestamp: i64::try_from(block_info.timestamp)?,
                    epoch: block_info.epoch,
                    slot_number: block_info.slot,
                    tx_index: delta.tx_identifier.tx_index().into(),
                    tx_hash: created.utxo.tx_hash,
                    utxo_index: created.utxo.output_index,
                    tx_inputs: delta.spent_utxos.iter().map(|s| s.utxo).collect(),
                    datum: datum.clone(),
                });
            }
        }

        Ok(())
    }

    fn collect_candidate_deregistrations(
        &self,
        delta: &ExtendedAddressDelta,
        block_info: &BlockInfo,
        block_created_registrations: &HashSet<UTxOIdentifier>,
    ) -> Result<Vec<DeregistrationEvent>> {
        let mut deregistrations = Vec::new();

        if delta.address != self.config.mapping_validator_address {
            return Ok(deregistrations);
        }

        for spent in &delta.spent_utxos {
            if self.mapping_candidates.registration_index.contains_key(&spent.utxo)
                || block_created_registrations.contains(&spent.utxo)
            {
                deregistrations.push(DeregistrationEvent {
                    registration_utxo: spent.utxo,
                    spent_block_hash: block_info.hash,
                    spent_block_timestamp: i64::try_from(block_info.timestamp)?,
                    spent_tx_hash: spent.spent_by,
                    spent_tx_index: delta.tx_identifier.tx_index().into(),
                });
            }
        }

        Ok(deregistrations)
    }

    fn collect_committee_candidate_registrations(
        &self,
        delta: &ExtendedAddressDelta,
        block_info: &BlockInfo,
        registrations: &mut Vec<RegistrationEvent>,
        block_created_registrations: &mut HashSet<UTxOIdentifier>,
    ) -> Result<()> {
        if delta.address != self.config.committee_candidate_address {
            return Ok(());
        }

        for created in &delta.created_utxos {
            if let Some(datum) = &created.datum {
                block_created_registrations.insert(created.utxo);

                registrations.push(RegistrationEvent {
                    block_number: block_info.number,
                    block_hash: block_info.hash,
                    block_timestamp: i64::try_from(block_info.timestamp)?,
                    epoch: block_info.epoch,
                    slot_number: block_info.slot,
                    tx_index: delta.tx_identifier.tx_index().into(),
                    tx_hash: created.utxo.tx_hash,
                    utxo_index: created.utxo.output_index,
                    tx_inputs: delta.spent_utxos.iter().map(|s| s.utxo).collect(),
                    datum: datum.clone(),
                });
            }
        }

        Ok(())
    }

    fn collect_committee_candidate_deregistrations(
        &self,
        delta: &ExtendedAddressDelta,
        block_info: &BlockInfo,
        block_created_registrations: &HashSet<UTxOIdentifier>,
    ) -> Result<Vec<DeregistrationEvent>> {
        let mut deregistrations = Vec::new();

        if delta.address != self.config.committee_candidate_address {
            return Ok(deregistrations);
        }

        for spent in &delta.spent_utxos {
            if self.committee_candidates.registration_index.contains_key(&spent.utxo)
                || block_created_registrations.contains(&spent.utxo)
            {
                deregistrations.push(DeregistrationEvent {
                    registration_utxo: spent.utxo,
                    spent_block_hash: block_info.hash,
                    spent_block_timestamp: i64::try_from(block_info.timestamp)?,
                    spent_tx_hash: spent.spent_by,
                    spent_tx_index: delta.tx_identifier.tx_index().into(),
                });
            }
        }

        Ok(deregistrations)
    }

    fn collect_parameter_datums(&mut self, delta: &ExtendedAddressDelta, epoch: Epoch) -> usize {
        if !delta.received.assets.contains_key(&self.config.permissioned_candidate_policy) {
            return 0;
        }

        let mut indexed = 0usize;
        for created in &delta.created_utxos {
            if !created.value.assets.contains_key(&self.config.permissioned_candidate_policy) {
                continue;
            }

            match &created.datum {
                Some(datum) if self.parameters.add_parameter_datum(epoch, datum.clone()) => {
                    indexed += 1;
                }
                _ => {}
            }
        }
        indexed
    }

    fn collect_governance_datums(
        &mut self,
        delta: &ExtendedAddressDelta,
        block_number: BlockNumber,
    ) -> (usize, usize) {
        let is_technical_committee_address =
            delta.address == self.config.technical_committee_address;
        let is_council_address = delta.address == self.config.council_address;
        if !is_technical_committee_address && !is_council_address {
            return (0, 0);
        }

        let mut indexed_technical_committee = 0usize;
        let mut indexed_council = 0usize;
        for created in &delta.created_utxos {
            let Some(datum) = &created.datum else {
                continue;
            };

            if is_technical_committee_address
                && created.value.assets.contains_key(&self.config.technical_committee_policy_id)
                && self.governance.insert_technical_committee_datum(block_number, datum.clone())
            {
                indexed_technical_committee += 1;
            }

            if is_council_address
                && created.value.assets.contains_key(&self.config.council_policy_id)
                && self.governance.insert_council_datum(block_number, datum.clone())
            {
                indexed_council += 1;
            }
        }
        (indexed_technical_committee, indexed_council)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use acropolis_common::{
        messages::AddressDeltasMessage,
        state_history::{StateHistory, StateHistoryStore},
        Address, AssetName, BlockHash, BlockInfo, BlockIntent, BlockStatus, CreatedUTxOExtended,
        Datum, Era, ExtendedAddressDelta, PolicyId, ShelleyAddress, SpentUTxOExtended, TxHash,
        TxIdentifier, UTxOIdentifier, ValueMap,
    };

    use crate::{
        configuration::MidnightConfig, indexes::bridge_state::BridgeCheckpoint, state::State,
    };

    fn test_block_info() -> BlockInfo {
        BlockInfo {
            status: BlockStatus::Volatile,
            intent: BlockIntent::Apply,
            slot: 1000,
            number: 1,
            hash: BlockHash::default(),
            epoch: 1,
            epoch_slot: 1000,
            new_epoch: false,
            is_new_era: false,
            tip_slot: Some(1000),
            timestamp: 0,
            era: Era::Conway,
        }
    }

    fn test_block_info_for(number: u64, epoch: u64) -> BlockInfo {
        let mut block = test_block_info();
        block.number = number;
        block.epoch = epoch;
        block
    }

    fn test_config_cnight(policy: PolicyId, asset: AssetName) -> MidnightConfig {
        MidnightConfig {
            cnight_policy_id: policy,
            cnight_asset_name: asset,
            ..Default::default()
        }
    }

    fn test_config_candidate(
        address: Address,
        policy: PolicyId,
        asset: AssetName,
    ) -> MidnightConfig {
        MidnightConfig {
            mapping_validator_address: address,
            auth_token_policy_id: policy,
            auth_token_asset_name: asset,
            ..Default::default()
        }
    }

    fn test_config_epoch_candidate(address: Address) -> MidnightConfig {
        MidnightConfig {
            committee_candidate_address: address,
            ..Default::default()
        }
    }

    fn test_config_bridge(address: Address, policy: PolicyId, asset: AssetName) -> MidnightConfig {
        MidnightConfig {
            illiquid_circulation_supply_validator_address: address,
            bridge_token_policy_id: policy,
            bridge_token_asset_name: asset,
            ..Default::default()
        }
    }

    fn test_config_governance(
        technical_committee_address: Address,
        technical_committee_policy_id: PolicyId,
        council_address: Address,
        council_policy_id: PolicyId,
    ) -> MidnightConfig {
        MidnightConfig {
            technical_committee_address,
            technical_committee_policy_id,
            council_address,
            council_policy_id,
            ..Default::default()
        }
    }

    fn test_value_with_token(policy: PolicyId, asset: AssetName, amount: u64) -> ValueMap {
        let mut inner = HashMap::new();
        inner.insert(asset, amount);

        let mut outer = HashMap::new();
        outer.insert(policy, inner);

        ValueMap {
            lovelace: 0,
            assets: outer,
        }
    }

    fn test_value_no_token() -> ValueMap {
        ValueMap::default()
    }

    fn test_parameters_datum_delta(
        policy: PolicyId,
        datum: Datum,
        output_index: u16,
    ) -> ExtendedAddressDelta {
        let asset = AssetName::new(b"params").unwrap();
        ExtendedAddressDelta {
            address: Address::default(),
            tx_identifier: TxIdentifier::default(),
            created_utxos: vec![CreatedUTxOExtended {
                utxo: UTxOIdentifier::new(TxHash::default(), output_index),
                value: test_value_with_token(policy, asset, 1),
                datum: Some(datum),
            }],
            spent_utxos: vec![],
            received: test_value_with_token(policy, asset, 1),
            sent: ValueMap::default(),
        }
    }

    fn test_bridge_delta(
        address: Address,
        tx_index: u16,
        created: Vec<(UTxOIdentifier, ValueMap, Option<Datum>)>,
        spent_utxos: Vec<(UTxOIdentifier, TxHash)>,
    ) -> ExtendedAddressDelta {
        let mut received = ValueMap::default();
        for (_, value, _) in &created {
            for (policy, assets) in &value.assets {
                let entry = received.assets.entry(*policy).or_default();
                for (asset_name, amount) in assets {
                    *entry.entry(*asset_name).or_default() += *amount;
                }
            }
        }

        ExtendedAddressDelta {
            address,
            tx_identifier: TxIdentifier::new(1, tx_index),
            created_utxos: created
                .into_iter()
                .map(|(utxo, value, datum)| CreatedUTxOExtended { utxo, value, datum })
                .collect(),
            spent_utxos: spent_utxos
                .into_iter()
                .map(|(utxo, spent_by)| SpentUTxOExtended { utxo, spent_by })
                .collect(),
            received,
            sent: ValueMap::default(),
        }
    }

    fn test_candidate_delta(
        address: Address,
        tx_index: u16,
        created: Vec<(UTxOIdentifier, ValueMap, Option<Datum>)>,
        spent_utxos: Vec<(UTxOIdentifier, TxHash)>,
    ) -> ExtendedAddressDelta {
        let mut received = ValueMap::default();
        for (_, value, _) in &created {
            for (policy, assets) in &value.assets {
                let entry = received.assets.entry(*policy).or_default();
                for (asset_name, amount) in assets {
                    *entry.entry(*asset_name).or_default() += *amount;
                }
            }
        }

        ExtendedAddressDelta {
            address,
            tx_identifier: TxIdentifier::new(1, tx_index),
            created_utxos: created
                .into_iter()
                .map(|(utxo, value, datum)| CreatedUTxOExtended { utxo, value, datum })
                .collect(),
            spent_utxos: spent_utxos
                .into_iter()
                .map(|(utxo, spent_by)| SpentUTxOExtended { utxo, spent_by })
                .collect(),
            received,
            sent: ValueMap::default(),
        }
    }

    fn test_address(value: &str) -> Address {
        Address::from_string(value).unwrap()
    }

    fn test_governance_datum_delta(
        address: Address,
        policy: PolicyId,
        asset_name: AssetName,
        datum: Datum,
        output_index: u16,
    ) -> ExtendedAddressDelta {
        ExtendedAddressDelta {
            address,
            tx_identifier: TxIdentifier::default(),
            created_utxos: vec![CreatedUTxOExtended {
                utxo: UTxOIdentifier::new(TxHash::default(), output_index),
                value: test_value_with_token(policy, asset_name, 1),
                datum: Some(datum),
            }],
            spent_utxos: vec![],
            received: test_value_with_token(policy, asset_name, 1),
            sent: ValueMap::default(),
        }
    }

    #[test]
    fn should_collect_governance_datums_when_technical_committee_and_council_deltas_present() {
        let block_info = test_block_info();
        let technical_committee_policy = PolicyId::new([0x11u8; 28]);
        let council_policy = PolicyId::new([0x22u8; 28]);
        let technical_committee_asset = AssetName::new(b"tc").unwrap();
        let council_asset = AssetName::new(b"council").unwrap();
        let technical_committee_address = Address::Shelley(
            ShelleyAddress::from_string(
                "addr_test1wqx3yfmsp82nmtyjj4k86s3l04l6lvwaqh2vk2ygcge7kdsk4xc7j",
            )
            .unwrap(),
        );
        let council_address = Address::Shelley(
            ShelleyAddress::from_string(
                "addr_test1wqqwkauz0ypglg5e4u780kcp8hzt75u72yg6z7td62gnk0qed0p06",
            )
            .unwrap(),
        );

        let mut state = State::new(test_config_governance(
            technical_committee_address.clone(),
            technical_committee_policy,
            council_address.clone(),
            council_policy,
        ));

        let technical_committee_datum = Datum::Inline(vec![0xAA]);
        let council_datum = Datum::Inline(vec![0xBB]);

        let technical_committee_delta = ExtendedAddressDelta {
            address: technical_committee_address,
            tx_identifier: TxIdentifier::default(),
            created_utxos: vec![CreatedUTxOExtended {
                utxo: UTxOIdentifier::new(TxHash::new([1u8; 32]), 0),
                value: test_value_with_token(
                    technical_committee_policy,
                    technical_committee_asset,
                    1,
                ),
                datum: Some(technical_committee_datum.clone()),
            }],
            spent_utxos: vec![],
            received: test_value_with_token(
                technical_committee_policy,
                technical_committee_asset,
                1,
            ),
            sent: ValueMap::default(),
        };
        state.collect_governance_datums(&technical_committee_delta, block_info.number);

        let council_delta = ExtendedAddressDelta {
            address: council_address,
            tx_identifier: TxIdentifier::default(),
            created_utxos: vec![CreatedUTxOExtended {
                utxo: UTxOIdentifier::new(TxHash::new([2u8; 32]), 0),
                value: test_value_with_token(council_policy, council_asset, 1),
                datum: Some(council_datum.clone()),
            }],
            spent_utxos: vec![],
            received: test_value_with_token(council_policy, council_asset, 1),
            sent: ValueMap::default(),
        };
        state.collect_governance_datums(&council_delta, block_info.number);

        assert_eq!(
            state.governance.get_technical_committee_datum(block_info.number),
            Some((block_info.number, technical_committee_datum))
        );
        assert_eq!(
            state.governance.get_council_datum(block_info.number),
            Some((block_info.number, council_datum))
        );
    }

    #[test]
    fn should_collect_cnight_creations_and_spends_when_token_present() {
        let block_info = test_block_info();
        let policy = PolicyId::new([1u8; 28]);
        let asset = AssetName::new(b"").unwrap();

        let mut state = State::new(test_config_cnight(policy, asset));

        let token_value_5 = test_value_with_token(policy, asset, 5);
        let token_value_10 = test_value_with_token(policy, asset, 10);
        let no_token_value = test_value_no_token();

        // Creation delta
        let create_delta = ExtendedAddressDelta {
            address: test_address(
                "addr1q82peck5fynytkgjsp9vnpul59zswsd4jqnzafd0mfzykma625r684xsx574ltpznecr9cnc7n9e2hfq9lyart3h5hpszffds5",
            ),
            tx_identifier: TxIdentifier::default(),
            created_utxos: vec![
                CreatedUTxOExtended {
                    utxo: UTxOIdentifier::new(TxHash::default(), 1),
                    value: token_value_5,
                    datum: None,
                },
                CreatedUTxOExtended {
                    utxo: UTxOIdentifier::new(TxHash::default(), 2),
                    value: token_value_10,
                    datum: None,
                },
                CreatedUTxOExtended {
                    utxo: UTxOIdentifier::new(TxHash::default(), 3),
                    value: no_token_value,
                    datum: None,
                },
            ],
            spent_utxos: vec![],
            received: test_value_with_token(policy, asset, 15),
            sent: ValueMap::default(),
        };

        // Collect the CNight UTxO creations
        let mut creations = Vec::new();
        let mut creations_set = HashSet::new();
        state
            .collect_cnight_creations(
                &create_delta,
                &block_info,
                &mut creations,
                &mut creations_set,
            )
            .unwrap();
        assert_eq!(creations.len(), 2);
        assert_eq!(creations[0].quantity, 5);

        // Index the CNightCreation
        state.utxos.add_created_utxos(block_info.number, creations);

        // Retrieve the UTxO from state using the getter
        let utxos = state.utxos.get_asset_creates(block_info.number, 0, 50).unwrap();
        assert_eq!(utxos.len(), 2);
        assert_eq!(utxos[0].quantity, 5);
        assert_eq!(utxos[1].quantity, 10);

        // Spend delta
        let spend_delta = ExtendedAddressDelta {
            address: Address::default(),
            tx_identifier: TxIdentifier::default(),
            created_utxos: vec![],
            spent_utxos: vec![
                SpentUTxOExtended {
                    spent_by: TxHash::default(),
                    utxo: UTxOIdentifier::new(TxHash::default(), 4),
                },
                SpentUTxOExtended {
                    spent_by: TxHash::new([2u8; 32]),
                    utxo: UTxOIdentifier::new(TxHash::default(), 2),
                },
                SpentUTxOExtended {
                    spent_by: TxHash::default(),
                    utxo: UTxOIdentifier::new(TxHash::default(), 3),
                },
            ],
            received: ValueMap::default(),
            sent: ValueMap::default(),
        };

        let spends =
            state.collect_cnight_spends(&spend_delta, &block_info, &HashSet::new()).unwrap();
        assert_eq!(spends.len(), 1);
        assert_eq!(*spends[0].1.tx_hash, [2u8; 32]);

        // Index the CNightSpend
        state.utxos.add_spent_utxos(block_info.number, spends).unwrap();

        // Retrieve the UTxO from state using the getter
        let utxos = state.utxos.get_asset_spends(block_info.number, 0, 50).unwrap();
        assert_eq!(utxos.len(), 1);
        assert_eq!(utxos[0].quantity, 10);
    }

    #[test]
    fn should_collect_candidate_registrations_and_deregistrations_when_mapping_events_present() {
        let block_info = test_block_info();
        let address = Address::Shelley(
            ShelleyAddress::from_string(
                "addr_test1wplxjzranravtp574s2wz00md7vz9rzpucu252je68u9a8qzjheng",
            )
            .unwrap(),
        );
        let policy = PolicyId::new([9u8; 28]);
        let asset = AssetName::new(b"auth").unwrap();

        let config = test_config_candidate(address.clone(), policy, asset);

        let mut state = State::new(config);

        let value_with_token = test_value_with_token(policy, asset, 1);
        let value_without_token = test_value_no_token();

        let registration_delta = ExtendedAddressDelta {
            address: address.clone(),
            tx_identifier: TxIdentifier::new(block_info.number as u32, 9),
            created_utxos: vec![
                CreatedUTxOExtended {
                    utxo: UTxOIdentifier::new(TxHash::new([1u8; 32]), 1),
                    value: value_with_token,
                    datum: Some(Datum::Inline(vec![3])),
                },
                CreatedUTxOExtended {
                    utxo: UTxOIdentifier::new(TxHash::new([2u8; 32]), 2),
                    value: value_without_token,
                    datum: Some(Datum::Inline(vec![2])),
                },
            ],
            spent_utxos: vec![],
            received: ValueMap::default(),
            sent: ValueMap::default(),
        };

        let mut registrations = Vec::new();
        let mut block_created_registrations = HashSet::new();
        state
            .collect_candidate_registrations(
                &registration_delta,
                &block_info,
                &mut registrations,
                &mut block_created_registrations,
            )
            .unwrap();
        assert_eq!(registrations.len(), 1);

        state.mapping_candidates.register_candidates(block_info.number, registrations);

        let indexed = state.mapping_candidates.get_registrations(block_info.number, 0, 50);

        assert_eq!(indexed.len(), 1);
        assert_eq!(indexed[0].full_datum, Datum::Inline(vec![3]));
        assert_eq!(indexed[0].block_number, block_info.number);
        assert_eq!(indexed[0].block_hash, block_info.hash);
        assert_eq!(indexed[0].block_timestamp, block_info.timestamp as i64);
        assert_eq!(indexed[0].tx_index_in_block, 9);
        assert_eq!(indexed[0].tx_hash, TxHash::new([1u8; 32]));
        assert_eq!(indexed[0].utxo_index, 1);

        let deregistration_delta = ExtendedAddressDelta {
            address,
            tx_identifier: TxIdentifier::new(block_info.number as u32, 2),
            created_utxos: vec![],
            spent_utxos: vec![
                SpentUTxOExtended {
                    utxo: UTxOIdentifier::new(TxHash::new([1u8; 32]), 1),
                    spent_by: TxHash::new([3u8; 32]),
                },
                SpentUTxOExtended {
                    utxo: UTxOIdentifier::new(TxHash::new([4u8; 32]), 2),
                    spent_by: TxHash::new([5u8; 32]),
                },
            ],
            received: ValueMap::default(),
            sent: ValueMap::default(),
        };

        let deregistrations = state
            .collect_candidate_deregistrations(&deregistration_delta, &block_info, &HashSet::new())
            .unwrap();
        assert_eq!(deregistrations.len(), 1);

        state.mapping_candidates.deregister_candidates(block_info.number, deregistrations);

        let indexed = state.mapping_candidates.get_deregistrations(block_info.number, 0, 50);

        // Only 1 deregistration indexed
        assert_eq!(indexed.len(), 1);

        // All fields match
        assert_eq!(indexed[0].full_datum, Datum::Inline(vec![3]));
        assert_eq!(indexed[0].block_number, block_info.number);
        assert_eq!(indexed[0].block_hash, block_info.hash);
        assert_eq!(indexed[0].block_timestamp, block_info.timestamp as i64);
        assert_eq!(indexed[0].tx_index_in_block, 2);
        assert_eq!(indexed[0].tx_hash, TxHash::new([3u8; 32]));
        assert_eq!(indexed[0].utxo_tx_hash, TxHash::new([1u8; 32]));
        assert_eq!(indexed[0].utxo_index, 1);
    }

    #[test]
    fn should_index_epoch_candidates_from_committee_candidate_address() {
        let block_info = test_block_info();
        let committee_address =
            test_address("addr_test1wz5ax0hjvhx2uqef8sqrxnmfywd37hea4truhqxu4yxp9hsvggkfm");
        let mapping_address =
            test_address("addr_test1wplxjzranravtp574s2wz00md7vz9rzpucu252je68u9a8qzjheng");
        let auth_policy = PolicyId::new([9u8; 28]);
        let auth_asset = AssetName::new(b"auth").unwrap();

        let mut config = test_config_candidate(mapping_address.clone(), auth_policy, auth_asset);
        config.committee_candidate_address = committee_address.clone();
        let mut state = State::new(config);

        let committee_utxo = UTxOIdentifier::new(TxHash::new([7u8; 32]), 0);
        let mapping_utxo = UTxOIdentifier::new(TxHash::new([8u8; 32]), 1);

        state
            .handle_address_deltas(
                &block_info,
                &AddressDeltasMessage::ExtendedDeltas(vec![
                    test_candidate_delta(
                        committee_address,
                        0,
                        vec![(
                            committee_utxo,
                            test_value_no_token(),
                            Some(Datum::Inline(vec![0xCA])),
                        )],
                        vec![],
                    ),
                    test_candidate_delta(
                        mapping_address,
                        1,
                        vec![(
                            mapping_utxo,
                            test_value_with_token(auth_policy, auth_asset, 1),
                            Some(Datum::Inline(vec![0xAA])),
                        )],
                        vec![],
                    ),
                ]),
            )
            .unwrap();

        state.handle_new_epoch(&block_info, None);

        let epoch_candidates = state.get_epoch_candidates(block_info.epoch);
        assert_eq!(epoch_candidates.len(), 1);
        assert_eq!(
            epoch_candidates[0].utxo_tx_hash,
            committee_utxo.tx_hash.to_vec()
        );
        assert_eq!(
            epoch_candidates[0].utxo_index,
            u32::from(committee_utxo.output_index)
        );
        assert_eq!(epoch_candidates[0].full_datum, vec![0xCA]);

        let registrations = state.mapping_candidates.get_registrations(block_info.number, 0, 50);
        assert_eq!(registrations.len(), 1);
        assert_eq!(registrations[0].tx_hash, mapping_utxo.tx_hash);
    }

    #[test]
    fn should_remove_spent_committee_candidates_from_future_epoch_snapshots() {
        let committee_address =
            test_address("addr_test1wz5ax0hjvhx2uqef8sqrxnmfywd37hea4truhqxu4yxp9hsvggkfm");
        let mut state = State::new(test_config_epoch_candidate(committee_address.clone()));

        let registration_block = test_block_info_for(1, 1);
        let deregistration_block = test_block_info_for(2, 2);
        let committee_utxo = UTxOIdentifier::new(TxHash::new([6u8; 32]), 0);

        state
            .handle_address_deltas(
                &registration_block,
                &AddressDeltasMessage::ExtendedDeltas(vec![test_candidate_delta(
                    committee_address.clone(),
                    0,
                    vec![(
                        committee_utxo,
                        test_value_no_token(),
                        Some(Datum::Inline(vec![0xAB])),
                    )],
                    vec![],
                )]),
            )
            .unwrap();
        state.handle_new_epoch(&registration_block, None);
        assert_eq!(state.get_epoch_candidates(1).len(), 1);

        state
            .handle_address_deltas(
                &deregistration_block,
                &AddressDeltasMessage::ExtendedDeltas(vec![test_candidate_delta(
                    committee_address,
                    0,
                    vec![],
                    vec![(committee_utxo, TxHash::new([7u8; 32]))],
                )]),
            )
            .unwrap();
        state.handle_new_epoch(&deregistration_block, None);

        assert!(state.get_epoch_candidates(2).is_empty());
    }

    #[test]
    fn should_index_parameters_when_policy_matches() {
        let block_info = test_block_info();
        let cnight_policy = PolicyId::new([1u8; 28]);
        let cnight_asset = AssetName::new(b"").unwrap();
        let parameter_policy = PolicyId::new([9u8; 28]);
        let parameter_asset = AssetName::new(b"params").unwrap();

        let mut config = test_config_cnight(cnight_policy, cnight_asset);
        config.permissioned_candidate_policy = parameter_policy;
        let mut state = State::new(config);

        let expected_datum = Datum::Inline(vec![0xAA, 0xBB, 0xCC]);

        let delta = ExtendedAddressDelta {
            address: Address::default(),
            tx_identifier: TxIdentifier::default(),
            created_utxos: vec![CreatedUTxOExtended {
                utxo: UTxOIdentifier::new(TxHash::default(), 1),
                value: test_value_with_token(parameter_policy, parameter_asset, 1),
                datum: Some(expected_datum.clone()),
            }],
            spent_utxos: vec![],
            received: test_value_with_token(parameter_policy, parameter_asset, 1),
            sent: ValueMap::default(),
        };

        state
            .handle_address_deltas(
                &block_info,
                &AddressDeltasMessage::ExtendedDeltas(vec![delta]),
            )
            .unwrap();

        assert_eq!(
            state.parameters.get_ariadne_parameters_with_epoch(block_info.epoch),
            Some((block_info.epoch, expected_datum))
        );
    }

    #[test]
    fn should_ignore_parameters_when_policy_does_not_match() {
        let block_info = test_block_info();
        let cnight_policy = PolicyId::new([1u8; 28]);
        let cnight_asset = AssetName::new(b"").unwrap();
        let parameter_policy = PolicyId::new([9u8; 28]);
        let other_policy = PolicyId::new([8u8; 28]);
        let parameter_asset = AssetName::new(b"params").unwrap();

        let mut config = test_config_cnight(cnight_policy, cnight_asset);
        config.permissioned_candidate_policy = parameter_policy;
        let mut state = State::new(config);

        let delta = ExtendedAddressDelta {
            address: Address::default(),
            tx_identifier: TxIdentifier::default(),
            created_utxos: vec![CreatedUTxOExtended {
                utxo: UTxOIdentifier::new(TxHash::default(), 1),
                value: test_value_with_token(other_policy, parameter_asset, 1),
                datum: Some(Datum::Inline(vec![0x01])),
            }],
            spent_utxos: vec![],
            received: test_value_with_token(other_policy, parameter_asset, 1),
            sent: ValueMap::default(),
        };

        state
            .handle_address_deltas(
                &block_info,
                &AddressDeltasMessage::ExtendedDeltas(vec![delta]),
            )
            .unwrap();

        assert_eq!(
            state.parameters.get_ariadne_parameters_with_epoch(block_info.epoch),
            None
        );
    }

    #[test]
    fn should_restore_previous_parameter_datum_when_rollback_and_replay() {
        let cnight_policy = PolicyId::new([1u8; 28]);
        let cnight_asset = AssetName::new(b"").unwrap();
        let parameter_policy = PolicyId::new([9u8; 28]);
        let mut config = test_config_cnight(cnight_policy, cnight_asset);
        config.permissioned_candidate_policy = parameter_policy;

        let mut history = StateHistory::<State>::new(
            "midnight_state_parameters_test",
            StateHistoryStore::Unbounded,
        );

        let block1 = test_block_info_for(1, 10);
        let block2 = test_block_info_for(2, 10);

        let datum_a = Datum::Inline(vec![0x0A]);
        let datum_b = Datum::Inline(vec![0x0B]);
        let datum_c = Datum::Inline(vec![0x0C]);

        let mut state = history.get_or_init_with(|| State::new(config.clone()));
        state
            .handle_address_deltas(
                &block1,
                &AddressDeltasMessage::ExtendedDeltas(vec![test_parameters_datum_delta(
                    parameter_policy,
                    datum_a.clone(),
                    1,
                )]),
            )
            .unwrap();
        history.commit(block1.number, state);

        let mut state = history.get_or_init_with(|| State::new(config.clone()));
        state
            .handle_address_deltas(
                &block2,
                &AddressDeltasMessage::ExtendedDeltas(vec![test_parameters_datum_delta(
                    parameter_policy,
                    datum_b.clone(),
                    2,
                )]),
            )
            .unwrap();
        history.commit(block2.number, state);

        assert_eq!(
            history.current().unwrap().parameters.get_ariadne_parameters_with_epoch(block2.epoch),
            Some((block2.epoch, datum_b.clone()))
        );

        let mut rolled_back_state = history.get_rolled_back_state(block2.number);
        assert_eq!(
            rolled_back_state.parameters.get_ariadne_parameters_with_epoch(block2.epoch),
            Some((block2.epoch, datum_a))
        );

        rolled_back_state
            .handle_address_deltas(
                &block2,
                &AddressDeltasMessage::ExtendedDeltas(vec![test_parameters_datum_delta(
                    parameter_policy,
                    datum_c.clone(),
                    3,
                )]),
            )
            .unwrap();
        history.commit(block2.number, rolled_back_state);

        assert_eq!(
            history.current().unwrap().parameters.get_ariadne_parameters_with_epoch(block2.epoch),
            Some((block2.epoch, datum_c))
        );
    }

    #[test]
    fn should_index_governance_datums_when_address_and_policy_match() {
        let block_info = test_block_info();
        let cnight_policy = PolicyId::new([1u8; 28]);
        let cnight_asset = AssetName::new(b"").unwrap();
        let technical_policy = PolicyId::new([7u8; 28]);
        let council_policy = PolicyId::new([8u8; 28]);
        let technical_asset = AssetName::new(b"tc").unwrap();
        let council_asset = AssetName::new(b"council").unwrap();
        let technical_address =
            test_address("addr_test1wqx3yfmsp82nmtyjj4k86s3l04l6lvwaqh2vk2ygcge7kdsk4xc7j");
        let council_address =
            test_address("addr_test1wqqwkauz0ypglg5e4u780kcp8hzt75u72yg6z7td62gnk0qed0p06");

        let mut config = test_config_cnight(cnight_policy, cnight_asset);
        config.technical_committee_address = technical_address.clone();
        config.technical_committee_policy_id = technical_policy;
        config.council_address = council_address.clone();
        config.council_policy_id = council_policy;
        let mut state = State::new(config);

        let technical_datum = Datum::Inline(vec![0x10, 0x20]);
        let council_datum = Datum::Inline(vec![0x30, 0x40]);

        let technical_delta = ExtendedAddressDelta {
            address: technical_address,
            tx_identifier: TxIdentifier::default(),
            created_utxos: vec![CreatedUTxOExtended {
                utxo: UTxOIdentifier::new(TxHash::default(), 1),
                value: test_value_with_token(technical_policy, technical_asset, 1),
                datum: Some(technical_datum.clone()),
            }],
            spent_utxos: vec![],
            received: test_value_with_token(technical_policy, technical_asset, 1),
            sent: ValueMap::default(),
        };

        let council_delta = ExtendedAddressDelta {
            address: council_address,
            tx_identifier: TxIdentifier::default(),
            created_utxos: vec![CreatedUTxOExtended {
                utxo: UTxOIdentifier::new(TxHash::default(), 2),
                value: test_value_with_token(council_policy, council_asset, 1),
                datum: Some(council_datum.clone()),
            }],
            spent_utxos: vec![],
            received: test_value_with_token(council_policy, council_asset, 1),
            sent: ValueMap::default(),
        };

        state
            .handle_address_deltas(
                &block_info,
                &AddressDeltasMessage::ExtendedDeltas(vec![technical_delta, council_delta]),
            )
            .unwrap();

        assert_eq!(
            state.governance.get_technical_committee_datum(block_info.number),
            Some((block_info.number, technical_datum))
        );
        assert_eq!(
            state.governance.get_council_datum(block_info.number),
            Some((block_info.number, council_datum))
        );
    }

    #[test]
    fn should_ignore_governance_datums_when_address_does_not_match() {
        let block_info = test_block_info();
        let cnight_policy = PolicyId::new([1u8; 28]);
        let cnight_asset = AssetName::new(b"").unwrap();
        let technical_policy = PolicyId::new([7u8; 28]);
        let technical_asset = AssetName::new(b"tc").unwrap();
        let technical_address =
            test_address("addr_test1wqx3yfmsp82nmtyjj4k86s3l04l6lvwaqh2vk2ygcge7kdsk4xc7j");
        let wrong_address =
            test_address("addr_test1wplxjzranravtp574s2wz00md7vz9rzpucu252je68u9a8qzjheng");

        let mut config = test_config_cnight(cnight_policy, cnight_asset);
        config.technical_committee_address = technical_address;
        config.technical_committee_policy_id = technical_policy;
        let mut state = State::new(config);

        let delta = ExtendedAddressDelta {
            address: wrong_address,
            tx_identifier: TxIdentifier::default(),
            created_utxos: vec![CreatedUTxOExtended {
                utxo: UTxOIdentifier::new(TxHash::default(), 1),
                value: test_value_with_token(technical_policy, technical_asset, 1),
                datum: Some(Datum::Inline(vec![0x01])),
            }],
            spent_utxos: vec![],
            received: test_value_with_token(technical_policy, technical_asset, 1),
            sent: ValueMap::default(),
        };

        state
            .handle_address_deltas(
                &block_info,
                &AddressDeltasMessage::ExtendedDeltas(vec![delta]),
            )
            .unwrap();

        assert_eq!(
            state.governance.get_technical_committee_datum(block_info.number),
            None
        );
    }

    #[test]
    fn should_restore_previous_governance_datum_when_rollback_and_replay() {
        let cnight_policy = PolicyId::new([1u8; 28]);
        let cnight_asset = AssetName::new(b"").unwrap();
        let technical_policy = PolicyId::new([7u8; 28]);
        let technical_asset = AssetName::new(b"tc").unwrap();
        let technical_address =
            test_address("addr_test1wqx3yfmsp82nmtyjj4k86s3l04l6lvwaqh2vk2ygcge7kdsk4xc7j");

        let mut config = test_config_cnight(cnight_policy, cnight_asset);
        config.technical_committee_address = technical_address.clone();
        config.technical_committee_policy_id = technical_policy;

        let mut history = StateHistory::<State>::new(
            "midnight_state_governance_test",
            StateHistoryStore::Unbounded,
        );

        let block1 = test_block_info_for(1, 10);
        let block2 = test_block_info_for(2, 10);

        let datum_a = Datum::Inline(vec![0x11]);
        let datum_b = Datum::Inline(vec![0x22]);
        let datum_c = Datum::Inline(vec![0x33]);

        let mut state = history.get_or_init_with(|| State::new(config.clone()));
        state
            .handle_address_deltas(
                &block1,
                &AddressDeltasMessage::ExtendedDeltas(vec![test_governance_datum_delta(
                    technical_address.clone(),
                    technical_policy,
                    technical_asset,
                    datum_a.clone(),
                    1,
                )]),
            )
            .unwrap();
        history.commit(block1.number, state);

        let mut state = history.get_or_init_with(|| State::new(config.clone()));
        state
            .handle_address_deltas(
                &block2,
                &AddressDeltasMessage::ExtendedDeltas(vec![test_governance_datum_delta(
                    technical_address.clone(),
                    technical_policy,
                    technical_asset,
                    datum_b.clone(),
                    2,
                )]),
            )
            .unwrap();
        history.commit(block2.number, state);

        assert_eq!(
            history.current().unwrap().governance.get_technical_committee_datum(block2.number),
            Some((block2.number, datum_b.clone()))
        );

        let mut rolled_back_state = history.get_rolled_back_state(block2.number);
        assert_eq!(
            rolled_back_state.governance.get_technical_committee_datum(block2.number),
            Some((block1.number, datum_a))
        );

        rolled_back_state
            .handle_address_deltas(
                &block2,
                &AddressDeltasMessage::ExtendedDeltas(vec![test_governance_datum_delta(
                    technical_address,
                    technical_policy,
                    technical_asset,
                    datum_c.clone(),
                    3,
                )]),
            )
            .unwrap();
        history.commit(block2.number, rolled_back_state);

        assert_eq!(
            history.current().unwrap().governance.get_technical_committee_datum(block2.number),
            Some((block2.number, datum_c))
        );
    }

    #[test]
    fn should_index_bridge_utxos_only_at_ics_address_with_configured_asset() {
        let block_info = test_block_info();
        let bridge_policy = PolicyId::new([0x31; 28]);
        let bridge_asset = AssetName::new(b"").unwrap();
        let bridge_address =
            test_address("addr_test1wzga9g4tw69twfvpjynyvdyzvpf5f0e88v6hnu9eh9qgdnqaw66xk");
        let other_address =
            test_address("addr_test1wplxjzranravtp574s2wz00md7vz9rzpucu252je68u9a8qzjheng");

        let mut state = State::new(test_config_bridge(
            bridge_address.clone(),
            bridge_policy,
            bridge_asset,
        ));

        let bridge_utxo = UTxOIdentifier::new(TxHash::new([1u8; 32]), 0);
        let ignored_utxo = UTxOIdentifier::new(TxHash::new([2u8; 32]), 0);

        state
            .handle_address_deltas(
                &block_info,
                &AddressDeltasMessage::ExtendedDeltas(vec![
                    test_bridge_delta(
                        bridge_address,
                        0,
                        vec![(
                            bridge_utxo,
                            test_value_with_token(bridge_policy, bridge_asset, 42),
                            Some(Datum::Inline(vec![0xAA])),
                        )],
                        vec![],
                    ),
                    test_bridge_delta(
                        other_address,
                        1,
                        vec![(
                            ignored_utxo,
                            test_value_with_token(bridge_policy, bridge_asset, 99),
                            Some(Datum::Inline(vec![0xBB])),
                        )],
                        vec![],
                    ),
                ]),
            )
            .unwrap();

        let indexed = state
            .bridge
            .get_bridge_utxos(BridgeCheckpoint::Block(0), block_info.number, 10)
            .unwrap();

        assert_eq!(indexed.len(), 1);
        assert_eq!(indexed[0].tx_hash, bridge_utxo.tx_hash);
        assert_eq!(indexed[0].tokens_out, 42);
    }

    #[test]
    fn should_set_bridge_tokens_in_to_zero_for_deposit_only_transaction() {
        let block_info = test_block_info();
        let bridge_policy = PolicyId::new([0x41; 28]);
        let bridge_asset = AssetName::new(b"").unwrap();
        let bridge_address =
            test_address("addr_test1wzga9g4tw69twfvpjynyvdyzvpf5f0e88v6hnu9eh9qgdnqaw66xk");
        let mut state = State::new(test_config_bridge(
            bridge_address.clone(),
            bridge_policy,
            bridge_asset,
        ));

        let created_utxo = UTxOIdentifier::new(TxHash::new([3u8; 32]), 0);

        state
            .handle_address_deltas(
                &block_info,
                &AddressDeltasMessage::ExtendedDeltas(vec![test_bridge_delta(
                    bridge_address,
                    0,
                    vec![(
                        created_utxo,
                        test_value_with_token(bridge_policy, bridge_asset, 11),
                        Some(Datum::Inline(vec![0x01])),
                    )],
                    vec![],
                )]),
            )
            .unwrap();

        let indexed = state
            .bridge
            .get_bridge_utxos(BridgeCheckpoint::Block(0), block_info.number, 10)
            .unwrap();

        assert_eq!(indexed[0].tokens_in, 0);
        assert_eq!(indexed[0].tokens_out, 11);
    }

    #[test]
    fn should_compute_bridge_tokens_in_from_previously_created_ics_inputs() {
        let bridge_policy = PolicyId::new([0x51; 28]);
        let bridge_asset = AssetName::new(b"").unwrap();
        let bridge_address =
            test_address("addr_test1wzga9g4tw69twfvpjynyvdyzvpf5f0e88v6hnu9eh9qgdnqaw66xk");
        let mut state = State::new(test_config_bridge(
            bridge_address.clone(),
            bridge_policy,
            bridge_asset,
        ));

        let block1 = test_block_info_for(1, 1);
        let block2 = test_block_info_for(2, 1);
        let input_a = UTxOIdentifier::new(TxHash::new([4u8; 32]), 0);
        let input_b = UTxOIdentifier::new(TxHash::new([5u8; 32]), 0);
        let output = UTxOIdentifier::new(TxHash::new([6u8; 32]), 0);

        state
            .handle_address_deltas(
                &block1,
                &AddressDeltasMessage::ExtendedDeltas(vec![
                    test_bridge_delta(
                        bridge_address.clone(),
                        0,
                        vec![(
                            input_a,
                            test_value_with_token(bridge_policy, bridge_asset, 7),
                            Some(Datum::Inline(vec![0x10])),
                        )],
                        vec![],
                    ),
                    test_bridge_delta(
                        bridge_address.clone(),
                        1,
                        vec![(
                            input_b,
                            test_value_with_token(bridge_policy, bridge_asset, 8),
                            Some(Datum::Inline(vec![0x11])),
                        )],
                        vec![],
                    ),
                ]),
            )
            .unwrap();

        state
            .handle_address_deltas(
                &block2,
                &AddressDeltasMessage::ExtendedDeltas(vec![test_bridge_delta(
                    bridge_address,
                    0,
                    vec![(
                        output,
                        test_value_with_token(bridge_policy, bridge_asset, 20),
                        Some(Datum::Inline(vec![0x12])),
                    )],
                    vec![(input_a, output.tx_hash), (input_b, output.tx_hash)],
                )]),
            )
            .unwrap();

        let indexed =
            state.bridge.get_bridge_utxos(BridgeCheckpoint::Block(1), block2.number, 10).unwrap();

        assert_eq!(indexed.len(), 1);
        assert_eq!(indexed[0].tokens_in, 15);
        assert_eq!(indexed[0].tokens_out, 20);
    }

    #[test]
    fn should_compute_bridge_tokens_in_from_same_block_ics_inputs() {
        let block_info = test_block_info();
        let bridge_policy = PolicyId::new([0x61; 28]);
        let bridge_asset = AssetName::new(b"").unwrap();
        let bridge_address =
            test_address("addr_test1wzga9g4tw69twfvpjynyvdyzvpf5f0e88v6hnu9eh9qgdnqaw66xk");
        let mut state = State::new(test_config_bridge(
            bridge_address.clone(),
            bridge_policy,
            bridge_asset,
        ));

        let first = UTxOIdentifier::new(TxHash::new([7u8; 32]), 0);
        let second = UTxOIdentifier::new(TxHash::new([8u8; 32]), 0);

        state
            .handle_address_deltas(
                &block_info,
                &AddressDeltasMessage::ExtendedDeltas(vec![
                    test_bridge_delta(
                        bridge_address.clone(),
                        0,
                        vec![(
                            first,
                            test_value_with_token(bridge_policy, bridge_asset, 13),
                            Some(Datum::Inline(vec![0x20])),
                        )],
                        vec![],
                    ),
                    test_bridge_delta(
                        bridge_address,
                        1,
                        vec![(
                            second,
                            test_value_with_token(bridge_policy, bridge_asset, 17),
                            Some(Datum::Inline(vec![0x21])),
                        )],
                        vec![(first, second.tx_hash)],
                    ),
                ]),
            )
            .unwrap();

        let indexed = state
            .bridge
            .get_bridge_utxos(BridgeCheckpoint::Block(0), block_info.number, 10)
            .unwrap();

        assert_eq!(indexed.len(), 2);
        assert_eq!(indexed[1].tokens_in, 13);
        assert_eq!(indexed[1].tokens_out, 17);
    }

    #[test]
    fn should_restore_previous_bridge_state_when_rollback_and_replay() {
        let bridge_policy = PolicyId::new([0x71; 28]);
        let bridge_asset = AssetName::new(b"").unwrap();
        let bridge_address =
            test_address("addr_test1wzga9g4tw69twfvpjynyvdyzvpf5f0e88v6hnu9eh9qgdnqaw66xk");
        let config = test_config_bridge(bridge_address.clone(), bridge_policy, bridge_asset);

        let mut history =
            StateHistory::<State>::new("midnight_state_bridge_test", StateHistoryStore::Unbounded);

        let block1 = test_block_info_for(1, 10);
        let block2 = test_block_info_for(2, 10);

        let utxo_a = UTxOIdentifier::new(TxHash::new([9u8; 32]), 0);
        let utxo_b = UTxOIdentifier::new(TxHash::new([10u8; 32]), 0);
        let utxo_c = UTxOIdentifier::new(TxHash::new([11u8; 32]), 0);

        let mut state = history.get_or_init_with(|| State::new(config.clone()));
        state
            .handle_address_deltas(
                &block1,
                &AddressDeltasMessage::ExtendedDeltas(vec![test_bridge_delta(
                    bridge_address.clone(),
                    0,
                    vec![(
                        utxo_a,
                        test_value_with_token(bridge_policy, bridge_asset, 3),
                        Some(Datum::Inline(vec![0x30])),
                    )],
                    vec![],
                )]),
            )
            .unwrap();
        history.commit(block1.number, state);

        let mut state = history.get_or_init_with(|| State::new(config.clone()));
        state
            .handle_address_deltas(
                &block2,
                &AddressDeltasMessage::ExtendedDeltas(vec![test_bridge_delta(
                    bridge_address.clone(),
                    0,
                    vec![(
                        utxo_b,
                        test_value_with_token(bridge_policy, bridge_asset, 5),
                        Some(Datum::Inline(vec![0x31])),
                    )],
                    vec![(utxo_a, utxo_b.tx_hash)],
                )]),
            )
            .unwrap();
        history.commit(block2.number, state);

        let current = history.current().unwrap();
        let current_bridge =
            current.bridge.get_bridge_utxos(BridgeCheckpoint::Block(0), block2.number, 10).unwrap();
        assert_eq!(current_bridge.len(), 2);
        assert_eq!(current_bridge[1].tx_hash, utxo_b.tx_hash);

        let mut rolled_back_state = history.get_rolled_back_state(block2.number);
        let rolled_back_bridge = rolled_back_state
            .bridge
            .get_bridge_utxos(BridgeCheckpoint::Block(0), block2.number, 10)
            .unwrap();
        assert_eq!(rolled_back_bridge.len(), 1);
        assert_eq!(rolled_back_bridge[0].tx_hash, utxo_a.tx_hash);

        rolled_back_state
            .handle_address_deltas(
                &block2,
                &AddressDeltasMessage::ExtendedDeltas(vec![test_bridge_delta(
                    bridge_address,
                    0,
                    vec![(
                        utxo_c,
                        test_value_with_token(bridge_policy, bridge_asset, 7),
                        Some(Datum::Inline(vec![0x32])),
                    )],
                    vec![(utxo_a, utxo_c.tx_hash)],
                )]),
            )
            .unwrap();
        history.commit(block2.number, rolled_back_state);

        let replayed_bridge = history
            .current()
            .unwrap()
            .bridge
            .get_bridge_utxos(BridgeCheckpoint::Block(0), block2.number, 10)
            .unwrap();
        assert_eq!(replayed_bridge.len(), 2);
        assert_eq!(replayed_bridge[1].tx_hash, utxo_c.tx_hash);
        assert_eq!(replayed_bridge[1].tokens_in, 3);
    }
}
