use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use super::monitor::MonitorData;

/// Maximum number of historical snapshots to keep
const MAX_HISTORY_SIZE: usize = 60;

/// Tracks historical data for trending and sparklines
#[derive(Debug, Clone)]
pub struct History {
    /// Historical read counts per module (module_name -> readings)
    pub module_reads: HashMap<String, VecDeque<u64>>,
    /// Historical write counts per module
    pub module_writes: HashMap<String, VecDeque<u64>>,
    /// Previous snapshot for computing deltas
    pub previous: Option<Snapshot>,
    /// Timestamps of snapshots
    pub timestamps: VecDeque<Instant>,
}

/// A snapshot of key metrics at a point in time
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub timestamp: Instant,
    pub module_reads: HashMap<String, u64>,
    pub module_writes: HashMap<String, u64>,
    pub module_max_pending: HashMap<String, Duration>,
    pub module_total_unread: HashMap<String, u64>,
}

impl Snapshot {
    pub fn from_monitor_data(data: &MonitorData) -> Self {
        let mut module_reads = HashMap::new();
        let mut module_writes = HashMap::new();
        let mut module_max_pending = HashMap::new();
        let mut module_total_unread = HashMap::new();

        for module in &data.modules {
            module_reads.insert(module.name.clone(), module.total_read);
            module_writes.insert(module.name.clone(), module.total_written);

            // Find max pending_for across all topics
            let max_pending = module
                .reads
                .iter()
                .filter_map(|r| r.pending_for)
                .chain(module.writes.iter().filter_map(|w| w.pending_for))
                .max()
                .unwrap_or(Duration::ZERO);
            module_max_pending.insert(module.name.clone(), max_pending);

            // Sum unread counts
            let total_unread: u64 = module.reads.iter().filter_map(|r| r.unread).sum();
            module_total_unread.insert(module.name.clone(), total_unread);
        }

        Self {
            timestamp: data.last_updated,
            module_reads,
            module_writes,
            module_max_pending,
            module_total_unread,
        }
    }
}

/// Delta information between two snapshots
#[derive(Debug, Clone)]
pub struct Delta {
    pub reads_delta: i64,
    pub writes_delta: i64,
    pub pending_trend: Trend,
    pub unread_trend: Trend,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trend {
    Improving,
    Stable,
    Degrading,
}

impl Trend {
    pub fn symbol(&self) -> &'static str {
        match self {
            Trend::Improving => "↓",
            Trend::Stable => "→",
            Trend::Degrading => "↑",
        }
    }
}

impl Default for History {
    fn default() -> Self {
        Self::new()
    }
}

impl History {
    pub fn new() -> Self {
        Self {
            module_reads: HashMap::new(),
            module_writes: HashMap::new(),
            previous: None,
            timestamps: VecDeque::new(),
        }
    }

    /// Record a new data snapshot
    pub fn record(&mut self, data: &MonitorData) {
        let snapshot = Snapshot::from_monitor_data(data);

        // Store current as previous for delta calculation
        let old_previous = self.previous.take();
        self.previous = Some(snapshot.clone());

        // Record historical values for sparklines
        for module in &data.modules {
            let reads = self.module_reads.entry(module.name.clone()).or_insert_with(VecDeque::new);
            reads.push_back(module.total_read);
            if reads.len() > MAX_HISTORY_SIZE {
                reads.pop_front();
            }

            let writes =
                self.module_writes.entry(module.name.clone()).or_insert_with(VecDeque::new);
            writes.push_back(module.total_written);
            if writes.len() > MAX_HISTORY_SIZE {
                writes.pop_front();
            }
        }

        self.timestamps.push_back(data.last_updated);
        if self.timestamps.len() > MAX_HISTORY_SIZE {
            self.timestamps.pop_front();
        }

        // Keep old_previous reference for first delta
        if self.previous.is_some() && old_previous.is_none() {
            // First recording, no delta yet
        }
    }

    /// Get delta for a module compared to previous snapshot
    pub fn get_delta(&self, module_name: &str) -> Option<Delta> {
        let previous = self.previous.as_ref()?;
        let current_reads = self.module_reads.get(module_name)?.back()?;
        let current_writes = self.module_writes.get(module_name)?.back()?;

        let prev_reads = previous.module_reads.get(module_name)?;
        let prev_writes = previous.module_writes.get(module_name)?;

        // Need at least 2 data points
        let reads_history = self.module_reads.get(module_name)?;
        if reads_history.len() < 2 {
            return None;
        }

        let prev_read = reads_history.get(reads_history.len() - 2)?;
        let prev_write = self.module_writes.get(module_name)?.get(reads_history.len() - 2)?;

        let reads_delta = *current_reads as i64 - *prev_read as i64;
        let writes_delta = *current_writes as i64 - *prev_write as i64;

        // Compute trends based on pending/unread (would need more history)
        // For now, use simple comparison
        let pending_trend = Trend::Stable;
        let unread_trend = Trend::Stable;

        Some(Delta {
            reads_delta,
            writes_delta,
            pending_trend,
            unread_trend,
        })
    }

    /// Get sparkline data for reads (normalized to 0-7 for 8 bar levels)
    pub fn get_reads_sparkline(&self, module_name: &str) -> Vec<u8> {
        self.normalize_sparkline(self.module_reads.get(module_name))
    }

    /// Get sparkline data for writes
    pub fn get_writes_sparkline(&self, module_name: &str) -> Vec<u8> {
        self.normalize_sparkline(self.module_writes.get(module_name))
    }

    fn normalize_sparkline(&self, data: Option<&VecDeque<u64>>) -> Vec<u8> {
        let Some(values) = data else {
            return Vec::new();
        };

        if values.len() < 2 {
            return Vec::new();
        }

        // Compute deltas between consecutive values
        let deltas: Vec<i64> =
            values.iter().zip(values.iter().skip(1)).map(|(a, b)| *b as i64 - *a as i64).collect();

        if deltas.is_empty() {
            return Vec::new();
        }

        let max = deltas.iter().copied().max().unwrap_or(1).max(1);
        let min = deltas.iter().copied().min().unwrap_or(0).min(0);
        let range = (max - min).max(1) as f64;

        deltas
            .iter()
            .map(|&v| {
                let normalized = ((v - min) as f64 / range * 7.0) as u8;
                normalized.min(7)
            })
            .collect()
    }

    /// Get the rate of change (messages per second) for reads
    pub fn get_read_rate(&self, module_name: &str) -> Option<f64> {
        let reads = self.module_reads.get(module_name)?;
        if reads.len() < 2 || self.timestamps.len() < 2 {
            return None;
        }

        let current = *reads.back()?;
        let previous = *reads.get(reads.len() - 2)?;
        let delta = current as i64 - previous as i64;

        let current_time = self.timestamps.back()?;
        let previous_time = self.timestamps.get(self.timestamps.len() - 2)?;
        let elapsed = current_time.duration_since(*previous_time).as_secs_f64();

        if elapsed > 0.0 {
            Some(delta as f64 / elapsed)
        } else {
            None
        }
    }
}
