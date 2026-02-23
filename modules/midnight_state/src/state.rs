use std::collections::HashSet;

use anyhow::Result;

use acropolis_common::{
    messages::AddressDeltasMessage, BlockInfo, ExtendedAddressDelta, UTxOIdentifier,
};

use crate::{
    configuration::MidnightConfig,
    epoch_totals::{EpochSummary, EpochTotals},
    indexes::{
        candidate_state::CandidateState, cnight_utxo_state::CNightUTxOState,
        governance_state::GovernanceState, parameters_state::ParametersState,
    },
    types::{CNightCreation, CNightSpend, DeregistrationEvent, RegistrationEvent},
};

#[derive(Clone, Default)]
pub struct State {
    // Runtime-active in this PR: epoch totals observer used for logging summaries.
    epoch_totals: EpochTotals,

    // CNight UTxO spends and creations indexed by block
    pub utxos: CNightUTxOState,
    // Candidate (Node operator) sets by epoch and registrations/deregistrations by block
    candidates: CandidateState,
    // Governance indexed by block
    _governance: GovernanceState,
    // Parameters indexed by epoch
    _parameters: ParametersState,
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

    pub fn handle_new_epoch(&mut self, block_info: &BlockInfo) -> Result<EpochSummary> {
        let summary = self.epoch_totals.summarise_completed_epoch(block_info);
        self.epoch_totals.reset_epoch();
        Ok(summary)
    }

    pub fn start_block(&mut self, block: &BlockInfo) {
        self.epoch_totals.start_block(block);
    }

    pub fn finalise_block(&mut self, block: &BlockInfo) {
        self.epoch_totals.finalise_block(block);
    }

    pub fn handle_address_deltas(
        &mut self,
        block_info: &BlockInfo,
        address_deltas: &AddressDeltasMessage,
    ) -> Result<()> {
        let deltas = address_deltas.as_extended_deltas()?;
        self.epoch_totals.observe_deltas(deltas);

        let mut cnight_creations = Vec::new();
        let mut block_created_utxos = HashSet::new();
        let mut cnight_spends = Vec::new();

        let mut candidate_registrations = Vec::new();
        let mut block_created_registrations = HashSet::new();
        let mut candidate_deregistrations = Vec::new();
        for delta in deltas {
            // Collect CNight UTxO creations and spends for the block
            self.collect_cnight_creations(
                delta,
                block_info,
                &mut cnight_creations,
                &mut block_created_utxos,
            );
            cnight_spends.extend(self.collect_cnight_spends(
                delta,
                block_info,
                &block_created_utxos,
            ));

            // Collect candidate registrations and deregistrations
            self.collect_candidate_registrations(
                delta,
                block_info,
                &mut candidate_registrations,
                &mut block_created_registrations,
            );
            candidate_deregistrations.extend(self.collect_candidate_deregistrations(
                delta,
                block_info,
                &block_created_registrations,
            ));
        }

        // Add created and spent CNight utxos to state
        if !cnight_creations.is_empty() {
            self.utxos.add_created_utxos(block_info.number, cnight_creations);
        }
        if !cnight_spends.is_empty() {
            self.utxos.add_spent_utxos(block_info.number, cnight_spends)?;
        }

        // Add registered and deregistered candidates to state
        if !candidate_registrations.is_empty() {
            self.candidates.register_candidates(block_info.number, candidate_registrations);
        }
        if !candidate_deregistrations.is_empty() {
            self.candidates.deregister_candidates(block_info.number, candidate_deregistrations);
        }
        Ok(())
    }

    fn collect_cnight_creations(
        &self,
        delta: &ExtendedAddressDelta,
        block_info: &BlockInfo,
        cnight_creations: &mut Vec<CNightCreation>,
        block_created_utxos: &mut HashSet<UTxOIdentifier>,
    ) {
        if !delta.received.assets.contains_key(&self.config.cnight_policy_id) {
            return;
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

            let creation = CNightCreation {
                address: delta.address.clone(),
                quantity: token_amount,
                utxo: created.utxo,
                block_number: block_info.number,
                block_hash: block_info.hash,
                tx_index: delta.tx_identifier.tx_index() as u32,
                block_timestamp: block_info.timestamp as i64,
            };

            block_created_utxos.insert(created.utxo);
            cnight_creations.push(creation);
        }
    }

    fn collect_cnight_spends(
        &self,
        delta: &ExtendedAddressDelta,
        block_info: &BlockInfo,
        cnight_creations: &HashSet<UTxOIdentifier>,
    ) -> Vec<(UTxOIdentifier, CNightSpend)> {
        delta
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
                            tx_index: delta.tx_identifier.tx_index() as u32,
                            block_timestamp: block_info.timestamp as i64,
                        },
                    ))
                } else {
                    None
                }
            })
            .collect()
    }

    fn collect_candidate_registrations(
        &self,
        delta: &ExtendedAddressDelta,
        block_info: &BlockInfo,
        registrations: &mut Vec<RegistrationEvent>,
        block_created_registrations: &mut HashSet<UTxOIdentifier>,
    ) {
        if delta.address != self.config.mapping_validator_address {
            return;
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
                    block_hash: block_info.hash,
                    block_timestamp: block_info.to_naive_datetime(),
                    tx_index: delta.tx_identifier.tx_index() as u32,
                    tx_hash: created.utxo.tx_hash,
                    utxo_index: created.utxo.output_index,
                    datum: datum.clone(),
                });
            }
        }
    }

    fn collect_candidate_deregistrations(
        &self,
        delta: &ExtendedAddressDelta,
        block_info: &BlockInfo,
        block_created_registrations: &HashSet<UTxOIdentifier>,
    ) -> Vec<DeregistrationEvent> {
        let mut deregistrations = Vec::new();

        if delta.address != self.config.mapping_validator_address {
            return deregistrations;
        }

        for spent in &delta.spent_utxos {
            if self.candidates.registration_index.contains_key(&spent.utxo)
                || block_created_registrations.contains(&spent.utxo)
            {
                deregistrations.push(DeregistrationEvent {
                    registration_utxo: spent.utxo,
                    spent_block_hash: block_info.hash,
                    spent_block_timestamp: block_info.to_naive_datetime(),
                    spent_tx_hash: spent.spent_by,
                    spent_tx_index: delta.tx_identifier.tx_index() as u32,
                });
            }
        }

        deregistrations
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use acropolis_common::{
        Address, AssetName, BlockHash, BlockInfo, BlockIntent, BlockStatus, CreatedUTxOExtended,
        Datum, Era, ExtendedAddressDelta, PolicyId, ShelleyAddress, SpentUTxOExtended, TxHash,
        TxIdentifier, UTxOIdentifier, ValueMap,
    };

    use crate::{configuration::MidnightConfig, state::State};

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

    #[test]
    fn collects_cnight_creations_and_spends_when_token_present() {
        let block_info = test_block_info();
        let policy = PolicyId::new([1u8; 28]);
        let asset = AssetName::new(b"").unwrap();

        let mut state = State::new(test_config_cnight(policy, asset));

        let token_value_5 = test_value_with_token(policy, asset, 5);
        let token_value_10 = test_value_with_token(policy, asset, 10);
        let no_token_value = test_value_no_token();

        // Creation delta
        let create_delta = ExtendedAddressDelta {
            address: Address::default(),
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
        state.collect_cnight_creations(
            &create_delta,
            &block_info,
            &mut creations,
            &mut creations_set,
        );
        assert_eq!(creations.len(), 2);
        assert_eq!(creations[0].quantity, 5);

        // Index the CNightCreation
        state.utxos.add_created_utxos(block_info.number, creations);

        // Retrieve the UTxO from state using the getter
        let utxos = state.utxos.get_asset_creates(block_info.number, block_info.number).unwrap();
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

        let spends = state.collect_cnight_spends(&spend_delta, &block_info, &HashSet::new());
        assert_eq!(spends.len(), 1);
        assert_eq!(*spends[0].1.tx_hash, [2u8; 32]);

        // Index the CNightSpend
        state.utxos.add_spent_utxos(block_info.number, spends).unwrap();

        // Retrieve the UTxO from state using the getter
        let utxos = state.utxos.get_asset_spends(block_info.number, block_info.number).unwrap();
        assert_eq!(utxos.len(), 1);
        assert_eq!(utxos[0].quantity, 10);
    }

    #[test]
    fn collects_candidate_registrations_and_deregistrations() {
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
        state.collect_candidate_registrations(
            &registration_delta,
            &block_info,
            &mut registrations,
            &mut block_created_registrations,
        );
        assert_eq!(registrations.len(), 1);

        state.candidates.register_candidates(block_info.number, registrations);

        let indexed = state.candidates.get_registrations(block_info.number, block_info.number);

        assert_eq!(indexed.len(), 1);
        assert_eq!(indexed[0].full_datum, Datum::Inline(vec![3]));
        assert_eq!(indexed[0].block_number, block_info.number);
        assert_eq!(indexed[0].block_hash, block_info.hash);
        assert_eq!(indexed[0].block_timestamp, block_info.to_naive_datetime());
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

        let deregistrations = state.collect_candidate_deregistrations(
            &deregistration_delta,
            &block_info,
            &HashSet::new(),
        );
        assert_eq!(deregistrations.len(), 1);

        state.candidates.deregister_candidates(block_info.number, deregistrations);

        let indexed = state.candidates.get_deregistrations(block_info.number, block_info.number);

        // Only 1 deregistration indexed
        assert_eq!(indexed.len(), 1);

        // All fields match
        assert_eq!(indexed[0].full_datum, Datum::Inline(vec![3]));
        assert_eq!(indexed[0].block_number, block_info.number);
        assert_eq!(indexed[0].block_hash, block_info.hash);
        assert_eq!(indexed[0].block_timestamp, block_info.to_naive_datetime());
        assert_eq!(indexed[0].tx_index_in_block, 2);
        assert_eq!(indexed[0].tx_hash, TxHash::new([3u8; 32]));
        assert_eq!(indexed[0].utxo_tx_hash, TxHash::new([1u8; 32]));
        assert_eq!(indexed[0].utxo_index, 1);
    }
}
