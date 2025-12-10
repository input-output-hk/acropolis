use crate::{messages::RawBlockMessage, BlockInfo};
use anyhow::{anyhow, bail, Context, Result};
use std::{
    fs::File,
    io::{BufReader, Write},
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UpstreamCacheRecord {
    pub id: BlockInfo,
    pub message: Arc<RawBlockMessage>,
}

pub trait Storage {
    fn read_chunk(&mut self, chunk_no: usize) -> Result<Vec<UpstreamCacheRecord>>;
    fn write_chunk(&mut self, chunk_no: usize, chunk: &[UpstreamCacheRecord]) -> Result<()>;
}

pub struct FileStorage {
    path: PathBuf,
}

impl FileStorage {
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        std::fs::create_dir_all(&path)?;
        Ok(Self { path })
    }

    fn get_file_name(&self, chunk_no: usize) -> PathBuf {
        self.path.join(format!("chunk-{chunk_no}.json"))
    }
}

pub type UpstreamCache = UpstreamCacheImpl<FileStorage>;

impl UpstreamCache {
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        Ok(UpstreamCache::new_impl(FileStorage::new(path)?))
    }
}

pub struct UpstreamCacheImpl<S: Storage> {
    density: usize,

    // Cache invariant: current chunk/record point either to:
    // (a) current record, which is found in chunk_cached[current_record].
    // (b) first empty record after last cached record.
    // (c) therefore, chunk_cached should always be loaded and actual.

    // If current_record < density and current_record points outside of chunk_cached,
    // then we're at the first empty record after cached records.
    current_chunk: usize,
    current_record: usize,
    chunk_cached: Vec<UpstreamCacheRecord>,

    // Reader/writer functions --- to abstract actual struct encoder/storage from chunk logic
    storage: S,
}

impl<S: Storage> UpstreamCacheImpl<S> {
    pub fn new_impl(storage: S) -> Self {
        Self {
            storage,
            density: 1000,
            current_chunk: 0,
            current_record: 0,
            chunk_cached: vec![],
        }
    }

    pub fn start_reading(&mut self) -> Result<()> {
        self.current_chunk = 0;
        self.current_record = 0;
        self.chunk_cached = self.storage.read_chunk(0)?;
        Ok(())
    }

    /// Returns true if we're in the middle of cache, returns false if pointer points
    /// to first record after the end of cache.
    fn has_record(&self) -> bool {
        self.current_record < self.chunk_cached.len()
    }

    /// Moves current_chunk/_record to next record. If we're already outside of
    /// filled cache, this function does nothing.
    pub fn next_record(&mut self) -> Result<()> {
        if self.has_record() {
            self.current_record += 1;

            if self.current_record >= self.density {
                if self.chunk_cached.len() > self.density {
                    bail!(
                        "Full chunk actual length {}, expected {}",
                        self.chunk_cached.len(),
                        self.density
                    );
                }

                self.current_record = 0;
                self.current_chunk += 1;
                self.chunk_cached = self.storage.read_chunk(self.current_chunk)?
            }
        }

        Ok(())
    }

    pub fn read_record(&mut self) -> Result<Option<UpstreamCacheRecord>> {
        if self.has_record() {
            let record = self.chunk_cached.get(self.current_record).ok_or(anyhow!(
                "Error reading {}:{}",
                self.current_chunk,
                self.current_record
            ))?;

            return Ok(Some(record.clone()));
        };

        Ok(None)
    }

    pub fn write_record(&mut self, record: &UpstreamCacheRecord) -> Result<()> {
        self.chunk_cached.push(record.clone());
        self.storage
            .write_chunk(self.current_chunk, &self.chunk_cached)
            .context("could not write cache record")?;

        self.current_record += 1;
        if self.current_record >= self.density {
            self.current_record = 0;
            self.current_chunk += 1;
            self.chunk_cached = vec![];
        }

        Ok(())
    }
}

impl Storage for FileStorage {
    fn read_chunk(&mut self, chunk_no: usize) -> Result<Vec<UpstreamCacheRecord>> {
        let path = self.get_file_name(chunk_no);
        if !path.try_exists()? {
            return Ok(vec![]);
        }

        let file = File::open(&path)?;
        let reader = BufReader::new(file);
        serde_json::from_reader(reader).with_context(|| {
            format!(
                "Error reading upstream cache chunk JSON from {}",
                path.display()
            )
        })
    }

    fn write_chunk(&mut self, chunk_no: usize, data: &[UpstreamCacheRecord]) -> Result<()> {
        let mut file =
            File::create(self.get_file_name(chunk_no)).context("could not write chunk")?;
        file.write_all(&serde_json::to_vec(data)?)?;
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use crate::upstream_cache::{Storage, UpstreamCacheImpl, UpstreamCacheRecord};
    use crate::{messages::RawBlockMessage, BlockHash, BlockInfo, BlockIntent, BlockStatus, Era};
    use anyhow::Result;
    use std::{collections::HashMap, sync::Arc};

    fn blk(n: u64) -> BlockInfo {
        BlockInfo {
            status: BlockStatus::Volatile,
            intent: BlockIntent::Apply,
            slot: n,
            number: n,
            hash: BlockHash::default(),
            epoch: 0,
            epoch_slot: n,
            new_epoch: false,
            timestamp: n,
            era: Era::default(),
            tip_slot: None,
        }
    }

    fn ucr(n: u64, hdr: usize, body: usize) -> UpstreamCacheRecord {
        UpstreamCacheRecord {
            id: blk(n),
            message: Arc::new(RawBlockMessage {
                header: vec![hdr as u8],
                body: vec![body as u8],
            }),
        }
    }

    #[derive(Default)]
    struct TestStorage {
        rec: HashMap<usize, Vec<UpstreamCacheRecord>>,
    }

    impl Storage for TestStorage {
        fn read_chunk(&mut self, chunk_no: usize) -> Result<Vec<UpstreamCacheRecord>> {
            Ok(self.rec.get(&chunk_no).unwrap_or(&vec![]).clone())
        }

        fn write_chunk(&mut self, chunk_no: usize, chunk: &[UpstreamCacheRecord]) -> Result<()> {
            self.rec.insert(chunk_no, chunk.to_vec());
            Ok(())
        }
    }

    #[test]
    fn test_empty_write_read() -> Result<()> {
        let mut cache = UpstreamCacheImpl::<TestStorage>::new_impl(TestStorage::default());
        cache.start_reading()?;
        assert!(cache.read_record()?.is_none());
        Ok(())
    }

    #[test]
    fn test_write_read() -> Result<()> {
        let mut cache = UpstreamCacheImpl::<TestStorage>::new_impl(TestStorage::default());
        cache.density = 3;
        let perm: [u64; 11] = [3, 1, 4, 1, 5, 9, 2, 6, 5, 3, 5];

        for (n, p) in perm.iter().enumerate() {
            cache.write_record(&ucr(*p, n, n + 100))?;
        }

        assert_eq!(cache.storage.rec.len(), 4);
        for ch in 0..3 {
            let chunk = cache.storage.rec.get(&ch).unwrap();
            assert_eq!(chunk.first().unwrap().id.number, perm[ch * 3]);
            assert_eq!(chunk.get(1).unwrap().id.number, perm[ch * 3 + 1]);
            assert_eq!(chunk.get(2).unwrap().id.number, perm[ch * 3 + 2]);
            assert_eq!(chunk.len(), 3);
        }
        assert_eq!(
            cache.storage.rec.get(&3).unwrap().first().unwrap().id.number,
            perm[9]
        );
        assert_eq!(
            cache.storage.rec.get(&3).unwrap().get(1).unwrap().id.number,
            perm[10]
        );
        assert_eq!(cache.storage.rec.get(&3).unwrap().len(), 2);

        cache.start_reading()?;
        for (n, p) in perm.iter().enumerate() {
            let record = cache.read_record()?.unwrap();
            assert_eq!(record.id.number, *p);
            assert_eq!(record.message.header, vec![n as u8]);
            assert_eq!(record.message.body, vec![(n + 100) as u8]);

            cache.next_record()?;
        }
        assert!(cache.read_record()?.is_none());
        Ok(())
    }

    #[test]
    fn test_end_of_cache_reading() -> Result<()> {
        let mut cache = UpstreamCacheImpl::<TestStorage>::new_impl(TestStorage::default());
        cache.density = 3;

        for edge in 0..11 {
            cache.storage.rec.clear();
            cache.start_reading()?;

            for n in 0..edge {
                cache.write_record(&ucr(n, n as usize, n as usize))?;
            }

            cache.start_reading()?;
            for n in 0..edge {
                assert_eq!(cache.read_record()?.unwrap().id.number, n);
                cache.next_record()?;
            }
            assert!(cache.read_record()?.is_none());
            cache.next_record()?;
            assert!(cache.read_record()?.is_none());
            cache.next_record()?;
            assert!(cache.read_record()?.is_none());

            for n in edge..11 {
                cache.write_record(&ucr(n, n as usize, n as usize))?;
            }

            cache.start_reading()?;
            for n in 0..11 {
                assert_eq!(cache.read_record()?.unwrap().id.number, n);
                cache.next_record()?;
            }

            assert!(cache.read_record()?.is_none());
        }
        Ok(())
    }
}
