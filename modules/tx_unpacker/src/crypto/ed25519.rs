//! Ed25519 and Ed25519Extended Asymmetric Keys
//!
use cryptoxide::ed25519::{self, PUBLIC_KEY_LENGTH, SIGNATURE_LENGTH};
use std::{convert::TryFrom, fmt, str::FromStr};
use thiserror::Error;

/// Ed25519 Public Key. Can be used to verify a [`Signature`]. A [`PublicKey`]
/// is associated to a [`SecretKey`]
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct PublicKey([u8; Self::SIZE]);

/// Ed25519 Signature. Is created by a [`SecretKey`] and is verified
/// with a [`PublicKey`].
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Signature([u8; Self::SIZE]);

/// Error type used when retrieving a [`PublicKey`] via the [`TryFrom`]
/// trait.
#[derive(Debug, Error)]
pub enum TryFromPublicKeyError {
    #[error("Invalid size, expecting {}", PublicKey::SIZE)]
    InvalidSize,
}

/// Error type used when retrieving a [`Signature`] via the [`TryFrom`]
/// trait.
#[derive(Debug, Error)]
pub enum TryFromSignatureError {
    #[error("Invalid size, expecting {}", Signature::SIZE)]
    InvalidSize,
}

macro_rules! impl_size_zero {
    ($Type:ty, $Size:expr) => {
        impl $Type {
            /// This is the size of the type in bytes.
            pub const SIZE: usize = $Size;

            /// create a zero object. This is not a _"valid"_ one. It is
            /// used to initialize a ready to use data structure in this module.
            #[inline]
            fn zero() -> Self {
                Self([0; Self::SIZE])
            }
        }
    };
}

impl_size_zero!(PublicKey, PUBLIC_KEY_LENGTH);
impl_size_zero!(Signature, SIGNATURE_LENGTH);

impl PublicKey {
    /// verify the cryptographic [`Signature`] against the `message` and the
    /// [`PublicKey`] `self`.
    #[inline]
    pub fn verify<T>(&self, message: T, signature: &Signature) -> bool
    where
        T: AsRef<[u8]>,
    {
        ed25519::verify(message.as_ref(), &self.0, &signature.0)
    }
}

/* Format ****************************************************************** */

impl fmt::Display for Signature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&hex::encode(self.as_ref()))
    }
}

impl fmt::Display for PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&hex::encode(self.as_ref()))
    }
}

impl fmt::Debug for Signature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Signature<Ed25519>").field(&hex::encode(self.as_ref())).finish()
    }
}

impl fmt::Debug for PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("PublicKey<Ed25519>").field(&hex::encode(self.as_ref())).finish()
    }
}

/* AsRef ******************************************************************* */

impl AsRef<[u8]> for PublicKey {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl AsRef<[u8]> for Signature {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

/* Conversion ************************************************************** */

impl<'a> From<&'a Signature> for String {
    fn from(s: &'a Signature) -> Self {
        s.to_string()
    }
}

impl From<Signature> for String {
    fn from(s: Signature) -> Self {
        s.to_string()
    }
}

impl From<[u8; Self::SIZE]> for PublicKey {
    fn from(bytes: [u8; Self::SIZE]) -> Self {
        Self(bytes)
    }
}

impl From<PublicKey> for [u8; PublicKey::SIZE] {
    fn from(pk: PublicKey) -> Self {
        pk.0
    }
}

impl From<[u8; Self::SIZE]> for Signature {
    fn from(bytes: [u8; Self::SIZE]) -> Self {
        Self(bytes)
    }
}

impl<'a> TryFrom<&'a [u8]> for PublicKey {
    type Error = TryFromPublicKeyError;
    fn try_from(value: &'a [u8]) -> Result<Self, Self::Error> {
        if value.len() != Self::SIZE {
            Err(Self::Error::InvalidSize)
        } else {
            let mut s = Self::zero();
            s.0.copy_from_slice(value);
            Ok(s)
        }
    }
}

impl<'a> TryFrom<&'a [u8]> for Signature {
    type Error = TryFromSignatureError;
    fn try_from(value: &'a [u8]) -> Result<Self, Self::Error> {
        if value.len() != Self::SIZE {
            Err(Self::Error::InvalidSize)
        } else {
            let mut s = Self::zero();
            s.0.copy_from_slice(value);
            Ok(s)
        }
    }
}

impl FromStr for PublicKey {
    type Err = hex::FromHexError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut r = Self::zero();
        hex::decode_to_slice(s, &mut r.0)?;
        Ok(r)
    }
}

impl FromStr for Signature {
    type Err = hex::FromHexError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut r = Self::zero();
        hex::decode_to_slice(s, &mut r.0)?;
        Ok(r)
    }
}

impl<'a> TryFrom<&'a str> for Signature {
    type Error = <Self as FromStr>::Err;
    fn try_from(s: &'a str) -> Result<Self, Self::Error> {
        s.parse()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickcheck::{Arbitrary, Gen, TestResult};
    use quickcheck_macros::quickcheck;

    impl Arbitrary for PublicKey {
        fn arbitrary(g: &mut Gen) -> Self {
            let mut s = Self::zero();
            s.0.iter_mut().for_each(|byte| {
                *byte = u8::arbitrary(g);
            });
            s
        }
    }

    impl Arbitrary for Signature {
        fn arbitrary(g: &mut Gen) -> Self {
            let mut s = Self::zero();
            s.0.iter_mut().for_each(|byte| {
                *byte = u8::arbitrary(g);
            });
            s
        }
    }

    #[quickcheck]
    fn verify_random_signature_does_not_work(
        public_key: PublicKey,
        signature: Signature,
        message: Vec<u8>,
    ) -> bool {
        // NOTE: this test may fail but it is impossible to see this happening in normal
        // condition. We are generating 32 random bytes of public key and
        // 64 random bytes of signature with an randomly generated message
        // of a random number of bytes in. If the message were empty, the
        // probability to have a signature that matches the verify key

        // would still be 1 out of 2^96.
        //
        // if this test fails and it is not a bug, go buy a lottery ticket.
        !public_key.verify(message, &signature)
    }

    #[quickcheck]
    fn public_key_try_from_correct_size(public_key: PublicKey) -> TestResult {
        match PublicKey::try_from(public_key.as_ref()) {
            Ok(_) => TestResult::passed(),
            Err(TryFromPublicKeyError::InvalidSize) => {
                TestResult::error("was expecting the test to pass")
            }
        }
    }

    #[quickcheck]
    fn public_key_try_from_incorrect_size(bytes: Vec<u8>) -> TestResult {
        if bytes.len() == PublicKey::SIZE {
            return TestResult::discard();
        }
        match PublicKey::try_from(bytes.as_slice()) {
            Ok(_) => TestResult::error(
                "Expecting to fail with invalid size instead of having a valid value",
            ),
            Err(TryFromPublicKeyError::InvalidSize) => TestResult::passed(),
        }
    }

    #[quickcheck]
    fn signature_try_from_correct_size(signature: Signature) -> TestResult {
        match Signature::try_from(signature.as_ref()) {
            Ok(_) => TestResult::passed(),
            Err(TryFromSignatureError::InvalidSize) => {
                TestResult::error("was expecting the test to pass")
            }
        }
    }

    #[quickcheck]
    fn signature_try_from_incorrect_size(bytes: Vec<u8>) -> TestResult {
        if bytes.len() == Signature::SIZE {
            return TestResult::discard();
        }
        match Signature::try_from(bytes.as_slice()) {
            Ok(_) => TestResult::error(
                "Expecting to fail with invalid size instead of having a valid value",
            ),
            Err(TryFromSignatureError::InvalidSize) => TestResult::passed(),
        }
    }

    #[quickcheck]
    fn public_key_from_str(public_key: PublicKey) -> TestResult {
        let s = public_key.to_string();

        match s.parse::<PublicKey>() {
            Ok(decoded) => {
                if decoded == public_key {
                    TestResult::passed()
                } else {
                    TestResult::error("the decoded key is not equal")
                }
            }
            Err(error) => TestResult::error(error.to_string()),
        }
    }

    #[quickcheck]
    fn signature_from_str(signature: Signature) -> TestResult {
        let s = signature.to_string();

        match s.parse::<Signature>() {
            Ok(decoded) => {
                if decoded == signature {
                    TestResult::passed()
                } else {
                    TestResult::error("the decoded signature is not equal")
                }
            }
            Err(error) => TestResult::error(error.to_string()),
        }
    }
}
