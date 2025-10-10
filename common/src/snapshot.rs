//! Snapshot parsing utilities (Conway+ only) — scaffold
//!
//! This module provides a minimal public API for the operator CLI to call.
//! Implementations will be filled in Phase 2. For now, functions return
//! placeholder strings to enable CLI wiring without breaking the build.

use anyhow::{anyhow, bail, Context, Result};
use minicbor::{data::Type, decode::Decoder};
use std::fmt::{Display, Formatter};
use std::fs;
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

    let meta = fs::metadata(path).with_context(|| "reading snapshot metadata")?;
    let file_size = meta.len();
    let bytes = fs::read(path).with_context(|| "reading snapshot bytes")?;

    // Epoch
    let epoch = get_epoch(&bytes)?;
    let era_label = if epoch >= 505 { "Conway+" } else { "pre-Conway" };

    // Treasury/Reserves (pots)
    let (treasury, reserves) = get_pots(&bytes)?;

    // UTXO count (may scan entire file)
    let utxo_count = count_utxos(&bytes)?;

    let mut out = String::new();
    out.push_str(&format!("Snapshot: {}\n", path.display()));
    out.push_str(&format!("Era: {}\n", era_label));
    out.push_str(&format!("Epoch: {}\n", epoch));
    out.push_str(&format!("File Size: {} bytes\n", file_size));
    out.push_str(&format!("Treasury: {} lovelace\n", treasury));
    out.push_str(&format!("Reserves: {} lovelace\n", reserves));
    out.push_str(&format!("UTxOs: {}\n", utxo_count));
    Ok(out)
}

/// Produce human-readable output for selected sections.
///
/// Placeholder implementation: validates the file exists and returns a stub.
pub fn snapshot_sections(path: &Path, sections: &[Section]) -> Result<String> {
    if !path.exists() {
        bail!("snapshot file not found: {}", path.display());
    }
    let bytes = fs::read(path).with_context(|| "reading snapshot bytes")?;

    let mut lines: Vec<String> = Vec::new();
    for s in sections {
        match s {
            Section::Params => {
                // Presence-only indicator for now (params at [3][1][1][3][3])
                let present = params_present(&bytes).unwrap_or(false);
                lines.push(format!("params: {}", if present { "present" } else { "missing" }));
            }
            Section::Governance => {
                let (dreps, proposals) = governance_counts(&bytes)?;
                lines.push(format!("governance: dreps={}, proposals={}", dreps, proposals));
            }
            Section::Pools => {
                let pools = pools_count(&bytes)?;
                lines.push(format!("pools: {}", pools));
            }
            Section::Accounts => {
                let accounts = accounts_count(&bytes)?;
                lines.push(format!("accounts: {}", accounts));
            }
            Section::Utxo => {
                let utxos = count_utxos(&bytes)?;
                lines.push(format!("utxo: {}", utxos));
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
/// Placeholder implementation: returns an error to indicate not implemented.
pub fn snapshot_bootstrap(path: &Path) -> Result<()> {
    if !path.exists() {
        bail!("snapshot file not found: {}", path.display());
    }
    bail!("snapshot bootstrap not implemented yet")
}

// End of scaffold

// ---------------------------
// Internal helpers (Phase 2)
// ---------------------------

fn get_epoch(bytes: &[u8]) -> Result<u64> {
    let mut d = Decoder::new(bytes);
    d.array()?; // top-level
    let epoch = d.u64()?;
    Ok(epoch)
}

fn get_pots(bytes: &[u8]) -> Result<(u64, u64)> {
    let mut d = Decoder::new(bytes);
    d.array()?; // top
    d.skip()?; // [0] epoch
    d.skip()?; // [1] prev blocks
    d.skip()?; // [2] curr blocks
    d.array()?; // [3] Epoch State
    d.array()?; // [3][0] Account State
    let treasury: i64 = d.decode()?;
    let reserves: i64 = d.decode()?;
    Ok((treasury as u64, reserves as u64))
}

fn navigate_to_utxo_map(d: &mut Decoder) -> Result<()> {
    d.array()?; // top
    d.skip()?; // [0]
    d.skip()?; // [1]
    d.skip()?; // [2]
    d.array()?; // [3] Epoch State
    d.skip()?; // [3][0] Account State
    d.array()?; // [3][1] Ledger State
    d.skip()?; // [3][1][0] Cert State
    d.array()?; // [3][1][1] UTxO State
    // Now at start of UTxO State; next is [0] map
    Ok(())
}

fn count_utxos(bytes: &[u8]) -> Result<u64> {
    let mut d = Decoder::new(bytes);
    navigate_to_utxo_map(&mut d)?;
    match d.map()? {
        Some(len) => Ok(len),
        None => {
            let mut c = 0u64;
            loop {
                match d.datatype()? {
                    Type::Break => break,
                    _ => {
                        d.skip()?; // key
                        d.skip()?; // value
                        c = c.checked_add(1).ok_or_else(|| anyhow!("overflow counting UTXOs"))?;
                    }
                }
            }
            Ok(c)
        }
    }
}

fn pools_count(bytes: &[u8]) -> Result<u64> {
    // [3][1][0][1][0] = pools map
    let mut d = Decoder::new(bytes);
    d.array()?; // top
    d.skip()?; d.skip()?; d.skip()?;
    d.array()?; // [3]
    d.skip()?; // [3][0]
    d.array()?; // [3][1]
    d.array()?; // [3][1][0] Cert State
    d.skip()?; // [Voting State]
    d.array()?; // [Pool State]
    match d.map()? { // pools map
        Some(len) => Ok(len),
        None => {
            let mut c = 0u64;
            loop {
                match d.datatype()? {
                    Type::Break => break,
                    _ => { d.skip()?; d.skip()?; c += 1; }
                }
            }
            Ok(c)
        }
    }
}

fn accounts_count(bytes: &[u8]) -> Result<u64> {
    // [3][1][0][2][0][0] = credentials map
    let mut d = Decoder::new(bytes);
    d.array()?; // top
    d.skip()?; d.skip()?; d.skip()?;
    d.array()?; // [3]
    d.skip()?; // [3][0]
    d.array()?; // [3][1]
    d.array()?; // [3][1][0] Cert State
    d.skip()?; // Voting State
    d.skip()?; // Pool State
    d.array()?; // Delegation State
    d.array()?; // dsUnified
    match d.map()? { // credentials map
        Some(len) => Ok(len),
        None => {
            let mut c = 0u64;
            loop {
                match d.datatype()? {
                    Type::Break => break,
                    _ => { d.skip()?; d.skip()?; c += 1; }
                }
            }
            Ok(c)
        }
    }
}

fn governance_counts(bytes: &[u8]) -> Result<(u64, u64)> {
    // dreps: [3][1][0][0][0] map; proposals: [3][1][1][3][0][1] array
    let mut d = Decoder::new(bytes);
    d.array()?; // top
    d.skip()?; d.skip()?; d.skip()?;
    d.array()?; // [3]
    d.skip()?; // [3][0]
    d.array()?; // [3][1]
    d.array()?; // [3][1][0] Cert State
    // Voting State
    match d.map()? { // dreps map
        Some(len) => {
            // We consumed a map() call too early if Voting State is array. Adjust:
            // Correct path: Voting State is Array; first element is dreps map.
            // So revert: we should read array then map.
            return governance_counts_slow(bytes);
        }
        None => {
            return governance_counts_slow(bytes);
        }
    }
}

fn governance_counts_slow(bytes: &[u8]) -> Result<(u64, u64)> {
    let mut d = Decoder::new(bytes);
    // Navigate to Voting State
    d.array()?; d.skip()?; d.skip()?; d.skip()?; // top and [0..2]
    d.array()?; d.skip()?; // [3], [3][0]
    d.array()?; // [3][1]
    d.array()?; // [3][1][0] Cert State
    d.array()?; // Voting State (array)
    // dreps map
    let dreps = match d.map()? { Some(len) => len, None => count_indef_map(&mut d)? };
    // cc_members map (skip)
    match d.map()? { Some(len) => { for _ in 0..len { d.skip()?; d.skip()?; } }, None => { loop { match d.datatype()? { Type::Break => break, _ => { d.skip()?; d.skip()?; } } } } }
    d.skip()?; // dormant_epoch

    // Pool State (skip entire array)
    d.array()?; // Pool State
    d.skip()?; d.skip()?; d.skip()?; d.skip()?; // pools, updates, retirements, deposits

    // Delegation State (skip)
    d.array()?; d.array()?; d.skip()?; d.skip()?; // dsUnified, credentials map (skip), pointers
    d.skip()?; d.skip()?; // dsFutureGenDelegs, dsGenDelegs
    d.skip()?; // dsIRewards

    // Move to UTxO State and then governance proposals
    d.array()?; // UTxO State
    d.skip()?; d.skip()?; d.skip()?; // utxo, deposited, fees
    d.array()?; // utxosGovState
    d.array()?; // Proposals [roots, proposals]
    d.skip()?; // roots
    // proposals array
    let proposals = match d.array()? { Some(len) => len, None => count_indef_array(&mut d)? };
    Ok((dreps, proposals))
}

fn count_indef_map(d: &mut Decoder) -> Result<u64> {
    let mut c = 0u64;
    loop {
        match d.datatype()? {
            Type::Break => break,
            _ => {
                d.skip()?; d.skip()?; c += 1;
            }
        }
    }
    Ok(c)
}

fn count_indef_array(d: &mut Decoder) -> Result<u64> {
    let mut c = 0u64;
    loop {
        match d.datatype()? {
            Type::Break => break,
            _ => { d.skip()?; c += 1; }
        }
    }
    Ok(c)
}

fn params_present(bytes: &[u8]) -> Result<bool> {
    // Navigate to UTxO State[3] governance state and read [3] current params presence
    let mut d = Decoder::new(bytes);
    d.array()?; d.skip()?; d.skip()?; d.skip()?; // top and skips
    d.array()?; d.skip()?; // Epoch State, Account State
    d.array()?; d.skip()?; // Ledger State, Cert State
    d.array()?; // UTxO State
    d.skip()?; d.skip()?; d.skip()?; // utxo, deposited, fees
    d.array()?; // utxosGovState
    d.skip()?; d.skip()?; d.skip()?; // proposals, cc_state, constitution
    // Now attempt to decode something at index 3
    match d.datatype()? {
        Type::Map | Type::Array | Type::Bytes | Type::String | Type::U8 | Type::U16 | Type::U32 | Type::U64 | Type::I8 | Type::I16 | Type::I32 | Type::I64 | Type::Tag | Type::Bool | Type::Null | Type::F16 | Type::F32 | Type::F64 => Ok(true),
        Type::Break => Ok(false),
        _ => Ok(false),
    }
}
