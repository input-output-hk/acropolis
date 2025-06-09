//! Variable length integer encoding according to CIP19

// ANBF:
// VARIABLE-LENGTH-UINT = (%b1 | UINT7 | VARIABLE-LENGTH-UINT)
//                     / (%b0 | UINT7)
//
// UINT7 = 7BIT

pub struct VarIntEncoder {
    data: Vec<u8>
}

/// Variable-length integer encoder
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
    pub fn to_vec(self) -> Vec<u8> { self.data }
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
    fn unit_serialization_test() {
        assert_eq!(serialize_uint(0), vec![0]);
        assert_eq!(serialize_uint(1), vec![1]);
        assert_eq!(serialize_uint(0x7f), vec![0x7f]);
        assert_eq!(serialize_uint(0x80), vec![0x81,0]);
        assert_eq!(serialize_uint(0x4000), vec![0x81,0x80,0]);
        assert_eq!(serialize_uint(0x400), vec![0x88,0]);

        for x in 7..63 {
            let val = 1 << x;
            let mut s = Vec::new();
            s.push(0x80 | (1 << (x % 7)));
            for _i in 1..(x / 7) { s.push(0x80); }
            s.push(0);
            assert_eq!(serialize_uint(val), s);
        }
    }
}
