use std::{collections::HashMap, path::Path, sync::Arc};

use acropolis_common::{BlockInfo, TxHash};
use anyhow::{bail, Result};
use config::Config;
use fjall::{Batch, Keyspace, Partition};

pub struct FjallStore {
    keyspace: Keyspace,
    blocks: FjallBlockStore,
    txs: FjallTXStore,
}

const DEFAULT_DATABASE_PATH: &str = "fjall-blocks";
const BLOCKS_PARTITION: &str = "blocks";
const BLOCK_HASHES_BY_SLOT_PARTITION: &str = "block-hashes-by-slot";
const BLOCK_HASHES_BY_NUMBER_PARTITION: &str = "block-hashes-by-number";
const TXS_PARTITION: &str = "txs";

impl FjallStore {
    pub fn new(config: Arc<Config>) -> Result<Self> {
        let path = config.get_string("database-path").unwrap_or(DEFAULT_DATABASE_PATH.to_string());
        let fjall_config = fjall::Config::new(Path::new(&path));
        let keyspace = fjall_config.open()?;
        let blocks = FjallBlockStore::new(&keyspace)?;
        let txs = FjallTXStore::new(&keyspace)?;
        Ok(Self {
            keyspace,
            blocks,
            txs,
        })
    }
}

impl super::Store for FjallStore {
    fn insert_block(&self, block: &super::Block) -> Result<()> {
        let mut txs_with_hash = Vec::with_capacity(block.txs.len());
        let mut persisted_block = PersistedBlock {
            slot: block.slot,
            number: block.number,
            hash: block.hash.clone(),
            tx_hashes: vec![],
        };
        for tx in &block.txs {
            let hash = super::hash_tx(tx)?;
            persisted_block.tx_hashes.push(hash);
            txs_with_hash.push((tx, hash));
        }

        let mut batch = self.keyspace.batch();
        self.blocks.insert(&mut batch, &persisted_block);
        for (tx, hash) in txs_with_hash {
            self.txs.insert_tx(&mut batch, tx, hash);
        }

        batch.commit()?;

        Ok(())
    }

    fn get_block_by_hash(&self, hash: &[u8]) -> Result<super::Block> {
        let block = self.blocks.get_by_hash(hash)?;
        self.txs.hydrate_block(block)
    }

    fn get_block_by_slot(&self, slot: u64) -> Result<super::Block> {
        let block = self.blocks.get_by_slot(slot)?;
        self.txs.hydrate_block(block)
    }

    fn get_block_by_number(&self, number: u64) -> Result<super::Block> {
        let block = self.blocks.get_by_number(number)?;
        self.txs.hydrate_block(block)
    }

    fn get_latest_block(&self) -> Result<super::Block> {
        let block = self.blocks.get_latest()?;
        self.txs.hydrate_block(block)
    }
}

struct FjallBlockStore {
    blocks: Partition,
    block_hashes_by_slot: Partition,
    block_hashes_by_number: Partition,
}

impl FjallBlockStore {
    fn new(keyspace: &Keyspace) -> Result<Self> {
        let blocks =
            keyspace.open_partition(BLOCKS_PARTITION, fjall::PartitionCreateOptions::default())?;
        let block_hashes_by_slot = keyspace.open_partition(
            BLOCK_HASHES_BY_SLOT_PARTITION,
            fjall::PartitionCreateOptions::default(),
        )?;
        let block_hashes_by_number = keyspace.open_partition(
            BLOCK_HASHES_BY_NUMBER_PARTITION,
            fjall::PartitionCreateOptions::default(),
        )?;
        Ok(Self {
            blocks,
            block_hashes_by_slot,
            block_hashes_by_number,
        })
    }

    fn insert(&self, batch: &mut Batch, block: &PersistedBlock) {
        let encoded = {
            let mut bytes = vec![];
            minicbor::encode(block, &mut bytes).expect("infallible");
            bytes
        };
        batch.insert(&self.blocks, &block.hash, encoded);
        batch.insert(
            &self.block_hashes_by_slot,
            block.slot.to_be_bytes(),
            &block.hash,
        );
        batch.insert(
            &self.block_hashes_by_number,
            block.number.to_be_bytes(),
            &block.hash,
        );
    }

    fn get_by_hash(&self, hash: &[u8]) -> Result<PersistedBlock> {
        let Some(block) = self.blocks.get(hash)? else {
            bail!("No block found with hash {}", hex::encode(hash));
        };
        Ok(minicbor::decode(&block)?)
    }

    fn get_by_slot(&self, slot: u64) -> Result<PersistedBlock> {
        let Some(hash) = self.block_hashes_by_slot.get(slot.to_be_bytes())? else {
            bail!("No block found for slot {slot}");
        };
        self.get_by_hash(&hash)
    }

    fn get_by_number(&self, number: u64) -> Result<PersistedBlock> {
        let Some(hash) = self.block_hashes_by_number.get(number.to_be_bytes())? else {
            bail!("No block found with number {number}");
        };
        self.get_by_hash(&hash)
    }

    fn get_latest(&self) -> Result<PersistedBlock> {
        let Some((_, hash)) = self.block_hashes_by_slot.last_key_value()? else {
            bail!("No blocks found");
        };
        self.get_by_hash(&hash)
    }
}

#[derive(minicbor::Decode, minicbor::Encode)]
struct PersistedBlock {
    #[n(0)]
    slot: u64,
    #[n(1)]
    number: u64,
    #[b(2)]
    hash: Vec<u8>,
    #[b(3)]
    tx_hashes: Vec<TxHash>,
}

struct FjallTXStore {
    txs: Partition,
}
impl FjallTXStore {
    fn new(keyspace: &Keyspace) -> Result<Self> {
        let txs =
            keyspace.open_partition(TXS_PARTITION, fjall::PartitionCreateOptions::default())?;
        Ok(Self { txs })
    }

    fn insert_tx(&self, batch: &mut Batch, tx: &[u8], hash: TxHash) {
        batch.insert(&self.txs, hash, tx);
    }

    fn hydrate_block(&self, block: PersistedBlock) -> Result<super::Block> {
        let mut txs = vec![];
        for hash in block.tx_hashes {
            let Some(tx) = self.txs.get(hash)? else {
                bail!("Could not find TX {}", hex::encode(hash));
            };
            txs.push(tx.to_vec());
        }
        Ok(super::Block {
            slot: block.slot,
            number: block.number,
            hash: block.hash,
            txs,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::stores::{Block, Store};

    use super::*;
    use tempfile::TempDir;

    const TEST_TX: &str = "84a500d90102828258200000000000000000000000000000000000000000000000000000000000000000008258200000000000000000000000000000000000000000000000000000000000000000050183a300583930be4d215663909bb5935b923c2df611723480935bb4722d5f152b646a7467ae52afc8e9f5603c9265e7ce24853863a34f6b12d12a098f880801821a002dc6c0a3581c99b071ce8580d6a3a11b4902145adb8bfd0d2a03935af8cf66403e15a144555344431a01312d00581cbe4d215663909bb5935b923c2df611723480935bb4722d5f152b646aa15820000de140f6a207f7eb0b2aca50c96d0b83b7b6cf0cb2161aa73648e8161ddcc601581cfa3eff2047fdf9293c5feef4dc85ce58097ea1c6da4845a351535183a14574494e44591a01312d00028201d81858f3d8799f581cf6a207f7eb0b2aca50c96d0b83b7b6cf0cb2161aa73648e8161ddcc69f9f581c99b071ce8580d6a3a11b4902145adb8bfd0d2a03935af8cf66403e154455534443ff9f581cfa3eff2047fdf9293c5feef4dc85ce58097ea1c6da4845a3515351834574494e4459ffff1a01312d000505d87a80051a002dc6c0d8799f581c60c5ca218d3fa6ba7ecf4697a7a566ead9feb87068fc1229eddcf287ffd8799fd8799fa1581c633a136877ed6ad0ab33e69a22611319673474c8bd0a79a4c76d9289a158200014df10a933477ea168013e2b5af4a9e029e36d26738eb6dfe382e1f3eab3e21a05f5e100d87a80ffffffa300581d60035dee66d57cc271697711d63c8c35ffa0b6c4468a6a98024feac73b01821a001e8480a1581cbe4d215663909bb5935b923c2df611723480935bb4722d5f152b646aa15820000643b0f6a207f7eb0b2aca50c96d0b83b7b6cf0cb2161aa73648e8161ddcc601028201d81843d8798082583900c279a3fb3b4e62bbc78e288783b58045d4ae82a18867d8352d02775a121fd22e0b57ac206fefc763f8bfa0771919f5218b40691eea4514d0821a001e8480a1581cbe4d215663909bb5935b923c2df611723480935bb4722d5f152b646aa158200014df10f6a207f7eb0b2aca50c96d0b83b7b6cf0cb2161aa73648e8161ddcc61a01312d00020009a1581cbe4d215663909bb5935b923c2df611723480935bb4722d5f152b646aa35820000643b0f6a207f7eb0b2aca50c96d0b83b7b6cf0cb2161aa73648e8161ddcc6015820000de140f6a207f7eb0b2aca50c96d0b83b7b6cf0cb2161aa73648e8161ddcc60158200014df10f6a207f7eb0b2aca50c96d0b83b7b6cf0cb2161aa73648e8161ddcc61a01312d0012d901028982582016beda82efb0f2341fdb0bf6dec4a153b94681679826ae1e644070256601fcec0082582016beda82efb0f2341fdb0bf6dec4a153b94681679826ae1e644070256601fcec0182582016beda82efb0f2341fdb0bf6dec4a153b94681679826ae1e644070256601fcec0282582016beda82efb0f2341fdb0bf6dec4a153b94681679826ae1e644070256601fcec0382582045394d375379204a64d3fd6987afa83d1dd0c4f14a36094056f136bc21ed07b50082582045394d375379204a64d3fd6987afa83d1dd0c4f14a36094056f136bc21ed07b50182582045394d375379204a64d3fd6987afa83d1dd0c4f14a36094056f136bc21ed07b5028258200e6d53b393d19cfbd2307b104b4822d9267792493c58a480c8ea69eca8dd2ce20082582045ae0839622478c3ed2fbf5eea03c54ca3fd57607b7a2660445166ea8a42d98c00a200d9010281825820000000000000000000000000000000000000000000000000000000000000000058400000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000005a182010082d87a9f9f9f581c99b071ce8580d6a3a11b4902145adb8bfd0d2a03935af8cf66403e154455534443ff9f581cfa3eff2047fdf9293c5feef4dc85ce58097ea1c6da4845a3515351834574494e4459ffff0001ff821a00d59f801b00000002540be400f5f6";

    fn test_block() -> Block {
        let tx = hex::decode(TEST_TX).unwrap();
        Block {
            number: 1,
            slot: 3,
            hash: vec![0xca, 0xfe, 0xd0, 0x0d],
            txs: vec![tx],
        }
    }

    struct TestState {
        #[expect(unused)]
        dir: TempDir,
        store: FjallStore,
    }

    fn init_state() -> TestState {
        let dir = tempfile::tempdir().unwrap();
        let dir_name = dir.path().to_str().expect("dir_name cannot be stored as string");
        let config =
            Config::builder().set_default("database-path", dir_name).unwrap().build().unwrap();
        let store = FjallStore::new(Arc::new(config)).unwrap();
        TestState { dir, store }
    }

    #[test]
    fn should_get_block_by_hash() {
        let state = init_state();
        let block = test_block();

        state.store.insert_block(&block).unwrap();

        let new_block = state.store.get_block_by_hash(&block.hash).unwrap();
        assert_eq!(block, new_block);
    }

    #[test]
    fn should_get_block_by_slot() {
        let state = init_state();
        let block = test_block();

        state.store.insert_block(&block).unwrap();

        let new_block = state.store.get_block_by_slot(block.slot).unwrap();
        assert_eq!(block, new_block);
    }

    #[test]
    fn should_get_block_by_number() {
        let state = init_state();
        let block = test_block();

        state.store.insert_block(&block).unwrap();

        let new_block = state.store.get_block_by_number(block.number).unwrap();
        assert_eq!(block, new_block);
    }

    #[test]
    fn should_get_latest_block() {
        let state = init_state();
        let block = test_block();

        state.store.insert_block(&block).unwrap();

        let new_block = state.store.get_latest_block().unwrap();
        assert_eq!(block, new_block);
    }
}
