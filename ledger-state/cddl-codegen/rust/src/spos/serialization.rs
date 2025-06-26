// This file was code-generated using an experimental CDDL to rust tool:
// https://github.com/dcSpark/cddl-codegen

use super::*;
use crate::error::*;
use crate::serialization::*;
use cbor_event::de::Deserializer;
use cbor_event::se::{Serialize, Serializer};
use std::io::{BufRead, Seek, SeekFrom, Write};

impl cbor_event::se::Serialize for DnsName {
    fn serialize<'se, W: Write>(
        &self,
        serializer: &'se mut Serializer<W>,
    ) -> cbor_event::Result<&'se mut Serializer<W>> {
        serializer.write_text(&self.0)
    }
}

impl Deserialize for DnsName {
    fn deserialize<R: BufRead + Seek>(raw: &mut Deserializer<R>) -> Result<Self, DeserializeError> {
        let inner = raw.text()? as String;
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

impl cbor_event::se::Serialize for Ipv4 {
    fn serialize<'se, W: Write>(
        &self,
        serializer: &'se mut Serializer<W>,
    ) -> cbor_event::Result<&'se mut Serializer<W>> {
        serializer.write_bytes(&self.0)
    }
}

impl Deserialize for Ipv4 {
    fn deserialize<R: BufRead + Seek>(raw: &mut Deserializer<R>) -> Result<Self, DeserializeError> {
        let inner = raw.bytes()? as Vec<u8>;
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

impl cbor_event::se::Serialize for Ipv6 {
    fn serialize<'se, W: Write>(
        &self,
        serializer: &'se mut Serializer<W>,
    ) -> cbor_event::Result<&'se mut Serializer<W>> {
        serializer.write_bytes(&self.0)
    }
}

impl Deserialize for Ipv6 {
    fn deserialize<R: BufRead + Seek>(raw: &mut Deserializer<R>) -> Result<Self, DeserializeError> {
        let inner = raw.bytes()? as Vec<u8>;
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

impl cbor_event::se::Serialize for MultiHostName {
    fn serialize<'se, W: Write>(
        &self,
        serializer: &'se mut Serializer<W>,
    ) -> cbor_event::Result<&'se mut Serializer<W>> {
        serializer.write_array(cbor_event::Len::Len(2))?;
        self.serialize_as_embedded_group(serializer)
    }
}

impl SerializeEmbeddedGroup for MultiHostName {
    fn serialize_as_embedded_group<'se, W: Write>(
        &self,
        serializer: &'se mut Serializer<W>,
    ) -> cbor_event::Result<&'se mut Serializer<W>> {
        serializer.write_unsigned_integer(2u64)?;
        self.dns_name.serialize(serializer)?;
        Ok(serializer)
    }
}

impl Deserialize for MultiHostName {
    fn deserialize<R: BufRead + Seek>(raw: &mut Deserializer<R>) -> Result<Self, DeserializeError> {
        let len = raw.array()?;
        let mut read_len = CBORReadLen::new(len);
        read_len.read_elems(2)?;
        read_len.finish()?;
        let ret = Self::deserialize_as_embedded_group(raw, &mut read_len, len);
        match len {
            cbor_event::Len::Len(_) => (),
            cbor_event::Len::Indefinite => match raw.special()? {
                cbor_event::Special::Break => (),
                _ => return Err(DeserializeFailure::EndingBreakMissing.into()),
            },
        }
        ret
    }
}

impl DeserializeEmbeddedGroup for MultiHostName {
    fn deserialize_as_embedded_group<R: BufRead + Seek>(
        raw: &mut Deserializer<R>,
        _read_len: &mut CBORReadLen,
        len: cbor_event::Len,
    ) -> Result<Self, DeserializeError> {
        (|| -> Result<_, DeserializeError> {
            (|| -> Result<_, DeserializeError> {
                let index_0_value = raw.unsigned_integer()?;
                if index_0_value != 2 {
                    return Err(DeserializeFailure::FixedValueMismatch {
                        found: Key::Uint(index_0_value),
                        expected: Key::Uint(2),
                    }
                    .into());
                }
                Ok(())
            })()
            .map_err(|e| e.annotate("index_0"))?;
            let dns_name =
                DnsName::deserialize(raw).map_err(|e: DeserializeError| e.annotate("dns_name"))?;
            Ok(MultiHostName { dns_name })
        })()
        .map_err(|e| e.annotate("MultiHostName"))
    }
}

impl cbor_event::se::Serialize for PoolMetadata {
    fn serialize<'se, W: Write>(
        &self,
        serializer: &'se mut Serializer<W>,
    ) -> cbor_event::Result<&'se mut Serializer<W>> {
        serializer.write_array(cbor_event::Len::Len(2))?;
        self.url.serialize(serializer)?;
        serializer.write_bytes(&self.index_1)?;
        Ok(serializer)
    }
}

impl Deserialize for PoolMetadata {
    fn deserialize<R: BufRead + Seek>(raw: &mut Deserializer<R>) -> Result<Self, DeserializeError> {
        let len = raw.array()?;
        let mut read_len = CBORReadLen::new(len);
        read_len.read_elems(2)?;
        read_len.finish()?;
        (|| -> Result<_, DeserializeError> {
            let url = Url::deserialize(raw).map_err(|e: DeserializeError| e.annotate("url"))?;
            let index_1 =
                Ok(raw.bytes()? as Vec<u8>).map_err(|e: DeserializeError| e.annotate("index_1"))?;
            match len {
                cbor_event::Len::Len(_) => (),
                cbor_event::Len::Indefinite => match raw.special()? {
                    cbor_event::Special::Break => (),
                    _ => return Err(DeserializeFailure::EndingBreakMissing.into()),
                },
            }
            Ok(PoolMetadata { url, index_1 })
        })()
        .map_err(|e| e.annotate("PoolMetadata"))
    }
}

impl cbor_event::se::Serialize for PoolParameters {
    fn serialize<'se, W: Write>(
        &self,
        serializer: &'se mut Serializer<W>,
    ) -> cbor_event::Result<&'se mut Serializer<W>> {
        serializer.write_array(cbor_event::Len::Len(9))?;
        self.operator.serialize(serializer)?;
        self.vrf_keyhash.serialize(serializer)?;
        serializer.write_unsigned_integer(self.pledge)?;
        serializer.write_unsigned_integer(self.cost)?;
        self.margin.serialize(serializer)?;
        serializer.write_bytes(&self.reward_account)?;
        self.pool_owners.serialize(serializer)?;
        serializer.write_array(cbor_event::Len::Len(self.relays.len() as u64))?;
        for element in self.relays.iter() {
            element.serialize(serializer)?;
        }
        match &self.pool_metadata {
            Some(x) => x.serialize(serializer),
            None => serializer.write_special(cbor_event::Special::Null),
        }?;
        Ok(serializer)
    }
}

impl Deserialize for PoolParameters {
    fn deserialize<R: BufRead + Seek>(raw: &mut Deserializer<R>) -> Result<Self, DeserializeError> {
        let len = raw.array()?;
        let mut read_len = CBORReadLen::new(len);
        read_len.read_elems(9)?;
        read_len.finish()?;
        (|| -> Result<_, DeserializeError> {
            let operator =
                Hash28::deserialize(raw).map_err(|e: DeserializeError| e.annotate("operator"))?;
            let vrf_keyhash = Hash32::deserialize(raw)
                .map_err(|e: DeserializeError| e.annotate("vrf_keyhash"))?;
            let pledge = Ok(raw.unsigned_integer()? as u64)
                .map_err(|e: DeserializeError| e.annotate("pledge"))?;
            let cost = Ok(raw.unsigned_integer()? as u64)
                .map_err(|e: DeserializeError| e.annotate("cost"))?;
            let margin = UnitInterval::deserialize(raw)
                .map_err(|e: DeserializeError| e.annotate("margin"))?;
            let reward_account = Ok(raw.bytes()? as Vec<u8>)
                .map_err(|e: DeserializeError| e.annotate("reward_account"))?;
            let pool_owners = NonemptySetKeyhash::deserialize(raw)
                .map_err(|e: DeserializeError| e.annotate("pool_owners"))?;
            let relays = (|| -> Result<_, DeserializeError> {
                let mut relays_arr = Vec::new();
                let len = raw.array()?;
                while match len {
                    cbor_event::Len::Len(n) => (relays_arr.len() as u64) < n,
                    cbor_event::Len::Indefinite => true,
                } {
                    if raw.cbor_type()? == cbor_event::Type::Special {
                        assert_eq!(raw.special()?, cbor_event::Special::Break);
                        break;
                    }
                    relays_arr.push(Relay::deserialize(raw)?);
                }
                Ok(relays_arr)
            })()
            .map_err(|e| e.annotate("relays"))?;
            let pool_metadata = (|| -> Result<_, DeserializeError> {
                Ok(match raw.cbor_type()? != cbor_event::Type::Special {
                    true => Some(PoolMetadata::deserialize(raw)?),
                    false => {
                        if raw.special()? != cbor_event::Special::Null {
                            return Err(DeserializeFailure::ExpectedNull.into());
                        }
                        None
                    }
                })
            })()
            .map_err(|e| e.annotate("pool_metadata"))?;
            match len {
                cbor_event::Len::Len(_) => (),
                cbor_event::Len::Indefinite => match raw.special()? {
                    cbor_event::Special::Break => (),
                    _ => return Err(DeserializeFailure::EndingBreakMissing.into()),
                },
            }
            Ok(PoolParameters {
                operator,
                vrf_keyhash,
                pledge,
                cost,
                margin,
                reward_account,
                pool_owners,
                relays,
                pool_metadata,
            })
        })()
        .map_err(|e| e.annotate("PoolParameters"))
    }
}

impl cbor_event::se::Serialize for Relay {
    fn serialize<'se, W: Write>(
        &self,
        serializer: &'se mut Serializer<W>,
    ) -> cbor_event::Result<&'se mut Serializer<W>> {
        match self {
            Relay::SingleHostAddr(single_host_addr) => single_host_addr.serialize(serializer),
            Relay::SingleHostName(single_host_name) => single_host_name.serialize(serializer),
            Relay::MultiHostName(multi_host_name) => multi_host_name.serialize(serializer),
        }
    }
}

impl Deserialize for Relay {
    fn deserialize<R: BufRead + Seek>(raw: &mut Deserializer<R>) -> Result<Self, DeserializeError> {
        (|| -> Result<_, DeserializeError> {
            let len = raw.array()?;
            let initial_position = raw.as_mut_ref().stream_position().unwrap();
            let mut errs = Vec::new();
            let deser_variant = (|raw: &mut Deserializer<_>| -> Result<_, DeserializeError> {
                let mut read_len = CBORReadLen::new(len);
                read_len.read_elems(4)?;
                read_len.finish()?;
                let ret = SingleHostAddr::deserialize_as_embedded_group(raw, &mut read_len, len);
                match len {
                    cbor_event::Len::Len(_) => (),
                    cbor_event::Len::Indefinite => match raw.special()? {
                        cbor_event::Special::Break => (),
                        _ => return Err(DeserializeFailure::EndingBreakMissing.into()),
                    },
                }
                ret
            })(raw);
            match deser_variant {
                Ok(single_host_addr) => return Ok(Self::SingleHostAddr(single_host_addr)),
                Err(e) => {
                    errs.push(e.annotate("SingleHostAddr"));
                    raw.as_mut_ref()
                        .seek(SeekFrom::Start(initial_position))
                        .unwrap();
                }
            };
            let deser_variant = (|raw: &mut Deserializer<_>| -> Result<_, DeserializeError> {
                let mut read_len = CBORReadLen::new(len);
                read_len.read_elems(3)?;
                read_len.finish()?;
                let ret = SingleHostName::deserialize_as_embedded_group(raw, &mut read_len, len);
                match len {
                    cbor_event::Len::Len(_) => (),
                    cbor_event::Len::Indefinite => match raw.special()? {
                        cbor_event::Special::Break => (),
                        _ => return Err(DeserializeFailure::EndingBreakMissing.into()),
                    },
                }
                ret
            })(raw);
            match deser_variant {
                Ok(single_host_name) => return Ok(Self::SingleHostName(single_host_name)),
                Err(e) => {
                    errs.push(e.annotate("SingleHostName"));
                    raw.as_mut_ref()
                        .seek(SeekFrom::Start(initial_position))
                        .unwrap();
                }
            };
            let deser_variant = (|raw: &mut Deserializer<_>| -> Result<_, DeserializeError> {
                let mut read_len = CBORReadLen::new(len);
                read_len.read_elems(2)?;
                read_len.finish()?;
                let ret = MultiHostName::deserialize_as_embedded_group(raw, &mut read_len, len);
                match len {
                    cbor_event::Len::Len(_) => (),
                    cbor_event::Len::Indefinite => match raw.special()? {
                        cbor_event::Special::Break => (),
                        _ => return Err(DeserializeFailure::EndingBreakMissing.into()),
                    },
                }
                ret
            })(raw);
            match deser_variant {
                Ok(multi_host_name) => return Ok(Self::MultiHostName(multi_host_name)),
                Err(e) => {
                    errs.push(e.annotate("MultiHostName"));
                    raw.as_mut_ref()
                        .seek(SeekFrom::Start(initial_position))
                        .unwrap();
                }
            };
            Err(DeserializeError::new(
                "Relay",
                DeserializeFailure::NoVariantMatchedWithCauses(errs),
            ))
        })()
        .map_err(|e| e.annotate("Relay"))
    }
}

impl cbor_event::se::Serialize for SingleHostAddr {
    fn serialize<'se, W: Write>(
        &self,
        serializer: &'se mut Serializer<W>,
    ) -> cbor_event::Result<&'se mut Serializer<W>> {
        serializer.write_array(cbor_event::Len::Len(4))?;
        self.serialize_as_embedded_group(serializer)
    }
}

impl SerializeEmbeddedGroup for SingleHostAddr {
    fn serialize_as_embedded_group<'se, W: Write>(
        &self,
        serializer: &'se mut Serializer<W>,
    ) -> cbor_event::Result<&'se mut Serializer<W>> {
        serializer.write_unsigned_integer(0u64)?;
        match &self.port {
            Some(x) => serializer.write_unsigned_integer(*x as u64),
            None => serializer.write_special(cbor_event::Special::Null),
        }?;
        match &self.ipv4 {
            Some(x) => x.serialize(serializer),
            None => serializer.write_special(cbor_event::Special::Null),
        }?;
        match &self.ipv6 {
            Some(x) => x.serialize(serializer),
            None => serializer.write_special(cbor_event::Special::Null),
        }?;
        Ok(serializer)
    }
}

impl Deserialize for SingleHostAddr {
    fn deserialize<R: BufRead + Seek>(raw: &mut Deserializer<R>) -> Result<Self, DeserializeError> {
        let len = raw.array()?;
        let mut read_len = CBORReadLen::new(len);
        read_len.read_elems(4)?;
        read_len.finish()?;
        let ret = Self::deserialize_as_embedded_group(raw, &mut read_len, len);
        match len {
            cbor_event::Len::Len(_) => (),
            cbor_event::Len::Indefinite => match raw.special()? {
                cbor_event::Special::Break => (),
                _ => return Err(DeserializeFailure::EndingBreakMissing.into()),
            },
        }
        ret
    }
}

impl DeserializeEmbeddedGroup for SingleHostAddr {
    fn deserialize_as_embedded_group<R: BufRead + Seek>(
        raw: &mut Deserializer<R>,
        _read_len: &mut CBORReadLen,
        len: cbor_event::Len,
    ) -> Result<Self, DeserializeError> {
        (|| -> Result<_, DeserializeError> {
            (|| -> Result<_, DeserializeError> {
                let index_0_value = raw.unsigned_integer()?;
                if index_0_value != 0 {
                    return Err(DeserializeFailure::FixedValueMismatch {
                        found: Key::Uint(index_0_value),
                        expected: Key::Uint(0),
                    }
                    .into());
                }
                Ok(())
            })()
            .map_err(|e| e.annotate("index_0"))?;
            let port = (|| -> Result<_, DeserializeError> {
                Ok(match raw.cbor_type()? != cbor_event::Type::Special {
                    true => Some(raw.unsigned_integer()? as u16),
                    false => {
                        if raw.special()? != cbor_event::Special::Null {
                            return Err(DeserializeFailure::ExpectedNull.into());
                        }
                        None
                    }
                })
            })()
            .map_err(|e| e.annotate("port"))?;
            let ipv4 = (|| -> Result<_, DeserializeError> {
                Ok(match raw.cbor_type()? != cbor_event::Type::Special {
                    true => Some(Ipv4::deserialize(raw)?),
                    false => {
                        if raw.special()? != cbor_event::Special::Null {
                            return Err(DeserializeFailure::ExpectedNull.into());
                        }
                        None
                    }
                })
            })()
            .map_err(|e| e.annotate("ipv4"))?;
            let ipv6 = (|| -> Result<_, DeserializeError> {
                Ok(match raw.cbor_type()? != cbor_event::Type::Special {
                    true => Some(Ipv6::deserialize(raw)?),
                    false => {
                        if raw.special()? != cbor_event::Special::Null {
                            return Err(DeserializeFailure::ExpectedNull.into());
                        }
                        None
                    }
                })
            })()
            .map_err(|e| e.annotate("ipv6"))?;
            Ok(SingleHostAddr { port, ipv4, ipv6 })
        })()
        .map_err(|e| e.annotate("SingleHostAddr"))
    }
}

impl cbor_event::se::Serialize for SingleHostName {
    fn serialize<'se, W: Write>(
        &self,
        serializer: &'se mut Serializer<W>,
    ) -> cbor_event::Result<&'se mut Serializer<W>> {
        serializer.write_array(cbor_event::Len::Len(3))?;
        self.serialize_as_embedded_group(serializer)
    }
}

impl SerializeEmbeddedGroup for SingleHostName {
    fn serialize_as_embedded_group<'se, W: Write>(
        &self,
        serializer: &'se mut Serializer<W>,
    ) -> cbor_event::Result<&'se mut Serializer<W>> {
        serializer.write_unsigned_integer(1u64)?;
        match &self.port {
            Some(x) => serializer.write_unsigned_integer(*x as u64),
            None => serializer.write_special(cbor_event::Special::Null),
        }?;
        self.dns_name.serialize(serializer)?;
        Ok(serializer)
    }
}

impl Deserialize for SingleHostName {
    fn deserialize<R: BufRead + Seek>(raw: &mut Deserializer<R>) -> Result<Self, DeserializeError> {
        let len = raw.array()?;
        let mut read_len = CBORReadLen::new(len);
        read_len.read_elems(3)?;
        read_len.finish()?;
        let ret = Self::deserialize_as_embedded_group(raw, &mut read_len, len);
        match len {
            cbor_event::Len::Len(_) => (),
            cbor_event::Len::Indefinite => match raw.special()? {
                cbor_event::Special::Break => (),
                _ => return Err(DeserializeFailure::EndingBreakMissing.into()),
            },
        }
        ret
    }
}

impl DeserializeEmbeddedGroup for SingleHostName {
    fn deserialize_as_embedded_group<R: BufRead + Seek>(
        raw: &mut Deserializer<R>,
        _read_len: &mut CBORReadLen,
        len: cbor_event::Len,
    ) -> Result<Self, DeserializeError> {
        (|| -> Result<_, DeserializeError> {
            (|| -> Result<_, DeserializeError> {
                let index_0_value = raw.unsigned_integer()?;
                if index_0_value != 1 {
                    return Err(DeserializeFailure::FixedValueMismatch {
                        found: Key::Uint(index_0_value),
                        expected: Key::Uint(1),
                    }
                    .into());
                }
                Ok(())
            })()
            .map_err(|e| e.annotate("index_0"))?;
            let port = (|| -> Result<_, DeserializeError> {
                Ok(match raw.cbor_type()? != cbor_event::Type::Special {
                    true => Some(raw.unsigned_integer()? as u16),
                    false => {
                        if raw.special()? != cbor_event::Special::Null {
                            return Err(DeserializeFailure::ExpectedNull.into());
                        }
                        None
                    }
                })
            })()
            .map_err(|e| e.annotate("port"))?;
            let dns_name =
                DnsName::deserialize(raw).map_err(|e: DeserializeError| e.annotate("dns_name"))?;
            Ok(SingleHostName { port, dns_name })
        })()
        .map_err(|e| e.annotate("SingleHostName"))
    }
}

impl cbor_event::se::Serialize for SpoState {
    fn serialize<'se, W: Write>(
        &self,
        serializer: &'se mut Serializer<W>,
    ) -> cbor_event::Result<&'se mut Serializer<W>> {
        serializer.write_map(cbor_event::Len::Len(2))?;
        serializer.write_text("pools")?;
        serializer.write_map(cbor_event::Len::Len(self.pools.len() as u64))?;
        for (key, value) in self.pools.iter() {
            key.serialize(serializer)?;
            value.serialize(serializer)?;
        }
        serializer.write_text("retiring")?;
        serializer.write_map(cbor_event::Len::Len(self.retiring.len() as u64))?;
        for (key, value) in self.retiring.iter() {
            key.serialize(serializer)?;
            serializer.write_unsigned_integer(*value)?;
        }
        Ok(serializer)
    }
}

impl Deserialize for SpoState {
    fn deserialize<R: BufRead + Seek>(raw: &mut Deserializer<R>) -> Result<Self, DeserializeError> {
        let len = raw.map()?;
        let mut read_len = CBORReadLen::new(len);
        read_len.read_elems(2)?;
        read_len.finish()?;
        (|| -> Result<_, DeserializeError> {
            let mut pools = None;
            let mut retiring = None;
            let mut read = 0;
            while match len {
                cbor_event::Len::Len(n) => read < n,
                cbor_event::Len::Indefinite => true,
            } {
                match raw.cbor_type()? {
                    cbor_event::Type::UnsignedInteger => {
                        return Err(DeserializeFailure::UnknownKey(Key::Uint(
                            raw.unsigned_integer()?,
                        ))
                        .into())
                    }
                    cbor_event::Type::Text => match raw.text()?.as_str() {
                        "pools" => {
                            if pools.is_some() {
                                return Err(DeserializeFailure::DuplicateKey(Key::Str(
                                    "pools".into(),
                                ))
                                .into());
                            }
                            pools = Some(
                                (|| -> Result<_, DeserializeError> {
                                    let mut pools_table = BTreeMap::new();
                                    let pools_len = raw.map()?;
                                    while match pools_len {
                                        cbor_event::Len::Len(n) => (pools_table.len() as u64) < n,
                                        cbor_event::Len::Indefinite => true,
                                    } {
                                        if raw.cbor_type()? == cbor_event::Type::Special {
                                            assert_eq!(raw.special()?, cbor_event::Special::Break);
                                            break;
                                        }
                                        let pools_key = Hash28::deserialize(raw)?;
                                        let pools_value = PoolParameters::deserialize(raw)?;
                                        if pools_table
                                            .insert(pools_key.clone(), pools_value)
                                            .is_some()
                                        {
                                            return Err(DeserializeFailure::DuplicateKey(
                                                Key::Str(String::from(
                                                    "some complicated/unsupported type",
                                                )),
                                            )
                                            .into());
                                        }
                                    }
                                    Ok(pools_table)
                                })()
                                .map_err(|e| e.annotate("pools"))?,
                            );
                        }
                        "retiring" => {
                            if retiring.is_some() {
                                return Err(DeserializeFailure::DuplicateKey(Key::Str(
                                    "retiring".into(),
                                ))
                                .into());
                            }
                            retiring = Some(
                                (|| -> Result<_, DeserializeError> {
                                    let mut retiring_table = BTreeMap::new();
                                    let retiring_len = raw.map()?;
                                    while match retiring_len {
                                        cbor_event::Len::Len(n) => {
                                            (retiring_table.len() as u64) < n
                                        }
                                        cbor_event::Len::Indefinite => true,
                                    } {
                                        if raw.cbor_type()? == cbor_event::Type::Special {
                                            assert_eq!(raw.special()?, cbor_event::Special::Break);
                                            break;
                                        }
                                        let retiring_key = Hash28::deserialize(raw)?;
                                        let retiring_value = raw.unsigned_integer()? as u64;
                                        if retiring_table
                                            .insert(retiring_key.clone(), retiring_value)
                                            .is_some()
                                        {
                                            return Err(DeserializeFailure::DuplicateKey(
                                                Key::Str(String::from(
                                                    "some complicated/unsupported type",
                                                )),
                                            )
                                            .into());
                                        }
                                    }
                                    Ok(retiring_table)
                                })()
                                .map_err(|e| e.annotate("retiring"))?,
                            );
                        }
                        unknown_key => {
                            return Err(DeserializeFailure::UnknownKey(Key::Str(
                                unknown_key.to_owned(),
                            ))
                            .into())
                        }
                    },
                    cbor_event::Type::Special => match len {
                        cbor_event::Len::Len(_) => {
                            return Err(DeserializeFailure::BreakInDefiniteLen.into())
                        }
                        cbor_event::Len::Indefinite => match raw.special()? {
                            cbor_event::Special::Break => break,
                            _ => return Err(DeserializeFailure::EndingBreakMissing.into()),
                        },
                    },
                    other_type => {
                        return Err(DeserializeFailure::UnexpectedKeyType(other_type).into())
                    }
                }
                read += 1;
            }
            let pools = match pools {
                Some(x) => x,
                None => {
                    return Err(
                        DeserializeFailure::MandatoryFieldMissing(Key::Str(String::from("pools")))
                            .into(),
                    )
                }
            };
            let retiring =
                match retiring {
                    Some(x) => x,
                    None => {
                        return Err(DeserializeFailure::MandatoryFieldMissing(Key::Str(
                            String::from("retiring"),
                        ))
                        .into())
                    }
                };
            ();
            Ok(Self { pools, retiring })
        })()
        .map_err(|e| e.annotate("SpoState"))
    }
}
