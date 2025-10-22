use crate::serialization::{Bech32Conversion, Bech32WithHrp};
use anyhow::Error;
use serde_with::{hex::Hex, serde_as};
use std::ops::Deref;

macro_rules! declare_byte_array_type {
    ($name:ident, $size:expr) => {
        /// $name
        #[serde_as]
        #[derive(
            Default, Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
        )]
        pub struct $name(#[serde_as(as = "Hex")] pub [u8; $size]);

        impl From<[u8; $size]> for $name {
            fn from(bytes: [u8; $size]) -> Self {
                Self(bytes)
            }
        }

        impl TryFrom<Vec<u8>> for $name {
            type Error = Vec<u8>;
            fn try_from(vec: Vec<u8>) -> Result<Self, Self::Error> {
                Ok($name(vec.try_into()?))
            }
        }

        impl TryFrom<&[u8]> for $name {
            type Error = std::array::TryFromSliceError;
            fn try_from(arr: &[u8]) -> Result<Self, Self::Error> {
                Ok($name(arr.try_into()?))
            }
        }

        impl AsRef<[u8]> for $name {
            fn as_ref(&self) -> &[u8] {
                &self.0
            }
        }

        impl Deref for $name {
            type Target = [u8; $size];
            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }
    };
}

macro_rules! declare_byte_array_type_with_bech32 {
    ($name:ident, $size:expr, $hrp:expr) => {
        declare_byte_array_type!($name, $size);
        impl Bech32Conversion for $name {
            fn to_bech32(&self) -> Result<String, anyhow::Error> {
                self.0.to_vec().to_bech32_with_hrp($hrp)
            }
            fn from_bech32(s: &str) -> Result<Self, anyhow::Error> {
                match Vec::<u8>::from_bech32_with_hrp(s, $hrp) {
                    Ok(v) => match Self::try_from(v) {
                        Ok(s) => Ok(s),
                        Err(_) => Err(Error::msg(format!(
                            "Bad vector input to {}",
                            stringify!($name)
                        ))),
                    },
                    Err(e) => Err(e),
                }
            }
        }
    };
}

declare_byte_array_type!(BlockHash, 32);

declare_byte_array_type!(TxHash, 32);

declare_byte_array_type_with_bech32!(VRFKey, 32, "vrf_vk");
