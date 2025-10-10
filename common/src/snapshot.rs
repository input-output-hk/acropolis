//! Snapshot parsing utilities (Conway+ only) — scaffold
//!
//! This module provides a minimal public API for the operator CLI to call.
//! Implementations will be filled in Phase 2. For now, functions return
//! placeholder strings to enable CLI wiring without breaking the build.

use anyhow::{bail, Result};
use std::fmt::{Display, Formatter};
use std::path::Path;

/// Sections that can be displayed from the snapshot
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Params,
    Governance,
    Pools,
    Accounts,
    Utxo,
}

impl Display for Section {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Section::Params => write!(f, "params"),
            Section::Governance => write!(f, "governance"),
            Section::Pools => write!(f, "pools"),
            Section::Accounts => write!(f, "accounts"),
            Section::Utxo => write!(f, "utxo"),
        }
    }
}

/// Produce a human-readable summary string for an Amaru snapshot.
///
/// Placeholder implementation: validates the file exists and returns a stub.
pub fn snapshot_summary(path: &Path) -> Result<String> {
    if !path.exists() {
        bail!("snapshot file not found: {}", path.display());
    }
    Ok(format!(
        "[summary] Snapshot at {} — Conway+ parser not implemented yet",
        path.display()
    ))
}

/// Produce human-readable output for selected sections.
///
/// Placeholder implementation: validates the file exists and returns a stub.
pub fn snapshot_sections(path: &Path, sections: &[Section]) -> Result<String> {
    if !path.exists() {
        bail!("snapshot file not found: {}", path.display());
    }
    let list = sections
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    Ok(format!(
        "[sections] {} from {} — Conway+ parser not implemented yet",
        list,
        path.display()
    ))
}

/// Bootstrap the node from a snapshot by dispatching per-module data.
///
/// Placeholder implementation: returns an error to indicate not implemented.
pub fn snapshot_bootstrap(path: &Path) -> Result<()> {
    if !path.exists() {
        bail!("snapshot file not found: {}", path.display());
    }
    bail!("snapshot bootstrap not implemented yet")
}

// End of scaffold
