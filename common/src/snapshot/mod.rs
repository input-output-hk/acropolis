// SPDX-License-Identifier: Apache-2.0
// Copyright © 2025, Acropolis team.

//! Cardano snapshot parsing and validation.
//!
//! This module provides:
//! - Manifest parsing and validation (`parser.rs`)
//! - Streaming callback-based parser for bootstrap (`streaming_snapshot.rs`)
//! - Pool parameters types (`pool_params.rs`)
//! - Error types (`error.rs`)

// Submodules
mod error;
pub mod mark_set_go;
mod parser;
pub mod streaming_snapshot;

// Re-export error types
pub use error::SnapshotError;

// Re-export parser functions
pub use parser::{compute_sha256, parse_manifest, validate_era, validate_integrity};

// Re-export streaming snapshot APIs
pub use streaming_snapshot::{
    AccountState, Anchor, CollectingCallbacks, DRepCallback, DRepInfo, EpochCallback,
    GovernanceProposal, PoolCallback, PotBalances, ProposalCallback, Relay, SnapshotCallbacks,
    SnapshotMetadata, StakeAddressState, StakeCallback, StreamingSnapshotParser, UtxoCallback,
    UtxoEntry,
};

// Re-export snapshot types
pub use mark_set_go::{RawSnapshotsContainer, SnapshotsCallback, VMap};
