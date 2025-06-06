use serde::{Serializer, ser::SerializeMap};
use serde_with::{SerializeAs, ser::SerializeAsWrap};

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
