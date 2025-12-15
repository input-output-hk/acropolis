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
mod decode;
mod error;
pub mod mark_set_go;
mod parser;
pub mod protocol_parameters;
pub mod streaming_snapshot;
pub mod utxo;
pub use error::SnapshotError;

pub use parser::{compute_sha256, parse_manifest, validate_era, validate_integrity};

pub use streaming_snapshot::{
    AccountState, AccountsBootstrapData, AccountsCallback, Anchor, DRepCallback, DRepInfo,
    EpochCallback, GovernanceProposal, PoolCallback, ProposalCallback, SnapshotCallbacks,
    SnapshotMetadata, StakeAddressState, StreamingSnapshotParser, UtxoCallback,
};

pub use mark_set_go::{RawSnapshot, RawSnapshotsContainer, SnapshotsCallback, VMap};
