// Custom codec module for u128 (similar to minicbor::bytes pattern)
// CBOR doesn't natively support 128-bit integers, so we encode as 16 bytes
pub mod u128_cbor_codec {
    use minicbor::{Decoder, Encoder};

    /// Encode u128 as 16 bytes in big-endian format
    /// For use with `#[cbor(with = "u128_cbor_codec")]`
    pub fn encode<C, W: minicbor::encode::Write>(
        v: &u128,
        e: &mut Encoder<W>,
        _ctx: &mut C,
    ) -> Result<(), minicbor::encode::Error<W::Error>> {
        e.bytes(&v.to_be_bytes())?;
        Ok(())
    }

    /// Decode u128 from 16 bytes in big-endian format
    /// For use with `#[cbor(with = "u128_cbor_codec")]`
    pub fn decode<'b, C>(
        d: &mut Decoder<'b>,
        _ctx: &mut C,
    ) -> Result<u128, minicbor::decode::Error> {
        let bytes = d.bytes()?;
        if bytes.len() != 16 {
            return Err(minicbor::decode::Error::message(
                "Expected 16 bytes for u128",
            ));
        }
        let mut arr = [0u8; 16];
        arr.copy_from_slice(bytes);
        Ok(u128::from_be_bytes(arr))
    }
}
