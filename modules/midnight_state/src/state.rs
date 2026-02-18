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
    // Runtime-active in this PR: epoch totals observer used for logging summaries.
    epoch_totals: EpochTotals,

    // -----------------------------------------------------------------------
    // NOTE: Indexing scaffolding retained for follow-up work.
    // These fields are intentionally inactive in the runtime path for this PR.
    // -----------------------------------------------------------------------
    // CNight UTxO spends and creations indexed by block
    utxos: CNightUTxOState,
    // Candidate (Node operator) sets by epoch and registrations/deregistrations by block
    _candidates: CandidateState,
    // Governance indexed by block
    _governance: GovernanceState,
    // Parameters indexed by epoch
    parameters: ParametersState,
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

    /// Snapshot Ariadne parameters at epoch boundary
    pub fn handle_new_epoch(&mut self, block_info: &BlockInfo) -> Result<EpochSummary> {
        self.parameters.snapshot_parameters(block_info.epoch);
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
        let mut cnight_spends = Vec::new();
        for delta in deltas {
            // Collect CNight UTxO creations and spends for the block
            cnight_creations.append(&mut self.collect_cnight_creations(delta, block_info));
            cnight_spends.append(&mut self.collect_cnight_spends(delta, block_info))
        }

        // Add created and spent CNight utxos to state
        self.utxos.add_created_utxos(block_info.number, cnight_creations);
        self.utxos.add_spent_utxos(block_info.number, cnight_spends)?;
        Ok(())
    }

    fn collect_cnight_creations(
        &self,
        delta: &ExtendedAddressDelta,
        block_info: &BlockInfo,
    ) -> Vec<CNightCreation> {
        delta
            .created_utxos
            .iter()
            .filter_map(|created| {
                let token_amount = created.value.token_amount(
                    &self.config.cnight_policy_id,
                    &self.config.cnight_asset_name,
                );

                if token_amount > 0 {
                    Some(CNightCreation {
                        address: delta.address.clone(),
                        quantity: token_amount,
                        utxo: created.utxo,
                        block_number: block_info.number,
                        block_hash: block_info.hash,
                        tx_index: delta.tx_identifier.tx_index() as u32,
                        block_timestamp: block_info.to_naive_datetime(),
                    })
                } else {
                    None
                }
            })
            .collect()
    }

    fn collect_cnight_spends(
        &self,
        delta: &ExtendedAddressDelta,
        block_info: &BlockInfo,
    ) -> Vec<(UTxOIdentifier, CNightSpend)> {
        delta
            .spent_utxos
            .iter()
            .filter_map(|spent| {
                if self.utxos.utxo_index.contains_key(&spent.utxo) {
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
