use anyhow::{anyhow, Result};
use std::collections::{BTreeMap, HashMap};

use acropolis_common::{BlockNumber, UTxOIdentifier};

use crate::types::{AssetCreate, AssetSpend, CNightCreation, CNightSpend, UTxOMeta};

#[derive(Clone, Default)]
pub struct CNightUTxOState {
    // Created UTxOs receiving CNight indexed by block
    pub created_utxos: BTreeMap<BlockNumber, Vec<UTxOIdentifier>>,
    // Spent UTxOs sending CNight indexed by block
    pub spent_utxos: BTreeMap<BlockNumber, Vec<UTxOIdentifier>>,
    // An index mapping UTxO identifiers to their corresponding metadata
    pub utxo_index: HashMap<UTxOIdentifier, UTxOMeta>,
}

impl CNightUTxOState {
    #[allow(dead_code)]
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

    #[allow(dead_code)]
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
    pub fn get_asset_creates(&self, start: BlockNumber, end: BlockNumber) -> Vec<AssetCreate> {
        self.created_utxos
            .range(start..=end)
            .flat_map(|(_, utxos)| {
                utxos.iter().map(|utxo_id| {
                    let meta = self
                        .utxo_index
                        .get(utxo_id)
                        .expect("UTxO index out of sync with created_utxos");

                    AssetCreate {
                        block_number: meta.creation.block_number,
                        block_hash: meta.creation.block_hash,
                        block_timestamp: meta.creation.block_timestamp,
                        tx_index_in_block: meta.creation.tx_index,
                        quantity: meta.creation.quantity,
                        holder_address: meta.creation.address.clone(),
                        tx_hash: meta.creation.utxo.tx_hash,
                        utxo_index: meta.creation.utxo.output_index,
                    }
                })
            })
            .collect()
    }

    #[allow(dead_code)]
    /// Get the CNight UTxO spends within a specified block range
    pub fn get_asset_spends(&self, start: BlockNumber, end: BlockNumber) -> Vec<AssetSpend> {
        self.spent_utxos
            .range(start..=end)
            .flat_map(|(_, utxos)| {
                utxos.iter().map(|utxo_id| {
                    let meta = self
                        .utxo_index
                        .get(utxo_id)
                        .expect("UTxO index out of sync with spent_utxos");

                    let spend =
                        meta.spend.as_ref().expect("UTxO index out of sync with spent_utxos");
                    AssetSpend {
                        block_number: spend.block_number,
                        block_hash: spend.block_hash,
                        block_timestamp: spend.block_timestamp,
                        tx_index_in_block: spend.tx_index,
                        quantity: meta.creation.quantity,
                        holder_address: meta.creation.address.clone(),
                        utxo_tx_hash: meta.creation.utxo.tx_hash,
                        utxo_index: meta.creation.utxo.output_index,
                        spending_tx_hash: spend.tx_hash,
                    }
                })
            })
            .collect()
    }
}
