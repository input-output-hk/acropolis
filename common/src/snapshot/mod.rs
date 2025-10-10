//! Snapshot parsing utilities (Conway+ only)
//!
//! This module provides parsing and inspection capabilities for Cardano snapshots,
//! with a focus on the Amaru/Haskell node EpochState format used in Conway+ era.
//!
//! ## Module Structure
//! - `snapshot`: Full-featured Amaru snapshot parser with memory-efficient streaming
//! - `error`: Error types for snapshot parsing operations
//! - Public API for CLI integration (summary, sections, bootstrap)

pub mod snapshot;
pub mod error;

// Re-export commonly used types
pub use error::SnapshotError;
pub use snapshot::{
    AmaruSnapshot, EpochStateMetadata, SnapshotData, TipInfo, UtxoEntry,
    extract_tip_from_filename, estimate_block_height_from_slot,
};

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
/// This function extracts minimal metadata without loading the entire file:
/// - Epoch number (validates Conway+ era)
/// - File size
/// - Treasury and reserves balances
/// - Estimated counts for key resources
///
/// For multi-GB snapshots, this is significantly faster than full parsing.
pub fn snapshot_summary(path: &Path) -> Result<String> {
    if !path.exists() {
        bail!("snapshot file not found: {}", path.display());
    }

    let path_str = path.to_str().ok_or_else(|| anyhow::anyhow!("invalid path"))?;

    // Use the snapshot module's extract_boot_data for comprehensive metadata
    let data = snapshot::extract_boot_data(path_str)
        .map_err(|e| anyhow::anyhow!("failed to extract snapshot data: {}", e))?;

    let mut out = String::new();
    out.push_str(&format!("Snapshot: {}\n", path.display()));
    out.push_str(&format!("Era: Conway+ (epoch >= 505)\n"));
    out.push_str(&format!("Epoch: {}\n", data.epoch));
    out.push_str(&format!("File Size: {} bytes\n", data.file_size));
    out.push_str(&format!("Treasury: {} lovelace\n", data.treasury));
    out.push_str(&format!("Reserves: {} lovelace\n", data.reserves));
    out.push_str(&format!("Stake Pools: {}\n", data.stake_pools));
    out.push_str(&format!("DReps: {}\n", data.dreps));
    out.push_str(&format!("Stake Accounts: {}\n", data.stake_accounts));
    out.push_str(&format!("Governance Proposals: {}\n", data.governance_proposals));

    Ok(out)
}

/// Produce human-readable output for selected sections.
///
/// This function extracts specific sections from the snapshot on-demand,
/// avoiding unnecessary parsing of sections the user didn't request.
pub fn snapshot_sections(path: &Path, sections: &[Section]) -> Result<String> {
    if !path.exists() {
        bail!("snapshot file not found: {}", path.display());
    }

    let path_str = path.to_str().ok_or_else(|| anyhow::anyhow!("invalid path"))?;

    // For now, extract all boot data once (future optimization: section-specific extraction)
    let data = snapshot::extract_boot_data(path_str)
        .map_err(|e| anyhow::anyhow!("failed to extract snapshot data: {}", e))?;

    let mut lines: Vec<String> = Vec::new();
    for s in sections {
        match s {
            Section::Params => {
                // Protocol parameters are in [3][1][1][3][3] - not extracted yet
                lines.push("params: (extraction not yet implemented)".to_string());
            }
            Section::Governance => {
                lines.push(format!(
                    "governance: dreps={}, proposals={}",
                    data.dreps, data.governance_proposals
                ));
            }
            Section::Pools => {
                lines.push(format!("pools: {}", data.stake_pools));
            }
            Section::Accounts => {
                lines.push(format!("accounts: {}", data.stake_accounts));
            }
            Section::Utxo => {
                // For UTXO count, we could use count_ledger_state_utxos,
                // but it's slow. For now, just indicate it's available.
                lines.push("utxo: (count available via parse_all_utxos)".to_string());
            }
        }
    }

    if lines.is_empty() {
        lines.push("(no sections requested)".to_string());
    }

    Ok(lines.join("\n"))
}

/// Bootstrap the node from a snapshot by dispatching per-module data.
///
/// This function extracts data from the snapshot and dispatches it to the
/// appropriate state modules for initialization.
///
/// **Warning**: This operation loads significant portions of the snapshot
/// and may take time for multi-GB files. Progress indication is recommended.
pub fn snapshot_bootstrap(path: &Path) -> Result<()> {
    if !path.exists() {
        bail!("snapshot file not found: {}", path.display());
    }

    let path_str = path.to_str().ok_or_else(|| anyhow::anyhow!("invalid path"))?;

    // Extract boot data
    let data = snapshot::extract_boot_data(path_str)
        .map_err(|e| anyhow::anyhow!("failed to extract snapshot data: {}", e))?;

    // TODO: Dispatch to state modules
    // - parameters_state: Protocol parameters
    // - accounts_state: Stake accounts and rewards
    // - spo_state: Stake pool registrations and delegations
    // - drep_state: DRep registrations
    // - governance_state: Active proposals
    // - utxo_state: Transaction outputs (via streaming parse)

    println!("Extracted snapshot data:");
    println!("  Epoch: {}", data.epoch);
    println!("  Treasury: {} lovelace", data.treasury);
    println!("  Reserves: {} lovelace", data.reserves);
    println!("  Stake Pools: {}", data.stake_pools);
    println!("  DReps: {}", data.dreps);
    println!("  Stake Accounts: {}", data.stake_accounts);
    println!("  Governance Proposals: {}", data.governance_proposals);

    bail!("bootstrap dispatch to state modules not yet implemented")
}
