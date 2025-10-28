//! Acropolis epoch activity counter: state storage

use acropolis_common::{
    crypto::keyhash_224,
    genesis_values::GenesisValues,
    messages::{BlockTxsMessage, EpochActivityMessage, ProtocolParamsMessage},
    params::EPOCH_LENGTH,
    protocol_params::{Nonces, PraosParams},
    BlockHash, BlockInfo, KeyHash,
};
use anyhow::Result;
use imbl::HashMap;
use pallas::ledger::traverse::MultiEraHeader;
use tracing::info;

#[derive(Default, Debug, Clone)]
pub struct State {
    // block number
    block: u64,

    // epoch number N
    epoch: u64,

    // epoch start time
    // UNIX timestamp
    epoch_start_time: u64,

    // first block time
    // UNIX timestamp
    first_block_time: u64,

    // first block height
    first_block_height: u64,

    // last block time
    // UNIX timestamp
    last_block_time: u64,

    // last block height
    last_block_height: u64,

    // Map of counts by VRF key hashes
    blocks_minted: HashMap<KeyHash, usize>,

    // blocks seen this epoch
    epoch_blocks: usize,

    // transactions seen this epoch
    epoch_txs: u64,

    // total outputs of all tx seen in this epoch
    epoch_outputs: u128,

    // fees seen this epoch
    epoch_fees: u64,

    // nonces will be set starting from Shelly Era
    nonces: Option<Nonces>,

    // protocol parameter for Praos and TPraos
    praos_params: Option<PraosParams>,
}

impl State {
    // Constructor
    pub fn new(genesis: &GenesisValues) -> Self {
        Self {
            block: 0,
            epoch: 0,
            epoch_start_time: genesis.byron_timestamp,
            first_block_time: genesis.byron_timestamp,
            first_block_height: 0,
            last_block_time: 0,
            last_block_height: 0,
            blocks_minted: HashMap::new(),
            epoch_blocks: 0,
            epoch_txs: 0,
            epoch_outputs: 0,
            epoch_fees: 0,
            nonces: None,
            praos_params: None,
        }
    }

    /// Handle protocol parameters updates
    pub fn handle_protocol_parameters(&mut self, msg: &ProtocolParamsMessage) {
        if let Some(shelly_params) = msg.params.shelley.as_ref() {
            self.praos_params = Some(shelly_params.into());
        }
    }

    // Handle a block header
    pub fn handle_block_header(
        &mut self,
        genesis: &GenesisValues,
        block_info: &BlockInfo,
        header: &MultiEraHeader,
    ) -> Result<()> {
        let new_epoch = block_info.new_epoch;

        // update nonces starting from Shelley Era
        if block_info.epoch >= genesis.shelley_epoch {
            let Some(praos_params) = self.praos_params.as_ref() else {
                return Err(anyhow::anyhow!("Praos Param is not set"));
            };

            // if Shelley Era's first epoch
            if new_epoch && block_info.epoch == genesis.shelley_epoch {
                self.nonces = Some(Nonces::shelley_genesis_nonces(genesis));
            }

            // current nonces must be set
            let Some(current_nonces) = self.nonces.as_ref() else {
                return Err(anyhow::anyhow!(
                    "Current Nonces are not set after Shelley Era"
                ));
            };

            // check for stability window
            let is_within_stability_window = Nonces::randomness_stability_window(
                block_info.era,
                header.slot(),
                genesis,
                praos_params,
            );

            // extract header's nonce vrf output
            let Some(nonce_vrf_output) = header.nonce_vrf_output().ok() else {
                return Err(anyhow::anyhow!("Header Nonce VRF output error"));
            };

            // Compute the new evolving nonce by combining it with the current one and the header's VRF
            // output.
            let evolving = Nonces::evolve(&current_nonces.evolving, &nonce_vrf_output)?;

            // there must be parent hash
            let Some(parent_hash) = header.previous_hash().map(|h| BlockHash::new(*h)) else {
                return Err(anyhow::anyhow!("Header Parent hash error"));
            };

            let new_nonces = Nonces {
                epoch: block_info.epoch,
                evolving: evolving.clone(),
                // On epoch changes, compute the new active nonce by combining:
                //   1. the (now stable) candidate; and
                //   2. the previous epoch's last block's parent header hash.
                //
                // If the epoch hasn't changed, then our active nonce is unchanged.
                active: if new_epoch {
                    Nonces::from_candidate(&current_nonces.candidate, &current_nonces.prev_lab)?
                } else {
                    current_nonces.active.clone()
                },
                // Unless we are within the randomness stability window, we also update the candidate. This
                // means that outside of the stability window, we always have:
                //
                //   evolving == candidate
                //
                // They only diverge for the last blocks of each epoch; The candidate remains stable while
                // the rolling nonce keeps evolving in preparation of the next epoch. Another way to look
                // at it is to think that there's always an entire epoch length contributing to the nonce
                // randomness, but it spans over two epochs.
                candidate: if is_within_stability_window {
                    evolving.clone()
                } else {
                    current_nonces.candidate.clone()
                },
                // Last Applied Block is the Header's Prev hash.
                lab: parent_hash.into(),
                // Previous LAB stay same during epoch
                // only Epoch's Boundary, will be last block's Previous Epoch's LAB
                prev_lab: if new_epoch {
                    current_nonces.lab.clone()
                } else {
                    current_nonces.prev_lab.clone()
                },
            };

            self.nonces = Some(new_nonces);
        };

        Ok(())
    }

    // Handle mint
    // This will update last block time
    pub fn handle_mint(&mut self, block_info: &BlockInfo, issuer_vkey: &[u8]) {
        self.last_block_time = block_info.timestamp;
        self.last_block_height = block_info.number;
        self.epoch_blocks += 1;
        let spo_id = keyhash_224(issuer_vkey);

        // Count one on this hash
        *(self.blocks_minted.entry(spo_id.clone()).or_insert(0)) += 1;
    }

    // Handle Block Txs
    pub fn handle_block_txs(&mut self, _block_info: &BlockInfo, msg: &BlockTxsMessage) {
        self.epoch_fees += msg.total_fees;
        self.epoch_txs += msg.total_txs;
        self.epoch_outputs += msg.total_output;
    }

    // Handle end of epoch, returns message to be published
    // block is the first block of coming epoch
    pub fn end_epoch(&mut self, block_info: &BlockInfo) -> EpochActivityMessage {
        info!(
            epoch = block_info.epoch - 1,
            blocks = self.epoch_blocks,
            unique_spo_ids = self.blocks_minted.len(),
            fees = self.epoch_fees,
            outputs = self.epoch_outputs,
            txs = self.epoch_txs,
            "End of epoch"
        );

        // set epoch end time
        let epoch_activity = self.get_epoch_info();

        // clear epoch state for new epoch
        self.block = block_info.number;
        self.epoch = block_info.epoch;
        self.epoch_start_time = block_info.timestamp;
        self.first_block_time = block_info.timestamp;
        self.first_block_height = block_info.number;
        self.last_block_time = block_info.timestamp;
        self.last_block_height = block_info.number;
        self.blocks_minted.clear();
        self.epoch_blocks = 0;
        self.epoch_txs = 0;
        self.epoch_outputs = 0;
        self.epoch_fees = 0;

        epoch_activity
    }

    pub fn get_epoch_info(&self) -> EpochActivityMessage {
        EpochActivityMessage {
            epoch: self.epoch,
            epoch_start_time: self.epoch_start_time,
            epoch_end_time: self.epoch_start_time + EPOCH_LENGTH,
            first_block_time: self.first_block_time,
            first_block_height: self.first_block_height,
            last_block_time: self.last_block_time,
            last_block_height: self.last_block_height,
            // NOTE:
            // total_blocks will be missing one
            // This is only because we now ignore EBBs
            total_blocks: self.epoch_blocks,
            total_txs: self.epoch_txs,
            total_outputs: self.epoch_outputs,
            total_fees: self.epoch_fees,
            spo_blocks: self.blocks_minted.iter().map(|(k, v)| (k.clone(), *v)).collect(),
            nonce: self.nonces.as_ref().and_then(|n| n.active.hash),
        }
    }

    pub fn get_latest_epoch_blocks_minted_by_pool(&self, spo_id: &KeyHash) -> u64 {
        self.blocks_minted.get(spo_id).map(|v| *v as u64).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use acropolis_common::{
        crypto::keyhash_224,
        protocol_params::{Nonce, NonceHash},
        state_history::{StateHistory, StateHistoryStore},
        BlockHash, BlockInfo, BlockStatus, Era,
    };
    use tokio::sync::Mutex;

    fn make_block(epoch: u64) -> BlockInfo {
        BlockInfo {
            status: BlockStatus::Immutable,
            slot: 0,
            number: epoch * 10,
            hash: BlockHash::default(),
            epoch,
            epoch_slot: 99,
            new_epoch: false,
            timestamp: 99999,
            era: Era::Shelley,
        }
    }

    fn make_new_epoch_block(epoch: u64) -> BlockInfo {
        BlockInfo {
            status: BlockStatus::Immutable,
            slot: 0,
            number: epoch * 10,
            hash: BlockHash::default(),
            epoch,
            epoch_slot: 99,
            new_epoch: true,
            timestamp: 99999,
            era: Era::Shelley,
        }
    }

    fn make_rolled_back_block(epoch: u64) -> BlockInfo {
        BlockInfo {
            status: BlockStatus::RolledBack,
            slot: 0,
            number: epoch * 10,
            hash: BlockHash::default(),
            epoch,
            epoch_slot: 99,
            new_epoch: false,
            timestamp: 99999,
            era: Era::Conway,
        }
    }

    #[test]
    fn initial_state_is_zeroed() {
        let state = State::new(&GenesisValues::mainnet());
        assert_eq!(state.epoch_blocks, 0);
        assert_eq!(state.epoch_txs, 0);
        assert_eq!(state.epoch_outputs, 0);
        assert_eq!(state.epoch_fees, 0);
        assert!(state.blocks_minted.is_empty());
    }

    #[test]
    fn handle_mint_single_issuer_records_counts() {
        let mut state = State::new(&GenesisValues::mainnet());
        let issuer = b"issuer_key";
        let mut block = make_block(100);
        state.handle_mint(&block, issuer);
        block.number += 1;
        state.handle_mint(&block, issuer);

        assert_eq!(state.epoch_blocks, 2);
        assert_eq!(state.blocks_minted.len(), 1);
        assert_eq!(state.blocks_minted.get(&keyhash_224(issuer)), Some(&2));
    }

    #[test]
    fn handle_mint_multiple_issuer_records_counts() {
        let mut state = State::new(&GenesisValues::mainnet());
        let mut block = make_block(100);
        state.handle_mint(&block, b"issuer_1");
        block.number += 1;
        state.handle_mint(&block, b"issuer_2");
        block.number += 1;
        state.handle_mint(&block, b"issuer_2");

        assert_eq!(state.epoch_blocks, 3);
        assert_eq!(state.blocks_minted.len(), 2);
        assert_eq!(
            state
                .blocks_minted
                .iter()
                .find(|(k, _)| *k == &keyhash_224(b"issuer_1"))
                .map(|(_, v)| *v),
            Some(1)
        );
        assert_eq!(
            state
                .blocks_minted
                .iter()
                .find(|(k, _)| *k == &keyhash_224(b"issuer_2"))
                .map(|(_, v)| *v),
            Some(2)
        );

        let blocks_minted = state.get_latest_epoch_blocks_minted_by_pool(&keyhash_224(b"issuer_2"));
        assert_eq!(blocks_minted, 2);
    }

    #[test]
    fn handle_block_txs_correctly() {
        let mut state = State::new(&GenesisValues::mainnet());
        let mut block = make_block(100);

        state.handle_block_txs(
            &block,
            &BlockTxsMessage {
                total_txs: 1,
                total_output: 100,
                total_fees: 100,
            },
        );
        block.number += 1;
        state.handle_block_txs(
            &block,
            &BlockTxsMessage {
                total_txs: 2,
                total_output: 250,
                total_fees: 250,
            },
        );

        assert_eq!(state.epoch_txs, 3);
        assert_eq!(state.epoch_outputs, 350);
        assert_eq!(state.epoch_fees, 350);
    }

    #[test]
    fn end_epoch_resets_and_returns_message() {
        let genesis = GenesisValues::mainnet();
        let mut state = State::new(&genesis);
        let block = make_block(1);
        state.handle_mint(&block, b"issuer_1");
        state.handle_block_txs(
            &block,
            &BlockTxsMessage {
                total_txs: 1,
                total_output: 123,
                total_fees: 123,
            },
        );

        // Check the message returned
        let ea = state.end_epoch(&block);
        assert_eq!(ea.epoch, 0);
        assert_eq!(ea.total_blocks, 1);
        assert_eq!(ea.total_txs, 1);
        assert_eq!(ea.total_outputs, 123);
        assert_eq!(ea.total_fees, 123);
        assert_eq!(ea.spo_blocks.len(), 1);
        assert_eq!(
            ea.spo_blocks.iter().find(|(k, _)| k == &keyhash_224(b"issuer_1")).map(|(_, v)| *v),
            Some(1)
        );
        assert_eq!(ea.epoch_start_time, genesis.byron_timestamp);
        assert_eq!(ea.epoch_end_time, genesis.byron_timestamp + EPOCH_LENGTH);
        assert_eq!(ea.first_block_time, genesis.byron_timestamp);
        assert_eq!(ea.last_block_time, block.timestamp);

        // State must be reset
        assert_eq!(state.epoch, 1);
        assert_eq!(state.epoch_blocks, 0);
        assert_eq!(state.epoch_txs, 0);
        assert_eq!(state.epoch_outputs, 0);
        assert_eq!(state.epoch_fees, 0);
        assert!(state.blocks_minted.is_empty());
        assert_eq!(state.epoch_start_time, block.timestamp);
        assert_eq!(state.first_block_time, block.timestamp);
        assert_eq!(state.first_block_height, block.number);
        assert_eq!(state.last_block_time, block.timestamp);
        assert_eq!(state.last_block_height, block.number);

        let blocks_minted = state.get_latest_epoch_blocks_minted_by_pool(&keyhash_224(b"vrf_1"));
        assert_eq!(blocks_minted, 0);
    }

    #[tokio::test]
    async fn state_is_rolled_back() {
        let history = Arc::new(Mutex::new(StateHistory::<State>::new(
            "epochs_state",
            StateHistoryStore::default_block_store(),
        )));
        let mut state = history.lock().await.get_current_state();
        let mut block = make_block(1);
        state.handle_mint(&block, b"issuer_1");
        state.handle_block_txs(
            &block,
            &BlockTxsMessage {
                total_txs: 1,
                total_output: 123,
                total_fees: 123,
            },
        );
        history.lock().await.commit(block.number, state);

        let mut state = history.lock().await.get_current_state();
        block.number += 1;
        state.handle_mint(&block, b"issuer_1");
        state.handle_block_txs(
            &block,
            &BlockTxsMessage {
                total_txs: 1,
                total_output: 123,
                total_fees: 123,
            },
        );
        assert_eq!(
            state.get_latest_epoch_blocks_minted_by_pool(&keyhash_224(b"issuer_1")),
            2
        );
        history.lock().await.commit(block.number, state);

        block = make_rolled_back_block(0);
        let mut state = history.lock().await.get_rolled_back_state(block.number);
        state.handle_mint(&block, b"issuer_2");
        state.handle_block_txs(
            &block,
            &BlockTxsMessage {
                total_txs: 1,
                total_output: 123,
                total_fees: 123,
            },
        );
        assert_eq!(
            state.get_latest_epoch_blocks_minted_by_pool(&keyhash_224(b"issuer_1")),
            0
        );
        assert_eq!(
            state.get_latest_epoch_blocks_minted_by_pool(&keyhash_224(b"issuer_2")),
            1
        );
        history.lock().await.commit(block.number, state);
    }

    #[test]
    fn test_epoch_208_nonce() {
        let mut state = State::new(&GenesisValues::mainnet());
        state.praos_params = Some(PraosParams::mainnet());
        let genesis_value = GenesisValues::mainnet();

        let e_208_first_block_header_cbor =
            hex::decode(include_str!("../data/4490511.cbor")).unwrap();
        let block = make_new_epoch_block(208);
        let block_header = MultiEraHeader::decode(1, None, &e_208_first_block_header_cbor).unwrap();
        assert!(state.handle_block_header(&genesis_value, &block, &block_header).is_ok());

        let nonces = state.nonces.unwrap();
        let evolved = Nonce::from(
            NonceHash::try_from(
                hex::decode("2af15f57076a8ff225746624882a77c8d2736fe41d3db70154a22b50af851246")
                    .unwrap()
                    .as_slice(),
            )
            .unwrap(),
        );
        assert!(nonces.active.eq(&Nonce::from(genesis_value.shelley_genesis_hash)));
        assert!(nonces.candidate.eq(&evolved));
        assert!(nonces.evolving.eq(&evolved));
        assert!(nonces.lab.eq(&Nonce::from(*block_header.previous_hash().unwrap())));
        assert!(nonces.prev_lab.eq(&Nonce::default()));
    }

    #[test]
    fn test_nonce_evolving() {
        let mut state = State::new(&GenesisValues::mainnet());
        state.praos_params = Some(PraosParams::mainnet());
        let genesis_value = GenesisValues::mainnet();

        let e_208_first_block_header_cbor =
            hex::decode(include_str!("../data/4490511.cbor")).unwrap();
        let e_208_second_block_header_cbor =
            hex::decode(include_str!("../data/4490512.cbor")).unwrap();
        let block = make_new_epoch_block(208);
        let block_header = MultiEraHeader::decode(1, None, &e_208_first_block_header_cbor).unwrap();
        assert!(state.handle_block_header(&genesis_value, &block, &block_header).is_ok());

        let block = make_block(208);
        let block_header =
            MultiEraHeader::decode(1, None, &e_208_second_block_header_cbor).unwrap();
        assert!(state.handle_block_header(&genesis_value, &block, &block_header).is_ok());

        let evolved = Nonce::from(
            NonceHash::try_from(
                hex::decode("a815ff978369b57df09b0072485c26920dc0ec8e924a852a42f0715981cf0042")
                    .unwrap()
                    .as_slice(),
            )
            .unwrap(),
        );
        let nonces = state.nonces.unwrap();
        assert!(nonces.active.eq(&Nonce::from(genesis_value.shelley_genesis_hash)));
        assert!(nonces.evolving.eq(&evolved));
        assert!(nonces.candidate.eq(&evolved));
        assert!(nonces.lab.eq(&Nonce::from(*block_header.previous_hash().unwrap())));
        assert!(nonces.prev_lab.eq(&Nonce::default()));
    }

    #[test]
    fn test_epoch_209_nonce() {
        let mut state = State::new(&GenesisValues::mainnet());
        state.praos_params = Some(PraosParams::mainnet());
        let genesis_value = GenesisValues::mainnet();
        let e_208_candidate = Nonce::from(
            NonceHash::try_from(
                hex::decode("ea98cb2dac7208296ac89030f24cdc0dc6fbfebc4bf1f5b7a8331ec47e3bb311")
                    .unwrap()
                    .as_slice(),
            )
            .unwrap(),
        );
        let e_208_lab = Nonce::from(
            NonceHash::try_from(
                hex::decode("dfc1d6e6dbce685b5cf85899c6e3c89539b081c62222265910423ced4096390a")
                    .unwrap()
                    .as_slice(),
            )
            .unwrap(),
        );
        state.nonces = Some(Nonces {
            epoch: 208,
            active: Nonce::from(genesis_value.shelley_genesis_hash),
            candidate: e_208_candidate.clone(),
            evolving: Nonce::from(
                NonceHash::try_from(
                    hex::decode("bd331d2334012dfd828a0cbdeb552368052af48b39f171b2b9343330924db6b1")
                        .unwrap()
                        .as_slice(),
                )
                .unwrap(),
            ),
            lab: e_208_lab.clone(),
            prev_lab: Nonce::default(),
        });

        let e_209_first_block_header_cbor =
            hex::decode(include_str!("../data/4512067.cbor")).unwrap();
        let block = make_new_epoch_block(209);
        let block_header = MultiEraHeader::decode(1, None, &e_209_first_block_header_cbor).unwrap();
        assert!(state.handle_block_header(&genesis_value, &block, &block_header).is_ok());

        let nonces = state.nonces.unwrap();
        let evolved = Nonce::from(
            NonceHash::try_from(
                hex::decode("5221b5541f5fc2a7eebd4316ff2f430b54709eeb1fe9ad7c30272d716656e601")
                    .unwrap()
                    .as_slice(),
            )
            .unwrap(),
        );
        assert!(nonces.active.eq(&e_208_candidate));
        assert!(nonces.evolving.eq(&evolved));
        assert!(nonces.candidate.eq(&evolved));
        assert!(nonces.lab.eq(&Nonce::from(*block_header.previous_hash().unwrap())));
        assert!(nonces.prev_lab.eq(&e_208_lab));
    }

    #[test]
    fn test_epoch_210_nonce() {
        let mut state = State::new(&GenesisValues::mainnet());
        state.praos_params = Some(PraosParams::mainnet());
        let genesis_value = GenesisValues::mainnet();
        let e_209_lab = Nonce::from(
            NonceHash::try_from(
                hex::decode("e5e914ba8c727baf3c3465ae6a62508186772eb20649aa7a99a637328f62803e")
                    .unwrap()
                    .as_slice(),
            )
            .unwrap(),
        );
        state.nonces = Some(Nonces {
            epoch: 209,
            active: Nonce::from(
                NonceHash::try_from(
                    hex::decode("ea98cb2dac7208296ac89030f24cdc0dc6fbfebc4bf1f5b7a8331ec47e3bb311")
                        .unwrap()
                        .as_slice(),
                )
                .unwrap(),
            ),
            candidate: Nonce::from(
                NonceHash::try_from(
                    hex::decode("a9543bc3820138abfaaad606d19c50df70c896336a88ab01da0eb34c1129bf31")
                        .unwrap()
                        .as_slice(),
                )
                .unwrap(),
            ),
            evolving: Nonce::from(
                NonceHash::try_from(
                    hex::decode("6a21c46c01aa5a9d840beec28dd201a0bc9fc144d3a48b485ed0a3790b276520")
                        .unwrap()
                        .as_slice(),
                )
                .unwrap(),
            ),
            lab: e_209_lab.clone(),
            prev_lab: Nonce::from(
                NonceHash::try_from(
                    hex::decode("dfc1d6e6dbce685b5cf85899c6e3c89539b081c62222265910423ced4096390a")
                        .unwrap()
                        .as_slice(),
                )
                .unwrap(),
            ),
        });

        let e_210_first_block_header_cbor =
            hex::decode(include_str!("../data/4533637.cbor")).unwrap();
        let block = make_new_epoch_block(209);
        let block_header = MultiEraHeader::decode(1, None, &e_210_first_block_header_cbor).unwrap();
        assert!(state.handle_block_header(&genesis_value, &block, &block_header).is_ok());

        let nonces = state.nonces.unwrap();
        let evolved = Nonce::from(
            NonceHash::try_from(
                hex::decode("2bc39f25e92a59b3e8044783560eac6dd8aba2e55b2b1aba132db58d5a1e7155")
                    .unwrap()
                    .as_slice(),
            )
            .unwrap(),
        );
        assert!(nonces.active.eq(&Nonce::from(
            NonceHash::try_from(
                hex::decode("ddf346732e6a47323b32e1e3eeb7a45fad678b7f533ef1f2c425e13c704ba7e3")
                    .unwrap()
                    .as_slice(),
            )
            .unwrap(),
        )));
        assert!(nonces.evolving.eq(&evolved));
        assert!(nonces.candidate.eq(&evolved));
        assert!(nonces.lab.eq(&Nonce::from(*block_header.previous_hash().unwrap())));
        assert!(nonces.prev_lab.eq(&e_209_lab));
    }
}
