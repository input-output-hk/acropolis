use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{fmt, ops::Deref, str::FromStr};

/// Data that is a cryptographic hash of `BYTES` long.
///
/// This is a generic wrapper around a fixed-size byte array that provides:
/// - Hexadecimal serialization/deserialization
/// - CBOR encoding/decoding via minicbor
/// - Type-safe conversions from various byte representations
/// - Display and debug formatting
///
/// # Common Hash Sizes in Cardano
///
/// - **32 bytes**: Block hashes, transaction hashes
/// - **28 bytes**: Script hashes, address key hashes
///
/// # Examples
///
/// ```ignore
/// use your_crate::Hash;
///
/// // Parse from hex string
/// let hash: Hash<32> = "0d8d00cdd4657ac84d82f0a56067634a7adfdf43da41cb534bcaa45060973d21"
///     .parse()
///     .unwrap();
///
/// // Create from byte array
/// let bytes = [0u8; 28];
/// let hash = Hash::new(bytes);
///
/// // Convert to hex string
/// let hex_string = hash.to_string();
/// ```
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Hash<const BYTES: usize>([u8; BYTES]);

impl<const BYTES: usize> Default for Hash<BYTES> {
    fn default() -> Self {
        Self::new([0u8; BYTES])
    }
}

// Implement Serialize/Deserialize manually since generic const arrays don't auto-derive
impl<const BYTES: usize> Serialize for Hash<BYTES> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&hex::encode(self.0))
    }
}

impl<'de, const BYTES: usize> Deserialize<'de> for Hash<BYTES> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s: String = Deserialize::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

impl<const BYTES: usize> Hash<BYTES> {
    /// Creates a new hash from a byte array.
    ///
    /// This is a const function, allowing hashes to be created at compile time.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use your_crate::Hash;
    ///
    /// const MY_HASH: Hash<32> = Hash::new([0u8; 32]);
    /// ```
    #[inline]
    pub const fn new(bytes: [u8; BYTES]) -> Self {
        Self(bytes)
    }

    /// Converts the hash to a `Vec<u8>`.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use your_crate::Hash;
    ///
    /// let hash = Hash::new([1u8; 28]);
    /// let vec = hash.to_vec();
    /// assert_eq!(vec.len(), 28);
    /// ```
    #[inline]
    pub fn to_vec(&self) -> Vec<u8> {
        self.0.to_vec()
    }

    /// Consumes the hash and returns the inner byte array.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use your_crate::Hash;
    ///
    /// let hash = Hash::new([1u8; 28]);
    /// let bytes: [u8; 28] = hash.into_inner();
    /// ```
    #[inline]
    pub fn into_inner(self) -> [u8; BYTES] {
        self.0
    }
}

impl<const BYTES: usize> From<[u8; BYTES]> for Hash<BYTES> {
    #[inline]
    fn from(bytes: [u8; BYTES]) -> Self {
        Self::new(bytes)
    }
}

impl<const BYTES: usize> TryFrom<&[u8]> for Hash<BYTES> {
    type Error = std::array::TryFromSliceError;

    /// Attempts to create a hash from a byte slice.
    ///
    /// # Errors
    ///
    /// Returns an error if the slice length does not match `BYTES`.
    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        let hash: [u8; BYTES] = value.try_into()?;
        Ok(Self::new(hash))
    }
}

impl<const BYTES: usize> TryFrom<Vec<u8>> for Hash<BYTES> {
    type Error = Vec<u8>;

    /// Attempts to create a hash from a `Vec<u8>`.
    ///
    /// # Errors
    ///
    /// Returns the original vector if its length does not match `BYTES`.
    fn try_from(value: Vec<u8>) -> Result<Self, Self::Error> {
        let hash: [u8; BYTES] = value.try_into()?;
        Ok(Self::new(hash))
    }
}

impl<const BYTES: usize> From<Hash<BYTES>> for Vec<u8> {
    fn from(hash: Hash<BYTES>) -> Self {
        hash.0.to_vec()
    }
}

impl<const BYTES: usize> From<Hash<BYTES>> for [u8; BYTES] {
    fn from(hash: Hash<BYTES>) -> Self {
        hash.0
    }
}

impl<const BYTES: usize> AsRef<[u8]> for Hash<BYTES> {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl<const BYTES: usize> Deref for Hash<BYTES> {
    type Target = [u8; BYTES];

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<const BYTES: usize> PartialEq<[u8]> for Hash<BYTES> {
    fn eq(&self, other: &[u8]) -> bool {
        self.0.eq(other)
    }
}

impl<const BYTES: usize> fmt::Debug for Hash<BYTES> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple(&format!("Hash<{BYTES}>")).field(&hex::encode(self)).finish()
    }
}

impl<const BYTES: usize> fmt::Display for Hash<BYTES> {
    /// Formats the hash as a lowercase hexadecimal string.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&hex::encode(self))
    }
}

impl<const BYTES: usize> FromStr for Hash<BYTES> {
    type Err = hex::FromHexError;

    /// Parses a hash from a hexadecimal string.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The string is not valid hexadecimal
    /// - The decoded bytes do not match the expected length `BYTES`
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use your_crate::Hash;
    ///
    /// let hash: Hash<28> = "276fd18711931e2c0e21430192dbeac0e458093cd9d1fcd7210f64b3"
    ///     .parse()
    ///     .unwrap();
    /// ```
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut bytes = [0; BYTES];
        hex::decode_to_slice(s, &mut bytes)?;
        Ok(Self::new(bytes))
    }
}

impl<const BYTES: usize> hex::FromHex for Hash<BYTES> {
    type Error = hex::FromHexError;

    /// Decodes a hash from hexadecimal bytes.
    ///
    /// # Errors
    ///
    /// Returns an error if the decoded length does not match `BYTES`.
    fn from_hex<T: AsRef<[u8]>>(hex: T) -> Result<Self, Self::Error> {
        match Self::try_from(Vec::<u8>::from_hex(hex)?) {
            Ok(h) => Ok(h),
            Err(_) => Err(hex::FromHexError::InvalidStringLength),
        }
    }
}

impl<C, const BYTES: usize> minicbor::Encode<C> for Hash<BYTES> {
    fn encode<W: minicbor::encode::Write>(
        &self,
        e: &mut minicbor::Encoder<W>,
        _ctx: &mut C,
    ) -> Result<(), minicbor::encode::Error<W::Error>> {
        e.bytes(&self.0)?.ok()
    }
}

impl<'a, C, const BYTES: usize> minicbor::Decode<'a, C> for Hash<BYTES> {
    fn decode(
        d: &mut minicbor::Decoder<'a>,
        _ctx: &mut C,
    ) -> Result<Self, minicbor::decode::Error> {
        let bytes = d.bytes()?;
        if bytes.len() == BYTES {
            let mut hash = [0; BYTES];
            hash.copy_from_slice(bytes);
            Ok(Self::new(hash))
        } else {
            // TODO: minicbor does not allow for expecting a specific size byte array
            //       (in fact cbor is not good at it at all anyway)
            Err(minicbor::decode::Error::message("Invalid hash size"))
        }
    }
}

// Type aliases for common hash sizes in Cardano
/// A 28-byte hash used for scripts in Cardano addresses.
pub type ScriptHash = Hash<28>;

/// A 28-byte hash of an address key in Cardano.
pub type AddrKeyhash = Hash<28>;

/// Declares a type alias for a hash with optional documentation.
///
/// # Examples
///
/// ```ignore
/// declare_hash_type!(BlockHash, 32);
/// declare_hash_type!(TxHash, 32);
/// ```
#[macro_export]
macro_rules! declare_hash_type {
    ($name:ident, $size:expr) => {
        #[doc = concat!(stringify!($name), " - a ", stringify!($size), "-byte hash.")]
        pub type $name = Hash<$size>;
    };
    ($(#[$meta:meta])* $name:ident, $size:expr) => {
        $(#[$meta])*
        pub type $name = Hash<$size>;
    };
}

/// Declares a type alias for a hash with Bech32 encoding support.
///
/// This macro creates a type alias and implements the `Bech32Conversion` trait
/// for encoding/decoding the hash using a specified human-readable part (HRP).
///
/// **WARNING**: You can only use this macro once per hash size, as it implements
/// a trait on `Hash<SIZE>`. If you need multiple distinct types with different
/// Bech32 HRPs for the same hash size, use `declare_hash_newtype_with_bech32!` instead.
///
/// # Examples
///
/// ```ignore
/// declare_hash_type_with_bech32!(VRFKey, 32, "vrf_vk");
///
/// let key: VRFKey = // ... get key
/// let bech32_string = key.to_bech32().unwrap();
/// let decoded = VRFKey::from_bech32(&bech32_string).unwrap();
/// ```
#[macro_export]
macro_rules! declare_hash_type_with_bech32 {
    ($name:ident, $size:expr, $hrp:expr) => {
        declare_hash_type!($name, $size);

        impl crate::serialization::Bech32Conversion for $name {
            fn to_bech32(&self) -> Result<String, anyhow::Error> {
                use crate::serialization::Bech32WithHrp;
                self.to_vec().to_bech32_with_hrp($hrp)
            }

            fn from_bech32(s: &str) -> Result<Self, anyhow::Error> {
                use crate::serialization::Bech32WithHrp;
                let v = Vec::<u8>::from_bech32_with_hrp(s, $hrp)?;
                Self::try_from(v).map_err(|_| {
                    anyhow::Error::msg(format!(
                        "Bad vector input to {}",
                        stringify!($name)
                    ))
                })
            }
        }
    };
    ($(#[$meta:meta])* $name:ident, $size:expr, $hrp:expr) => {
        declare_hash_type!($(#[$meta])* $name, $size);

        impl crate::serialization::Bech32Conversion for $name {
            fn to_bech32(&self) -> Result<String, anyhow::Error> {
                use crate::serialization::Bech32WithHrp;
                self.to_vec().to_bech32_with_hrp($hrp)
            }

            fn from_bech32(s: &str) -> Result<Self, anyhow::Error> {
                use crate::serialization::Bech32WithHrp;
                let v = Vec::<u8>::from_bech32_with_hrp(s, $hrp)?;
                Self::try_from(v).map_err(|_| {
                    anyhow::Error::msg(format!(
                        "Bad vector input to {}",
                        stringify!($name)
                    ))
                })
            }
        }
    };
}

/// Declares a newtype wrapper around Hash with Bech32 encoding support.
///
/// Unlike `declare_hash_type_with_bech32!`, this creates a distinct type (not an alias),
/// allowing you to have multiple types of the same hash size with different Bech32 HRPs.
///
/// # Examples
///
/// ```ignore
/// // Both are 28 bytes but have different Bech32 encodings
/// declare_hash_newtype_with_bech32!(PoolId, 28, "pool");
/// declare_hash_newtype_with_bech32!(DrepId, 28, "drep");
/// ```
#[macro_export]
macro_rules! declare_hash_newtype_with_bech32 {
    ($name:ident, $size:expr, $hrp:expr) => {
        #[doc = concat!(stringify!($name), " - a ", stringify!($size), "-byte hash.")]
        #[derive(
            Default,
            Debug,
            Clone,
            Copy,
            PartialEq,
            Eq,
            PartialOrd,
            Ord,
            Hash,
            serde::Serialize,
            serde::Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(Hash<$size>);

        impl $name {
            pub const fn new(hash: Hash<$size>) -> Self {
                Self(hash)
            }

            pub fn to_vec(&self) -> Vec<u8> {
                self.0.to_vec()
            }

            pub fn into_inner(self) -> Hash<$size> {
                self.0
            }
        }

        impl From<Hash<$size>> for $name {
            fn from(hash: Hash<$size>) -> Self {
                Self(hash)
            }
        }

        impl From<[u8; $size]> for $name {
            fn from(bytes: [u8; $size]) -> Self {
                Self(Hash::new(bytes))
            }
        }

        impl TryFrom<Vec<u8>> for $name {
            type Error = Vec<u8>;
            fn try_from(vec: Vec<u8>) -> Result<Self, Self::Error> {
                Ok(Self(Hash::try_from(vec)?))
            }
        }

        impl TryFrom<&[u8]> for $name {
            type Error = std::array::TryFromSliceError;
            fn try_from(arr: &[u8]) -> Result<Self, Self::Error> {
                Ok(Self(Hash::try_from(arr)?))
            }
        }

        impl AsRef<[u8]> for $name {
            fn as_ref(&self) -> &[u8] {
                self.0.as_ref()
            }
        }

        impl std::ops::Deref for $name {
            type Target = Hash<$size>;
            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }

        impl std::str::FromStr for $name {
            type Err = hex::FromHexError;
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Ok(Self(s.parse()?))
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.0.fmt(f)
            }
        }

        impl<C> minicbor::Encode<C> for $name {
            fn encode<W: minicbor::encode::Write>(
                &self,
                e: &mut minicbor::Encoder<W>,
                ctx: &mut C,
            ) -> Result<(), minicbor::encode::Error<W::Error>> {
                self.0.encode(e, ctx)
            }
        }

        impl<'a, C> minicbor::Decode<'a, C> for $name {
            fn decode(
                d: &mut minicbor::Decoder<'a>,
                ctx: &mut C,
            ) -> Result<Self, minicbor::decode::Error> {
                Ok(Self(Hash::decode(d, ctx)?))
            }
        }

        impl crate::serialization::Bech32Conversion for $name {
            fn to_bech32(&self) -> Result<String, anyhow::Error> {
                use crate::serialization::Bech32WithHrp;
                self.0.to_vec().to_bech32_with_hrp($hrp)
            }

            fn from_bech32(s: &str) -> Result<Self, anyhow::Error> {
                use crate::serialization::Bech32WithHrp;
                let v = Vec::<u8>::from_bech32_with_hrp(s, $hrp)?;
                Self::try_from(v).map_err(|_| {
                    anyhow::Error::msg(format!("Bad vector input to {}", stringify!($name)))
                })
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_str() {
        let _digest: Hash<28> =
            "276fd18711931e2c0e21430192dbeac0e458093cd9d1fcd7210f64b3".parse().unwrap();

        let _digest: Hash<32> =
            "0d8d00cdd4657ac84d82f0a56067634a7adfdf43da41cb534bcaa45060973d21".parse().unwrap();
    }

    #[test]
    #[should_panic]
    fn from_str_fail_1() {
        let _digest: Hash<28> = "27".parse().unwrap();
    }

    #[test]
    #[should_panic]
    fn from_str_fail_2() {
        let _digest: Hash<32> = "0d8d00cdd465".parse().unwrap();
    }

    #[test]
    fn try_from_slice() {
        let bytes = vec![0u8; 28];
        let hash: Hash<28> = bytes.as_slice().try_into().unwrap();
        assert_eq!(hash.as_ref(), bytes.as_slice());
    }

    #[test]
    fn try_from_vec() {
        let bytes = vec![0u8; 28];
        let hash: Hash<28> = bytes.clone().try_into().unwrap();
        assert_eq!(hash.as_ref(), bytes.as_slice());
    }

    #[test]
    fn into_vec() {
        let bytes = [0u8; 28];
        let hash = Hash::new(bytes);
        let vec: Vec<u8> = hash.into();
        assert_eq!(vec, bytes.to_vec());
    }

    #[test]
    #[should_panic]
    fn try_from_wrong_size() {
        let bytes = vec![0u8; 27]; // Wrong size
        let _hash: Hash<28> = bytes.as_slice().try_into().unwrap();
    }
}
