pub(crate) fn de_map_access_require_entry<'de, T, A>(
    map: &mut A,
    key: &'static str,
) -> Result<T, A::Error>
where
    T: Deserialize<'de>,
    A: de::MapAccess<'de>,
{
    de_map_access_require_entry_seed(map, key, PhantomData::<T>)
}

pub(crate) fn de_map_access_require_entry_seed<'de, S, A>(
    map: &mut A,
    key: &'static str,
    seed: S,
) -> Result<S::Value, A::Error>
where
    S: DeserializeSeed<'de>,
    A: de::MapAccess<'de>,
{
    map.next_key_seed(LiteralStr(key))?
        .ok_or_else(|| de::Error::missing_field(key))?;
    map.next_value_seed(seed)
}

mod literal_str {
    pub(crate) struct LiteralStr<'s>(pub &'s str);

    impl<'de> DeserializeSeed<'de> for LiteralStr<'_> {
        type Value = ();

        fn deserialize<D: Deserializer<'de>>(
            self,
            deserializer: D,
        ) -> Result<Self::Value, D::Error> {
            deserializer.deserialize_str(self)
        }
    }

    impl<'de> de::Visitor<'de> for LiteralStr<'_> {
        type Value = ();
        fn expecting(&self, f: &mut Formatter<'_>) -> fmt::Result {
            write!(f, "the string `{}`", self.0)
        }
        fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
            if v != self.0 {
                return Err(de::Error::invalid_value(de::Unexpected::Str(v), &self));
            }
            Ok(())
        }
    }

    use serde::de;
    use serde::de::DeserializeSeed;
    use serde::de::Deserializer;
    use std::fmt;
    use std::fmt::Formatter;
}
pub(crate) use literal_str::LiteralStr;

use serde::de;
use serde::de::DeserializeSeed;
use serde::Deserialize;
use std::marker::PhantomData;
