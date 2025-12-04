// Copyright 2025 PRAGMA
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
use minicbor as cbor;
#[cfg(test)]
use std::fmt::Display;

// Misc
// ----------------------------------------------------------------------------

pub fn decode_break<'d>(
    d: &mut cbor::Decoder<'d>,
    len: Option<u64>,
) -> Result<bool, cbor::decode::Error> {
    if d.datatype()? == cbor::data::Type::Break {
        // NOTE: If we encounter a rogue Break while decoding a definite map, that's an error.
        if len.is_some() {
            return Err(cbor::decode::Error::type_mismatch(cbor::data::Type::Break));
        }

        d.skip()?;

        return Ok(true);
    }

    Ok(false)
}

// Array
// ----------------------------------------------------------------------------

/// Decode any heterogeneous CBOR array, irrespective of whether they're indefinite or definite.
pub fn heterogeneous_array<'d, A>(
    d: &mut cbor::Decoder<'d>,
    elems: impl FnOnce(
        &mut cbor::Decoder<'d>,
        Box<dyn FnOnce(u64) -> Result<(), cbor::decode::Error>>,
    ) -> Result<A, cbor::decode::Error>,
) -> Result<A, cbor::decode::Error> {
    let len = d.array()?;

    match len {
        None => {
            let result = elems(d, Box::new(|_| Ok(())))?;
            decode_break(d, len)?;
            Ok(result)
        }
        Some(len) => elems(
            d,
            Box::new(move |expected_len| {
                if len != expected_len {
                    return Err(cbor::decode::Error::message(format!(
                        "CBOR array length mismatch: expected {expected_len} got {len}"
                    )));
                }

                Ok(())
            }),
        ),
    }
}

// Map
// ----------------------------------------------------------------------------

/// Decode any heterogeneous CBOR map, irrespective of whether they're indefinite or definite.
///
/// A good choice for `S` is generally to pick a tuple of `PartialDecoder<_>` for each field item
/// that needs decoding. For example:
///
/// ```rs
/// let (address, value, datum, script) = decode_map(
///     d,
///     (
///         missing_field::<Output, _>(0),
///         missing_field::<Output, _>(1),
///         with_default_value(MemoizedDatum::None),
///         with_default_value(None),
///     ),
///     |d| d.u8(),
///     |d, state, field| {
///         match field {
///             0 => state.0 = decode_chunk(d, |d| decode_address(d.bytes()?)),
///             1 => state.1 = decode_chunk(d, |d| d.decode()),
///             2 => state.2 = decode_chunk(d, decode_datum),
///             3 => state.3 = decode_chunk(d, decode_reference_script),
///             _ => return unexpected_field::<Output, _>(field),
///         }
///         Ok(())
///     },
/// )?;
/// ```
#[cfg(test)]
pub fn heterogeneous_map<K, S>(
    d: &mut cbor::Decoder<'_>,
    mut state: S,
    decode_key: impl Fn(&mut cbor::Decoder<'_>) -> Result<K, cbor::decode::Error>,
    mut decode_value: impl FnMut(&mut cbor::Decoder<'_>, &mut S, K) -> Result<(), cbor::decode::Error>,
) -> Result<S, cbor::decode::Error> {
    let len = d.map()?;

    let mut n = 0;
    while len.is_none() || Some(n) < len {
        if decode_break(d, len)? {
            break;
        }

        let k = decode_key(d)?;
        decode_value(d, &mut state, k)?;

        n += 1;
    }

    Ok(state)
}

// PartialDecoder
// ----------------------------------------------------------------------------

/// A decoder that is part of another larger one. This is particularly useful to decode map
/// key/value in an arbitrary order; while logically recomposing them in a readable order.
#[cfg(test)]
type PartialDecoder<A> = Box<dyn FnOnce() -> Result<A, cbor::decode::Error>>;

/// Wrap a decoder as a `PartialDecoder`; this is mostly a convenient utility to avoid boilerplate.
#[cfg(test)]
pub fn decode_chunk<A: 'static>(
    d: &mut cbor::Decoder<'_>,
    decode: impl FnOnce(&mut cbor::Decoder<'_>) -> Result<A, cbor::decode::Error>,
) -> PartialDecoder<A> {
    // NOTE: It is crucial that this happens *outside* of the boxed closure, to ensure bytes are consumed
    // when the closure is created; not when it is invoked!
    let a = decode(d);
    Box::new(|| a)
}

/// Yield a `PartialDecoder` that fails with a comprehensible error message when an expected field
/// is missing from the map.
#[cfg(test)]
pub fn missing_field<C: ?Sized, A>(field_tag: impl Display) -> PartialDecoder<A> {
    let msg = format!(
        "missing <{}> at field .{field_tag} in <{}> CBOR map",
        std::any::type_name::<A>(),
        std::any::type_name::<C>(),
    );
    Box::new(move || Err(cbor::decode::Error::message(msg)))
}

/// Yield a `PartialDecoder` that always succeeds with the given default value.
#[cfg(test)]
pub fn with_default_value<A: 'static>(default: A) -> PartialDecoder<A> {
    Box::new(move || Ok(default))
}

/// Yield a `Result<_, decode::Error>` that always fails with a comprehensible error message when a
/// map key is unexpected.
#[cfg(test)]
pub fn unexpected_field<C: ?Sized, A>(field_tag: impl Display) -> Result<A, cbor::decode::Error> {
    Err(cbor::decode::Error::message(format!(
        "unexpected field .{field_tag} in <{}> CBOR map",
        std::any::type_name::<C>(),
    )))
}

// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fmt::Debug;

    // Test fixtures
    #[derive(Debug, PartialEq, Eq, Clone, Copy)]
    struct Foo {
        field0: u64,
        field1: u64,
    }

    // Wrapper for encoding as definite-length array
    struct AsDefinite<T>(T);

    impl cbor::encode::Encode<()> for AsDefinite<&Foo> {
        fn encode<W: cbor::encode::Write>(
            &self,
            e: &mut cbor::Encoder<W>,
            _ctx: &mut (),
        ) -> Result<(), cbor::encode::Error<W::Error>> {
            e.array(2)?;
            e.u64(self.0.field0)?;
            e.u64(self.0.field1)?;
            Ok(())
        }
    }

    impl<'d, C> cbor::decode::Decode<'d, C> for AsDefinite<Foo> {
        fn decode(d: &mut cbor::Decoder<'d>, ctx: &mut C) -> Result<Self, cbor::decode::Error> {
            let len = d.array()?;
            if len != Some(2) {
                return Err(cbor::decode::Error::message(
                    "expected definite array of length 2",
                ));
            }
            Ok(AsDefinite(Foo {
                field0: d.decode_with(ctx)?,
                field1: d.decode_with(ctx)?,
            }))
        }
    }

    // Wrapper for encoding as indefinite-length array
    struct AsIndefinite<T>(T);

    impl cbor::encode::Encode<()> for AsIndefinite<&Foo> {
        fn encode<W: cbor::encode::Write>(
            &self,
            e: &mut cbor::Encoder<W>,
            _ctx: &mut (),
        ) -> Result<(), cbor::encode::Error<W::Error>> {
            e.begin_array()?;
            e.u64(self.0.field0)?;
            e.u64(self.0.field1)?;
            e.end()?;
            Ok(())
        }
    }

    impl<'d, C> cbor::decode::Decode<'d, C> for AsIndefinite<Foo> {
        fn decode(d: &mut cbor::Decoder<'d>, ctx: &mut C) -> Result<Self, cbor::decode::Error> {
            let len = d.array()?;
            if len.is_some() {
                return Err(cbor::decode::Error::message("expected indefinite array"));
            }
            let field0 = d.decode_with(ctx)?;
            let field1 = d.decode_with(ctx)?;
            if d.datatype()? != cbor::data::Type::Break {
                return Err(cbor::decode::Error::message("expected break"));
            }
            d.skip()?;
            Ok(AsIndefinite(Foo { field0, field1 }))
        }
    }

    // Wrapper for encoding as map
    struct AsMap<T>(T);

    impl cbor::encode::Encode<()> for AsMap<&Foo> {
        fn encode<W: cbor::encode::Write>(
            &self,
            e: &mut cbor::Encoder<W>,
            _ctx: &mut (),
        ) -> Result<(), cbor::encode::Error<W::Error>> {
            e.map(2)?;
            e.u8(0)?;
            e.u64(self.0.field0)?;
            e.u8(1)?;
            e.u64(self.0.field1)?;
            Ok(())
        }
    }

    // Composed encoders
    impl cbor::encode::Encode<()> for AsIndefinite<AsMap<&Foo>> {
        fn encode<W: cbor::encode::Write>(
            &self,
            e: &mut cbor::Encoder<W>,
            _ctx: &mut (),
        ) -> Result<(), cbor::encode::Error<W::Error>> {
            e.begin_map()?;
            e.u8(0)?;
            e.u64(self.0 .0.field0)?;
            e.u8(1)?;
            e.u64(self.0 .0.field1)?;
            e.end()?;
            Ok(())
        }
    }

    impl cbor::encode::Encode<()> for AsDefinite<AsMap<&Foo>> {
        fn encode<W: cbor::encode::Write>(
            &self,
            e: &mut cbor::Encoder<W>,
            _ctx: &mut (),
        ) -> Result<(), cbor::encode::Error<W::Error>> {
            e.map(2)?;
            e.u8(0)?;
            e.u64(self.0 .0.field0)?;
            e.u8(1)?;
            e.u64(self.0 .0.field1)?;
            Ok(())
        }
    }

    // Helper functions
    fn to_cbor<T: for<'c> cbor::encode::Encode<()>>(value: &T) -> Vec<u8> {
        let mut buf = Vec::new();
        let mut encoder = cbor::Encoder::new(&mut buf);
        encoder.encode(value).unwrap();
        buf
    }

    fn from_cbor<'d, T: cbor::decode::Decode<'d, ()>>(bytes: &'d [u8]) -> Option<T> {
        cbor::decode(bytes).ok()
    }

    fn from_cbor_no_leftovers<'d, T: cbor::decode::Decode<'d, ()>>(
        bytes: &'d [u8],
    ) -> Result<T, cbor::decode::Error> {
        let mut decoder = cbor::Decoder::new(bytes);
        let result = decoder.decode()?;
        if decoder.position() != bytes.len() {
            return Err(cbor::decode::Error::message("leftover bytes"));
        }
        Ok(result)
    }

    fn assert_ok<T: Eq + Debug + for<'d> cbor::decode::Decode<'d, ()>>(left: T, bytes: &[u8]) {
        assert_eq!(
            Ok(left),
            from_cbor_no_leftovers::<T>(bytes).map_err(|e| e.to_string())
        );
    }

    fn assert_err<T: Debug + for<'d> cbor::decode::Decode<'d, ()>>(msg: &str, bytes: &[u8]) {
        match from_cbor_no_leftovers::<T>(bytes).map_err(|e| e.to_string()) {
            Err(e) => assert!(e.contains(msg), "{e}"),
            Ok(ok) => panic!("expected error but got {:#?}", ok),
        }
    }

    const FIXTURE: Foo = Foo {
        field0: 14,
        field1: 42,
    };

    mod heterogeneous_array_tests {
        use super::*;

        #[test]
        fn happy_case() {
            #[derive(Debug, PartialEq, Eq)]
            struct TestCase<A>(A);

            // A flexible decoder that can ingest both definite and indefinite arrays.
            impl<'d, C> cbor::decode::Decode<'d, C> for TestCase<Foo> {
                fn decode(
                    d: &mut cbor::Decoder<'d>,
                    ctx: &mut C,
                ) -> Result<Self, cbor::decode::Error> {
                    heterogeneous_array(d, |d, assert_len| {
                        assert_len(2)?;
                        Ok(TestCase(Foo {
                            field0: d.decode_with(ctx)?,
                            field1: d.decode_with(ctx)?,
                        }))
                    })
                }
            }

            assert_ok(TestCase(FIXTURE), &to_cbor(&AsDefinite(&FIXTURE)));
            assert_ok(TestCase(FIXTURE), &to_cbor(&AsIndefinite(&FIXTURE)));
        }

        #[test]
        fn smaller_definite_length() {
            #[derive(Debug, PartialEq, Eq)]
            struct TestCase<A>(A);

            // A decoder which expects less elements than actually supplied.
            impl<'d, C> cbor::decode::Decode<'d, C> for TestCase<Foo> {
                fn decode(
                    d: &mut cbor::Decoder<'d>,
                    ctx: &mut C,
                ) -> Result<Self, cbor::decode::Error> {
                    heterogeneous_array(d, |d, assert_len| {
                        assert_len(1)?;
                        Ok(TestCase(Foo {
                            field0: d.decode_with(ctx)?,
                            field1: d.decode_with(ctx)?,
                        }))
                    })
                }
            }

            assert_err::<TestCase<Foo>>("array length mismatch", &to_cbor(&AsDefinite(&FIXTURE)));
        }

        #[test]
        fn larger_definite_length() {
            #[derive(Debug, PartialEq, Eq)]
            struct TestCase<A>(A);

            // A decoder which expects more elements than actually supplied.
            impl<'d, C> cbor::decode::Decode<'d, C> for TestCase<Foo> {
                fn decode(
                    d: &mut cbor::Decoder<'d>,
                    ctx: &mut C,
                ) -> Result<Self, cbor::decode::Error> {
                    heterogeneous_array(d, |d, assert_len| {
                        assert_len(3)?;
                        Ok(TestCase(Foo {
                            field0: d.decode_with(ctx)?,
                            field1: d.decode_with(ctx)?,
                        }))
                    })
                }
            }

            assert_err::<TestCase<Foo>>("array length mismatch", &to_cbor(&AsDefinite(&FIXTURE)))
        }

        #[test]
        fn incomplete_indefinite() {
            #[derive(Debug, PartialEq, Eq)]
            struct TestCase<A>(A);

            // An incomplete encoder, which skips the final break on indefinite arrays.
            impl cbor::encode::Encode<()> for TestCase<&Foo> {
                fn encode<W: cbor::encode::Write>(
                    &self,
                    e: &mut cbor::Encoder<W>,
                    _ctx: &mut (),
                ) -> Result<(), cbor::encode::Error<W::Error>> {
                    e.begin_array()?;
                    e.u64(self.0.field0)?;
                    e.u64(self.0.field1)?;
                    Ok(())
                }
            }

            let bytes = to_cbor(&TestCase(&FIXTURE));

            assert!(from_cbor::<AsDefinite<Foo>>(&bytes).is_none());
            assert!(from_cbor::<AsIndefinite<Foo>>(&bytes).is_none());
        }
    }

    mod heterogeneous_map_tests {
        use super::*;

        /// A decoder for `Foo` that interpret it as a map, and fails in case of a missing field.
        #[derive(Debug, PartialEq, Eq)]
        struct NoMissingFields<A>(A);
        impl<'d, C> cbor::decode::Decode<'d, C> for NoMissingFields<Foo> {
            fn decode(
                d: &mut cbor::Decoder<'d>,
                _ctx: &mut C,
            ) -> Result<Self, cbor::decode::Error> {
                let (field0, field1) = heterogeneous_map(
                    d,
                    (missing_field::<Foo, _>(0), missing_field::<Foo, _>(1)),
                    |d| d.u8(),
                    |d, state, field| {
                        match field {
                            0 => state.0 = decode_chunk(d, |d| d.u64()),
                            1 => state.1 = decode_chunk(d, |d| d.u64()),
                            _ => return unexpected_field::<Foo, _>(field),
                        }
                        Ok(())
                    },
                )?;

                Ok(NoMissingFields(Foo {
                    field0: field0()?,
                    field1: field1()?,
                }))
            }
        }

        /// A decoder for `Foo` that interpret it as a map, but allows fields to be missing.
        #[derive(Debug, PartialEq, Eq)]
        struct WithDefaultValues<A>(A);
        impl<'d, C> cbor::decode::Decode<'d, C> for WithDefaultValues<Foo> {
            fn decode(
                d: &mut cbor::Decoder<'d>,
                _ctx: &mut C,
            ) -> Result<Self, cbor::decode::Error> {
                let (field0, field1) = heterogeneous_map(
                    d,
                    (with_default_value(14_u64), with_default_value(42_u64)),
                    |d| d.u8(),
                    |d, state, field| {
                        match field {
                            0 => state.0 = decode_chunk(d, |d| d.u64()),
                            1 => state.1 = decode_chunk(d, |d| d.u64()),
                            _ => return unexpected_field::<Foo, _>(field),
                        }
                        Ok(())
                    },
                )?;

                Ok(WithDefaultValues(Foo {
                    field0: field0()?,
                    field1: field1()?,
                }))
            }
        }

        #[test]
        fn no_optional_fields_no_missing_fields() {
            assert_ok(
                NoMissingFields(FIXTURE),
                &to_cbor(&AsIndefinite(AsMap(&FIXTURE))),
            );

            assert_ok(
                NoMissingFields(FIXTURE),
                &to_cbor(&AsDefinite(AsMap(&FIXTURE))),
            );
        }

        #[test]
        fn out_of_order_fields() {
            #[derive(Debug, PartialEq, Eq)]
            struct TestCase<A>(A);

            // An invalid encoder, which adds an extra break in an definite map.
            impl cbor::encode::Encode<()> for TestCase<&Foo> {
                fn encode<W: cbor::encode::Write>(
                    &self,
                    e: &mut cbor::Encoder<W>,
                    _ctx: &mut (),
                ) -> Result<(), cbor::encode::Error<W::Error>> {
                    e.map(2)?;
                    e.u8(1)?;
                    e.u64(self.0.field1)?;
                    e.u8(0)?;
                    e.u64(self.0.field0)?;
                    Ok(())
                }
            }

            assert_ok(NoMissingFields(FIXTURE), &to_cbor(&TestCase(&FIXTURE)));
        }

        #[test]
        fn optional_fields_no_missing_fields() {
            assert_ok(
                WithDefaultValues(FIXTURE),
                &to_cbor(&AsIndefinite(AsMap(&FIXTURE))),
            );

            assert_ok(
                WithDefaultValues(FIXTURE),
                &to_cbor(&AsDefinite(AsMap(&FIXTURE))),
            );
        }

        #[test]
        fn one_field_missing() {
            #[derive(Debug, PartialEq, Eq)]
            struct TestCase<A>(A);

            impl cbor::encode::Encode<()> for TestCase<AsIndefinite<&Foo>> {
                fn encode<W: cbor::encode::Write>(
                    &self,
                    e: &mut cbor::Encoder<W>,
                    _ctx: &mut (),
                ) -> Result<(), cbor::encode::Error<W::Error>> {
                    e.map(1)?;
                    e.u8(0)?;
                    e.u64(self.0 .0.field0)?;
                    Ok(())
                }
            }

            impl cbor::encode::Encode<()> for TestCase<AsDefinite<&Foo>> {
                fn encode<W: cbor::encode::Write>(
                    &self,
                    e: &mut cbor::Encoder<W>,
                    _ctx: &mut (),
                ) -> Result<(), cbor::encode::Error<W::Error>> {
                    e.begin_map()?;
                    e.u8(1)?;
                    e.u64(self.0 .0.field1)?;
                    e.end()?;
                    Ok(())
                }
            }

            assert_err::<NoMissingFields<Foo>>(
                "missing <u64> at field .1",
                &to_cbor(&TestCase(AsIndefinite(&FIXTURE))),
            );

            assert_ok(
                WithDefaultValues(FIXTURE),
                &to_cbor(&TestCase(AsIndefinite(&FIXTURE))),
            );

            assert_err::<NoMissingFields<Foo>>(
                "missing <u64> at field .0",
                &to_cbor(&TestCase(AsDefinite(&FIXTURE))),
            );

            assert_ok(
                WithDefaultValues(FIXTURE),
                &to_cbor(&TestCase(AsDefinite(&FIXTURE))),
            );
        }

        #[test]
        fn rogue_break() {
            #[derive(Debug, PartialEq, Eq)]
            struct TestCase<A>(A);

            // An invalid encoder, which adds an extra break in an definite map.
            impl cbor::encode::Encode<()> for TestCase<&Foo> {
                fn encode<W: cbor::encode::Write>(
                    &self,
                    e: &mut cbor::Encoder<W>,
                    _ctx: &mut (),
                ) -> Result<(), cbor::encode::Error<W::Error>> {
                    e.map(2)?;
                    e.u8(0)?;
                    e.u64(self.0.field0)?;
                    e.end()?;
                    Ok(())
                }
            }

            assert_err::<WithDefaultValues<Foo>>(
                "unexpected type break",
                &to_cbor(&TestCase(&FIXTURE)),
            );
        }

        #[test]
        fn unexpected_field_tag() {
            #[derive(Debug, PartialEq, Eq)]
            struct TestCase<A>(A);

            // An invalid encoder, which adds an extra break in an definite map.
            impl cbor::encode::Encode<()> for TestCase<&Foo> {
                fn encode<W: cbor::encode::Write>(
                    &self,
                    e: &mut cbor::Encoder<W>,
                    _ctx: &mut (),
                ) -> Result<(), cbor::encode::Error<W::Error>> {
                    e.map(2)?;
                    e.u8(0)?;
                    e.u64(self.0.field0)?;
                    e.u8(14)?;
                    e.u64(self.0.field0)?;
                    Ok(())
                }
            }

            assert_err::<WithDefaultValues<Foo>>(
                "unexpected field .14",
                &to_cbor(&TestCase(&FIXTURE)),
            );
        }
    }
}
