use std::{fs, path::PathBuf, sync::Arc};

use acropolis_common::{BlockInfo, TxHash};
use anyhow::{anyhow, Result};
use config::Config;
use fjall::{Database, Keyspace, OwnedWriteBatch};

use crate::stores::{Block, ExtraBlockData, Tx, TxBlockReference};

pub struct FjallStore {
    database: Database,
    blocks: FjallBlockStore,
    txs: FjallTXStore,
    last_persisted_block: Option<u64>,
}

const DEFAULT_DATABASE_PATH: &str = "fjall-blocks";
const DEFAULT_CLEAR_ON_START: bool = true;
const DEFAULT_NETWORK_NAME: &str = "mainnet";
const BLOCKS_KEYSPACE: &str = "blocks";
const BLOCK_HASHES_BY_SLOT_KEYSPACE: &str = "block-hashes-by-slot";
const BLOCK_HASHES_BY_NUMBER_KEYSPACE: &str = "block-hashes-by-number";
const BLOCK_HASHES_BY_EPOCH_SLOT_KEYSPACE: &str = "block-hashes-by-epoch-slot";
const TXS_KEYSPACE: &str = "txs";

impl FjallStore {
    pub fn new(config: Arc<Config>) -> Result<Self> {
        let path = config.get_string("database-path").unwrap_or_else(|_| {
            format!(
                "{DEFAULT_DATABASE_PATH}-{}",
                Self::network_scope_from_config(config.as_ref())
            )
        });
        let clear = config.get_bool("clear-on-start").unwrap_or(DEFAULT_CLEAR_ON_START);
        let path = PathBuf::from(path);
        if clear && path.exists() {
            fs::remove_dir_all(&path)?;
        }
        let database = Database::builder(&path).open()?;
        let blocks = FjallBlockStore::new(&database)?;
        let txs = FjallTXStore::new(&database)?;

        let last_persisted_block = if !clear {
            blocks.block_hashes_by_number.iter().next_back().and_then(|res| {
                res.key().ok().and_then(|key| key.as_ref().try_into().ok().map(u64::from_be_bytes))
            })
        } else {
            None
        };

        Ok(Self {
            database,
            blocks,
            txs,
            last_persisted_block,
        })
    }

    fn network_scope_from_config(config: &Config) -> String {
        config
            .get_string("startup.network-name")
            .or_else(|_| config.get_string("network-name"))
            .or_else(|_| config.get_string("network-id"))
            .unwrap_or_else(|_| DEFAULT_NETWORK_NAME.to_string())
    }
}

impl super::Store for FjallStore {
    fn insert_block(&self, info: &BlockInfo, block: &[u8]) -> Result<()> {
        let extra = ExtraBlockData {
            epoch: info.epoch,
            epoch_slot: info.epoch_slot,
            timestamp: info.timestamp,
        };
        let tx_hashes = super::extract_tx_hashes(block)?;
        let raw = Block {
            bytes: block.to_vec(),
            extra,
        };

        let mut batch = self.database.batch();
        self.blocks.insert(&mut batch, info, &raw);
        for (index, hash) in tx_hashes.iter().enumerate() {
            let block_ref = TxBlockReference {
                block_hash: info.hash.to_vec(),
                index,
            };
            self.txs.insert_tx(&mut batch, *hash, block_ref);
        }

        batch.commit()?;

        Ok(())
    }

    fn should_persist(&self, block_number: u64) -> bool {
        match self.last_persisted_block {
            Some(last) => block_number > last,
            None => true,
        }
    }

    fn get_block_by_hash(&self, hash: &[u8]) -> Result<Option<Block>> {
        self.blocks.get_by_hash(hash)
    }

    fn get_block_by_slot(&self, slot: u64) -> Result<Option<Block>> {
        self.blocks.get_by_slot(slot)
    }

    fn get_block_by_number(&self, number: u64) -> Result<Option<Block>> {
        self.blocks.get_by_number(number)
    }

    fn get_blocks_by_number_range(&self, min_number: u64, max_number: u64) -> Result<Vec<Block>> {
        self.blocks.get_by_number_range(min_number, max_number)
    }

    fn get_block_by_epoch_slot(&self, epoch: u64, epoch_slot: u64) -> Result<Option<Block>> {
        self.blocks.get_by_epoch_slot(epoch, epoch_slot)
    }

    fn get_latest_block(&self) -> Result<Option<Block>> {
        self.blocks.get_latest()
    }

    fn get_tx_by_hash(&self, hash: &[u8]) -> Result<Option<Tx>> {
        let Some(block_ref) = self.txs.get_by_hash(hash)? else {
            return Ok(None);
        };
        let Some(block) = self.blocks.get_by_hash(block_ref.block_hash.as_ref())? else {
            return Err(anyhow!("Referenced block not found"));
        };
        Ok(Some(Tx {
            block,
            index: block_ref.index as u64,
        }))
    }

    fn get_tx_block_ref_by_hash(&self, hash: &[u8]) -> Result<Option<TxBlockReference>> {
        self.txs.get_by_hash(hash)
    }
}

struct FjallBlockStore {
    blocks: Keyspace,
    block_hashes_by_slot: Keyspace,
    block_hashes_by_number: Keyspace,
    block_hashes_by_epoch_slot: Keyspace,
}

impl FjallBlockStore {
    fn new(database: &Database) -> Result<Self> {
        let blocks = database.keyspace(BLOCKS_KEYSPACE, fjall::KeyspaceCreateOptions::default)?;
        let block_hashes_by_slot = database.keyspace(
            BLOCK_HASHES_BY_SLOT_KEYSPACE,
            fjall::KeyspaceCreateOptions::default,
        )?;
        let block_hashes_by_number = database.keyspace(
            BLOCK_HASHES_BY_NUMBER_KEYSPACE,
            fjall::KeyspaceCreateOptions::default,
        )?;
        let block_hashes_by_epoch_slot = database.keyspace(
            BLOCK_HASHES_BY_EPOCH_SLOT_KEYSPACE,
            fjall::KeyspaceCreateOptions::default,
        )?;

        Ok(Self {
            blocks,
            block_hashes_by_slot,
            block_hashes_by_number,
            block_hashes_by_epoch_slot,
        })
    }

    fn insert(&self, batch: &mut OwnedWriteBatch, info: &BlockInfo, raw: &Block) {
        let encoded = {
            let mut bytes = vec![];
            minicbor::encode(raw, &mut bytes).expect("infallible");
            bytes
        };
        batch.insert(&self.blocks, *info.hash, encoded);
        batch.insert(
            &self.block_hashes_by_slot,
            info.slot.to_be_bytes(),
            *info.hash,
        );
        batch.insert(
            &self.block_hashes_by_number,
            info.number.to_be_bytes(),
            *info.hash,
        );
        batch.insert(
            &self.block_hashes_by_epoch_slot,
            epoch_slot_key(info.epoch, info.epoch_slot),
            *info.hash,
        );
    }

    fn get_by_hash(&self, hash: &[u8]) -> Result<Option<Block>> {
        let Some(block) = self.blocks.get(hash)? else {
            return Ok(None);
        };
        Ok(minicbor::decode(&block)?)
    }

    fn get_by_slot(&self, slot: u64) -> Result<Option<Block>> {
        let Some(hash) = self.block_hashes_by_slot.get(slot.to_be_bytes())? else {
            return Ok(None);
        };
        self.get_by_hash(&hash)
    }

    fn get_by_number(&self, number: u64) -> Result<Option<Block>> {
        let Some(hash) = self.block_hashes_by_number.get(number.to_be_bytes())? else {
            return Ok(None);
        };
        self.get_by_hash(&hash)
    }

    fn get_by_number_range(&self, min_number: u64, max_number: u64) -> Result<Vec<Block>> {
        if max_number < min_number {
            return Err(anyhow::anyhow!(
                "Invalid number range min={min_number}, max={max_number}"
            ));
        }
        let expected_count = max_number - min_number + 1;

        let min_number_bytes = min_number.to_be_bytes();
        let max_number_bytes = max_number.to_be_bytes();
        let mut blocks = vec![];
        for res in self.block_hashes_by_number.range(min_number_bytes..=max_number_bytes) {
            let hash = res.value()?;
            if let Some(block) = self.get_by_hash(&hash)? {
                blocks.push(block);
            }
        }
        if blocks.len() as u64 != expected_count {
            return Err(anyhow::anyhow!(
                "Expected {expected_count} blocks, got {}",
                blocks.len()
            ));
        }
        Ok(blocks)
    }

    fn get_by_epoch_slot(&self, epoch: u64, epoch_slot: u64) -> Result<Option<Block>> {
        let Some(hash) = self.block_hashes_by_epoch_slot.get(epoch_slot_key(epoch, epoch_slot))?
        else {
            return Ok(None);
        };
        self.get_by_hash(&hash)
    }

    fn get_latest(&self) -> Result<Option<Block>> {
        let Some(res) = self.block_hashes_by_slot.last_key_value() else {
            return Ok(None);
        };
        let hash = res.value()?;
        self.get_by_hash(&hash)
    }
}

fn epoch_slot_key(epoch: u64, epoch_slot: u64) -> [u8; 16] {
    let mut key = [0; 16];
    key[..8].copy_from_slice(epoch.to_be_bytes().as_slice());
    key[8..].copy_from_slice(epoch_slot.to_be_bytes().as_slice());
    key
}

struct FjallTXStore {
    txs: Keyspace,
}
impl FjallTXStore {
    fn new(database: &Database) -> Result<Self> {
        let txs = database.keyspace(TXS_KEYSPACE, fjall::KeyspaceCreateOptions::default)?;
        Ok(Self { txs })
    }

    fn insert_tx(&self, batch: &mut OwnedWriteBatch, hash: TxHash, block_ref: TxBlockReference) {
        let bytes = minicbor::to_vec(block_ref).expect("infallible");
        batch.insert(&self.txs, hash.as_ref(), bytes);
    }

    fn get_by_hash(&self, hash: &[u8]) -> Result<Option<TxBlockReference>> {
        let Some(block_ref) = self.txs.get(hash)? else {
            return Ok(None);
        };
        Ok(minicbor::decode(&block_ref)?)
    }
}

#[cfg(test)]
mod tests {
    use crate::stores::Store;

    use super::*;
    use acropolis_common::BlockHash;
    use pallas_traverse::{wellknown::GenesisValues, MultiEraBlock};
    use tempfile::TempDir;

    const TEST_BLOCK: &str = "820785828a1a0010afaa1a0150d7925820a22f65265e7a71cfc3b637d6aefe8f8241d562f5b1b787ff36697ae4c3886f185820e856c84a3d90c8526891bd58d957afadc522de37b14ae04c395db8a7a1b08c4a582015587d5633be324f8de97168399ab59d7113f0a74bc7412b81f7cc1007491671825840af9ff8cb146880eba1b12beb72d86be46fbc98f6b88110cd009bd6746d255a14bb0637e3a29b7204bff28236c1b9f73e501fed1eb5634bd741be120332d25e5e5850a9f1de24d01ba43b025a3351b25de50cc77f931ed8cdd0be632ad1a437ec9cf327b24eb976f91dbf68526f15bacdf8f0c1ea4a2072df9412796b34836a816760f4909b98c0e76b160d9aec6b2da060071903705820b5858c659096fcc19f2f3baef5fdd6198641a623bd43e792157b5ea3a2ecc85c8458200ca1ec2c1c2af308bd9e7a86eb12d603a26157752f3f71c337781c456e6ed0c90018a558408e554b644a2b25cb5892d07a26c273893829f1650ec33bf6809d953451c519c32cfd48d044cd897a17cdef154d5f5c9b618d9b54f8c49e170082c08c236524098209005901c05a96b747789ef6678b2f4a2a7caca92e270f736e9b621686f95dd1332005102faee21ed50cf6fa6c67e38b33df686c79c91d55f30769f7c964d98aa84cbefe0a808ee6f45faaf9badcc3f746e6a51df1aa979195871fd5ffd91037ea216803be7e7fccbf4c13038c459c7a14906ab57f3306fe155af7877c88866eede7935f642f6a72f1368c33ed5cc7607c995754af787a5af486958edb531c0ae65ce9fdce423ad88925e13ef78700950093ae707bb1100299a66a5bb15137f7ba62132ba1c9b74495aac50e1106bacb5db2bed4592f66b610c2547f485d061c6c149322b0c92bdde644eb672267fdab5533157ff398b9e16dd6a06edfd67151e18a3ac93fc28a51f9a73f8b867f5f432b1d9b5ae454ef63dea7e1a78631cf3fee1ba82db61726701ac5db1c4fee4bb6316768c82c0cdc4ebd58ccc686be882f9608592b3c718e4b5d356982a6b83433fe76d37394eff9f3a8e4773e3bab9a8b93b4ea90fa33bfbcf0dc5a21bfe64be2eefaa82c0494ab729e50596110f60ae9ad64b3eb9ddb54001b03cc264b65634c071d3b24a44322f39a9eae239fd886db8d429969433cb2d0a82d7877f174b0e154262f1af44ce5bc053b62daadd2926f957440ff3981a600d9010281825820af09d312a642fecb47da719156517bec678469c15789bcf002ce2ef563edf54200018182581d6052e63f22c5107ed776b70f7b92248b02552fd08f3e747bc745099441821b00000001373049f4a1581c34250edd1e9836f5378702fbf9416b709bc140e04f668cc355208518a1494154414441636f696e1953a6021a000306b5031a01525e0209a1581c34250edd1e9836f5378702fbf9416b709bc140e04f668cc355208518a1494154414441636f696e010758206cf243cc513691d9edc092b1030c6d1e5f9a8621a4d4383032b3d292d4679d5c81a200d90102828258201287e9ce9e00a603d250b557146aa0581fc4edf277a244ce39d3b2f2ced5072f5840d40fbe736892d8dab09e864a25f2e59fb7bfe445d960bbace30996965dc12a34c59746febf9d32ade65b6a9e1a1a6efc53830a3acaab699972cd4f240c024c0f825820742d8af3543349b5b18f3cba28f23b2d6e465b9c136c42e1fae6b2390f565427584005637b5645784bd998bb8ed837021d520200211fdd958b9a4d4b3af128fa6e695fb86abad7a9ddad6f1db946f8b812113fa16cfb7025e2397277b14e8c9bed0a01d90102818200581c45d70e54f3b5e9c5a2b0cd417028197bd6f5fa5378c2f5eba896678da100d90103a100a11902a2a1636d73678f78264175746f2d4c6f6f702d5472616e73616374696f6e202336323733363820627920415441444160783c4c6976652045706f6368203235352c207765206861766520303131682035396d20323573206c65667420756e74696c20746865206e657874206f6e6578344974277320536f6e6e746167202d20323520466562727561722032303234202d2031333a33303a333520696e20417573747269616060607820412072616e646f6d205a656e2d51756f746520666f7220796f753a20f09f998f78344974206973206e6576657220746f6f206c61746520746f206265207768617420796f75206d696768742068617665206265656e2e6f202d2047656f72676520456c696f746078374e6f64652d5265766973696f6e3a203462623230343864623737643632336565366533363738363138633264386236633436373633333360782953616e63686f4e657420697320617765736f6d652c206861766520736f6d652066756e2120f09f988d7819204265737420726567617264732c204d617274696e203a2d2980";

    // Mainnet blocks 1-9
    const TEST_BLOCKS: [&str; 9] = [
        "820183851a2d964a09582089d9b5a5b8ddc8d7e5a6795e9774d97faf1efea59b2caf7eaf9f8c5b32059df484830058200e5751c026e543b2e8ab2eb06099daa1d1e5df47778f7787faab45cdf12fe3a85820afc0da64183bf2664f3d4eec7238d524ba607faeeab24fc100eb861dba69971b8300582025777aca9e4a73d48fc73b4f961d345b06d4a6f349cb7916570d35537d53479f5820d36a2619a672494604e11bb447cbcf5231e9f2ba25c2169177edc941bd50ad6c5820afc0da64183bf2664f3d4eec7238d524ba607faeeab24fc100eb861dba69971b58204e66280cd94d591072349bec0a3090a53aa945562efb6d08d56e53654b0e40988482000058401bc97a2fe02c297880ce8ecfd997fe4c1ec09ee10feeee9f686760166b05281d6283468ffd93becb0c956ccddd642df9b1244c915911185fa49355f6f22bfab98101820282840058401bc97a2fe02c297880ce8ecfd997fe4c1ec09ee10feeee9f686760166b05281d6283468ffd93becb0c956ccddd642df9b1244c915911185fa49355f6f22bfab9584061261a95b7613ee6bf2067dad77b70349729b0c50d57bc1cf30de0db4a1e73a885d0054af7c23fc6c37919dba41c602a57e2d0f9329a7954b867338d6fb2c9455840e03e62f083df5576360e60a32e22bbb07b3c8df4fcab8079f1d6f61af3954d242ba8a06516c395939f24096f3df14e103a7d9c2b80a68a9363cf1f27c7a4e307584044f18ef23db7d2813415cb1b62e8f3ead497f238edf46bb7a97fd8e9105ed9775e8421d18d47e05a2f602b700d932c181e8007bbfb231d6f1a050da4ebeeba048483000000826a63617264616e6f2d736c00a058204ba92aa320c60acc9ad7b9a64f2eda55c4d2ec28e604faf186708b4f0c4e8edf849fff8300d9010280d90102809fff82809fff81a0",
        "820183851a2d964a095820f0f7892b5c333cffc4b3c4344de48af4cc63f55e44936196f365a9ef2244134f84830058200e5751c026e543b2e8ab2eb06099daa1d1e5df47778f7787faab45cdf12fe3a85820afc0da64183bf2664f3d4eec7238d524ba607faeeab24fc100eb861dba69971b8300582025777aca9e4a73d48fc73b4f961d345b06d4a6f349cb7916570d35537d53479f5820d36a2619a672494604e11bb447cbcf5231e9f2ba25c2169177edc941bd50ad6c5820afc0da64183bf2664f3d4eec7238d524ba607faeeab24fc100eb861dba69971b58204e66280cd94d591072349bec0a3090a53aa945562efb6d08d56e53654b0e409884820001584050733161fdafb6c8cb6fae0e25bdf9555105b3678efb08f1775b9e90de4f5c77bcc8cefff8d9011cb278b28fddc86d9bab099656d77a7856c7619108cbf6575281028202828400584050733161fdafb6c8cb6fae0e25bdf9555105b3678efb08f1775b9e90de4f5c77bcc8cefff8d9011cb278b28fddc86d9bab099656d77a7856c7619108cbf657525840e8c03a03c0b2ddbea4195caf39f41e669f7d251ecf221fbb2f275c0a5d7e05d190dcc246f56c8e33ac0037066e2f664ddaa985ea5284082643308dde4f5bfedf5840c8b39f094dc00608acb2d20ff274cb3e0c022ccb0ce558ea7c1a2d3a32cd54b42cc30d32406bcfbb7f2f86d05d2032848be15b178e3ad776f8b1bc56a671400d5840923c7714af7fe4b1272fc042111ece6fd08f5f16298d62bae755c70c1e1605697cbaed500e196330f40813128250d9ede9c8557b33f48e8a5f32f765929e4a0d8483000000826a63617264616e6f2d736c00a058204ba92aa320c60acc9ad7b9a64f2eda55c4d2ec28e604faf186708b4f0c4e8edf849fff8300d9010280d90102809fff82809fff81a0",
        "820183851a2d964a0958201dbc81e3196ba4ab9dcb07e1c37bb28ae1c289c0707061f28b567c2f48698d5084830058200e5751c026e543b2e8ab2eb06099daa1d1e5df47778f7787faab45cdf12fe3a85820afc0da64183bf2664f3d4eec7238d524ba607faeeab24fc100eb861dba69971b8300582025777aca9e4a73d48fc73b4f961d345b06d4a6f349cb7916570d35537d53479f5820d36a2619a672494604e11bb447cbcf5231e9f2ba25c2169177edc941bd50ad6c5820afc0da64183bf2664f3d4eec7238d524ba607faeeab24fc100eb861dba69971b58204e66280cd94d591072349bec0a3090a53aa945562efb6d08d56e53654b0e409884820002584050733161fdafb6c8cb6fae0e25bdf9555105b3678efb08f1775b9e90de4f5c77bcc8cefff8d9011cb278b28fddc86d9bab099656d77a7856c7619108cbf6575281038202828400584050733161fdafb6c8cb6fae0e25bdf9555105b3678efb08f1775b9e90de4f5c77bcc8cefff8d9011cb278b28fddc86d9bab099656d77a7856c7619108cbf657525840e8c03a03c0b2ddbea4195caf39f41e669f7d251ecf221fbb2f275c0a5d7e05d190dcc246f56c8e33ac0037066e2f664ddaa985ea5284082643308dde4f5bfedf5840c8b39f094dc00608acb2d20ff274cb3e0c022ccb0ce558ea7c1a2d3a32cd54b42cc30d32406bcfbb7f2f86d05d2032848be15b178e3ad776f8b1bc56a671400d584094966ae05c576724fd892aa91959fc191833fade8e118c36a12eb453003b634ccc9bb7808bcf950c5da9145cffad9e26061bfe9853817706008f75a464c814038483000000826a63617264616e6f2d736c00a058204ba92aa320c60acc9ad7b9a64f2eda55c4d2ec28e604faf186708b4f0c4e8edf849fff8300d9010280d90102809fff82809fff81a0",
        "820183851a2d964a09582052b7912de176ab76c233d6e08ccdece53ac1863c08cc59d3c5dec8d924d9b53684830058200e5751c026e543b2e8ab2eb06099daa1d1e5df47778f7787faab45cdf12fe3a85820afc0da64183bf2664f3d4eec7238d524ba607faeeab24fc100eb861dba69971b8300582025777aca9e4a73d48fc73b4f961d345b06d4a6f349cb7916570d35537d53479f5820d36a2619a672494604e11bb447cbcf5231e9f2ba25c2169177edc941bd50ad6c5820afc0da64183bf2664f3d4eec7238d524ba607faeeab24fc100eb861dba69971b58204e66280cd94d591072349bec0a3090a53aa945562efb6d08d56e53654b0e409884820003584026566e86fc6b9b177c8480e275b2b112b573f6d073f9deea53b8d99c4ed976b335b2b3842f0e380001f090bc923caa9691ed9115e286da9421e2745c7acc87f181048202828400584026566e86fc6b9b177c8480e275b2b112b573f6d073f9deea53b8d99c4ed976b335b2b3842f0e380001f090bc923caa9691ed9115e286da9421e2745c7acc87f15840f14f712dc600d793052d4842d50cefa4e65884ea6cf83707079eb8ce302efc85dae922d5eb3838d2b91784f04824d26767bfb65bd36a36e74fec46d09d98858d58408ab43e904b06e799c1817c5ced4f3a7bbe15cdbf422dea9d2d5dc2c6105ce2f4d4c71e5d4779f6c44b770a133636109949e1f7786acb5a732bcdea0470fea4065840273c97ffc6e16c86772bdb9cb52bfe99585917f901ee90ce337a9654198fb09ca6bc51d74a492261c169ca5a196a04938c740ba6629254fe566a590370cc9b0f8483000000826a63617264616e6f2d736c00a058204ba92aa320c60acc9ad7b9a64f2eda55c4d2ec28e604faf186708b4f0c4e8edf849fff8300d9010280d90102809fff82809fff81a0",
        "820183851a2d964a095820be06c81f4ad34d98578b67840d8e65b2aeb148469b290f6b5235e41b75d3857284830058200e5751c026e543b2e8ab2eb06099daa1d1e5df47778f7787faab45cdf12fe3a85820afc0da64183bf2664f3d4eec7238d524ba607faeeab24fc100eb861dba69971b8300582025777aca9e4a73d48fc73b4f961d345b06d4a6f349cb7916570d35537d53479f5820d36a2619a672494604e11bb447cbcf5231e9f2ba25c2169177edc941bd50ad6c5820afc0da64183bf2664f3d4eec7238d524ba607faeeab24fc100eb861dba69971b58204e66280cd94d591072349bec0a3090a53aa945562efb6d08d56e53654b0e4098848200045840d2965c869901231798c5d02d39fca2a79aa47c3e854921b5855c82fd1470891517e1fa771655ec8cad13ecf6e5719adc5392fc057e1703d5f583311e837462f1810582028284005840d2965c869901231798c5d02d39fca2a79aa47c3e854921b5855c82fd1470891517e1fa771655ec8cad13ecf6e5719adc5392fc057e1703d5f583311e837462f158409180d818e69cd997e34663c418a648c076f2e19cd4194e486e159d8580bc6cda81344440c6ad0e5306fd035bef9281da5d8fbd38f59f588f7081016ee61113d25840cf6ddc111545f61c2442b68bd7864ea952c428d145438948ef48a4af7e3f49b175564007685be5ae3c9ece0ab27de09721db0cb63aa67dc081a9f82d7e84210d58409f9649c57d902a9fe94208b40eb31ffb4d703e5692c16bcd3a4370b448b4597edaa66f3e4f3bd5858d8e6a57cc0734ec04174d13cbc62eabe64af49271245f068483000000826a63617264616e6f2d736c00a058204ba92aa320c60acc9ad7b9a64f2eda55c4d2ec28e604faf186708b4f0c4e8edf849fff8300d9010280d90102809fff82809fff81a0",
        "820183851a2d964a09582046debe49b4fe0bc8c07cfe650de89632ca1ab5d58f04f8c88d8102da7ef79b7f84830058200e5751c026e543b2e8ab2eb06099daa1d1e5df47778f7787faab45cdf12fe3a85820afc0da64183bf2664f3d4eec7238d524ba607faeeab24fc100eb861dba69971b8300582025777aca9e4a73d48fc73b4f961d345b06d4a6f349cb7916570d35537d53479f5820d36a2619a672494604e11bb447cbcf5231e9f2ba25c2169177edc941bd50ad6c5820afc0da64183bf2664f3d4eec7238d524ba607faeeab24fc100eb861dba69971b58204e66280cd94d591072349bec0a3090a53aa945562efb6d08d56e53654b0e4098848200055840993a8f056d2d3e50b0ac60139f10df8f8123d5f7c4817b40dac2b5dd8aa94a82e8536832e6312ddfc0787d7b5310c815655ada4fdbcf6b12297d4458eccc2dfb810682028284005840993a8f056d2d3e50b0ac60139f10df8f8123d5f7c4817b40dac2b5dd8aa94a82e8536832e6312ddfc0787d7b5310c815655ada4fdbcf6b12297d4458eccc2dfb584089c29f8c4af27b7accbe589747820134ebbaa1caf3ce949270a3d0c7dcfd541b1def326d2ef0db780341c9e261f04890cdeef1f9c99f6d90b8edca7d3cfc09885840496b29b5c57e8ac7cffc6e8b5e40b3d260e407ad4d09792decb0a22d54da7f8828265688a18aa1a5c76d9e7477a5f4a650501409fdcd3855b300fd2e2bc3c6055840b3bea437aa37a2abdc1a35d9ff01cddb387c543d8034c565dc18525ccd16a0f761d3556d8b90add263db77ee6200aebd6ec2fcc2ec20153f9227b07053a7a50a8483000000826a63617264616e6f2d736c00a058204ba92aa320c60acc9ad7b9a64f2eda55c4d2ec28e604faf186708b4f0c4e8edf849fff8300d9010280d90102809fff82809fff81a0",
        "820183851a2d964a095820365201e928da50760fce4bdad09a7338ba43a43aff1c0e8d3ec458388c932ec884830058200e5751c026e543b2e8ab2eb06099daa1d1e5df47778f7787faab45cdf12fe3a85820afc0da64183bf2664f3d4eec7238d524ba607faeeab24fc100eb861dba69971b8300582025777aca9e4a73d48fc73b4f961d345b06d4a6f349cb7916570d35537d53479f5820d36a2619a672494604e11bb447cbcf5231e9f2ba25c2169177edc941bd50ad6c5820afc0da64183bf2664f3d4eec7238d524ba607faeeab24fc100eb861dba69971b58204e66280cd94d591072349bec0a3090a53aa945562efb6d08d56e53654b0e409884820006584050733161fdafb6c8cb6fae0e25bdf9555105b3678efb08f1775b9e90de4f5c77bcc8cefff8d9011cb278b28fddc86d9bab099656d77a7856c7619108cbf6575281078202828400584050733161fdafb6c8cb6fae0e25bdf9555105b3678efb08f1775b9e90de4f5c77bcc8cefff8d9011cb278b28fddc86d9bab099656d77a7856c7619108cbf657525840e8c03a03c0b2ddbea4195caf39f41e669f7d251ecf221fbb2f275c0a5d7e05d190dcc246f56c8e33ac0037066e2f664ddaa985ea5284082643308dde4f5bfedf5840c8b39f094dc00608acb2d20ff274cb3e0c022ccb0ce558ea7c1a2d3a32cd54b42cc30d32406bcfbb7f2f86d05d2032848be15b178e3ad776f8b1bc56a671400d584077ddc2fe0557a5c0454a7af6f29e39e603907b927aeeab23e18abe0022cf219197a9a359ab07986a6b42a6e970139edd4a36555661274ae3ac27d4e7c509790e8483000000826a63617264616e6f2d736c00a058204ba92aa320c60acc9ad7b9a64f2eda55c4d2ec28e604faf186708b4f0c4e8edf849fff8300d9010280d90102809fff82809fff81a0",
        "820183851a2d964a095820e39d988dd815fc2cb234c2abef0d7f57765eeffb67331814bdb01c590359325e84830058200e5751c026e543b2e8ab2eb06099daa1d1e5df47778f7787faab45cdf12fe3a85820afc0da64183bf2664f3d4eec7238d524ba607faeeab24fc100eb861dba69971b8300582025777aca9e4a73d48fc73b4f961d345b06d4a6f349cb7916570d35537d53479f5820d36a2619a672494604e11bb447cbcf5231e9f2ba25c2169177edc941bd50ad6c5820afc0da64183bf2664f3d4eec7238d524ba607faeeab24fc100eb861dba69971b58204e66280cd94d591072349bec0a3090a53aa945562efb6d08d56e53654b0e409884820007584050733161fdafb6c8cb6fae0e25bdf9555105b3678efb08f1775b9e90de4f5c77bcc8cefff8d9011cb278b28fddc86d9bab099656d77a7856c7619108cbf6575281088202828400584050733161fdafb6c8cb6fae0e25bdf9555105b3678efb08f1775b9e90de4f5c77bcc8cefff8d9011cb278b28fddc86d9bab099656d77a7856c7619108cbf657525840e8c03a03c0b2ddbea4195caf39f41e669f7d251ecf221fbb2f275c0a5d7e05d190dcc246f56c8e33ac0037066e2f664ddaa985ea5284082643308dde4f5bfedf5840c8b39f094dc00608acb2d20ff274cb3e0c022ccb0ce558ea7c1a2d3a32cd54b42cc30d32406bcfbb7f2f86d05d2032848be15b178e3ad776f8b1bc56a671400d58405b2f5d0f55ec53bf74a09e2154f7ad56f437a1a9198041e3ec96f5f17a0cfa8c7d71a7871efabd990184b5166b2ac83af0b63bb727fd7157541db7a232ffdc048483000000826a63617264616e6f2d736c00a058204ba92aa320c60acc9ad7b9a64f2eda55c4d2ec28e604faf186708b4f0c4e8edf849fff8300d9010280d90102809fff82809fff81a0",
        "820183851a2d964a0958202d9136c363c69ad07e1a918de2ff5aeeba4361e33b9c2597511874f211ca26e984830058200e5751c026e543b2e8ab2eb06099daa1d1e5df47778f7787faab45cdf12fe3a85820afc0da64183bf2664f3d4eec7238d524ba607faeeab24fc100eb861dba69971b8300582025777aca9e4a73d48fc73b4f961d345b06d4a6f349cb7916570d35537d53479f5820d36a2619a672494604e11bb447cbcf5231e9f2ba25c2169177edc941bd50ad6c5820afc0da64183bf2664f3d4eec7238d524ba607faeeab24fc100eb861dba69971b58204e66280cd94d591072349bec0a3090a53aa945562efb6d08d56e53654b0e4098848200085840d2965c869901231798c5d02d39fca2a79aa47c3e854921b5855c82fd1470891517e1fa771655ec8cad13ecf6e5719adc5392fc057e1703d5f583311e837462f1810982028284005840d2965c869901231798c5d02d39fca2a79aa47c3e854921b5855c82fd1470891517e1fa771655ec8cad13ecf6e5719adc5392fc057e1703d5f583311e837462f158409180d818e69cd997e34663c418a648c076f2e19cd4194e486e159d8580bc6cda81344440c6ad0e5306fd035bef9281da5d8fbd38f59f588f7081016ee61113d25840cf6ddc111545f61c2442b68bd7864ea952c428d145438948ef48a4af7e3f49b175564007685be5ae3c9ece0ab27de09721db0cb63aa67dc081a9f82d7e84210d58407b26babee8ad96bf5cdd20cac799ca56c90b6ff9df1f1140f50f021063f719e3791f22be92353a8ae16045b0d52a51c8b1219ce782fd4198cf15b745348021018483000000826a63617264616e6f2d736c00a058204ba92aa320c60acc9ad7b9a64f2eda55c4d2ec28e604faf186708b4f0c4e8edf849fff8300d9010280d90102809fff82809fff81a0",
    ];

    fn test_block_info(bytes: &[u8]) -> BlockInfo {
        let block = MultiEraBlock::decode(bytes).unwrap();
        let genesis = GenesisValues::mainnet();
        let (epoch, epoch_slot) = block.epoch(&genesis);
        let timestamp = block.wallclock(&genesis);
        BlockInfo {
            status: acropolis_common::BlockStatus::Immutable,
            intent: acropolis_common::BlockIntent::Apply,
            slot: block.slot(),
            number: block.number(),
            hash: BlockHash::from(*block.hash()),
            epoch,
            epoch_slot,
            new_epoch: false,
            is_new_era: false,
            timestamp,
            tip_slot: None,
            era: acropolis_common::Era::Conway,
        }
    }

    fn test_block_bytes() -> Vec<u8> {
        hex::decode(TEST_BLOCK).unwrap()
    }

    fn test_block_range_bytes(count: usize) -> Vec<Vec<u8>> {
        TEST_BLOCKS[0..count].iter().map(|b| hex::decode(b).unwrap()).collect()
    }

    fn build_block(info: &BlockInfo, bytes: &[u8]) -> Block {
        let extra = ExtraBlockData {
            epoch: info.epoch,
            epoch_slot: info.epoch_slot,
            timestamp: info.timestamp,
        };
        Block {
            bytes: bytes.to_vec(),
            extra,
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
        let bytes = test_block_bytes();
        let info = test_block_info(&bytes);
        let block = build_block(&info, &bytes);

        state.store.insert_block(&info, &bytes).unwrap();

        let new_block = state.store.get_block_by_hash(info.hash.as_ref()).unwrap();
        assert_eq!(block, new_block.unwrap());
    }

    #[test]
    fn should_not_error_when_block_not_found() {
        let state = init_state();
        let new_block = state.store.get_block_by_hash(&[0xfa, 0x15, 0xe]).unwrap();
        assert_eq!(new_block, None);
    }

    #[test]
    fn should_get_block_by_slot() {
        let state = init_state();
        let bytes = test_block_bytes();
        let info = test_block_info(&bytes);
        let block = build_block(&info, &bytes);

        state.store.insert_block(&info, &bytes).unwrap();

        let new_block = state.store.get_block_by_slot(info.slot).unwrap();
        assert_eq!(block, new_block.unwrap());
    }

    #[test]
    fn should_get_block_by_number() {
        let state = init_state();
        let bytes = test_block_bytes();
        let info = test_block_info(&bytes);
        let block = build_block(&info, &bytes);

        state.store.insert_block(&info, &bytes).unwrap();

        let new_block = state.store.get_block_by_number(info.number).unwrap();
        assert_eq!(block, new_block.unwrap());
    }

    #[test]
    fn should_get_blocks_by_number_range() {
        let state = init_state();
        let blocks_bytes = test_block_range_bytes(6);
        let mut blocks = Vec::new();
        for bytes in blocks_bytes {
            let info = test_block_info(&bytes);
            blocks.push(build_block(&info, &bytes));
            state.store.insert_block(&info, &bytes).unwrap();
        }
        let new_blocks = state.store.get_blocks_by_number_range(2, 4).unwrap();
        assert_eq!(blocks[1], new_blocks[0]);
        assert_eq!(blocks[2], new_blocks[1]);
        assert_eq!(blocks[3], new_blocks[2]);
    }

    #[test]
    fn should_get_block_by_epoch_slot() {
        let state = init_state();
        let bytes = test_block_bytes();
        let info = test_block_info(&bytes);
        let block = build_block(&info, &bytes);

        state.store.insert_block(&info, &bytes).unwrap();

        let new_block = state.store.get_block_by_epoch_slot(info.epoch, info.epoch_slot).unwrap();
        assert_eq!(block, new_block.unwrap());
    }

    #[test]
    fn should_get_latest_block() {
        let state = init_state();
        let bytes = test_block_bytes();
        let info = test_block_info(&bytes);
        let block = build_block(&info, &bytes);

        state.store.insert_block(&info, &bytes).unwrap();

        let new_block = state.store.get_latest_block().unwrap();
        assert_eq!(block, new_block.unwrap());
    }
}
