use acropolis_common::{TxHash, TxIdentifier};
use anyhow::Result;
use fjall::{Keyspace, Partition, PartitionCreateOptions, PersistMode};
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use tracing::info;

const DEFAULT_FLUSH_EVERY: usize = 1000;
const PARTITION_FWD: &str = "tx_registry_fwd";
const PARTITION_REV: &str = "tx_registry_rev";
const DEFAULT_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/registry");

pub struct TxRegistry {
    keyspace: Keyspace,
    fwd: Partition,
    rev: Partition,
    write_counter: AtomicUsize,
    flush_every: AtomicUsize,
}

impl TxRegistry {
    pub fn new(flush_every: Option<usize>) -> Result<Self> {
        let path = Path::new(DEFAULT_PATH);
        info!(
            "Storing Tx registry with Fjall on disk ({})",
            path.display()
        );

        if !path.exists() {
            fs::create_dir_all(path)?;
        }

        let mut fjall_config = fjall::Config::new(path);
        fjall_config = fjall_config.manual_journal_persist(true);

        let keyspace = fjall_config.open()?;
        let fwd = keyspace.open_partition(PARTITION_FWD, PartitionCreateOptions::default())?;
        let rev = keyspace.open_partition(PARTITION_REV, PartitionCreateOptions::default())?;

        Ok(Self {
            keyspace,
            fwd,
            rev,
            write_counter: AtomicUsize::new(0),
            flush_every: AtomicUsize::new(flush_every.unwrap_or(DEFAULT_FLUSH_EVERY)),
        })
    }

    fn should_flush(&self) -> bool {
        let count = self.write_counter.fetch_add(1, Ordering::Relaxed) + 1;
        let threshold = self.flush_every.load(Ordering::Relaxed);
        threshold != 0 && count % threshold == 0
    }

    pub fn insert(&self, block_number: u32, tx_index: u16, tx_hash: TxHash) -> Result<()> {
        let id = TxIdentifier::new(block_number, tx_index);
        let should_flush = self.should_flush();

        self.fwd.insert(id.as_bytes(), tx_hash)?;
        self.rev.insert(tx_hash, id.as_bytes())?;

        if should_flush {
            self.keyspace.persist(PersistMode::Buffer)?;
        }

        Ok(())
    }

    #[allow(dead_code)]
    pub fn lookup_by_index(&self, block_number: u32, tx_index: u16) -> Result<Option<TxHash>> {
        let id = TxIdentifier::new(block_number, tx_index);
        Ok(self.fwd.get(id.as_bytes())?.map(|ivec| ivec.as_ref().try_into().unwrap()))
    }

    pub fn lookup_by_hash(&self, tx_hash: &TxHash) -> Result<Option<TxIdentifier>> {
        match self.rev.get(tx_hash) {
            Ok(Some(ivec)) => {
                if ivec.len() == 6 {
                    let mut buf = [0u8; 6];
                    buf.copy_from_slice(&ivec);
                    Ok(Some(TxIdentifier::from_bytes(buf)))
                } else {
                    Err(anyhow::anyhow!(
                        "invalid value length in tx_registry: {}",
                        ivec.len()
                    ))
                }
            }
            Ok(None) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
}
