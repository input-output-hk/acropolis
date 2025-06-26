// This file was code-generated using an experimental CDDL to rust tool:
// https://github.com/dcSpark/cddl-codegen

use super::*;
use crate::error::*;
use crate::serialization::*;
use cbor_event::de::Deserializer;
use cbor_event::se::{Serialize, Serializer};
use std::io::{BufRead, Seek, SeekFrom, Write};

impl cbor_event::se::Serialize for Credential {
    fn serialize<'se, W: Write>(
        &self,
        serializer: &'se mut Serializer<W>,
    ) -> cbor_event::Result<&'se mut Serializer<W>> {
        match self {
            Credential::I0 => {
                serializer.write_array(cbor_event::Len::Len(1))?;
                serializer.write_unsigned_integer(0u64)
            }
            Credential::I1 => {
                serializer.write_array(cbor_event::Len::Len(1))?;
                serializer.write_unsigned_integer(1u64)
            }
        }
    }
}

impl Deserialize for Credential {
    fn deserialize<R: BufRead + Seek>(raw: &mut Deserializer<R>) -> Result<Self, DeserializeError> {
        (|| -> Result<_, DeserializeError> {
            let len = raw.array()?;
            let initial_position = raw.as_mut_ref().stream_position().unwrap();
            let mut errs = Vec::new();
            let deser_variant = (|raw: &mut Deserializer<_>| -> Result<_, DeserializeError> {
                let mut read_len = CBORReadLen::new(len);
                read_len.read_elems(1)?;
                read_len.finish()?;
                let i0_value = raw.unsigned_integer()?;
                if i0_value != 0 {
                    return Err(DeserializeFailure::FixedValueMismatch {
                        found: Key::Uint(i0_value),
                        expected: Key::Uint(0),
                    }
                    .into());
                }
                match len {
                    cbor_event::Len::Len(_) => (),
                    cbor_event::Len::Indefinite => match raw.special()? {
                        cbor_event::Special::Break => (),
                        _ => return Err(DeserializeFailure::EndingBreakMissing.into()),
                    },
                }
                Ok(())
            })(raw);
            match deser_variant {
                Ok(()) => return Ok(Credential::I0),
                Err(e) => {
                    errs.push(e.annotate("I0"));
                    raw.as_mut_ref()
                        .seek(SeekFrom::Start(initial_position))
                        .unwrap();
                }
            };
            let deser_variant = (|raw: &mut Deserializer<_>| -> Result<_, DeserializeError> {
                let mut read_len = CBORReadLen::new(len);
                read_len.read_elems(1)?;
                read_len.finish()?;
                let i1_value = raw.unsigned_integer()?;
                if i1_value != 1 {
                    return Err(DeserializeFailure::FixedValueMismatch {
                        found: Key::Uint(i1_value),
                        expected: Key::Uint(1),
                    }
                    .into());
                }
                match len {
                    cbor_event::Len::Len(_) => (),
                    cbor_event::Len::Indefinite => match raw.special()? {
                        cbor_event::Special::Break => (),
                        _ => return Err(DeserializeFailure::EndingBreakMissing.into()),
                    },
                }
                Ok(())
            })(raw);
            match deser_variant {
                Ok(()) => return Ok(Credential::I1),
                Err(e) => {
                    errs.push(e.annotate("I1"));
                    raw.as_mut_ref()
                        .seek(SeekFrom::Start(initial_position))
                        .unwrap();
                }
            };
            Err(DeserializeError::new(
                "Credential",
                DeserializeFailure::NoVariantMatchedWithCauses(errs),
            ))
        })()
        .map_err(|e| e.annotate("Credential"))
    }
}

impl cbor_event::se::Serialize for Denominator {
    fn serialize<'se, W: Write>(
        &self,
        serializer: &'se mut Serializer<W>,
    ) -> cbor_event::Result<&'se mut Serializer<W>> {
        serializer.write_unsigned_integer(self.0)
    }
}

impl Deserialize for Denominator {
    fn deserialize<R: BufRead + Seek>(raw: &mut Deserializer<R>) -> Result<Self, DeserializeError> {
        let inner = raw.unsigned_integer()? as u64;
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

impl cbor_event::se::Serialize for GovActionId {
    fn serialize<'se, W: Write>(
        &self,
        serializer: &'se mut Serializer<W>,
    ) -> cbor_event::Result<&'se mut Serializer<W>> {
        serializer.write_array(cbor_event::Len::Len(2))?;
        self.hash_32.serialize(serializer)?;
        serializer.write_unsigned_integer(self.uint as u64)?;
        Ok(serializer)
    }
}

impl Deserialize for GovActionId {
    fn deserialize<R: BufRead + Seek>(raw: &mut Deserializer<R>) -> Result<Self, DeserializeError> {
        let len = raw.array()?;
        let mut read_len = CBORReadLen::new(len);
        read_len.read_elems(2)?;
        read_len.finish()?;
        (|| -> Result<_, DeserializeError> {
            let hash_32 =
                Hash32::deserialize(raw).map_err(|e: DeserializeError| e.annotate("hash_32"))?;
            let uint = Ok(raw.unsigned_integer()? as u16)
                .map_err(|e: DeserializeError| e.annotate("uint"))?;
            match len {
                cbor_event::Len::Len(_) => (),
                cbor_event::Len::Indefinite => match raw.special()? {
                    cbor_event::Special::Break => (),
                    _ => return Err(DeserializeFailure::EndingBreakMissing.into()),
                },
            }
            Ok(GovActionId { hash_32, uint })
        })()
        .map_err(|e| e.annotate("GovActionId"))
    }
}

impl cbor_event::se::Serialize for Hash28 {
    fn serialize<'se, W: Write>(
        &self,
        serializer: &'se mut Serializer<W>,
    ) -> cbor_event::Result<&'se mut Serializer<W>> {
        serializer.write_bytes(&self.0)
    }
}

impl Deserialize for Hash28 {
    fn deserialize<R: BufRead + Seek>(raw: &mut Deserializer<R>) -> Result<Self, DeserializeError> {
        let inner = raw.bytes()? as Vec<u8>;
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

impl cbor_event::se::Serialize for Hash32 {
    fn serialize<'se, W: Write>(
        &self,
        serializer: &'se mut Serializer<W>,
    ) -> cbor_event::Result<&'se mut Serializer<W>> {
        serializer.write_bytes(&self.0)
    }
}

impl Deserialize for Hash32 {
    fn deserialize<R: BufRead + Seek>(raw: &mut Deserializer<R>) -> Result<Self, DeserializeError> {
        let inner = raw.bytes()? as Vec<u8>;
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

impl cbor_event::se::Serialize for UnitInterval {
    fn serialize<'se, W: Write>(
        &self,
        serializer: &'se mut Serializer<W>,
    ) -> cbor_event::Result<&'se mut Serializer<W>> {
        serializer.write_tag(30u64)?;
        serializer.write_array(cbor_event::Len::Len(2))?;
        serializer.write_unsigned_integer(self.index_0)?;
        serializer.write_unsigned_integer(self.index_1)?;
        Ok(serializer)
    }
}

impl Deserialize for UnitInterval {
    fn deserialize<R: BufRead + Seek>(raw: &mut Deserializer<R>) -> Result<Self, DeserializeError> {
        let tag = raw.tag()?;
        if tag != 30 {
            return Err(DeserializeError::new(
                "UnitInterval",
                DeserializeFailure::TagMismatch {
                    found: tag,
                    expected: 30,
                },
            ));
        }
        let len = raw.array()?;
        let mut read_len = CBORReadLen::new(len);
        read_len.read_elems(2)?;
        read_len.finish()?;
        (|| -> Result<_, DeserializeError> {
            let index_0 = Ok(raw.unsigned_integer()? as u64)
                .map_err(|e: DeserializeError| e.annotate("index_0"))?;
            let index_1 = Ok(raw.unsigned_integer()? as u64)
                .map_err(|e: DeserializeError| e.annotate("index_1"))?;
            match len {
                cbor_event::Len::Len(_) => (),
                cbor_event::Len::Indefinite => match raw.special()? {
                    cbor_event::Special::Break => (),
                    _ => return Err(DeserializeFailure::EndingBreakMissing.into()),
                },
            }
            Ok(UnitInterval { index_0, index_1 })
        })()
        .map_err(|e| e.annotate("UnitInterval"))
    }
}

impl cbor_event::se::Serialize for Url {
    fn serialize<'se, W: Write>(
        &self,
        serializer: &'se mut Serializer<W>,
    ) -> cbor_event::Result<&'se mut Serializer<W>> {
        serializer.write_text(&self.0)
    }
}

impl Deserialize for Url {
    fn deserialize<R: BufRead + Seek>(raw: &mut Deserializer<R>) -> Result<Self, DeserializeError> {
        let inner = raw.text()? as String;
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
