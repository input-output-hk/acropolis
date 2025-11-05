use std::marker::PhantomData;

use crate::PoolId;
use anyhow::anyhow;
use bech32::{Bech32, Hrp};
use serde::{ser::SerializeMap, Deserialize, Serialize, Serializer};
use serde_with::{ser::SerializeAsWrap, DeserializeAs, SerializeAs};
use caryatid_module_rest_server::messages::RESTResponse;
use crate::rest_error::RESTError;

pub struct SerializeMapAs<KAs, VAs>(std::marker::PhantomData<(KAs, VAs)>);

impl<T, K, V, KAs, VAs> SerializeAs<T> for SerializeMapAs<KAs, VAs>
where
    KAs: SerializeAs<K>,
    VAs: SerializeAs<V>,
    for<'a> &'a T: IntoIterator<Item = (&'a K, &'a V)>,
{
    fn serialize_as<S>(source: &T, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map_ser = serializer.serialize_map(None)?;
        for (k, v) in source {
            map_ser.serialize_entry(
                &SerializeAsWrap::<K, KAs>::new(k),
                &SerializeAsWrap::<V, VAs>::new(v),
            )?;
        }
        map_ser.end()
    }
}

pub trait Bech32Conversion {
    fn to_bech32(&self) -> Result<String, anyhow::Error>;
    fn from_bech32(s: &str) -> Result<Self, anyhow::Error>
    where
        Self: Sized;
}

// Marker types for different HRP prefixes
pub struct PoolPrefix;
pub struct StakePrefix;
pub struct AddrPrefix;

// Trait to get HRP string from marker types
pub trait HrpPrefix {
    const HRP: &'static str;
}

impl HrpPrefix for PoolPrefix {
    const HRP: &'static str = "pool";
}

impl HrpPrefix for StakePrefix {
    const HRP: &'static str = "stake";
}

impl HrpPrefix for AddrPrefix {
    const HRP: &'static str = "addr";
}

// Generic Bech32 converter with HRP parameter
pub struct DisplayFromBech32<PREFIX: HrpPrefix>(PhantomData<PREFIX>);

// PoolID serialization implementation
impl SerializeAs<PoolId> for DisplayFromBech32<PoolPrefix> {
    fn serialize_as<S>(source: &PoolId, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let bech32_string = source.to_bech32().map_err(serde::ser::Error::custom)?;
        serializer.serialize_str(&bech32_string)
    }
}

// PoolID deserialization implementation
impl<'de> DeserializeAs<'de, PoolId> for DisplayFromBech32<PoolPrefix> {
    fn deserialize_as<D>(deserializer: D) -> Result<PoolId, D::Error>
    where
        D: serde::de::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        PoolId::from_bech32(&s).map_err(serde::de::Error::custom)
    }
}

// Vec<u8> serialization implementation
impl<PREFIX> SerializeAs<Vec<u8>> for DisplayFromBech32<PREFIX>
where
    PREFIX: HrpPrefix,
{
    fn serialize_as<S>(source: &Vec<u8>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let bech32_string =
            source.to_bech32_with_hrp(PREFIX::HRP).map_err(serde::ser::Error::custom)?;

        serializer.serialize_str(&bech32_string)
    }
}

// Deserialization implementation
impl<'de, PREFIX> DeserializeAs<'de, Vec<u8>> for DisplayFromBech32<PREFIX>
where
    PREFIX: HrpPrefix,
{
    fn deserialize_as<D>(deserializer: D) -> Result<Vec<u8>, D::Error>
    where
        D: serde::de::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Vec::<u8>::from_bech32_with_hrp(&s, PREFIX::HRP).map_err(serde::de::Error::custom)
    }
}

pub trait Bech32WithHrp {
    fn to_bech32_with_hrp(&self, hrp: &str) -> Result<String, anyhow::Error>;
    fn from_bech32_with_hrp(s: &str, expected_hrp: &str) -> Result<Vec<u8>, anyhow::Error>;
}

impl Bech32WithHrp for Vec<u8> {
    fn to_bech32_with_hrp(&self, hrp: &str) -> Result<String, anyhow::Error> {
        let hrp = Hrp::parse(hrp).map_err(|e| anyhow!("Bech32 HRP parse error: {e}"))?;

        bech32::encode::<Bech32>(hrp, self.as_slice())
            .map_err(|e| anyhow!("Bech32 encoding error: {e}"))
    }

    fn from_bech32_with_hrp(s: &str, expected_hrp: &str) -> Result<Self, anyhow::Error> {
        let (hrp, data) = bech32::decode(s).map_err(|e| anyhow!("Invalid Bech32 string: {e}"))?;

        if hrp != Hrp::parse(expected_hrp)? {
            return Err(anyhow!(
                "Invalid HRP, expected '{expected_hrp}', got '{hrp}'"
            ));
        }

        Ok(data.to_vec())
    }
}

impl Bech32WithHrp for [u8] {
    fn to_bech32_with_hrp(&self, hrp: &str) -> Result<String, anyhow::Error> {
        let hrp = Hrp::parse(hrp).map_err(|e| anyhow!("Bech32 HRP parse error: {e}"))?;

        bech32::encode::<Bech32>(hrp, self).map_err(|e| anyhow!("Bech32 encoding error: {e}"))
    }

    fn from_bech32_with_hrp(s: &str, expected_hrp: &str) -> Result<Vec<u8>, anyhow::Error> {
        let (hrp, data) = bech32::decode(s).map_err(|e| anyhow!("Invalid Bech32 string: {e}"))?;

        if hrp != Hrp::parse(expected_hrp)? {
            return Err(anyhow!(
                "Invalid HRP, expected '{expected_hrp}', got '{hrp}'"
            ));
        }

        Ok(data.to_vec())
    }
}

/// Helper to serialize a result to JSON REST response
pub fn serialize_to_json_response<T: Serialize>(data: &T) -> Result<RESTResponse, RESTError> {
    let json = serde_json::to_string_pretty(data)?;
    Ok(RESTResponse::with_json(200, &json))
}
