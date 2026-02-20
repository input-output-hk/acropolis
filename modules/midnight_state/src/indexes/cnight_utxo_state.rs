use anyhow::{anyhow, Result};
use std::collections::{BTreeMap, HashMap};

use acropolis_common::{BlockNumber, UTxOIdentifier};

use crate::types::{AssetCreate, AssetSpend, CNightCreation, CNightSpend, UTxOMeta};

#[derive(Clone, Default)]
pub struct CNightUTxOState {
    // Created UTxOs receiving CNight indexed by block
    created_utxos: BTreeMap<BlockNumber, Vec<UTxOIdentifier>>,
    // Spent UTxOs sending CNight indexed by block
    spent_utxos: BTreeMap<BlockNumber, Vec<UTxOIdentifier>>,
    // An index mapping UTxO identifiers to their corresponding metadata
    pub utxo_index: HashMap<UTxOIdentifier, UTxOMeta>,
}

impl CNightUTxOState {
    /// Add the created UTxOs for one block to state
    pub fn add_created_utxos(&mut self, block: BlockNumber, utxos: Vec<CNightCreation>) {
        let mut identifiers = Vec::with_capacity(utxos.len());

        for creation in utxos {
            identifiers.push(creation.utxo);
            self.utxo_index.insert(
                creation.utxo,
                UTxOMeta {
                    creation,
                    spend: None,
                },
            );
        }

        self.created_utxos.insert(block, identifiers);
    }

    /// Add the spent UTxOs for one block to state
    pub fn add_spent_utxos(
        &mut self,
        block: BlockNumber,
        utxos: Vec<(UTxOIdentifier, CNightSpend)>,
    ) -> Result<()> {
        let mut identifiers = Vec::with_capacity(utxos.len());

        for (identifier, spend) in utxos {
            if let Some(record) = self.utxo_index.get_mut(&identifier) {
                record.spend = Some(spend);
                identifiers.push(identifier);
            } else {
                return Err(anyhow!("UTxO spend without existing record"));
            }
        }

        self.spent_utxos.insert(block, identifiers);

        Ok(())
    }

    #[allow(dead_code)]
    /// Get the CNight UTxO creations within a specified block range
    pub fn get_asset_creates(
        &self,
        start: BlockNumber,
        end: BlockNumber,
    ) -> Result<Vec<AssetCreate>> {
        self.created_utxos
            .range(start..=end)
            .flat_map(|(_, utxos)| utxos.iter())
            .map(|utxo_id| AssetCreate::try_from(self.utxo_index.get(utxo_id)))
            .collect()
    }

    #[allow(dead_code)]
    /// Get the CNight UTxO spends within a specified block range
    pub fn get_asset_spends(
        &self,
        start: BlockNumber,
        end: BlockNumber,
    ) -> Result<Vec<AssetSpend>> {
        self.spent_utxos
            .range(start..=end)
            .flat_map(|(_, utxos)| utxos.iter())
            .map(|utxo_id| AssetSpend::try_from(self.utxo_index.get(utxo_id)))
            .collect()
    }
}
