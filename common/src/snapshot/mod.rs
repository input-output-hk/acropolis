// SPDX-License-Identifier: Apache-2.0
// Copyright © 2025, Acropolis team.

//! Cardano snapshot parsing and validation.
//!
//! This module provides:
//! - Manifest parsing and validation (`parser.rs`)
//! - Full Amaru/Haskell snapshot parsing (`snapshot.rs`)
//! - Streaming callback-based parser for bootstrap (`streaming_snapshot.rs`)
//! - Error types (`error.rs`)

// Submodules
mod error;
mod parser;
mod snapshot;
pub mod streaming_snapshot;

// Re-export error types
pub use error::SnapshotError;

// Re-export parser functions
pub use parser::{compute_sha256, parse_manifest, validate_era, validate_integrity};

// Re-export streaming snapshot APIs
pub use streaming_snapshot::{
    AccountState, Anchor, CollectingCallbacks, DRepCallback, DRepInfo, DelegatedDRep,
    GovernanceProposal, PoolCallback, PoolInfo, PoolMetadata, PotBalances, ProposalCallback, Relay,
    SnapshotCallbacks, SnapshotMetadata, StakeCallback, StreamingSnapshotParser, UtxoCallback,
    UtxoEntry,
};

// Re-export Amaru parser types and functions
pub use snapshot::{
    extract_boot_data, extract_tip_from_filename, parse_all_utxos, parse_sample_utxos,
    AmaruSnapshot, EpochStateMetadata, SnapshotData, TipInfo, UtxoEntry as AmaruUtxoEntry,
};
