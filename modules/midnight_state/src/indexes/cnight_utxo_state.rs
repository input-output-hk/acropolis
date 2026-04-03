use anyhow::{anyhow, Result};

use acropolis_common::{BlockNumber, UTxOIdentifier};
use imbl::{HashMap, OrdMap};

use crate::types::{AssetCreate, AssetSpend, CNightCreation, CNightSpend, UTxOMeta};

#[derive(Clone, Default, serde::Serialize)]
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
        start_tx_index: u32,
        utxo_capacity: usize,
    ) -> Result<Vec<AssetCreate>> {
        self.created_utxos
            .range(start..)
            .flat_map(|(block_number, utxos)| {
                utxos.iter().filter_map(move |utxo_id| {
                    let utxo = self.utxo_index.get(utxo_id)?;

                    if *block_number == start && utxo.creation.tx_index < start_tx_index {
                        return None;
                    }

                    Some(utxo)
                })
            })
            .take(utxo_capacity)
            .map(AssetCreate::try_from)
            .collect()
    }

    /// Get the CNight UTxO spends within a specified block range
    pub fn get_asset_spends(
        &self,
        start: BlockNumber,
        start_tx_index: u32,
        utxo_capacity: usize,
    ) -> Result<Vec<AssetSpend>> {
        self.spent_utxos
            .range(start..)
            .flat_map(|(block_number, utxos)| {
                utxos.iter().filter_map(move |utxo_id| {
                    let utxo = self.utxo_index.get(utxo_id)?;
                    let spend = utxo.spend.as_ref()?;

                    if *block_number == start && spend.tx_index < start_tx_index {
                        return None;
                    }

                    Some(utxo)
                })
            })
            .take(utxo_capacity)
            .map(AssetSpend::try_from)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use acropolis_common::{BlockHash, NetworkId, StakeAddress, StakeCredential, TxHash};

    fn id(i: u8) -> UTxOIdentifier {
        UTxOIdentifier::new(TxHash::from([i; 32]), 0)
    }

    fn creation(utxo: UTxOIdentifier, tx_index: u32) -> CNightCreation {
        CNightCreation {
            holder_address: StakeAddress::new(
                StakeCredential::AddrKeyHash([7u8; 28].into()),
                NetworkId::Testnet,
            ),
            quantity: 42,
            utxo,
            block_number: 10,
            block_hash: BlockHash::default(),
            tx_index,
            block_timestamp: 0,
        }
    }

    fn spend(tx_index: u32) -> CNightSpend {
        CNightSpend {
            block_number: 10,
            block_hash: BlockHash::default(),
            tx_hash: TxHash::default(),
            tx_index,
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
    fn get_asset_spends_ignores_missing_spend() {
        let mut state = CNightUTxOState::default();

        let utxo = id(1);

        state.utxo_index.insert(
            utxo,
            UTxOMeta {
                creation: creation(utxo, 0),
                spend: None,
            },
        );

        state.spent_utxos.insert(1, vec![utxo]);

        let result = state.get_asset_spends(1, 0, 10).unwrap();

        assert!(result.is_empty());
    }

    #[test]
    fn get_asset_creates_ignores_missing_creation() {
        let mut state = CNightUTxOState::default();

        let utxo = id(2);

        state.created_utxos.insert(1, vec![utxo]);

        let result = state.get_asset_creates(1, 0, 10).unwrap();

        assert!(result.is_empty());
    }

    #[test]
    fn asset_creates_returns_entries_at_or_after_start_tx_index() {
        let mut state = CNightUTxOState::default();

        let id0 = id(0);
        let id1 = id(1);
        let id2 = id(2);

        state.utxo_index.insert(
            id0,
            UTxOMeta {
                creation: creation(id0, 0),
                spend: None,
            },
        );

        state.utxo_index.insert(
            id1,
            UTxOMeta {
                creation: creation(id1, 1),
                spend: None,
            },
        );

        state.utxo_index.insert(
            id2,
            UTxOMeta {
                creation: creation(id2, 2),
                spend: None,
            },
        );

        state.created_utxos.insert(10, vec![id0, id1, id2]);

        let result = state.get_asset_creates(10, 1, 10).unwrap();

        let txs: Vec<u32> = result.iter().map(|r| r.tx_index_in_block).collect();

        assert_eq!(txs, vec![1, 2]);
    }

    #[test]
    fn asset_creates_limits_to_capacity() {
        let mut state = CNightUTxOState::default();

        let ids = [id(1), id(2), id(3), id(4)];

        for (i, identifier) in ids.iter().enumerate() {
            state.utxo_index.insert(
                *identifier,
                UTxOMeta {
                    creation: creation(*identifier, i as u32),
                    spend: None,
                },
            );
        }

        state.created_utxos.insert(10, ids.to_vec());

        let result = state.get_asset_creates(10, 0, 2).unwrap();

        assert_eq!(result.len(), 2);
    }

    #[test]
    fn asset_spends_returns_entries_at_or_after_start_tx_index() {
        let mut state = CNightUTxOState::default();

        let id0 = id(0);
        let id1 = id(1);
        let id2 = id(2);

        state.utxo_index.insert(
            id0,
            UTxOMeta {
                creation: creation(id0, 0),
                spend: Some(spend(0)),
            },
        );

        state.utxo_index.insert(
            id1,
            UTxOMeta {
                creation: creation(id1, 0),
                spend: Some(spend(1)),
            },
        );

        state.utxo_index.insert(
            id2,
            UTxOMeta {
                creation: creation(id2, 0),
                spend: Some(spend(2)),
            },
        );

        state.spent_utxos.insert(10, vec![id0, id1, id2]);

        let result = state.get_asset_spends(10, 1, 10).unwrap();

        let txs: Vec<u32> = result.iter().map(|r| r.tx_index_in_block).collect();

        assert_eq!(txs, vec![1, 2]);
    }

    #[test]
    fn asset_spends_limits_to_capacity() {
        let mut state = CNightUTxOState::default();

        let id0 = id(0);
        let id1 = id(1);
        let id2 = id(2);

        state.utxo_index.insert(
            id0,
            UTxOMeta {
                creation: creation(id0, 0),
                spend: Some(spend(0)),
            },
        );

        state.utxo_index.insert(
            id1,
            UTxOMeta {
                creation: creation(id1, 0),
                spend: Some(spend(1)),
            },
        );

        state.utxo_index.insert(
            id2,
            UTxOMeta {
                creation: creation(id2, 0),
                spend: Some(spend(2)),
            },
        );

        state.spent_utxos.insert(10, vec![id0, id1, id2]);

        let result = state.get_asset_spends(10, 0, 2).unwrap();

        assert_eq!(result.len(), 2);
    }
}
