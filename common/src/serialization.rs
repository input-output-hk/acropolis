use anyhow::anyhow;
use bech32::{Bech32, Hrp};
use serde::{ser::SerializeMap, Serializer};
use serde_with::{ser::SerializeAsWrap, SerializeAs};

pub struct SerializeMapAs<KAs, VAs>(std::marker::PhantomData<(KAs, VAs)>);

impl<T, K, V, KAs, VAs> SerializeAs<T> for SerializeMapAs<KAs, VAs>
where
    KAs: SerializeAs<K>,
    VAs: SerializeAs<V>,
    for<'a> &'a T: IntoIterator<Item = (&'a K, &'a V)>,
{
    fn serialize_as<S>(source: &T, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map_ser = serializer.serialize_map(None)?;
        for (k, v) in source {
            map_ser.serialize_entry(
                &SerializeAsWrap::<K, KAs>::new(k),
                &SerializeAsWrap::<V, VAs>::new(v),
            )?;
        }
        map_ser.end()
    }
}

pub trait Bech32Conversion {
    fn to_bech32(&self) -> Result<String, anyhow::Error>;
    fn from_bech32(s: &str) -> Result<Self, anyhow::Error>
    where
        Self: Sized;
}

pub trait Bech32WithHrp {
    fn to_bech32_with_hrp(&self, hrp: &str) -> Result<String, anyhow::Error>;
    fn from_bech32_with_hrp(s: &str, expected_hrp: &str) -> Result<Vec<u8>, anyhow::Error>;
}

impl Bech32WithHrp for Vec<u8> {
    fn to_bech32_with_hrp(&self, hrp: &str) -> Result<String, anyhow::Error> {
        let hrp = Hrp::parse(hrp).map_err(|e| anyhow!("Bech32 HRP parse error: {e}"))?;

        bech32::encode::<Bech32>(hrp, self.as_slice())
            .map_err(|e| anyhow!("Bech32 encoding error: {e}"))
    }

    fn from_bech32_with_hrp(s: &str, expected_hrp: &str) -> Result<Self, anyhow::Error> {
        let (hrp, data) = bech32::decode(s).map_err(|e| anyhow!("Invalid Bech32 string: {e}"))?;

        if hrp != Hrp::parse(expected_hrp)? {
            return Err(anyhow!(
                "Invalid HRP, expected '{expected_hrp}', got '{hrp}'"
            ));
        }

        Ok(data.to_vec())
    }
}
