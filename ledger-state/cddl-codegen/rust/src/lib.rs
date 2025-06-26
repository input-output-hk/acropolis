#![allow(clippy::too_many_arguments)]

pub mod common;
pub mod error;
pub mod spos;
pub mod utxos;
// This file was code-generated using an experimental CDDL to rust tool:
// https://github.com/dcSpark/cddl-codegen

pub mod serialization;

use crate::error::*;
use common::Keyhash;
use std::collections::BTreeMap;
use std::convert::TryFrom;

#[derive(Clone, Debug)]
pub struct AssetQuantityU64 {
    pub asset_id: Vec<u8>,
    pub quantity: u64,
}

impl AssetQuantityU64 {
    pub fn new(asset_id: Vec<u8>, quantity: u64) -> Result<Self, DeserializeError> {
        if asset_id.len() > 32 {
            return Err(DeserializeFailure::RangeCheck {
                found: asset_id.len() as isize,
                min: None,
                max: Some(32),
            }
            .into());
        }
        Ok(Self { asset_id, quantity })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum Int {
    Uint(u64),
    Nint(u64),
}

impl Int {
    pub fn new_uint(value: u64) -> Self {
        Self::Uint(value)
    }

    /// * `value` - Value as encoded in CBOR - note: a negative `x` here would be `|x + 1|` due to CBOR's `nint` encoding e.g. to represent -5, pass in 4.
    pub fn new_nint(value: u64) -> Self {
        Self::Nint(value)
    }
}

impl std::fmt::Display for Int {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Uint(x) => write!(f, "{}", x),
            Self::Nint(x) => write!(f, "-{}", x + 1),
        }
    }
}

impl std::str::FromStr for Int {
    type Err = IntError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let x = i128::from_str(s).map_err(IntError::Parsing)?;
        Self::try_from(x).map_err(IntError::Bounds)
    }
}

impl TryFrom<i128> for Int {
    type Error = std::num::TryFromIntError;

    fn try_from(x: i128) -> Result<Self, Self::Error> {
        if x >= 0 {
            u64::try_from(x).map(Self::Uint)
        } else {
            u64::try_from((x + 1).abs()).map(Self::Nint)
        }
    }
}

#[derive(Clone, Debug)]
pub enum IntError {
    Bounds(std::num::TryFromIntError),
    Parsing(std::num::ParseIntError),
}

pub type NonemptySetKeyhash = Vec<Keyhash>;
