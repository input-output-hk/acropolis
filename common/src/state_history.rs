//! Generic state history
//! Keeps per-block state for rollbacks or per-epoch state for historical lookups
//! Use imbl collections in the state to avoid memory explosion!

use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::VecDeque,
    fs::{self, File},
    io::Write,
    path::Path,
};
use tracing::info;

use crate::params::SECURITY_PARAMETER_K;

pub const DEFAULT_DUMP_INDEX: &str = "startup.dump-state-block";

pub fn debug_fingerprint<T: Serialize>(value: &T) -> String {
    match bincode::serialize(value) {
        Ok(bytes) => {
            let mut hasher = Sha256::new();
            hasher.update(&bytes);
            let digest = hasher.finalize();
            format!("len={} sha256={}", bytes.len(), hex::encode(digest))
        }
        Err(e) => format!("serialize_error={e}"),
    }
}

pub fn serialize_with_bincode<T: Serialize>(value: &T) -> Result<Vec<u8>, String> {
    bincode::serialize(value).map_err(|e| e.to_string())
}

pub fn summary_with_fingerprint<T: Serialize>(value: &T) -> String {
    debug_fingerprint(value)
}

type SerializerFn<S> = fn(&S) -> Result<Vec<u8>, String>;
type SummaryFn<S> = fn(&S) -> String;
type ArtifactDumperFn<S> = fn(&str, &S);
type MismatchReporterFn<S> = fn(&str, &S);

pub enum StateHistoryStore {
    Bounded(u64), // Used for rollbacks, bounded at k
    Unbounded,    // Used for historical lookups, unbounded
}

impl StateHistoryStore {
    pub fn default_block_store() -> Self {
        Self::Bounded(SECURITY_PARAMETER_K)
    }
    pub fn default_epoch_store() -> Self {
        Self::Bounded(2)
    }
}

struct HistoryEntry<S> {
    index: u64,
    state: S,
}

/// Generic state history - S is the state to be stored
pub struct StateHistory<S> {
    /// History, one per block or epoch
    history: VecDeque<HistoryEntry<S>>,

    /// Module name
    module: String,

    store: StateHistoryStore,

    dump_index: Option<u64>,

    rolled_back: bool,

    serializer_fn: Option<SerializerFn<S>>,

    summary_fn: Option<SummaryFn<S>>,

    artifact_dumper_fn: Option<ArtifactDumperFn<S>>,

    mismatch_reporter_fn: Option<MismatchReporterFn<S>>,
}

impl<S: Clone + Default> StateHistory<S> {
    /// Construct
    pub fn new(module: &str, store: StateHistoryStore) -> Self {
        Self {
            history: VecDeque::new(),
            module: module.to_string(),
            store,
            dump_index: None,
            rolled_back: false,
            serializer_fn: None,
            summary_fn: None,
            artifact_dumper_fn: None,
            mismatch_reporter_fn: None,
        }
    }

    pub fn with_dump_index(mut self, dump_index: Option<u64>) -> Self {
        self.dump_index = dump_index;
        self
    }

    pub fn with_serializer(mut self, serializer_fn: SerializerFn<S>) -> Self {
        self.serializer_fn = Some(serializer_fn);
        self
    }

    pub fn with_summary(mut self, summary_fn: SummaryFn<S>) -> Self {
        self.summary_fn = Some(summary_fn);
        self
    }

    pub fn with_artifact_dumper(mut self, artifact_dumper_fn: ArtifactDumperFn<S>) -> Self {
        self.artifact_dumper_fn = Some(artifact_dumper_fn);
        self
    }

    pub fn with_mismatch_reporter(mut self, mismatch_reporter_fn: MismatchReporterFn<S>) -> Self {
        self.mismatch_reporter_fn = Some(mismatch_reporter_fn);
        self
    }

    /// Get the current state (if any), direct ref
    pub fn current(&self) -> Option<&S> {
        self.history.back().map(|entry| &entry.state)
    }

    /// Get the current state assuming any rollback has been done
    /// Cloned for modification - call commit() when done
    pub fn get_current_state(&self) -> S {
        self.history.back().map(|entry| entry.state.clone()).unwrap_or_default()
    }

    /// Get all the states references in the history
    pub fn values(&self) -> Vec<&S> {
        self.history.iter().map(|entry| &entry.state).collect()
    }

    /// Get the previous state for the given block, handling rollbacks if required
    /// State returned is cloned ready for modification - call commit() when done
    pub fn get_rolled_back_state(&mut self, index: u64) -> S {
        while let Some(entry) = self.history.back() {
            if entry.index >= index {
                tracing::debug!(
                    "{} rolling back state to {} removing block {}",
                    self.module,
                    index,
                    entry.index
                );
                self.history.pop_back();
            } else {
                break;
            }
        }
        self.rolled_back = true;
        self.get_current_state()
    }

    /// Get the state for a given index (if any), direct ref
    pub fn get_by_index(&self, index: u64) -> Option<&S> {
        self.history.iter().find(|entry| entry.index == index).map(|entry| &entry.state)
    }

    /// Get the most recently stored state at or beore a given index, direct ref
    pub fn get_at_or_before(&self, index: u64) -> Option<&S> {
        self.history.iter().rev().find(|entry| entry.index <= index).map(|entry| &entry.state)
    }

    /// Return a reference to the state at the given block number, if it exists
    pub fn get_by_index_reverse(&self, index: u64) -> Option<&S> {
        self.history.iter().rev().find(|entry| entry.index == index).map(|entry| &entry.state)
    }

    /// Get state history's size
    pub fn len(&self) -> usize {
        self.history.len()
    }

    /// Get if state history is empty
    pub fn is_empty(&self) -> bool {
        self.history.is_empty()
    }

    /// Commit new state without checking the block number
    /// TODO: enhance block number logic to commit state without check (for bootstrapping)
    pub fn commit_forced(&mut self, state: S) {
        self.history.push_back(HistoryEntry { index: 0, state });
    }

    pub fn bootstrap_init_with(&mut self, state: S, index: u64) {
        self.history.push_back(HistoryEntry { index, state });
    }

    /// Commit the new state
    pub fn commit(&mut self, index: u64, state: S) {
        match self.store {
            StateHistoryStore::Bounded(k) => {
                while let Some(entry) = self.history.front() {
                    if index.saturating_sub(entry.index) > k {
                        self.history.pop_front();
                    } else {
                        break;
                    }
                }
                self.history.push_back(HistoryEntry { index, state });
            }
            StateHistoryStore::Unbounded => {
                self.history.push_back(HistoryEntry { index, state });
            }
        }

        self.maybe_dump_or_compare(index);
    }

    fn maybe_dump_or_compare(&self, index: u64) {
        if self.dump_index != Some(index) || self.serializer_fn.is_none() {
            return;
        }

        if self.rolled_back && self.dump_exists() {
            if self.compare_states() {
                tracing::info!("{} rollback validation success", self.module);
            } else {
                tracing::error!("{} rollback validation failed", self.module);
            }
        } else {
            if self.rolled_back {
                tracing::warn!(
                    "{} no rollback baseline found at index {}, dumping current state instead",
                    self.module,
                    index
                );
            }
            self.dump_to_file();
        }
    }

    fn dump_exists(&self) -> bool {
        Path::new(&self.module).exists()
    }

    fn summary_path(&self) -> Option<String> {
        self.summary_fn.map(|_| format!("{}.summary", self.module))
    }

    fn serialize_state(&self, state: &S) -> Result<Vec<u8>, String> {
        let serializer = self
            .serializer_fn
            .ok_or_else(|| format!("no serializer configured for {}", self.module))?;
        serializer(state)
    }

    fn dump_to_file(&self) {
        let Some(entry) = self.history.back() else {
            info!("{} no state to dump", self.module);
            return;
        };

        let bytes = match self.serialize_state(&entry.state) {
            Ok(bytes) => bytes,
            Err(e) => {
                tracing::error!("{e}");
                return;
            }
        };

        let mut file = match File::create(&self.module) {
            Ok(file) => file,
            Err(e) => {
                tracing::error!("{}", e);
                return;
            }
        };

        if let Err(e) = file.write_all(&bytes) {
            tracing::error!("{}", e);
            return;
        }

        info!(
            "{} dumped state at index {} to {} ({} bytes)",
            self.module,
            entry.index,
            self.module,
            bytes.len()
        );

        self.dump_summary_to_file(&entry.state);
        if let Some(artifact_dumper_fn) = self.artifact_dumper_fn {
            artifact_dumper_fn(&self.module, &entry.state);
        }
    }

    fn dump_summary_to_file(&self, state: &S) {
        let (Some(summary_fn), Some(summary_path)) = (self.summary_fn, self.summary_path()) else {
            return;
        };

        if let Err(e) = fs::write(&summary_path, summary_fn(state)) {
            tracing::error!("failed to write {} summary: {}", self.module, e);
        }
    }

    fn compare_states(&self) -> bool {
        let bytes_pre = match fs::read(&self.module) {
            Ok(bytes) => bytes,
            Err(e) => {
                tracing::error!("failed to read {}: {}", self.module, e);
                return false;
            }
        };

        let Some(after_state) = self.history.back().map(|entry| &entry.state) else {
            info!("{} no current state to compare", self.module);
            return false;
        };

        let bytes_after = match self.serialize_state(after_state) {
            Ok(bytes) => bytes,
            Err(e) => {
                tracing::error!("{e}");
                return false;
            }
        };

        let matches = bytes_pre == bytes_after;
        if !matches {
            self.log_summary_diff(after_state);
            if let Some(mismatch_reporter_fn) = self.mismatch_reporter_fn {
                mismatch_reporter_fn(&self.module, after_state);
            }
        }
        matches
    }

    fn log_summary_diff(&self, after_state: &S) {
        let (Some(summary_fn), Some(summary_path)) = (self.summary_fn, self.summary_path()) else {
            return;
        };

        let current_summary = summary_fn(after_state);

        match fs::read_to_string(&summary_path) {
            Ok(baseline_summary) => {
                tracing::error!(
                    module = %self.module,
                    baseline_summary = %baseline_summary,
                    "rollback baseline summary"
                );
            }
            Err(e) => {
                tracing::error!("failed to read {} summary: {}", self.module, e);
            }
        }

        tracing::error!(
            module = %self.module,
            current_summary = %current_summary,
            "rollback current summary"
        );
    }
}

/// Helper that lets callers initialize the first state with custom config.
impl<S: Clone> StateHistory<S> {
    pub fn get_or_init_with<F>(&mut self, init: F) -> S
    where
        F: FnOnce() -> S,
    {
        if let Some(current) = self.history.back() {
            current.state.clone()
        } else {
            init()
        }
    }

    /// Clear the history
    pub fn clear(&mut self) {
        self.history.clear();
    }
}
