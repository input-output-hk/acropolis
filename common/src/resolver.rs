//! # Resolver Module
//!
//! This module provides a uniform mechanism for resolving byte regions from either inline data or memory-mapped files.
//! It includes a registry for managing memory-mapped objects, a locator type for referencing regions, and a resolver for extracting slices.
//!
//! ## Features
//! - Registry for memory-mapped files with eviction support
//! - Uniform locator for inline or registry-backed data
//! - Thread-safe, concurrent access
//! - Out-of-bounds and overflow safety
//!
//! ## Example
//! ```rust
//! use acropolis_common::resolver::{Registry, Resolver, Loc, StoreId, ObjectId, Region};
//! // ... see tests for usage ...
//! ```

use anyhow::{bail, Context, Result};
use bytes::Bytes;
use dashmap::DashMap;
use memmap2::Mmap;
use serde::{Deserialize, Serialize};
use std::{fs::File, ops::Range, sync::Arc};

/***
Thoughts on eviction policy for the registry:
  - Rollback safety: keep at least k + safe_zone worth of recently-touched
  objects pinned in the registry; evict older ones.
  - LRU/TTL: track last_access_slot per object (update on resolve);
  run a periodic GC that calls evict for cold objects.
  - Snapshot barrier: after you create a new epoch snapshot,
  evict all objects strictly older than that snapshot (beyond rollback window).
Disk vs memory:
  munmap frees address space and resident pages for the mapping; the file
  stays on disk. If you also want to reclaim disk space, unlink the file
  after registration and keep the FD/mapping alive (or use memfd); the kernel
  discards it when all references are gone.
***/

/// Unique identifier for a storage backend (e.g., file, database).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StoreId(pub u16);

/// Unique identifier for an object within a store.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ObjectId(pub u128);

/// A region within an object, specified by offset and length.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Region {
    /// Byte offset from the start of the object.
    pub offset: u64,
    /// Length of the region in bytes.
    pub len: u32,
}

/// Uniform locator for all cases (inline or registry-backed) that doesn't use strings.
///
/// If `inline` is `Some`, the region is resolved from the provided bytes; otherwise, it is resolved from the registry.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Loc {
    /// Store identifier.
    pub store: StoreId,
    /// Object identifier within the store.
    pub object: ObjectId,
    /// Region to resolve.
    pub region: Region,
    /// Optional inline bytes for direct resolution.
    pub inline: Option<Bytes>,
}

enum Backing {
    Mmap(Arc<Mmap>),
    // add Memfd(Arc<Mmap>), Blob(Arc<[u8]>), etc., as needed
}

/// Thread-safe registry for memory-mapped objects.
#[derive(Default)]
pub struct Registry {
    map: DashMap<(StoreId, ObjectId), Backing>,
}

impl Registry {
    /// Register a file for memory-mapped access in the registry.
    pub fn register_file(&self, store: StoreId, object: ObjectId, file: &File) -> Result<()> {
        // SAFETY: This is safe because:
        // 1. We assume the file is stable (not truncated/modified during use)
        // 2. The mmap is wrapped in Arc for safe sharing across threads
        // 3. All access is bounds-checked in the resolve() method
        // 4. The file reference ensures the file descriptor stays valid
        let mmap = unsafe { Mmap::map(file) }.context("mmap failed")?;
        self.map.insert((store, object), Backing::Mmap(Arc::new(mmap)));
        Ok(())
    }

    /// Remove an object from the registry. If other holders (Resolved) still exist,
    /// memory is freed later when those holders are dropped.
    pub fn evict(&self, store: StoreId, object: ObjectId) -> bool {
        self.map.remove(&(store, object)).is_some()
    }

    /// Get a backing object from the registry, if present.
    fn get(&self, store: StoreId, object: ObjectId) -> Option<Backing> {
        self.map.get(&(store, object)).map(|e| match &*e {
            Backing::Mmap(m) => Backing::Mmap(Arc::clone(m)),
        })
    }
}

/// A resolved region, holding a reference to the underlying memory.
pub struct Resolved {
    backing: ResolvedBacking, // owns an Arc (or Bytes) to keep memory alive
    range: Range<usize>,
}

enum ResolvedBacking {
    Inline(Bytes),
    Mmap(Arc<Mmap>),
}

impl Resolved {
    /// Returns the resolved region as a byte slice.
    pub fn as_slice(&self) -> &[u8] {
        match &self.backing {
            ResolvedBacking::Inline(b) => &b[self.range.clone()],
            ResolvedBacking::Mmap(m) => &m[self.range.clone()],
        }
    }
}

/// Resolves regions from either inline data or registered memory-mapped files.
pub struct Resolver<'r> {
    registry: &'r Registry,
}

impl<'r> Resolver<'r> {
    /// Create a new resolver with a reference to a registry.
    pub fn new(registry: &'r Registry) -> Self {
        Self { registry }
    }

    /// Resolve a region described by `loc` into a `Resolved` view.
    ///
    /// Returns an error if the region is out of bounds or not found.
    pub fn resolve(&self, loc: &Loc) -> Result<Resolved> {
        let start = loc.region.offset as usize;
        let end = start.checked_add(loc.region.len as usize).context("range overflow")?;

        if let Some(bytes) = &loc.inline {
            if end > bytes.len() {
                bail!("inline payload shorter than region");
            }
            return Ok(Resolved {
                backing: ResolvedBacking::Inline(bytes.clone()),
                range: start..end,
            });
        }

        let backing = self.registry.get(loc.store, loc.object).context("object not found")?;

        match backing {
            Backing::Mmap(mm) => {
                if end > mm.len() {
                    bail!("region out of bounds");
                }
                Ok(Resolved {
                    backing: ResolvedBacking::Mmap(mm),
                    range: start..end,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use bytes::Bytes;
    use std::fs::{File, OpenOptions};
    use std::io::Write;
    use std::time::{SystemTime, UNIX_EPOCH};
    // use std::{sync::Arc, thread};

    fn unique_path() -> std::path::PathBuf {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        std::env::temp_dir().join(format!("acropolis_resolver_{nanos}.bin"))
    }

    fn create_file_with(bytes: &[u8]) -> Result<File> {
        let path = unique_path();
        let mut f =
            OpenOptions::new().read(true).write(true).create(true).truncate(true).open(&path)?;
        f.write_all(bytes)?;
        f.sync_all()?; // ensure size/content visible to mmap
                       // Reopen read-only (optional, but mirrors production “reader” role)
        drop(f);
        let f = OpenOptions::new().read(true).open(&path)?;
        Ok(f)
    }

    #[test]
    fn resolves_inline_payload() -> Result<()> {
        let reg = Registry::default();
        let resolver = Resolver::new(&reg);

        let payload = Bytes::from_static(b"\x82\x01\x02\x03\x04\x05"); // small CBOR-ish bytes
        let loc = Loc {
            store: StoreId(0),
            object: ObjectId(0),
            region: Region { offset: 2, len: 3 }, // expect [0x02,0x03,0x04]
            inline: Some(payload.clone()),
        };

        let r = resolver.resolve(&loc)?;
        assert_eq!(r.as_slice(), &payload[2..5]);

        // Out of bounds on inline should error
        let bad = Loc {
            region: Region { offset: 0, len: 99 },
            ..loc.clone()
        };
        assert!(resolver.resolve(&bad).is_err());
        Ok(())
    }

    #[test]
    fn register_and_resolve_file_slice() -> Result<()> {
        // File content: 0..=255 twice
        let mut bytes = Vec::with_capacity(512);
        bytes.extend(0u8..=255u8);
        bytes.extend(0u8..=255u8);

        let file = create_file_with(&bytes)?;
        let reg = Registry::default();
        let store = StoreId(1);
        let obj = ObjectId(0xDEAD_BEEF_A11C_E55Au128);

        reg.register_file(store, obj, &file)?;

        let resolver = Resolver::new(&reg);
        let loc = Loc {
            store,
            object: obj,
            region: Region {
                offset: 100,
                len: 20,
            },
            inline: None,
        };

        let r = resolver.resolve(&loc)?;
        assert_eq!(r.as_slice(), &bytes[100..120]);
        Ok(())
    }

    #[test]
    fn evict_makes_future_resolves_fail_but_existing_views_survive() -> Result<()> {
        // Create a small file
        let bytes = (0u8..=63u8).collect::<Vec<_>>();
        let file = create_file_with(&bytes)?;

        let reg = Registry::default();
        let store = StoreId(2);
        let obj = ObjectId(42);

        reg.register_file(store, obj, &file)?;
        let resolver = Resolver::new(&reg);

        // Take a view
        let loc = Loc {
            store,
            object: obj,
            region: Region { offset: 8, len: 8 },
            inline: None,
        };
        let view = resolver.resolve(&loc)?; // holds Arc to mmap internally

        // Evict from registry
        assert!(reg.evict(store, obj));
        // Second evict is idempotent (nothing to remove)
        assert!(!reg.evict(store, obj));

        // Existing view is still readable (Arc kept it alive)
        assert_eq!(view.as_slice(), &bytes[8..16]);

        // New resolves must fail after eviction
        assert!(resolver.resolve(&loc).is_err());
        Ok(())
    }

    #[test]
    fn resolve_out_of_bounds_fails() -> Result<()> {
        let file_bytes = vec![1u8, 2, 3, 4, 5];
        let file = create_file_with(&file_bytes)?;
        let reg = Registry::default();
        let store = StoreId(3);
        let obj = ObjectId(7);
        reg.register_file(store, obj, &file)?;

        let resolver = Resolver::new(&reg);
        // Ask past end of file
        let loc = Loc {
            store,
            object: obj,
            region: Region { offset: 4, len: 4 }, // end=8 > len=5
            inline: None,
        };
        assert!(resolver.resolve(&loc).is_err());
        Ok(())
    }

    #[test]
    fn range_overflow_fails_early() -> Result<()> {
        let reg = Registry::default();
        let resolver = Resolver::new(&reg);

        let loc = Loc {
            store: StoreId(9),
            object: ObjectId(9),
            region: Region {
                offset: u64::MAX - 5,
                len: 16, // offset + len overflows usize/checked_add
            },
            inline: Some(Bytes::from_static(&[0u8; 32])),
        };
        assert!(resolver.resolve(&loc).is_err());
        Ok(())
    }

    #[test]
    fn concurrent_resolves_share_backing() -> Result<()> {
        use std::sync::Arc;

        // 1) Create file content.
        let mut bytes = Vec::with_capacity(1024);
        for i in 0..1024u32 {
            bytes.push((i % 251) as u8);
        }

        // 2) Pre-compute the expected slice and share it via Arc.
        let expected: Arc<Vec<u8>> = Arc::new(bytes[128..384].to_vec());

        // 3) Register the file once.
        let file = create_file_with(&bytes)?;
        let reg = Arc::new(Registry::default());
        let store = StoreId(11);
        let obj = ObjectId(0xABCD);
        reg.register_file(store, obj, &file)?;

        let loc = Loc {
            store,
            object: obj,
            region: Region {
                offset: 128,
                len: 256,
            },
            inline: None,
        };

        // 4) Resolve in parallel without ever capturing `bytes`.
        let mut handles = Vec::new();
        for _ in 0..8 {
            let reg_cloned = Arc::clone(&reg);
            let loc_cloned = loc.clone();
            let expected_cloned = Arc::clone(&expected);

            handles.push(std::thread::spawn(move || {
                let resolver = Resolver::new(&reg_cloned);
                let r = resolver.resolve(&loc_cloned).expect("resolve");
                assert_eq!(r.as_slice(), &expected_cloned[..]);
            }));
        }

        // 4) Concurrently, sanity check in the main thread.
        let resolver_main = Resolver::new(&reg);
        let r_main = resolver_main.resolve(&loc)?;
        assert_eq!(r_main.as_slice(), &expected[..]);

        for h in handles {
            h.join().expect("thread join ok");
        }
        Ok(())
    }
}
