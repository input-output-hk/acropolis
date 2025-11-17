// Custom codec module for u128 using CBOR bignum encoding
pub mod u128_cbor_codec {
    use minicbor::{Decoder, Encoder};

    /// Encode u128 as CBOR Tag 2 (positive bignum)
    /// For use with `#[cbor(with = "u128_cbor_codec")]`
    pub fn encode<C, W: minicbor::encode::Write>(
        v: &u128,
        e: &mut Encoder<W>,
        _ctx: &mut C,
    ) -> Result<(), minicbor::encode::Error<W::Error>> {
        // Tag 2 = positive bignum
        e.tag(minicbor::data::Tag::new(2))?;

        // Optimize: only encode non-zero leading bytes
        let bytes = v.to_be_bytes();
        let first_nonzero = bytes.iter().position(|&b| b != 0).unwrap_or(15);
        e.bytes(&bytes[first_nonzero..])?;
        Ok(())
    }

    /// Decode u128 from CBOR Tag 2 (positive bignum)
    pub fn decode<'b, C>(
        d: &mut Decoder<'b>,
        _ctx: &mut C,
    ) -> Result<u128, minicbor::decode::Error> {
        // Expect Tag 2
        let tag = d.tag()?;
        if tag != minicbor::data::Tag::new(2) {
            return Err(minicbor::decode::Error::message(
                "Expected CBOR Tag 2 (positive bignum) for u128",
            ));
        }

        let bytes = d.bytes()?;
        if bytes.len() > 16 {
            return Err(minicbor::decode::Error::message(
                "Bignum too large for u128 (max 16 bytes)",
            ));
        }

        // Pad with leading zeros to make 16 bytes (big-endian)
        let mut arr = [0u8; 16];
        arr[16 - bytes.len()..].copy_from_slice(bytes);
        Ok(u128::from_be_bytes(arr))
    }
}

#[cfg(test)]
mod tests {
    use super::u128_cbor_codec;
    use minicbor::{Decode, Encode};

    #[derive(Debug, PartialEq, Encode, Decode)]
    struct TestStruct {
        #[cbor(n(0), with = "u128_cbor_codec")]
        value: u128,
    }

    #[test]
    fn test_u128_zero() {
        let original = TestStruct { value: 0 };
        let encoded = minicbor::to_vec(&original).unwrap();
        let decoded: TestStruct = minicbor::decode(&encoded).unwrap();
        assert_eq!(original, decoded);
    }

    #[test]
    fn test_u128_max() {
        let original = TestStruct { value: u128::MAX };
        let encoded = minicbor::to_vec(&original).unwrap();
        let decoded: TestStruct = minicbor::decode(&encoded).unwrap();
        assert_eq!(original, decoded);
    }

    #[test]
    fn test_u128_boundary_values() {
        let test_values = [
            0u128,
            1,
            127,                    // Max 1-byte value
            u64::MAX as u128,       // 18446744073709551615
            (u64::MAX as u128) + 1, // First value needing >64 bits
            u128::MAX - 1,          // Near max
            u128::MAX,              // Maximum u128 value
        ];

        for &val in &test_values {
            let original = TestStruct { value: val };
            let encoded = minicbor::to_vec(&original).unwrap();
            let decoded: TestStruct = minicbor::decode(&encoded).unwrap();
            assert_eq!(original, decoded, "Failed for value {}", val);
        }
    }
}
