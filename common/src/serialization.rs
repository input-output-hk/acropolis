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

pub trait ToBech32WithHrp {
    fn to_bech32_with_hrp(&self, hrp: &str) -> String;
}

impl ToBech32WithHrp for Vec<u8> {
    fn to_bech32_with_hrp(&self, hrp: &str) -> String {
        let hrp: Hrp = Hrp::parse(hrp).unwrap();
        bech32::encode::<Bech32>(hrp, self)
            .unwrap_or_else(|e| format!("Cannot convert {:?} to bech32: {e}", self))
    }
}
