// This file was code-generated using an experimental CDDL to rust tool:
// https://github.com/dcSpark/cddl-codegen

use super::*;
use crate::error::*;
use crate::serialization::*;
use cbor_event::de::Deserializer;
use cbor_event::se::{Serialize, Serializer};
use std::io::{BufRead, Seek, SeekFrom, Write};

impl cbor_event::se::Serialize for AssetValue {
    fn serialize<'se, W: Write>(
        &self,
        serializer: &'se mut Serializer<W>,
    ) -> cbor_event::Result<&'se mut Serializer<W>> {
        serializer.write_array(cbor_event::Len::Len(2))?;
        serializer.write_unsigned_integer(self.lovelace)?;
        self.multi_asset.serialize(serializer)?;
        Ok(serializer)
    }
}

impl Deserialize for AssetValue {
    fn deserialize<R: BufRead + Seek>(raw: &mut Deserializer<R>) -> Result<Self, DeserializeError> {
        let len = raw.array()?;
        let mut read_len = CBORReadLen::new(len);
        read_len.read_elems(2)?;
        read_len.finish()?;
        (|| -> Result<_, DeserializeError> {
            let lovelace = Ok(raw.unsigned_integer()? as u64)
                .map_err(|e: DeserializeError| e.annotate("lovelace"))?;
            let multi_asset = Multiasset::deserialize(raw)
                .map_err(|e: DeserializeError| e.annotate("multi_asset"))?;
            match len {
                cbor_event::Len::Len(_) => (),
                cbor_event::Len::Indefinite => match raw.special()? {
                    cbor_event::Special::Break => (),
                    _ => return Err(DeserializeFailure::EndingBreakMissing.into()),
                },
            }
            Ok(AssetValue {
                lovelace,
                multi_asset,
            })
        })()
        .map_err(|e| e.annotate("AssetValue"))
    }
}

impl cbor_event::se::Serialize for BabbageTxOut {
    fn serialize<'se, W: Write>(
        &self,
        serializer: &'se mut Serializer<W>,
    ) -> cbor_event::Result<&'se mut Serializer<W>> {
        serializer.write_map(cbor_event::Len::Len(
            2 + match &self.key_2 {
                Some(_) => 1,
                None => 0,
            } + match &self.key_3 {
                Some(_) => 1,
                None => 0,
            },
        ))?;
        serializer.write_unsigned_integer(0u64)?;
        serializer.write_bytes(&self.key_0)?;
        serializer.write_unsigned_integer(1u64)?;
        self.key_1.serialize(serializer)?;
        if let Some(field) = &self.key_2 {
            serializer.write_unsigned_integer(2u64)?;
            field.serialize(serializer)?;
        }
        if let Some(field) = &self.key_3 {
            serializer.write_unsigned_integer(3u64)?;
            serializer.write_tag(24u64)?;
            let mut key_3_inner_se = Serializer::new_vec();
            field.serialize(&mut key_3_inner_se)?;
            let key_3_bytes = key_3_inner_se.finalize();
            serializer.write_bytes(&key_3_bytes)?;
        }
        Ok(serializer)
    }
}

impl Deserialize for BabbageTxOut {
    fn deserialize<R: BufRead + Seek>(raw: &mut Deserializer<R>) -> Result<Self, DeserializeError> {
        let len = raw.map()?;
        let mut read_len = CBORReadLen::new(len);
        read_len.read_elems(2)?;
        (|| -> Result<_, DeserializeError> {
            let mut key_0 = None;
            let mut key_1 = None;
            let mut key_2 = None;
            let mut key_3 = None;
            let mut read = 0;
            while match len {
                cbor_event::Len::Len(n) => read < n,
                cbor_event::Len::Indefinite => true,
            } {
                match raw.cbor_type()? {
                    cbor_event::Type::UnsignedInteger => match raw.unsigned_integer()? {
                        0 => {
                            if key_0.is_some() {
                                return Err(DeserializeFailure::DuplicateKey(Key::Uint(0)).into());
                            }
                            key_0 = Some(
                                Ok(raw.bytes()? as Vec<u8>)
                                    .map_err(|e: DeserializeError| e.annotate("key_0"))?,
                            );
                        }
                        1 => {
                            if key_1.is_some() {
                                return Err(DeserializeFailure::DuplicateKey(Key::Uint(1)).into());
                            }
                            key_1 = Some(
                                Value::deserialize(raw)
                                    .map_err(|e: DeserializeError| e.annotate("key_1"))?,
                            );
                        }
                        2 => {
                            if key_2.is_some() {
                                return Err(DeserializeFailure::DuplicateKey(Key::Uint(2)).into());
                            }
                            key_2 = Some(
                                (|| -> Result<_, DeserializeError> {
                                    read_len.read_elems(1)?;
                                    DatumOption::deserialize(raw)
                                })()
                                .map_err(|e| e.annotate("key_2"))?,
                            );
                        }
                        3 => {
                            if key_3.is_some() {
                                return Err(DeserializeFailure::DuplicateKey(Key::Uint(3)).into());
                            }
                            key_3 = Some(
                                (|| -> Result<_, DeserializeError> {
                                    read_len.read_elems(1)?;
                                    match raw.tag()? {
                                        24 => {
                                            let key_3_bytes = raw.bytes()?;
                                            let inner_de = &mut Deserializer::from(
                                                std::io::Cursor::new(key_3_bytes),
                                            );
                                            Script::deserialize(inner_de)
                                        }
                                        tag => Err(DeserializeFailure::TagMismatch {
                                            found: tag,
                                            expected: 24,
                                        }
                                        .into()),
                                    }
                                })()
                                .map_err(|e| e.annotate("key_3"))?,
                            );
                        }
                        unknown_key => {
                            return Err(
                                DeserializeFailure::UnknownKey(Key::Uint(unknown_key)).into()
                            )
                        }
                    },
                    cbor_event::Type::Text => {
                        return Err(DeserializeFailure::UnknownKey(Key::Str(raw.text()?)).into())
                    }
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
            let key_0 = match key_0 {
                Some(x) => x,
                None => return Err(DeserializeFailure::MandatoryFieldMissing(Key::Uint(0)).into()),
            };
            let key_1 = match key_1 {
                Some(x) => x,
                None => return Err(DeserializeFailure::MandatoryFieldMissing(Key::Uint(1)).into()),
            };
            read_len.finish()?;
            Ok(Self {
                key_0,
                key_1,
                key_2,
                key_3,
            })
        })()
        .map_err(|e| e.annotate("BabbageTxOut"))
    }
}

impl cbor_event::se::Serialize for BigInt {
    fn serialize<'se, W: Write>(
        &self,
        serializer: &'se mut Serializer<W>,
    ) -> cbor_event::Result<&'se mut Serializer<W>> {
        match self {
            BigInt::Int(int) => int.serialize(serializer),
            BigInt::BigUint(big_uint) => {
                serializer.write_tag(2u64)?;
                big_uint.serialize(serializer)
            }
            BigInt::BigNint(big_nint) => {
                serializer.write_tag(3u64)?;
                big_nint.serialize(serializer)
            }
        }
    }
}

impl Deserialize for BigInt {
    fn deserialize<R: BufRead + Seek>(raw: &mut Deserializer<R>) -> Result<Self, DeserializeError> {
        (|| -> Result<_, DeserializeError> {
            let initial_position = raw.as_mut_ref().stream_position().unwrap();
            let mut errs = Vec::new();
            let deser_variant: Result<_, DeserializeError> = Int::deserialize(raw);
            match deser_variant {
                Ok(int) => return Ok(Self::Int(int)),
                Err(e) => {
                    errs.push(e.annotate("Int"));
                    raw.as_mut_ref()
                        .seek(SeekFrom::Start(initial_position))
                        .unwrap();
                }
            };
            let deser_variant = (|raw: &mut Deserializer<_>| -> Result<_, DeserializeError> {
                match raw.tag()? {
                    2 => BoundedBytes::deserialize(raw),
                    tag => Err(DeserializeFailure::TagMismatch {
                        found: tag,
                        expected: 2,
                    }
                    .into()),
                }
            })(raw);
            match deser_variant {
                Ok(big_uint) => return Ok(Self::BigUint(big_uint)),
                Err(e) => {
                    errs.push(e.annotate("BigUint"));
                    raw.as_mut_ref()
                        .seek(SeekFrom::Start(initial_position))
                        .unwrap();
                }
            };
            let deser_variant = (|raw: &mut Deserializer<_>| -> Result<_, DeserializeError> {
                match raw.tag()? {
                    3 => BoundedBytes::deserialize(raw),
                    tag => Err(DeserializeFailure::TagMismatch {
                        found: tag,
                        expected: 3,
                    }
                    .into()),
                }
            })(raw);
            match deser_variant {
                Ok(big_nint) => return Ok(Self::BigNint(big_nint)),
                Err(e) => {
                    errs.push(e.annotate("BigNint"));
                    raw.as_mut_ref()
                        .seek(SeekFrom::Start(initial_position))
                        .unwrap();
                }
            };
            Err(DeserializeError::new(
                "BigInt",
                DeserializeFailure::NoVariantMatchedWithCauses(errs),
            ))
        })()
        .map_err(|e| e.annotate("BigInt"))
    }
}

impl cbor_event::se::Serialize for BoundedBytes {
    fn serialize<'se, W: Write>(
        &self,
        serializer: &'se mut Serializer<W>,
    ) -> cbor_event::Result<&'se mut Serializer<W>> {
        serializer.write_bytes(&self.0)
    }
}

impl Deserialize for BoundedBytes {
    fn deserialize<R: BufRead + Seek>(raw: &mut Deserializer<R>) -> Result<Self, DeserializeError> {
        let inner = raw.bytes()? as Vec<u8>;
        if inner.len() > 64 {
            return Err(DeserializeError::new(
                "BoundedBytes",
                DeserializeFailure::RangeCheck {
                    found: inner.len() as isize,
                    min: Some(0),
                    max: Some(64),
                },
            ));
        }
        Ok(Self(inner))
    }
}

impl cbor_event::se::Serialize for Constr {
    fn serialize<'se, W: Write>(
        &self,
        serializer: &'se mut Serializer<W>,
    ) -> cbor_event::Result<&'se mut Serializer<W>> {
        match self {
            Constr::ArrPlutusData(arr_plutus_data) => {
                serializer.write_tag(121u64)?;
                serializer.write_array(cbor_event::Len::Len(arr_plutus_data.len() as u64))?;
                for element in arr_plutus_data.iter() {
                    element.serialize(serializer)?;
                }
                Ok(serializer)
            }
            Constr::ArrPlutusData2(arr_plutus_data2) => {
                serializer.write_tag(122u64)?;
                serializer.write_array(cbor_event::Len::Len(arr_plutus_data2.len() as u64))?;
                for element in arr_plutus_data2.iter() {
                    element.serialize(serializer)?;
                }
                Ok(serializer)
            }
            Constr::ArrPlutusData3(arr_plutus_data3) => {
                serializer.write_tag(123u64)?;
                serializer.write_array(cbor_event::Len::Len(arr_plutus_data3.len() as u64))?;
                for element in arr_plutus_data3.iter() {
                    element.serialize(serializer)?;
                }
                Ok(serializer)
            }
            Constr::ArrPlutusData4(arr_plutus_data4) => {
                serializer.write_tag(124u64)?;
                serializer.write_array(cbor_event::Len::Len(arr_plutus_data4.len() as u64))?;
                for element in arr_plutus_data4.iter() {
                    element.serialize(serializer)?;
                }
                Ok(serializer)
            }
            Constr::ArrPlutusData5(arr_plutus_data5) => {
                serializer.write_tag(125u64)?;
                serializer.write_array(cbor_event::Len::Len(arr_plutus_data5.len() as u64))?;
                for element in arr_plutus_data5.iter() {
                    element.serialize(serializer)?;
                }
                Ok(serializer)
            }
            Constr::ArrPlutusData6(arr_plutus_data6) => {
                serializer.write_tag(126u64)?;
                serializer.write_array(cbor_event::Len::Len(arr_plutus_data6.len() as u64))?;
                for element in arr_plutus_data6.iter() {
                    element.serialize(serializer)?;
                }
                Ok(serializer)
            }
            Constr::ArrPlutusData7(arr_plutus_data7) => {
                serializer.write_tag(127u64)?;
                serializer.write_array(cbor_event::Len::Len(arr_plutus_data7.len() as u64))?;
                for element in arr_plutus_data7.iter() {
                    element.serialize(serializer)?;
                }
                Ok(serializer)
            }
        }
    }
}

impl Deserialize for Constr {
    fn deserialize<R: BufRead + Seek>(raw: &mut Deserializer<R>) -> Result<Self, DeserializeError> {
        (|| -> Result<_, DeserializeError> {
            let initial_position = raw.as_mut_ref().stream_position().unwrap();
            let mut errs = Vec::new();
            let deser_variant = (|raw: &mut Deserializer<_>| -> Result<_, DeserializeError> {
                match raw.tag()? {
                    121 => {
                        let mut arr_plutus_data_arr = Vec::new();
                        let len = raw.array()?;
                        while match len {
                            cbor_event::Len::Len(n) => (arr_plutus_data_arr.len() as u64) < n,
                            cbor_event::Len::Indefinite => true,
                        } {
                            if raw.cbor_type()? == cbor_event::Type::Special {
                                assert_eq!(raw.special()?, cbor_event::Special::Break);
                                break;
                            }
                            arr_plutus_data_arr.push(PlutusData::deserialize(raw)?);
                        }
                        Ok(arr_plutus_data_arr)
                    }
                    tag => Err(DeserializeFailure::TagMismatch {
                        found: tag,
                        expected: 121,
                    }
                    .into()),
                }
            })(raw);
            match deser_variant {
                Ok(arr_plutus_data) => return Ok(Self::ArrPlutusData(arr_plutus_data)),
                Err(e) => {
                    errs.push(e.annotate("ArrPlutusData"));
                    raw.as_mut_ref()
                        .seek(SeekFrom::Start(initial_position))
                        .unwrap();
                }
            };
            let deser_variant = (|raw: &mut Deserializer<_>| -> Result<_, DeserializeError> {
                match raw.tag()? {
                    122 => {
                        let mut arr_plutus_data2_arr = Vec::new();
                        let len = raw.array()?;
                        while match len {
                            cbor_event::Len::Len(n) => (arr_plutus_data2_arr.len() as u64) < n,
                            cbor_event::Len::Indefinite => true,
                        } {
                            if raw.cbor_type()? == cbor_event::Type::Special {
                                assert_eq!(raw.special()?, cbor_event::Special::Break);
                                break;
                            }
                            arr_plutus_data2_arr.push(PlutusData::deserialize(raw)?);
                        }
                        Ok(arr_plutus_data2_arr)
                    }
                    tag => Err(DeserializeFailure::TagMismatch {
                        found: tag,
                        expected: 122,
                    }
                    .into()),
                }
            })(raw);
            match deser_variant {
                Ok(arr_plutus_data2) => return Ok(Self::ArrPlutusData2(arr_plutus_data2)),
                Err(e) => {
                    errs.push(e.annotate("ArrPlutusData2"));
                    raw.as_mut_ref()
                        .seek(SeekFrom::Start(initial_position))
                        .unwrap();
                }
            };
            let deser_variant = (|raw: &mut Deserializer<_>| -> Result<_, DeserializeError> {
                match raw.tag()? {
                    123 => {
                        let mut arr_plutus_data3_arr = Vec::new();
                        let len = raw.array()?;
                        while match len {
                            cbor_event::Len::Len(n) => (arr_plutus_data3_arr.len() as u64) < n,
                            cbor_event::Len::Indefinite => true,
                        } {
                            if raw.cbor_type()? == cbor_event::Type::Special {
                                assert_eq!(raw.special()?, cbor_event::Special::Break);
                                break;
                            }
                            arr_plutus_data3_arr.push(PlutusData::deserialize(raw)?);
                        }
                        Ok(arr_plutus_data3_arr)
                    }
                    tag => Err(DeserializeFailure::TagMismatch {
                        found: tag,
                        expected: 123,
                    }
                    .into()),
                }
            })(raw);
            match deser_variant {
                Ok(arr_plutus_data3) => return Ok(Self::ArrPlutusData3(arr_plutus_data3)),
                Err(e) => {
                    errs.push(e.annotate("ArrPlutusData3"));
                    raw.as_mut_ref()
                        .seek(SeekFrom::Start(initial_position))
                        .unwrap();
                }
            };
            let deser_variant = (|raw: &mut Deserializer<_>| -> Result<_, DeserializeError> {
                match raw.tag()? {
                    124 => {
                        let mut arr_plutus_data4_arr = Vec::new();
                        let len = raw.array()?;
                        while match len {
                            cbor_event::Len::Len(n) => (arr_plutus_data4_arr.len() as u64) < n,
                            cbor_event::Len::Indefinite => true,
                        } {
                            if raw.cbor_type()? == cbor_event::Type::Special {
                                assert_eq!(raw.special()?, cbor_event::Special::Break);
                                break;
                            }
                            arr_plutus_data4_arr.push(PlutusData::deserialize(raw)?);
                        }
                        Ok(arr_plutus_data4_arr)
                    }
                    tag => Err(DeserializeFailure::TagMismatch {
                        found: tag,
                        expected: 124,
                    }
                    .into()),
                }
            })(raw);
            match deser_variant {
                Ok(arr_plutus_data4) => return Ok(Self::ArrPlutusData4(arr_plutus_data4)),
                Err(e) => {
                    errs.push(e.annotate("ArrPlutusData4"));
                    raw.as_mut_ref()
                        .seek(SeekFrom::Start(initial_position))
                        .unwrap();
                }
            };
            let deser_variant = (|raw: &mut Deserializer<_>| -> Result<_, DeserializeError> {
                match raw.tag()? {
                    125 => {
                        let mut arr_plutus_data5_arr = Vec::new();
                        let len = raw.array()?;
                        while match len {
                            cbor_event::Len::Len(n) => (arr_plutus_data5_arr.len() as u64) < n,
                            cbor_event::Len::Indefinite => true,
                        } {
                            if raw.cbor_type()? == cbor_event::Type::Special {
                                assert_eq!(raw.special()?, cbor_event::Special::Break);
                                break;
                            }
                            arr_plutus_data5_arr.push(PlutusData::deserialize(raw)?);
                        }
                        Ok(arr_plutus_data5_arr)
                    }
                    tag => Err(DeserializeFailure::TagMismatch {
                        found: tag,
                        expected: 125,
                    }
                    .into()),
                }
            })(raw);
            match deser_variant {
                Ok(arr_plutus_data5) => return Ok(Self::ArrPlutusData5(arr_plutus_data5)),
                Err(e) => {
                    errs.push(e.annotate("ArrPlutusData5"));
                    raw.as_mut_ref()
                        .seek(SeekFrom::Start(initial_position))
                        .unwrap();
                }
            };
            let deser_variant = (|raw: &mut Deserializer<_>| -> Result<_, DeserializeError> {
                match raw.tag()? {
                    126 => {
                        let mut arr_plutus_data6_arr = Vec::new();
                        let len = raw.array()?;
                        while match len {
                            cbor_event::Len::Len(n) => (arr_plutus_data6_arr.len() as u64) < n,
                            cbor_event::Len::Indefinite => true,
                        } {
                            if raw.cbor_type()? == cbor_event::Type::Special {
                                assert_eq!(raw.special()?, cbor_event::Special::Break);
                                break;
                            }
                            arr_plutus_data6_arr.push(PlutusData::deserialize(raw)?);
                        }
                        Ok(arr_plutus_data6_arr)
                    }
                    tag => Err(DeserializeFailure::TagMismatch {
                        found: tag,
                        expected: 126,
                    }
                    .into()),
                }
            })(raw);
            match deser_variant {
                Ok(arr_plutus_data6) => return Ok(Self::ArrPlutusData6(arr_plutus_data6)),
                Err(e) => {
                    errs.push(e.annotate("ArrPlutusData6"));
                    raw.as_mut_ref()
                        .seek(SeekFrom::Start(initial_position))
                        .unwrap();
                }
            };
            let deser_variant = (|raw: &mut Deserializer<_>| -> Result<_, DeserializeError> {
                match raw.tag()? {
                    127 => {
                        let mut arr_plutus_data7_arr = Vec::new();
                        let len = raw.array()?;
                        while match len {
                            cbor_event::Len::Len(n) => (arr_plutus_data7_arr.len() as u64) < n,
                            cbor_event::Len::Indefinite => true,
                        } {
                            if raw.cbor_type()? == cbor_event::Type::Special {
                                assert_eq!(raw.special()?, cbor_event::Special::Break);
                                break;
                            }
                            arr_plutus_data7_arr.push(PlutusData::deserialize(raw)?);
                        }
                        Ok(arr_plutus_data7_arr)
                    }
                    tag => Err(DeserializeFailure::TagMismatch {
                        found: tag,
                        expected: 127,
                    }
                    .into()),
                }
            })(raw);
            match deser_variant {
                Ok(arr_plutus_data7) => return Ok(Self::ArrPlutusData7(arr_plutus_data7)),
                Err(e) => {
                    errs.push(e.annotate("ArrPlutusData7"));
                    raw.as_mut_ref()
                        .seek(SeekFrom::Start(initial_position))
                        .unwrap();
                }
            };
            Err(DeserializeError::new(
                "Constr",
                DeserializeFailure::NoVariantMatchedWithCauses(errs),
            ))
        })()
        .map_err(|e| e.annotate("Constr"))
    }
}

impl cbor_event::se::Serialize for CredentialDeposit {
    fn serialize<'se, W: Write>(
        &self,
        serializer: &'se mut Serializer<W>,
    ) -> cbor_event::Result<&'se mut Serializer<W>> {
        serializer.write_array(cbor_event::Len::Len(2))?;
        self.serialize_as_embedded_group(serializer)
    }
}

impl SerializeEmbeddedGroup for CredentialDeposit {
    fn serialize_as_embedded_group<'se, W: Write>(
        &self,
        serializer: &'se mut Serializer<W>,
    ) -> cbor_event::Result<&'se mut Serializer<W>> {
        serializer.write_unsigned_integer(0u64)?;
        self.credential.serialize(serializer)?;
        Ok(serializer)
    }
}

impl Deserialize for CredentialDeposit {
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

impl DeserializeEmbeddedGroup for CredentialDeposit {
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
            let credential = Credential::deserialize(raw)
                .map_err(|e: DeserializeError| e.annotate("credential"))?;
            Ok(CredentialDeposit { credential })
        })()
        .map_err(|e| e.annotate("CredentialDeposit"))
    }
}

impl cbor_event::se::Serialize for DatumOption {
    fn serialize<'se, W: Write>(
        &self,
        serializer: &'se mut Serializer<W>,
    ) -> cbor_event::Result<&'se mut Serializer<W>> {
        match self {
            DatumOption::I0 => {
                serializer.write_array(cbor_event::Len::Len(1))?;
                serializer.write_unsigned_integer(0u64)
            }
            DatumOption::I1 => {
                serializer.write_array(cbor_event::Len::Len(1))?;
                serializer.write_unsigned_integer(1u64)
            }
        }
    }
}

impl Deserialize for DatumOption {
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
                Ok(()) => return Ok(DatumOption::I0),
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
                Ok(()) => return Ok(DatumOption::I1),
                Err(e) => {
                    errs.push(e.annotate("I1"));
                    raw.as_mut_ref()
                        .seek(SeekFrom::Start(initial_position))
                        .unwrap();
                }
            };
            Err(DeserializeError::new(
                "DatumOption",
                DeserializeFailure::NoVariantMatchedWithCauses(errs),
            ))
        })()
        .map_err(|e| e.annotate("DatumOption"))
    }
}

impl cbor_event::se::Serialize for Deposit {
    fn serialize<'se, W: Write>(
        &self,
        serializer: &'se mut Serializer<W>,
    ) -> cbor_event::Result<&'se mut Serializer<W>> {
        match self {
            Deposit::CredentialDeposit(credential_deposit) => {
                credential_deposit.serialize(serializer)
            }
            Deposit::PoolDeposit(pool_deposit) => pool_deposit.serialize(serializer),
            Deposit::DrepDeposit(drep_deposit) => drep_deposit.serialize(serializer),
            Deposit::GovActionDeposit(gov_action_deposit) => {
                gov_action_deposit.serialize(serializer)
            }
        }
    }
}

impl Deserialize for Deposit {
    fn deserialize<R: BufRead + Seek>(raw: &mut Deserializer<R>) -> Result<Self, DeserializeError> {
        (|| -> Result<_, DeserializeError> {
            let len = raw.array()?;
            let initial_position = raw.as_mut_ref().stream_position().unwrap();
            let mut errs = Vec::new();
            let deser_variant = (|raw: &mut Deserializer<_>| -> Result<_, DeserializeError> {
                let mut read_len = CBORReadLen::new(len);
                read_len.read_elems(2)?;
                read_len.finish()?;
                let ret = CredentialDeposit::deserialize_as_embedded_group(raw, &mut read_len, len);
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
                Ok(credential_deposit) => return Ok(Self::CredentialDeposit(credential_deposit)),
                Err(e) => {
                    errs.push(e.annotate("CredentialDeposit"));
                    raw.as_mut_ref()
                        .seek(SeekFrom::Start(initial_position))
                        .unwrap();
                }
            };
            let deser_variant = (|raw: &mut Deserializer<_>| -> Result<_, DeserializeError> {
                let mut read_len = CBORReadLen::new(len);
                read_len.read_elems(2)?;
                read_len.finish()?;
                let ret = PoolDeposit::deserialize_as_embedded_group(raw, &mut read_len, len);
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
                Ok(pool_deposit) => return Ok(Self::PoolDeposit(pool_deposit)),
                Err(e) => {
                    errs.push(e.annotate("PoolDeposit"));
                    raw.as_mut_ref()
                        .seek(SeekFrom::Start(initial_position))
                        .unwrap();
                }
            };
            let deser_variant = (|raw: &mut Deserializer<_>| -> Result<_, DeserializeError> {
                let mut read_len = CBORReadLen::new(len);
                read_len.read_elems(2)?;
                read_len.finish()?;
                let ret = DrepDeposit::deserialize_as_embedded_group(raw, &mut read_len, len);
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
                Ok(drep_deposit) => return Ok(Self::DrepDeposit(drep_deposit)),
                Err(e) => {
                    errs.push(e.annotate("DrepDeposit"));
                    raw.as_mut_ref()
                        .seek(SeekFrom::Start(initial_position))
                        .unwrap();
                }
            };
            let deser_variant = (|raw: &mut Deserializer<_>| -> Result<_, DeserializeError> {
                let mut read_len = CBORReadLen::new(len);
                read_len.read_elems(2)?;
                read_len.finish()?;
                let ret = GovActionDeposit::deserialize_as_embedded_group(raw, &mut read_len, len);
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
                Ok(gov_action_deposit) => return Ok(Self::GovActionDeposit(gov_action_deposit)),
                Err(e) => {
                    errs.push(e.annotate("GovActionDeposit"));
                    raw.as_mut_ref()
                        .seek(SeekFrom::Start(initial_position))
                        .unwrap();
                }
            };
            Err(DeserializeError::new(
                "Deposit",
                DeserializeFailure::NoVariantMatchedWithCauses(errs),
            ))
        })()
        .map_err(|e| e.annotate("Deposit"))
    }
}

impl cbor_event::se::Serialize for DrepDeposit {
    fn serialize<'se, W: Write>(
        &self,
        serializer: &'se mut Serializer<W>,
    ) -> cbor_event::Result<&'se mut Serializer<W>> {
        serializer.write_array(cbor_event::Len::Len(2))?;
        self.serialize_as_embedded_group(serializer)
    }
}

impl SerializeEmbeddedGroup for DrepDeposit {
    fn serialize_as_embedded_group<'se, W: Write>(
        &self,
        serializer: &'se mut Serializer<W>,
    ) -> cbor_event::Result<&'se mut Serializer<W>> {
        serializer.write_unsigned_integer(2u64)?;
        self.credential.serialize(serializer)?;
        Ok(serializer)
    }
}

impl Deserialize for DrepDeposit {
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

impl DeserializeEmbeddedGroup for DrepDeposit {
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
            let credential = Credential::deserialize(raw)
                .map_err(|e: DeserializeError| e.annotate("credential"))?;
            Ok(DrepDeposit { credential })
        })()
        .map_err(|e| e.annotate("DrepDeposit"))
    }
}

impl cbor_event::se::Serialize for GovActionDeposit {
    fn serialize<'se, W: Write>(
        &self,
        serializer: &'se mut Serializer<W>,
    ) -> cbor_event::Result<&'se mut Serializer<W>> {
        serializer.write_array(cbor_event::Len::Len(2))?;
        self.serialize_as_embedded_group(serializer)
    }
}

impl SerializeEmbeddedGroup for GovActionDeposit {
    fn serialize_as_embedded_group<'se, W: Write>(
        &self,
        serializer: &'se mut Serializer<W>,
    ) -> cbor_event::Result<&'se mut Serializer<W>> {
        serializer.write_unsigned_integer(3u64)?;
        self.gov_action_id.serialize(serializer)?;
        Ok(serializer)
    }
}

impl Deserialize for GovActionDeposit {
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

impl DeserializeEmbeddedGroup for GovActionDeposit {
    fn deserialize_as_embedded_group<R: BufRead + Seek>(
        raw: &mut Deserializer<R>,
        _read_len: &mut CBORReadLen,
        len: cbor_event::Len,
    ) -> Result<Self, DeserializeError> {
        (|| -> Result<_, DeserializeError> {
            (|| -> Result<_, DeserializeError> {
                let index_0_value = raw.unsigned_integer()?;
                if index_0_value != 3 {
                    return Err(DeserializeFailure::FixedValueMismatch {
                        found: Key::Uint(index_0_value),
                        expected: Key::Uint(3),
                    }
                    .into());
                }
                Ok(())
            })()
            .map_err(|e| e.annotate("index_0"))?;
            let gov_action_id = GovActionId::deserialize(raw)
                .map_err(|e: DeserializeError| e.annotate("gov_action_id"))?;
            Ok(GovActionDeposit { gov_action_id })
        })()
        .map_err(|e| e.annotate("GovActionDeposit"))
    }
}

impl cbor_event::se::Serialize for InvalidBefore {
    fn serialize<'se, W: Write>(
        &self,
        serializer: &'se mut Serializer<W>,
    ) -> cbor_event::Result<&'se mut Serializer<W>> {
        serializer.write_array(cbor_event::Len::Len(2))?;
        self.serialize_as_embedded_group(serializer)
    }
}

impl SerializeEmbeddedGroup for InvalidBefore {
    fn serialize_as_embedded_group<'se, W: Write>(
        &self,
        serializer: &'se mut Serializer<W>,
    ) -> cbor_event::Result<&'se mut Serializer<W>> {
        serializer.write_unsigned_integer(4u64)?;
        serializer.write_unsigned_integer(self.slot_no)?;
        Ok(serializer)
    }
}

impl Deserialize for InvalidBefore {
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

impl DeserializeEmbeddedGroup for InvalidBefore {
    fn deserialize_as_embedded_group<R: BufRead + Seek>(
        raw: &mut Deserializer<R>,
        _read_len: &mut CBORReadLen,
        len: cbor_event::Len,
    ) -> Result<Self, DeserializeError> {
        (|| -> Result<_, DeserializeError> {
            (|| -> Result<_, DeserializeError> {
                let index_0_value = raw.unsigned_integer()?;
                if index_0_value != 4 {
                    return Err(DeserializeFailure::FixedValueMismatch {
                        found: Key::Uint(index_0_value),
                        expected: Key::Uint(4),
                    }
                    .into());
                }
                Ok(())
            })()
            .map_err(|e| e.annotate("index_0"))?;
            let slot_no = Ok(raw.unsigned_integer()? as u64)
                .map_err(|e: DeserializeError| e.annotate("slot_no"))?;
            Ok(InvalidBefore { slot_no })
        })()
        .map_err(|e| e.annotate("InvalidBefore"))
    }
}

impl cbor_event::se::Serialize for InvalidHereafter {
    fn serialize<'se, W: Write>(
        &self,
        serializer: &'se mut Serializer<W>,
    ) -> cbor_event::Result<&'se mut Serializer<W>> {
        serializer.write_array(cbor_event::Len::Len(2))?;
        self.serialize_as_embedded_group(serializer)
    }
}

impl SerializeEmbeddedGroup for InvalidHereafter {
    fn serialize_as_embedded_group<'se, W: Write>(
        &self,
        serializer: &'se mut Serializer<W>,
    ) -> cbor_event::Result<&'se mut Serializer<W>> {
        serializer.write_unsigned_integer(5u64)?;
        serializer.write_unsigned_integer(self.slot_no)?;
        Ok(serializer)
    }
}

impl Deserialize for InvalidHereafter {
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

impl DeserializeEmbeddedGroup for InvalidHereafter {
    fn deserialize_as_embedded_group<R: BufRead + Seek>(
        raw: &mut Deserializer<R>,
        _read_len: &mut CBORReadLen,
        len: cbor_event::Len,
    ) -> Result<Self, DeserializeError> {
        (|| -> Result<_, DeserializeError> {
            (|| -> Result<_, DeserializeError> {
                let index_0_value = raw.unsigned_integer()?;
                if index_0_value != 5 {
                    return Err(DeserializeFailure::FixedValueMismatch {
                        found: Key::Uint(index_0_value),
                        expected: Key::Uint(5),
                    }
                    .into());
                }
                Ok(())
            })()
            .map_err(|e| e.annotate("index_0"))?;
            let slot_no = Ok(raw.unsigned_integer()? as u64)
                .map_err(|e: DeserializeError| e.annotate("slot_no"))?;
            Ok(InvalidHereafter { slot_no })
        })()
        .map_err(|e| e.annotate("InvalidHereafter"))
    }
}

impl cbor_event::se::Serialize for Multiasset {
    fn serialize<'se, W: Write>(
        &self,
        serializer: &'se mut Serializer<W>,
    ) -> cbor_event::Result<&'se mut Serializer<W>> {
        serializer.write_map(cbor_event::Len::Len(2))?;
        serializer.write_text("policy_id")?;
        self.policy_id.serialize(serializer)?;
        serializer.write_text("asset_bundle")?;
        self.asset_bundle.serialize(serializer)?;
        Ok(serializer)
    }
}

impl Deserialize for Multiasset {
    fn deserialize<R: BufRead + Seek>(raw: &mut Deserializer<R>) -> Result<Self, DeserializeError> {
        let len = raw.map()?;
        let mut read_len = CBORReadLen::new(len);
        read_len.read_elems(2)?;
        read_len.finish()?;
        (|| -> Result<_, DeserializeError> {
            let mut policy_id = None;
            let mut asset_bundle = None;
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
                        "policy_id" => {
                            if policy_id.is_some() {
                                return Err(DeserializeFailure::DuplicateKey(Key::Str(
                                    "policy_id".into(),
                                ))
                                .into());
                            }
                            policy_id = Some(
                                Hash28::deserialize(raw)
                                    .map_err(|e: DeserializeError| e.annotate("policy_id"))?,
                            );
                        }
                        "asset_bundle" => {
                            if asset_bundle.is_some() {
                                return Err(DeserializeFailure::DuplicateKey(Key::Str(
                                    "asset_bundle".into(),
                                ))
                                .into());
                            }
                            asset_bundle = Some(
                                AssetQuantityU64::deserialize(raw)
                                    .map_err(|e: DeserializeError| e.annotate("asset_bundle"))?,
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
            let policy_id =
                match policy_id {
                    Some(x) => x,
                    None => {
                        return Err(DeserializeFailure::MandatoryFieldMissing(Key::Str(
                            String::from("policy_id"),
                        ))
                        .into())
                    }
                };
            let asset_bundle =
                match asset_bundle {
                    Some(x) => x,
                    None => {
                        return Err(DeserializeFailure::MandatoryFieldMissing(Key::Str(
                            String::from("asset_bundle"),
                        ))
                        .into())
                    }
                };
            ();
            Ok(Self {
                policy_id,
                asset_bundle,
            })
        })()
        .map_err(|e| e.annotate("Multiasset"))
    }
}

impl cbor_event::se::Serialize for NativeScript {
    fn serialize<'se, W: Write>(
        &self,
        serializer: &'se mut Serializer<W>,
    ) -> cbor_event::Result<&'se mut Serializer<W>> {
        match self {
            NativeScript::ScriptPubkey(script_pubkey) => script_pubkey.serialize(serializer),
            NativeScript::ScriptAll(script_all) => script_all.serialize(serializer),
            NativeScript::ScriptAny(script_any) => script_any.serialize(serializer),
            NativeScript::ScriptNOfK(script_n_of_k) => script_n_of_k.serialize(serializer),
            NativeScript::InvalidBefore(invalid_before) => invalid_before.serialize(serializer),
            NativeScript::InvalidHereafter(invalid_hereafter) => {
                invalid_hereafter.serialize(serializer)
            }
        }
    }
}

impl Deserialize for NativeScript {
    fn deserialize<R: BufRead + Seek>(raw: &mut Deserializer<R>) -> Result<Self, DeserializeError> {
        (|| -> Result<_, DeserializeError> {
            let len = raw.array()?;
            let initial_position = raw.as_mut_ref().stream_position().unwrap();
            let mut errs = Vec::new();
            let deser_variant = (|raw: &mut Deserializer<_>| -> Result<_, DeserializeError> {
                let mut read_len = CBORReadLen::new(len);
                read_len.read_elems(2)?;
                read_len.finish()?;
                let ret = ScriptPubkey::deserialize_as_embedded_group(raw, &mut read_len, len);
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
                Ok(script_pubkey) => return Ok(Self::ScriptPubkey(script_pubkey)),
                Err(e) => {
                    errs.push(e.annotate("ScriptPubkey"));
                    raw.as_mut_ref()
                        .seek(SeekFrom::Start(initial_position))
                        .unwrap();
                }
            };
            let deser_variant = (|raw: &mut Deserializer<_>| -> Result<_, DeserializeError> {
                let mut read_len = CBORReadLen::new(len);
                read_len.read_elems(2)?;
                read_len.finish()?;
                let ret = ScriptAll::deserialize_as_embedded_group(raw, &mut read_len, len);
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
                Ok(script_all) => return Ok(Self::ScriptAll(script_all)),
                Err(e) => {
                    errs.push(e.annotate("ScriptAll"));
                    raw.as_mut_ref()
                        .seek(SeekFrom::Start(initial_position))
                        .unwrap();
                }
            };
            let deser_variant = (|raw: &mut Deserializer<_>| -> Result<_, DeserializeError> {
                let mut read_len = CBORReadLen::new(len);
                read_len.read_elems(2)?;
                read_len.finish()?;
                let ret = ScriptAny::deserialize_as_embedded_group(raw, &mut read_len, len);
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
                Ok(script_any) => return Ok(Self::ScriptAny(script_any)),
                Err(e) => {
                    errs.push(e.annotate("ScriptAny"));
                    raw.as_mut_ref()
                        .seek(SeekFrom::Start(initial_position))
                        .unwrap();
                }
            };
            let deser_variant = (|raw: &mut Deserializer<_>| -> Result<_, DeserializeError> {
                let mut read_len = CBORReadLen::new(len);
                read_len.read_elems(3)?;
                read_len.finish()?;
                let ret = ScriptNOfK::deserialize_as_embedded_group(raw, &mut read_len, len);
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
                Ok(script_n_of_k) => return Ok(Self::ScriptNOfK(script_n_of_k)),
                Err(e) => {
                    errs.push(e.annotate("ScriptNOfK"));
                    raw.as_mut_ref()
                        .seek(SeekFrom::Start(initial_position))
                        .unwrap();
                }
            };
            let deser_variant = (|raw: &mut Deserializer<_>| -> Result<_, DeserializeError> {
                let mut read_len = CBORReadLen::new(len);
                read_len.read_elems(2)?;
                read_len.finish()?;
                let ret = InvalidBefore::deserialize_as_embedded_group(raw, &mut read_len, len);
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
                Ok(invalid_before) => return Ok(Self::InvalidBefore(invalid_before)),
                Err(e) => {
                    errs.push(e.annotate("InvalidBefore"));
                    raw.as_mut_ref()
                        .seek(SeekFrom::Start(initial_position))
                        .unwrap();
                }
            };
            let deser_variant = (|raw: &mut Deserializer<_>| -> Result<_, DeserializeError> {
                let mut read_len = CBORReadLen::new(len);
                read_len.read_elems(2)?;
                read_len.finish()?;
                let ret = InvalidHereafter::deserialize_as_embedded_group(raw, &mut read_len, len);
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
                Ok(invalid_hereafter) => return Ok(Self::InvalidHereafter(invalid_hereafter)),
                Err(e) => {
                    errs.push(e.annotate("InvalidHereafter"));
                    raw.as_mut_ref()
                        .seek(SeekFrom::Start(initial_position))
                        .unwrap();
                }
            };
            Err(DeserializeError::new(
                "NativeScript",
                DeserializeFailure::NoVariantMatchedWithCauses(errs),
            ))
        })()
        .map_err(|e| e.annotate("NativeScript"))
    }
}

impl cbor_event::se::Serialize for PlutusData {
    fn serialize<'se, W: Write>(
        &self,
        serializer: &'se mut Serializer<W>,
    ) -> cbor_event::Result<&'se mut Serializer<W>> {
        match self {
            PlutusData::Constr(constr) => constr.serialize(serializer),
            PlutusData::MapPlutusDataToPlutusData(map_plutus_data_to_plutus_data) => {
                serializer.write_map(cbor_event::Len::Len(
                    map_plutus_data_to_plutus_data.len() as u64
                ))?;
                for (key, value) in map_plutus_data_to_plutus_data.iter() {
                    key.serialize(serializer)?;
                    value.serialize(serializer)?;
                }
                Ok(serializer)
            }
            PlutusData::ArrPlutusData(arr_plutus_data) => {
                serializer.write_array(cbor_event::Len::Len(arr_plutus_data.len() as u64))?;
                for element in arr_plutus_data.iter() {
                    element.serialize(serializer)?;
                }
                Ok(serializer)
            }
            PlutusData::BigInt(big_int) => big_int.serialize(serializer),
            PlutusData::BoundedBytes(bounded_bytes) => bounded_bytes.serialize(serializer),
        }
    }
}

impl Deserialize for PlutusData {
    fn deserialize<R: BufRead + Seek>(raw: &mut Deserializer<R>) -> Result<Self, DeserializeError> {
        (|| -> Result<_, DeserializeError> {
            let initial_position = raw.as_mut_ref().stream_position().unwrap();
            let mut errs = Vec::new();
            let deser_variant: Result<_, DeserializeError> = Constr::deserialize(raw);
            match deser_variant {
                Ok(constr) => return Ok(Self::Constr(constr)),
                Err(e) => {
                    errs.push(e.annotate("Constr"));
                    raw.as_mut_ref()
                        .seek(SeekFrom::Start(initial_position))
                        .unwrap();
                }
            };
            let deser_variant = (|raw: &mut Deserializer<_>| -> Result<_, DeserializeError> {
                let mut map_plutus_data_to_plutus_data_table = BTreeMap::new();
                let map_plutus_data_to_plutus_data_len = raw.map()?;
                while match map_plutus_data_to_plutus_data_len {
                    cbor_event::Len::Len(n) => {
                        (map_plutus_data_to_plutus_data_table.len() as u64) < n
                    }
                    cbor_event::Len::Indefinite => true,
                } {
                    if raw.cbor_type()? == cbor_event::Type::Special {
                        assert_eq!(raw.special()?, cbor_event::Special::Break);
                        break;
                    }
                    let map_plutus_data_to_plutus_data_key = PlutusData::deserialize(raw)?;
                    let map_plutus_data_to_plutus_data_value = PlutusData::deserialize(raw)?;
                    if map_plutus_data_to_plutus_data_table
                        .insert(
                            map_plutus_data_to_plutus_data_key.clone(),
                            map_plutus_data_to_plutus_data_value,
                        )
                        .is_some()
                    {
                        return Err(DeserializeFailure::DuplicateKey(Key::Str(String::from(
                            "some complicated/unsupported type",
                        )))
                        .into());
                    }
                }
                Ok(map_plutus_data_to_plutus_data_table)
            })(raw);
            match deser_variant {
                Ok(map_plutus_data_to_plutus_data) => {
                    return Ok(Self::MapPlutusDataToPlutusData(
                        map_plutus_data_to_plutus_data,
                    ))
                }
                Err(e) => {
                    errs.push(e.annotate("MapPlutusDataToPlutusData"));
                    raw.as_mut_ref()
                        .seek(SeekFrom::Start(initial_position))
                        .unwrap();
                }
            };
            let deser_variant = (|raw: &mut Deserializer<_>| -> Result<_, DeserializeError> {
                let mut arr_plutus_data_arr = Vec::new();
                let len = raw.array()?;
                while match len {
                    cbor_event::Len::Len(n) => (arr_plutus_data_arr.len() as u64) < n,
                    cbor_event::Len::Indefinite => true,
                } {
                    if raw.cbor_type()? == cbor_event::Type::Special {
                        assert_eq!(raw.special()?, cbor_event::Special::Break);
                        break;
                    }
                    arr_plutus_data_arr.push(PlutusData::deserialize(raw)?);
                }
                Ok(arr_plutus_data_arr)
            })(raw);
            match deser_variant {
                Ok(arr_plutus_data) => return Ok(Self::ArrPlutusData(arr_plutus_data)),
                Err(e) => {
                    errs.push(e.annotate("ArrPlutusData"));
                    raw.as_mut_ref()
                        .seek(SeekFrom::Start(initial_position))
                        .unwrap();
                }
            };
            let deser_variant: Result<_, DeserializeError> = BigInt::deserialize(raw);
            match deser_variant {
                Ok(big_int) => return Ok(Self::BigInt(big_int)),
                Err(e) => {
                    errs.push(e.annotate("BigInt"));
                    raw.as_mut_ref()
                        .seek(SeekFrom::Start(initial_position))
                        .unwrap();
                }
            };
            let deser_variant: Result<_, DeserializeError> = BoundedBytes::deserialize(raw);
            match deser_variant {
                Ok(bounded_bytes) => return Ok(Self::BoundedBytes(bounded_bytes)),
                Err(e) => {
                    errs.push(e.annotate("BoundedBytes"));
                    raw.as_mut_ref()
                        .seek(SeekFrom::Start(initial_position))
                        .unwrap();
                }
            };
            Err(DeserializeError::new(
                "PlutusData",
                DeserializeFailure::NoVariantMatchedWithCauses(errs),
            ))
        })()
        .map_err(|e| e.annotate("PlutusData"))
    }
}

impl cbor_event::se::Serialize for PoolDeposit {
    fn serialize<'se, W: Write>(
        &self,
        serializer: &'se mut Serializer<W>,
    ) -> cbor_event::Result<&'se mut Serializer<W>> {
        serializer.write_array(cbor_event::Len::Len(2))?;
        self.serialize_as_embedded_group(serializer)
    }
}

impl SerializeEmbeddedGroup for PoolDeposit {
    fn serialize_as_embedded_group<'se, W: Write>(
        &self,
        serializer: &'se mut Serializer<W>,
    ) -> cbor_event::Result<&'se mut Serializer<W>> {
        serializer.write_unsigned_integer(1u64)?;
        self.keyhash.serialize(serializer)?;
        Ok(serializer)
    }
}

impl Deserialize for PoolDeposit {
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

impl DeserializeEmbeddedGroup for PoolDeposit {
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
            let keyhash =
                Hash28::deserialize(raw).map_err(|e: DeserializeError| e.annotate("keyhash"))?;
            Ok(PoolDeposit { keyhash })
        })()
        .map_err(|e| e.annotate("PoolDeposit"))
    }
}

impl cbor_event::se::Serialize for Script {
    fn serialize<'se, W: Write>(
        &self,
        serializer: &'se mut Serializer<W>,
    ) -> cbor_event::Result<&'se mut Serializer<W>> {
        match self {
            Script::Naitve => {
                serializer.write_array(cbor_event::Len::Len(1))?;
                serializer.write_unsigned_integer(0u64)
            }
            Script::PlutusV1 => {
                serializer.write_array(cbor_event::Len::Len(1))?;
                serializer.write_unsigned_integer(1u64)
            }
            Script::PlutusV2 => {
                serializer.write_array(cbor_event::Len::Len(1))?;
                serializer.write_unsigned_integer(2u64)
            }
            Script::PlutusV3 => {
                serializer.write_array(cbor_event::Len::Len(1))?;
                serializer.write_unsigned_integer(3u64)
            }
        }
    }
}

impl Deserialize for Script {
    fn deserialize<R: BufRead + Seek>(raw: &mut Deserializer<R>) -> Result<Self, DeserializeError> {
        (|| -> Result<_, DeserializeError> {
            let len = raw.array()?;
            let initial_position = raw.as_mut_ref().stream_position().unwrap();
            let mut errs = Vec::new();
            let deser_variant = (|raw: &mut Deserializer<_>| -> Result<_, DeserializeError> {
                let mut read_len = CBORReadLen::new(len);
                read_len.read_elems(1)?;
                read_len.finish()?;
                let naitve_value = raw.unsigned_integer()?;
                if naitve_value != 0 {
                    return Err(DeserializeFailure::FixedValueMismatch {
                        found: Key::Uint(naitve_value),
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
                Ok(()) => return Ok(Script::Naitve),
                Err(e) => {
                    errs.push(e.annotate("Naitve"));
                    raw.as_mut_ref()
                        .seek(SeekFrom::Start(initial_position))
                        .unwrap();
                }
            };
            let deser_variant = (|raw: &mut Deserializer<_>| -> Result<_, DeserializeError> {
                let mut read_len = CBORReadLen::new(len);
                read_len.read_elems(1)?;
                read_len.finish()?;
                let plutus_v1_value = raw.unsigned_integer()?;
                if plutus_v1_value != 1 {
                    return Err(DeserializeFailure::FixedValueMismatch {
                        found: Key::Uint(plutus_v1_value),
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
                Ok(()) => return Ok(Script::PlutusV1),
                Err(e) => {
                    errs.push(e.annotate("PlutusV1"));
                    raw.as_mut_ref()
                        .seek(SeekFrom::Start(initial_position))
                        .unwrap();
                }
            };
            let deser_variant = (|raw: &mut Deserializer<_>| -> Result<_, DeserializeError> {
                let mut read_len = CBORReadLen::new(len);
                read_len.read_elems(1)?;
                read_len.finish()?;
                let plutus_v2_value = raw.unsigned_integer()?;
                if plutus_v2_value != 2 {
                    return Err(DeserializeFailure::FixedValueMismatch {
                        found: Key::Uint(plutus_v2_value),
                        expected: Key::Uint(2),
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
                Ok(()) => return Ok(Script::PlutusV2),
                Err(e) => {
                    errs.push(e.annotate("PlutusV2"));
                    raw.as_mut_ref()
                        .seek(SeekFrom::Start(initial_position))
                        .unwrap();
                }
            };
            let deser_variant = (|raw: &mut Deserializer<_>| -> Result<_, DeserializeError> {
                let mut read_len = CBORReadLen::new(len);
                read_len.read_elems(1)?;
                read_len.finish()?;
                let plutus_v3_value = raw.unsigned_integer()?;
                if plutus_v3_value != 3 {
                    return Err(DeserializeFailure::FixedValueMismatch {
                        found: Key::Uint(plutus_v3_value),
                        expected: Key::Uint(3),
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
                Ok(()) => return Ok(Script::PlutusV3),
                Err(e) => {
                    errs.push(e.annotate("PlutusV3"));
                    raw.as_mut_ref()
                        .seek(SeekFrom::Start(initial_position))
                        .unwrap();
                }
            };
            Err(DeserializeError::new(
                "Script",
                DeserializeFailure::NoVariantMatchedWithCauses(errs),
            ))
        })()
        .map_err(|e| e.annotate("Script"))
    }
}

impl cbor_event::se::Serialize for ScriptAll {
    fn serialize<'se, W: Write>(
        &self,
        serializer: &'se mut Serializer<W>,
    ) -> cbor_event::Result<&'se mut Serializer<W>> {
        serializer.write_array(cbor_event::Len::Len(2))?;
        self.serialize_as_embedded_group(serializer)
    }
}

impl SerializeEmbeddedGroup for ScriptAll {
    fn serialize_as_embedded_group<'se, W: Write>(
        &self,
        serializer: &'se mut Serializer<W>,
    ) -> cbor_event::Result<&'se mut Serializer<W>> {
        serializer.write_unsigned_integer(1u64)?;
        serializer.write_array(cbor_event::Len::Len(self.native_scripts.len() as u64))?;
        for element in self.native_scripts.iter() {
            element.serialize(serializer)?;
        }
        Ok(serializer)
    }
}

impl Deserialize for ScriptAll {
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

impl DeserializeEmbeddedGroup for ScriptAll {
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
            let native_scripts = (|| -> Result<_, DeserializeError> {
                let mut native_scripts_arr = Vec::new();
                let len = raw.array()?;
                while match len {
                    cbor_event::Len::Len(n) => (native_scripts_arr.len() as u64) < n,
                    cbor_event::Len::Indefinite => true,
                } {
                    if raw.cbor_type()? == cbor_event::Type::Special {
                        assert_eq!(raw.special()?, cbor_event::Special::Break);
                        break;
                    }
                    native_scripts_arr.push(NativeScript::deserialize(raw)?);
                }
                Ok(native_scripts_arr)
            })()
            .map_err(|e| e.annotate("native_scripts"))?;
            Ok(ScriptAll { native_scripts })
        })()
        .map_err(|e| e.annotate("ScriptAll"))
    }
}

impl cbor_event::se::Serialize for ScriptAny {
    fn serialize<'se, W: Write>(
        &self,
        serializer: &'se mut Serializer<W>,
    ) -> cbor_event::Result<&'se mut Serializer<W>> {
        serializer.write_array(cbor_event::Len::Len(2))?;
        self.serialize_as_embedded_group(serializer)
    }
}

impl SerializeEmbeddedGroup for ScriptAny {
    fn serialize_as_embedded_group<'se, W: Write>(
        &self,
        serializer: &'se mut Serializer<W>,
    ) -> cbor_event::Result<&'se mut Serializer<W>> {
        serializer.write_unsigned_integer(2u64)?;
        serializer.write_array(cbor_event::Len::Len(self.native_scripts.len() as u64))?;
        for element in self.native_scripts.iter() {
            element.serialize(serializer)?;
        }
        Ok(serializer)
    }
}

impl Deserialize for ScriptAny {
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

impl DeserializeEmbeddedGroup for ScriptAny {
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
            let native_scripts = (|| -> Result<_, DeserializeError> {
                let mut native_scripts_arr = Vec::new();
                let len = raw.array()?;
                while match len {
                    cbor_event::Len::Len(n) => (native_scripts_arr.len() as u64) < n,
                    cbor_event::Len::Indefinite => true,
                } {
                    if raw.cbor_type()? == cbor_event::Type::Special {
                        assert_eq!(raw.special()?, cbor_event::Special::Break);
                        break;
                    }
                    native_scripts_arr.push(NativeScript::deserialize(raw)?);
                }
                Ok(native_scripts_arr)
            })()
            .map_err(|e| e.annotate("native_scripts"))?;
            Ok(ScriptAny { native_scripts })
        })()
        .map_err(|e| e.annotate("ScriptAny"))
    }
}

impl cbor_event::se::Serialize for ScriptNOfK {
    fn serialize<'se, W: Write>(
        &self,
        serializer: &'se mut Serializer<W>,
    ) -> cbor_event::Result<&'se mut Serializer<W>> {
        serializer.write_array(cbor_event::Len::Len(3))?;
        self.serialize_as_embedded_group(serializer)
    }
}

impl SerializeEmbeddedGroup for ScriptNOfK {
    fn serialize_as_embedded_group<'se, W: Write>(
        &self,
        serializer: &'se mut Serializer<W>,
    ) -> cbor_event::Result<&'se mut Serializer<W>> {
        serializer.write_unsigned_integer(3u64)?;
        if self.n >= 0 {
            serializer.write_unsigned_integer(self.n as u64)?;
        } else {
            serializer.write_negative_integer_sz(
                self.n as i128,
                cbor_event::Sz::canonical((self.n + 1).abs() as u64),
            )?;
        }
        serializer.write_array(cbor_event::Len::Len(self.native_scripts.len() as u64))?;
        for element in self.native_scripts.iter() {
            element.serialize(serializer)?;
        }
        Ok(serializer)
    }
}

impl Deserialize for ScriptNOfK {
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

impl DeserializeEmbeddedGroup for ScriptNOfK {
    fn deserialize_as_embedded_group<R: BufRead + Seek>(
        raw: &mut Deserializer<R>,
        _read_len: &mut CBORReadLen,
        len: cbor_event::Len,
    ) -> Result<Self, DeserializeError> {
        (|| -> Result<_, DeserializeError> {
            (|| -> Result<_, DeserializeError> {
                let index_0_value = raw.unsigned_integer()?;
                if index_0_value != 3 {
                    return Err(DeserializeFailure::FixedValueMismatch {
                        found: Key::Uint(index_0_value),
                        expected: Key::Uint(3),
                    }
                    .into());
                }
                Ok(())
            })()
            .map_err(|e| e.annotate("index_0"))?;
            let n = (|| -> Result<_, DeserializeError> {
                Ok(match raw.cbor_type()? {
                    cbor_event::Type::UnsignedInteger => raw.unsigned_integer()? as i64,
                    _ => raw.negative_integer_sz().map(|(x, _enc)| x)? as i64,
                })
            })()
            .map_err(|e| e.annotate("n"))?;
            let native_scripts = (|| -> Result<_, DeserializeError> {
                let mut native_scripts_arr = Vec::new();
                let len = raw.array()?;
                while match len {
                    cbor_event::Len::Len(n) => (native_scripts_arr.len() as u64) < n,
                    cbor_event::Len::Indefinite => true,
                } {
                    if raw.cbor_type()? == cbor_event::Type::Special {
                        assert_eq!(raw.special()?, cbor_event::Special::Break);
                        break;
                    }
                    native_scripts_arr.push(NativeScript::deserialize(raw)?);
                }
                Ok(native_scripts_arr)
            })()
            .map_err(|e| e.annotate("native_scripts"))?;
            Ok(ScriptNOfK { n, native_scripts })
        })()
        .map_err(|e| e.annotate("ScriptNOfK"))
    }
}

impl cbor_event::se::Serialize for ScriptPubkey {
    fn serialize<'se, W: Write>(
        &self,
        serializer: &'se mut Serializer<W>,
    ) -> cbor_event::Result<&'se mut Serializer<W>> {
        serializer.write_array(cbor_event::Len::Len(2))?;
        self.serialize_as_embedded_group(serializer)
    }
}

impl SerializeEmbeddedGroup for ScriptPubkey {
    fn serialize_as_embedded_group<'se, W: Write>(
        &self,
        serializer: &'se mut Serializer<W>,
    ) -> cbor_event::Result<&'se mut Serializer<W>> {
        serializer.write_unsigned_integer(0u64)?;
        self.hash_28.serialize(serializer)?;
        Ok(serializer)
    }
}

impl Deserialize for ScriptPubkey {
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

impl DeserializeEmbeddedGroup for ScriptPubkey {
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
            let hash_28 =
                Hash28::deserialize(raw).map_err(|e: DeserializeError| e.annotate("hash_28"))?;
            Ok(ScriptPubkey { hash_28 })
        })()
        .map_err(|e| e.annotate("ScriptPubkey"))
    }
}

impl cbor_event::se::Serialize for ShelleyTxOut {
    fn serialize<'se, W: Write>(
        &self,
        serializer: &'se mut Serializer<W>,
    ) -> cbor_event::Result<&'se mut Serializer<W>> {
        serializer.write_array(cbor_event::Len::Len(
            2 + match &self.hash_32 {
                Some(x) => 1,
                None => 0,
            },
        ))?;
        serializer.write_bytes(&self.address)?;
        self.value.serialize(serializer)?;
        if let Some(field) = &self.hash_32 {
            field.serialize(serializer)?;
        }
        Ok(serializer)
    }
}

impl Deserialize for ShelleyTxOut {
    fn deserialize<R: BufRead + Seek>(raw: &mut Deserializer<R>) -> Result<Self, DeserializeError> {
        let len = raw.array()?;
        let mut read_len = CBORReadLen::new(len);
        read_len.read_elems(2)?;
        (|| -> Result<_, DeserializeError> {
            let address =
                Ok(raw.bytes()? as Vec<u8>).map_err(|e: DeserializeError| e.annotate("address"))?;
            let value =
                Value::deserialize(raw).map_err(|e: DeserializeError| e.annotate("value"))?;
            let hash_32 = if raw
                .cbor_type()
                .map(|ty| ty == cbor_event::Type::Bytes)
                .unwrap_or(false)
            {
                (|| -> Result<_, DeserializeError> {
                    read_len.read_elems(1)?;
                    Hash32::deserialize(raw)
                })()
                .map_err(|e| e.annotate("hash_32"))
                .map(Some)
            } else {
                Ok(None)
            }?;
            match len {
                cbor_event::Len::Len(_) => read_len.finish()?,
                cbor_event::Len::Indefinite => match raw.special()? {
                    cbor_event::Special::Break => read_len.finish()?,
                    _ => return Err(DeserializeFailure::EndingBreakMissing.into()),
                },
            }
            Ok(ShelleyTxOut {
                address,
                value,
                hash_32,
            })
        })()
        .map_err(|e| e.annotate("ShelleyTxOut"))
    }
}

impl cbor_event::se::Serialize for TxIn {
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

impl Deserialize for TxIn {
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
            Ok(TxIn { hash_32, uint })
        })()
        .map_err(|e| e.annotate("TxIn"))
    }
}

impl cbor_event::se::Serialize for TxOut {
    fn serialize<'se, W: Write>(
        &self,
        serializer: &'se mut Serializer<W>,
    ) -> cbor_event::Result<&'se mut Serializer<W>> {
        match self {
            TxOut::ShelleyTxOut(shelley_tx_out) => shelley_tx_out.serialize(serializer),
            TxOut::BabbageTxOut(babbage_tx_out) => babbage_tx_out.serialize(serializer),
        }
    }
}

impl Deserialize for TxOut {
    fn deserialize<R: BufRead + Seek>(raw: &mut Deserializer<R>) -> Result<Self, DeserializeError> {
        (|| -> Result<_, DeserializeError> {
            match raw.cbor_type()? {
                cbor_event::Type::Array => Ok(TxOut::ShelleyTxOut(ShelleyTxOut::deserialize(raw)?)),
                cbor_event::Type::Map => Ok(TxOut::BabbageTxOut(BabbageTxOut::deserialize(raw)?)),
                _ => Err(DeserializeError::new(
                    "TxOut",
                    DeserializeFailure::NoVariantMatched,
                )),
            }
        })()
        .map_err(|e| e.annotate("TxOut"))
    }
}

impl cbor_event::se::Serialize for UtxoState {
    fn serialize<'se, W: Write>(
        &self,
        serializer: &'se mut Serializer<W>,
    ) -> cbor_event::Result<&'se mut Serializer<W>> {
        serializer.write_map(cbor_event::Len::Len(4))?;
        serializer.write_text("fees")?;
        serializer.write_unsigned_integer(self.fees)?;
        serializer.write_text("utxos")?;
        serializer.write_map(cbor_event::Len::Len(self.utxos.len() as u64))?;
        for (key, value) in self.utxos.iter() {
            key.serialize(serializer)?;
            value.serialize(serializer)?;
        }
        serializer.write_text("deposits")?;
        serializer.write_map(cbor_event::Len::Len(self.deposits.len() as u64))?;
        for (key, value) in self.deposits.iter() {
            key.serialize(serializer)?;
            serializer.write_unsigned_integer(*value)?;
        }
        serializer.write_text("donations")?;
        serializer.write_unsigned_integer(self.donations)?;
        Ok(serializer)
    }
}

impl Deserialize for UtxoState {
    fn deserialize<R: BufRead + Seek>(raw: &mut Deserializer<R>) -> Result<Self, DeserializeError> {
        let len = raw.map()?;
        let mut read_len = CBORReadLen::new(len);
        read_len.read_elems(4)?;
        read_len.finish()?;
        (|| -> Result<_, DeserializeError> {
            let mut fees = None;
            let mut utxos = None;
            let mut deposits = None;
            let mut donations = None;
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
                        "fees" => {
                            if fees.is_some() {
                                return Err(DeserializeFailure::DuplicateKey(Key::Str(
                                    "fees".into(),
                                ))
                                .into());
                            }
                            fees = Some(
                                Ok(raw.unsigned_integer()? as u64)
                                    .map_err(|e: DeserializeError| e.annotate("fees"))?,
                            );
                        }
                        "utxos" => {
                            if utxos.is_some() {
                                return Err(DeserializeFailure::DuplicateKey(Key::Str(
                                    "utxos".into(),
                                ))
                                .into());
                            }
                            utxos = Some(
                                (|| -> Result<_, DeserializeError> {
                                    let mut utxos_table = BTreeMap::new();
                                    let utxos_len = raw.map()?;
                                    while match utxos_len {
                                        cbor_event::Len::Len(n) => (utxos_table.len() as u64) < n,
                                        cbor_event::Len::Indefinite => true,
                                    } {
                                        if raw.cbor_type()? == cbor_event::Type::Special {
                                            assert_eq!(raw.special()?, cbor_event::Special::Break);
                                            break;
                                        }
                                        let utxos_key = TxIn::deserialize(raw)?;
                                        let utxos_value = TxOut::deserialize(raw)?;
                                        if utxos_table
                                            .insert(utxos_key.clone(), utxos_value)
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
                                    Ok(utxos_table)
                                })()
                                .map_err(|e| e.annotate("utxos"))?,
                            );
                        }
                        "deposits" => {
                            if deposits.is_some() {
                                return Err(DeserializeFailure::DuplicateKey(Key::Str(
                                    "deposits".into(),
                                ))
                                .into());
                            }
                            deposits = Some(
                                (|| -> Result<_, DeserializeError> {
                                    let mut deposits_table = BTreeMap::new();
                                    let deposits_len = raw.map()?;
                                    while match deposits_len {
                                        cbor_event::Len::Len(n) => {
                                            (deposits_table.len() as u64) < n
                                        }
                                        cbor_event::Len::Indefinite => true,
                                    } {
                                        if raw.cbor_type()? == cbor_event::Type::Special {
                                            assert_eq!(raw.special()?, cbor_event::Special::Break);
                                            break;
                                        }
                                        let deposits_key = Deposit::deserialize(raw)?;
                                        let deposits_value = raw.unsigned_integer()? as u64;
                                        if deposits_table
                                            .insert(deposits_key.clone(), deposits_value)
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
                                    Ok(deposits_table)
                                })()
                                .map_err(|e| e.annotate("deposits"))?,
                            );
                        }
                        "donations" => {
                            if donations.is_some() {
                                return Err(DeserializeFailure::DuplicateKey(Key::Str(
                                    "donations".into(),
                                ))
                                .into());
                            }
                            donations = Some(
                                Ok(raw.unsigned_integer()? as u64)
                                    .map_err(|e: DeserializeError| e.annotate("donations"))?,
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
            let utxos = match utxos {
                Some(x) => x,
                None => {
                    return Err(
                        DeserializeFailure::MandatoryFieldMissing(Key::Str(String::from("utxos")))
                            .into(),
                    )
                }
            };
            let fees = match fees {
                Some(x) => x,
                None => {
                    return Err(
                        DeserializeFailure::MandatoryFieldMissing(Key::Str(String::from("fees")))
                            .into(),
                    )
                }
            };
            let deposits =
                match deposits {
                    Some(x) => x,
                    None => {
                        return Err(DeserializeFailure::MandatoryFieldMissing(Key::Str(
                            String::from("deposits"),
                        ))
                        .into())
                    }
                };
            let donations =
                match donations {
                    Some(x) => x,
                    None => {
                        return Err(DeserializeFailure::MandatoryFieldMissing(Key::Str(
                            String::from("donations"),
                        ))
                        .into())
                    }
                };
            ();
            Ok(Self {
                utxos,
                fees,
                deposits,
                donations,
            })
        })()
        .map_err(|e| e.annotate("UtxoState"))
    }
}

impl cbor_event::se::Serialize for Value {
    fn serialize<'se, W: Write>(
        &self,
        serializer: &'se mut Serializer<W>,
    ) -> cbor_event::Result<&'se mut Serializer<W>> {
        match self {
            Value::Coin(coin) => serializer.write_unsigned_integer(*coin),
            Value::AssetValue(asset_value) => asset_value.serialize(serializer),
        }
    }
}

impl Deserialize for Value {
    fn deserialize<R: BufRead + Seek>(raw: &mut Deserializer<R>) -> Result<Self, DeserializeError> {
        (|| -> Result<_, DeserializeError> {
            match raw.cbor_type()? {
                cbor_event::Type::UnsignedInteger => {
                    Ok(Value::Coin(raw.unsigned_integer()? as u64))
                }
                cbor_event::Type::Array => Ok(Value::AssetValue(AssetValue::deserialize(raw)?)),
                _ => Err(DeserializeError::new(
                    "Value",
                    DeserializeFailure::NoVariantMatched,
                )),
            }
        })()
        .map_err(|e| e.annotate("Value"))
    }
}
