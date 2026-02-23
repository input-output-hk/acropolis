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
    types::{CNightCreation, CNightSpend},
};

#[derive(Clone, Default)]
pub struct State {
    // Epoch aggregate emitted as telemetry when crossing an epoch boundary.
    epoch_totals: EpochTotals,

    // CNight UTxO spends and creations indexed by block
    utxos: CNightUTxOState,
    // Candidate (Node operator) sets by epoch and registrations/deregistrations by block
    _candidates: CandidateState,
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

    pub fn finalise_block(&mut self, block: &BlockInfo) {
        self.epoch_totals.finalise_block(block);
    }

    pub fn handle_address_deltas(
        &mut self,
        block_info: &BlockInfo,
        address_deltas: &AddressDeltasMessage,
    ) -> Result<()> {
        let deltas = address_deltas.as_extended_deltas()?;

        let mut cnight_creations = Vec::new();
        let mut block_created_utxos: HashSet<UTxOIdentifier> = HashSet::new();
        let mut cnight_spends = Vec::new();
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
            ))
        }

        // Add created and spent CNight utxos to state
        let indexed_night_creations = if !cnight_creations.is_empty() {
            self.utxos.add_created_utxos(block_info.number, cnight_creations)
        } else {
            0
        };
        let indexed_night_spends = if !cnight_spends.is_empty() {
            self.utxos.add_spent_utxos(block_info.number, cnight_spends)?
        } else {
            0
        };
        self.epoch_totals.add_indexed_night_utxos(indexed_night_creations, indexed_night_spends);
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
                block_timestamp: block_info.to_naive_datetime(),
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
                            block_timestamp: block_info.to_naive_datetime(),
                        },
                    ))
                } else {
                    None
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use acropolis_common::{
        Address, AssetName, BlockHash, BlockInfo, BlockIntent, BlockStatus, CreatedUTxOExtended,
        Era, ExtendedAddressDelta, PolicyId, SpentUTxOExtended, TxHash, TxIdentifier,
        UTxOIdentifier, ValueMap,
    };
    use chrono::NaiveDateTime;

    use crate::{
        configuration::MidnightConfig,
        state::State,
        types::{CNightCreation, UTxOMeta},
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

    fn test_config(policy: PolicyId, asset: AssetName) -> MidnightConfig {
        MidnightConfig {
            cnight_policy_id: policy,
            cnight_asset_name: asset,
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
    fn collects_cnight_creation_when_token_present() {
        let block_info = test_block_info();
        let policy = PolicyId::new([1u8; 28]);
        let asset = AssetName::new(b"").unwrap();

        let mut state = State::new(test_config(policy, asset));

        let token_value_5 = test_value_with_token(policy, asset, 5);
        let token_value_10 = test_value_with_token(policy, asset, 10);
        let no_token_value = test_value_no_token();

        let delta = ExtendedAddressDelta {
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
        state.collect_cnight_creations(&delta, &block_info, &mut creations, &mut creations_set);
        assert_eq!(creations.len(), 2);
        assert_eq!(creations[0].quantity, 5);

        // Index the CNightCreation
        state.utxos.add_created_utxos(block_info.number, creations);

        // Retrieve the UTxO from state using the getter
        let utxos = state.utxos.get_asset_creates(block_info.number, block_info.number).unwrap();
        assert_eq!(utxos.len(), 2);
        assert_eq!(utxos[0].quantity, 5);
        assert_eq!(utxos[1].quantity, 10);
    }

    #[test]
    fn collects_cnight_spend_when_token_present() {
        let block_info = test_block_info();
        let policy = PolicyId::new([1u8; 28]);
        let asset = AssetName::new(b"").unwrap();

        let mut state = State::new(test_config(policy, asset));

        // Preseed the utxo_index with a UTxO creation
        state.utxos.utxo_index.insert(
            UTxOIdentifier {
                tx_hash: TxHash::default(),
                output_index: 2,
            },
            UTxOMeta {
                creation: CNightCreation {
                    address: Address::None,
                    quantity: 5,
                    utxo: UTxOIdentifier {
                        tx_hash: TxHash::default(),
                        output_index: 2,
                    },
                    block_number: 5,
                    block_hash: BlockHash::default(),
                    block_timestamp: NaiveDateTime::default(),
                    tx_index: 50,
                },
                spend: None,
            },
        );

        let delta = ExtendedAddressDelta {
            address: Address::default(),
            tx_identifier: TxIdentifier::default(),
            created_utxos: vec![],
            spent_utxos: vec![
                SpentUTxOExtended {
                    spent_by: TxHash::default(),
                    utxo: UTxOIdentifier::new(TxHash::default(), 1),
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

        // Collect the CNight UTxO spends
        let spends = state.collect_cnight_spends(&delta, &block_info, &HashSet::new());
        assert_eq!(spends.len(), 1);
        assert_eq!(*spends[0].1.tx_hash, [2u8; 32]);

        // Index the CNightSpend
        state.utxos.add_spent_utxos(block_info.number, spends).unwrap();

        // Retrieve the UTxO from state using the getter
        let utxos = state.utxos.get_asset_spends(block_info.number, block_info.number).unwrap();
        assert_eq!(utxos.len(), 1);
        assert_eq!(utxos[0].quantity, 5);
    }
}
