use anyhow::{anyhow, Result};

use acropolis_common::{BlockNumber, UTxOIdentifier};
use imbl::{HashMap, OrdMap};

use crate::types::{AssetCreate, AssetSpend, CNightCreation, CNightSpend, UTxOMeta};

#[derive(Clone, Default)]
pub struct CNightUTxOState {
    // Created UTxOs receiving CNight indexed by block
    created_utxos: OrdMap<BlockNumber, Vec<UTxOIdentifier>>,
    // Spent UTxOs sending CNight indexed by block
    spent_utxos: OrdMap<BlockNumber, Vec<UTxOIdentifier>>,
    // An index mapping UTxO identifiers to their corresponding metadata
    pub utxo_index: HashMap<UTxOIdentifier, UTxOMeta>,
}

impl CNightUTxOState {
    /// Add the created UTxOs for one block to state and return count inserted.
    pub fn add_created_utxos(&mut self, block: BlockNumber, utxos: Vec<CNightCreation>) -> usize {
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

        let inserted = identifiers.len();
        self.created_utxos.insert(block, identifiers);
        inserted
    }

    /// Add the spent UTxOs for one block to state and return count inserted.
    pub fn add_spent_utxos(
        &mut self,
        block: BlockNumber,
        utxos: Vec<(UTxOIdentifier, CNightSpend)>,
    ) -> Result<usize> {
        let mut identifiers = Vec::with_capacity(utxos.len());

        for (identifier, spend) in utxos {
            if let Some(record) = self.utxo_index.get_mut(&identifier) {
                record.spend = Some(spend);
                identifiers.push(identifier);
            } else {
                return Err(anyhow!("UTxO spend without existing record"));
            }
        }

        let inserted = identifiers.len();
        self.spent_utxos.insert(block, identifiers);

        Ok(inserted)
    }

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

#[cfg(test)]
mod tests {
    use super::*;
    use acropolis_common::{Address, BlockHash, TxHash};

    fn test_creation(utxo: UTxOIdentifier) -> CNightCreation {
        CNightCreation {
            address: Address::default(),
            quantity: 42,
            utxo,
            block_number: 1,
            block_hash: BlockHash::default(),
            tx_index: 7,
            block_timestamp: 0,
        }
    }

    #[test]
    fn add_spent_utxos_fails_without_creation() {
        let mut state = CNightUTxOState::default();
        let utxo = UTxOIdentifier::new(TxHash::default(), 0);
        let spend = CNightSpend {
            block_number: 2,
            block_hash: BlockHash::default(),
            tx_hash: TxHash::default(),
            tx_index: 3,
            block_timestamp: 0,
        };

        let err = state
            .add_spent_utxos(2, vec![(utxo, spend)])
            .expect_err("expected missing creation to error");
        assert!(err.to_string().contains("UTxO spend without existing record"));
    }

    #[test]
    fn get_asset_spends_errors_when_spend_missing() {
        let mut state = CNightUTxOState::default();
        let utxo = UTxOIdentifier::new(TxHash::default(), 1);

        state.utxo_index.insert(
            utxo,
            UTxOMeta {
                creation: test_creation(utxo),
                spend: None,
            },
        );
        state.spent_utxos.insert(1, vec![utxo]);

        match state.get_asset_spends(1, 1) {
            Ok(_) => panic!("expected missing spend to error"),
            Err(err) => assert!(err.to_string().contains("UTxO has no spend record")),
        }
    }

    #[test]
    fn get_asset_creates_errors_when_creation_missing() {
        let mut state = CNightUTxOState::default();
        let utxo = UTxOIdentifier::new(TxHash::default(), 2);

        state.created_utxos.insert(1, vec![utxo]);

        match state.get_asset_creates(1, 1) {
            Ok(_) => panic!("expected missing creation to error"),
            Err(err) => assert!(err.to_string().contains("UTxO creation without existing record")),
        }
    }
}
