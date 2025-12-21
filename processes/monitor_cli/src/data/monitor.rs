//! Monitor data parsing and health computation.
//!
//! This module transforms raw monitor snapshots into processed data
//! with health status computed based on configurable thresholds.

use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::Result;

use super::duration::parse_duration;
use crate::source::MonitorSnapshot;

/// Thresholds for health status computation.
///
/// These thresholds determine when a topic or module is considered
/// in warning or critical state.
#[derive(Debug, Clone)]
pub struct Thresholds {
    /// Duration after which a pending read/write triggers a warning.
    pub pending_warning: Duration,
    /// Duration after which a pending read/write triggers critical status.
    pub pending_critical: Duration,
    /// Unread message count that triggers a warning.
    pub unread_warning: u64,
    /// Unread message count that triggers critical status.
    pub unread_critical: u64,
}

impl Default for Thresholds {
    fn default() -> Self {
        Self {
            pending_warning: Duration::from_secs(1),
            pending_critical: Duration::from_secs(10),
            unread_warning: 1000,
            unread_critical: 5000,
        }
    }
}

/// Health status for a module or topic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum HealthStatus {
    Healthy,
    Warning,
    Critical,
}

impl HealthStatus {
    /// Returns a short symbol for display.
    pub fn symbol(&self) -> &'static str {
        match self {
            HealthStatus::Healthy => "OK",
            HealthStatus::Warning => "WARN",
            HealthStatus::Critical => "CRIT",
        }
    }
}

/// Parsed topic read data with computed health status.
#[derive(Debug, Clone)]
pub struct TopicRead {
    pub topic: String,
    pub read: u64,
    pub pending_for: Option<Duration>,
    pub unread: Option<u64>,
    pub status: HealthStatus,
}

/// Parsed topic write data with computed health status.
#[derive(Debug, Clone)]
pub struct TopicWrite {
    pub topic: String,
    pub written: u64,
    pub pending_for: Option<Duration>,
    pub status: HealthStatus,
}

/// Parsed module data with aggregated statistics and health.
#[derive(Debug, Clone)]
pub struct ModuleData {
    pub name: String,
    pub reads: Vec<TopicRead>,
    pub writes: Vec<TopicWrite>,
    pub total_read: u64,
    pub total_written: u64,
    pub health: HealthStatus,
}

/// Complete parsed monitor data ready for display.
#[derive(Debug, Clone)]
pub struct MonitorData {
    pub modules: Vec<ModuleData>,
    pub last_updated: Instant,
}

impl MonitorData {
    /// Load and parse monitor data from a JSON file.
    ///
    /// This is the traditional file-based loading method.
    pub fn load(path: &Path, thresholds: &Thresholds) -> Result<Self> {
        let content = fs::read_to_string(path)?;
        Self::parse(&content, thresholds)
    }

    /// Parse monitor data from a JSON string.
    pub fn parse(content: &str, thresholds: &Thresholds) -> Result<Self> {
        let snapshot: MonitorSnapshot = serde_json::from_str(content)?;
        Ok(Self::from_snapshot(snapshot, thresholds))
    }

    /// Convert a MonitorSnapshot into processed MonitorData.
    ///
    /// This is the primary conversion method used by all data sources.
    pub fn from_snapshot(snapshot: MonitorSnapshot, thresholds: &Thresholds) -> Self {
        let mut modules: Vec<ModuleData> = snapshot
            .into_iter()
            .map(|(name, state)| Self::parse_module(name, state, thresholds))
            .collect();

        // Sort by health status (critical first), then by name
        modules.sort_by(|a, b| b.health.cmp(&a.health).then_with(|| a.name.cmp(&b.name)));

        Self {
            modules,
            last_updated: Instant::now(),
        }
    }

    fn parse_module(
        name: String,
        state: crate::source::SerializedModuleState,
        thresholds: &Thresholds,
    ) -> ModuleData {
        let mut reads: Vec<TopicRead> = state
            .reads
            .into_iter()
            .map(|(topic, r)| {
                let pending_for = r.pending_for.as_ref().and_then(|s| parse_duration(s).ok());
                let status = Self::compute_read_status(pending_for, r.unread, thresholds);
                TopicRead {
                    topic,
                    read: r.read,
                    pending_for,
                    unread: r.unread,
                    status,
                }
            })
            .collect();

        let mut writes: Vec<TopicWrite> = state
            .writes
            .into_iter()
            .map(|(topic, w)| {
                let pending_for = w.pending_for.as_ref().and_then(|s| parse_duration(s).ok());
                let status = Self::compute_write_status(pending_for, thresholds);
                TopicWrite {
                    topic,
                    written: w.written,
                    pending_for,
                    status,
                }
            })
            .collect();

        // Sort topics by status (critical first), then by name
        reads.sort_by(|a, b| b.status.cmp(&a.status).then_with(|| a.topic.cmp(&b.topic)));
        writes.sort_by(|a, b| b.status.cmp(&a.status).then_with(|| a.topic.cmp(&b.topic)));

        let total_read = reads.iter().map(|r| r.read).sum();
        let total_written = writes.iter().map(|w| w.written).sum();

        // Module health is the worst of all its topics
        let health = reads
            .iter()
            .map(|r| r.status)
            .chain(writes.iter().map(|w| w.status))
            .max()
            .unwrap_or(HealthStatus::Healthy);

        ModuleData {
            name,
            reads,
            writes,
            total_read,
            total_written,
            health,
        }
    }

    fn compute_read_status(
        pending_for: Option<Duration>,
        unread: Option<u64>,
        thresholds: &Thresholds,
    ) -> HealthStatus {
        let pending_status = pending_for.map_or(HealthStatus::Healthy, |d| {
            if d >= thresholds.pending_critical {
                HealthStatus::Critical
            } else if d >= thresholds.pending_warning {
                HealthStatus::Warning
            } else {
                HealthStatus::Healthy
            }
        });

        let unread_status = unread.map_or(HealthStatus::Healthy, |u| {
            if u >= thresholds.unread_critical {
                HealthStatus::Critical
            } else if u >= thresholds.unread_warning {
                HealthStatus::Warning
            } else {
                HealthStatus::Healthy
            }
        });

        pending_status.max(unread_status)
    }

    fn compute_write_status(
        pending_for: Option<Duration>,
        thresholds: &Thresholds,
    ) -> HealthStatus {
        pending_for.map_or(HealthStatus::Healthy, |d| {
            if d >= thresholds.pending_critical {
                HealthStatus::Critical
            } else if d >= thresholds.pending_warning {
                HealthStatus::Warning
            } else {
                HealthStatus::Healthy
            }
        })
    }

    /// Get all unhealthy topics across all modules.
    pub fn unhealthy_topics(&self) -> Vec<(&ModuleData, UnhealthyTopic)> {
        let mut result = Vec::new();

        for module in &self.modules {
            for read in &module.reads {
                if read.status != HealthStatus::Healthy {
                    result.push((module, UnhealthyTopic::Read(read.clone())));
                }
            }
            for write in &module.writes {
                if write.status != HealthStatus::Healthy {
                    result.push((module, UnhealthyTopic::Write(write.clone())));
                }
            }
        }

        // Sort by status (critical first)
        result.sort_by(|a, b| b.1.status().cmp(&a.1.status()));
        result
    }
}

/// An unhealthy topic (either read or write).
#[derive(Debug, Clone)]
pub enum UnhealthyTopic {
    Read(TopicRead),
    Write(TopicWrite),
}

impl UnhealthyTopic {
    pub fn status(&self) -> HealthStatus {
        match self {
            UnhealthyTopic::Read(r) => r.status,
            UnhealthyTopic::Write(w) => w.status,
        }
    }

    pub fn topic(&self) -> &str {
        match self {
            UnhealthyTopic::Read(r) => &r.topic,
            UnhealthyTopic::Write(w) => &w.topic,
        }
    }

    pub fn pending_for(&self) -> Option<Duration> {
        match self {
            UnhealthyTopic::Read(r) => r.pending_for,
            UnhealthyTopic::Write(w) => w.pending_for,
        }
    }
}
