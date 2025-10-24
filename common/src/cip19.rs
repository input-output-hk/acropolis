//! Variable length integer encoding/decoding according to CIP19
use anyhow::{anyhow, Result};

// ANBF:
// VARIABLE-LENGTH-UINT = (%b1 | UINT7 | VARIABLE-LENGTH-UINT)
//                     / (%b0 | UINT7)
//
// UINT7 = 7BIT

pub struct VarIntEncoder {
    data: Vec<u8>,
}

/// Variable-length integer encoder
impl Default for VarIntEncoder {
    fn default() -> Self {
        Self::new()
    }
}

impl VarIntEncoder {
    /// Construct
    pub fn new() -> Self {
        Self { data: Vec::new() }
    }

    /// Push an integer
    pub fn push(&mut self, num: u64) {
        let mut len = 7;
        while (len != 70) && ((num >> len) != 0) {
            len += 7;
        }

        while len > 7 {
            len -= 7;
            self.data.push((num >> len) as u8 | 0x80);
        }
        self.data.push((num & 0x7f) as u8);
    }

    /// Get the resulting vector
    pub fn to_vec(self) -> Vec<u8> {
        self.data
    }
}

/// Variable length integer decoder
pub struct VarIntDecoder<'a> {
    data: &'a [u8],
    position: usize,
}

impl<'a> VarIntDecoder<'a> {
    /// Create a new decoder from a byte slice
    pub fn new(data: &'a [u8]) -> Self {
        VarIntDecoder { data, position: 0 }
    }

    /// Read the next CIP-19 varint (7-bit) from the stream
    pub fn read(&mut self) -> Result<u64> {
        let mut value: u64 = 0;

        while self.position < self.data.len() {
            let byte = self.data[self.position];
            self.position += 1;

            value = (value << 7) | (byte & 0x7F) as u64;

            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }

        Err(anyhow!("Variable integer ran out of data"))
    }

    /// Returns the current byte position (for diagnostics)
    pub fn position(&self) -> usize {
        self.position
    }

    /// Returns true if all input has been consumed
    pub fn is_finished(&self) -> bool {
        self.position >= self.data.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn serialize_uint(arg: u64) -> Vec<u8> {
        let mut e = VarIntEncoder::new();
        e.push(arg);
        e.to_vec()
    }

    #[test]
    fn uint_serialization_test() {
        assert_eq!(serialize_uint(0), vec![0]);
        assert_eq!(serialize_uint(1), vec![1]);
        assert_eq!(serialize_uint(0x7f), vec![0x7f]);
        assert_eq!(serialize_uint(0x80), vec![0x81, 0]);
        assert_eq!(serialize_uint(0x4000), vec![0x81, 0x80, 0]);
        assert_eq!(serialize_uint(0x400), vec![0x88, 0]);

        for x in 7..63 {
            let val = 1 << x;
            let mut s = Vec::new();
            s.push(0x80 | (1 << (x % 7)));
            for _i in 1..(x / 7) {
                s.push(0x80);
            }
            s.push(0);
            assert_eq!(serialize_uint(val), s);
        }
    }

    #[test]
    fn uint_deserialization_test() {
        let data: Vec<u8> = vec![0, 1, 0x7f, 0x81, 0, 0x81, 0x80, 0, 0x88, 0];

        let mut decoder = VarIntDecoder::new(&data);
        assert_eq!(decoder.read().unwrap(), 0);
        assert_eq!(decoder.read().unwrap(), 1);
        assert_eq!(decoder.read().unwrap(), 0x7f);
        assert_eq!(decoder.read().unwrap(), 0x80);
        assert_eq!(decoder.read().unwrap(), 0x4000);
        assert_eq!(decoder.read().unwrap(), 0x400);
        assert!(decoder.is_finished());
    }
}
