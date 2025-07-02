// This file was code-generated using an experimental CDDL to rust tool:
// https://github.com/dcSpark/cddl-codegen

pub mod serialization;

use crate::common::{Coin, Epoch, Hash32, Keyhash, RewardAccount, UnitInterval, Url};
use crate::error::*;
use crate::NonemptySetKeyhash;
use std::collections::BTreeMap;
use std::convert::TryFrom;

#[derive(Clone, Debug)]
pub struct DnsName(String);

impl DnsName {
    pub fn new(inner: String) -> Result<Self, DeserializeError> {
        if inner.len() > 128 {
            return Err(DeserializeError::new(
                "DnsName",
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

impl TryFrom<String> for DnsName {
    type Error = DeserializeError;

    fn try_from(inner: String) -> Result<Self, Self::Error> {
        DnsName::new(inner)
    }
}

#[derive(Clone, Debug)]
pub struct Ipv4(Vec<u8>);

impl Ipv4 {
    pub fn new(inner: Vec<u8>) -> Result<Self, DeserializeError> {
        if inner.len() != 4 {
            return Err(DeserializeError::new(
                "Ipv4",
                DeserializeFailure::RangeCheck {
                    found: inner.len() as isize,
                    min: Some(4),
                    max: Some(4),
                },
            ));
        }
        Ok(Self(inner))
    }
}

impl TryFrom<Vec<u8>> for Ipv4 {
    type Error = DeserializeError;

    fn try_from(inner: Vec<u8>) -> Result<Self, Self::Error> {
        Ipv4::new(inner)
    }
}

impl From<Ipv4> for Vec<u8> {
    fn from(wrapper: Ipv4) -> Self {
        wrapper.0
    }
}

#[derive(Clone, Debug)]
pub struct Ipv6(Vec<u8>);

impl Ipv6 {
    pub fn new(inner: Vec<u8>) -> Result<Self, DeserializeError> {
        if inner.len() != 16 {
            return Err(DeserializeError::new(
                "Ipv6",
                DeserializeFailure::RangeCheck {
                    found: inner.len() as isize,
                    min: Some(16),
                    max: Some(16),
                },
            ));
        }
        Ok(Self(inner))
    }
}

impl TryFrom<Vec<u8>> for Ipv6 {
    type Error = DeserializeError;

    fn try_from(inner: Vec<u8>) -> Result<Self, Self::Error> {
        Ipv6::new(inner)
    }
}

impl From<Ipv6> for Vec<u8> {
    fn from(wrapper: Ipv6) -> Self {
        wrapper.0
    }
}

#[derive(Clone, Debug)]
pub struct MultiHostName {
    pub dns_name: DnsName,
}

impl MultiHostName {
    pub fn new(dns_name: DnsName) -> Self {
        Self { dns_name }
    }
}

#[derive(Clone, Debug)]
pub struct PoolMetadata {
    pub url: Url,
    pub index_1: Vec<u8>,
}

impl PoolMetadata {
    pub fn new(url: Url, index_1: Vec<u8>) -> Self {
        Self { url, index_1 }
    }
}

#[derive(Clone, Debug)]
pub struct PoolParameters {
    pub operator: Keyhash,
    pub vrf_keyhash: Hash32,
    pub pledge: Coin,
    pub cost: Coin,
    pub margin: UnitInterval,
    pub reward_account: RewardAccount,
    pub pool_owners: NonemptySetKeyhash,
    pub relays: Vec<Relay>,
    pub pool_metadata: Option<PoolMetadata>,
}

impl PoolParameters {
    pub fn new(
        operator: Keyhash,
        vrf_keyhash: Hash32,
        pledge: Coin,
        cost: Coin,
        margin: UnitInterval,
        reward_account: RewardAccount,
        pool_owners: NonemptySetKeyhash,
        relays: Vec<Relay>,
        pool_metadata: Option<PoolMetadata>,
    ) -> Self {
        Self {
            operator,
            vrf_keyhash,
            pledge,
            cost,
            margin,
            reward_account,
            pool_owners,
            relays,
            pool_metadata,
        }
    }
}

pub type Port = u16;

#[derive(Clone, Debug)]
pub enum Relay {
    SingleHostAddr(SingleHostAddr),
    SingleHostName(SingleHostName),
    MultiHostName(MultiHostName),
}

impl Relay {
    pub fn new_single_host_addr(
        port: Option<Port>,
        ipv4: Option<Ipv4>,
        ipv6: Option<Ipv6>,
    ) -> Self {
        Self::SingleHostAddr(SingleHostAddr::new(port, ipv4, ipv6))
    }

    pub fn new_single_host_name(port: Option<Port>, dns_name: DnsName) -> Self {
        Self::SingleHostName(SingleHostName::new(port, dns_name))
    }

    pub fn new_multi_host_name(dns_name: DnsName) -> Self {
        Self::MultiHostName(MultiHostName::new(dns_name))
    }
}

#[derive(Clone, Debug)]
pub struct SingleHostAddr {
    pub port: Option<Port>,
    pub ipv4: Option<Ipv4>,
    pub ipv6: Option<Ipv6>,
}

impl SingleHostAddr {
    pub fn new(port: Option<Port>, ipv4: Option<Ipv4>, ipv6: Option<Ipv6>) -> Self {
        Self { port, ipv4, ipv6 }
    }
}

#[derive(Clone, Debug)]
pub struct SingleHostName {
    pub port: Option<Port>,
    pub dns_name: DnsName,
}

impl SingleHostName {
    pub fn new(port: Option<Port>, dns_name: DnsName) -> Self {
        Self { port, dns_name }
    }
}

#[derive(Clone, Debug)]
pub struct SpoState {
    pub pools: BTreeMap<Keyhash, PoolParameters>,
    pub retiring: BTreeMap<Keyhash, Epoch>,
}

impl SpoState {
    pub fn new(
        pools: BTreeMap<Keyhash, PoolParameters>,
        retiring: BTreeMap<Keyhash, Epoch>,
    ) -> Self {
        Self { pools, retiring }
    }
}

impl From<DnsName> for String {
    fn from(wrapper: DnsName) -> Self {
        wrapper.0
    }
}
