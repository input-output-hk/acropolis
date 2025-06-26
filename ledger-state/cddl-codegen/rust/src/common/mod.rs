// This file was code-generated using an experimental CDDL to rust tool:
// https://github.com/dcSpark/cddl-codegen

pub mod serialization;

use crate::error::*;
use std::collections::BTreeMap;
use std::convert::TryFrom;

pub type Address = Vec<u8>;

pub type Coin = u64;

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum Credential {
    I0,
    I1,
}

impl Credential {
    pub fn new_i0() -> Self {
        Self::I0
    }

    pub fn new_i1() -> Self {
        Self::I1
    }
}

#[derive(Clone, Debug, Copy)]
pub struct Denominator(u64);

impl Denominator {
    pub fn new(inner: u64) -> Result<Self, DeserializeError> {
        if inner < 1 {
            return Err(DeserializeError::new(
                "Denominator",
                DeserializeFailure::RangeCheck {
                    found: inner as isize,
                    min: Some(1),
                    max: None,
                },
            ));
        }
        Ok(Self(inner))
    }
}

impl TryFrom<u64> for Denominator {
    type Error = DeserializeError;

    fn try_from(inner: u64) -> Result<Self, Self::Error> {
        Denominator::new(inner)
    }
}

pub type Epoch = u64;

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct GovActionId {
    pub hash_32: Hash32,
    pub uint: u16,
}

impl GovActionId {
    pub fn new(hash_32: Hash32, uint: u16) -> Self {
        Self { hash_32, uint }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct Hash28(Vec<u8>);

impl Hash28 {
    pub fn new(inner: Vec<u8>) -> Result<Self, DeserializeError> {
        if inner.len() != 28 {
            return Err(DeserializeError::new(
                "Hash28",
                DeserializeFailure::RangeCheck {
                    found: inner.len() as isize,
                    min: Some(28),
                    max: Some(28),
                },
            ));
        }
        Ok(Self(inner))
    }
}

impl TryFrom<Vec<u8>> for Hash28 {
    type Error = DeserializeError;

    fn try_from(inner: Vec<u8>) -> Result<Self, Self::Error> {
        Hash28::new(inner)
    }
}

impl From<Hash28> for Vec<u8> {
    fn from(wrapper: Hash28) -> Self {
        wrapper.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct Hash32(Vec<u8>);

impl Hash32 {
    pub fn new(inner: Vec<u8>) -> Result<Self, DeserializeError> {
        if inner.len() != 32 {
            return Err(DeserializeError::new(
                "Hash32",
                DeserializeFailure::RangeCheck {
                    found: inner.len() as isize,
                    min: Some(32),
                    max: Some(32),
                },
            ));
        }
        Ok(Self(inner))
    }
}

impl TryFrom<Vec<u8>> for Hash32 {
    type Error = DeserializeError;

    fn try_from(inner: Vec<u8>) -> Result<Self, Self::Error> {
        Hash32::new(inner)
    }
}

impl From<Hash32> for Vec<u8> {
    fn from(wrapper: Hash32) -> Self {
        wrapper.0
    }
}

pub type Keyhash = Hash28;

pub type PositiveCoin = u64;

pub type RewardAccount = Vec<u8>;

impl From<Url> for String {
    fn from(wrapper: Url) -> Self {
        wrapper.0
    }
}

#[derive(Clone, Debug)]
pub struct UnitInterval {
    pub index_0: u64,
    pub index_1: u64,
}

impl UnitInterval {
    pub fn new(index_0: u64, index_1: u64) -> Self {
        Self { index_0, index_1 }
    }
}

#[derive(Clone, Debug)]
pub struct Url(String);

impl Url {
    pub fn new(inner: String) -> Result<Self, DeserializeError> {
        if inner.len() > 128 {
            return Err(DeserializeError::new(
                "Url",
                DeserializeFailure::RangeCheck {
                    found: inner.len() as isize,
                    min: Some(0),
                    max: Some(128),
                },
            ));
        }
        Ok(Self(inner))
    }
}

impl TryFrom<String> for Url {
    type Error = DeserializeError;

    fn try_from(inner: String) -> Result<Self, Self::Error> {
        Url::new(inner)
    }
}

impl From<Denominator> for u64 {
    fn from(wrapper: Denominator) -> Self {
        wrapper.0
    }
}
