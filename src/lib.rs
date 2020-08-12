use serde::{de, ser};

#[allow(clippy::needless_lifetimes)] // Not sure how to fix that.
pub fn literal<'a, T>(literal: &'a [u8]) -> impl Fn(T) -> Literal<'a> {
    move |_| Literal(literal)
}
pub struct Literal<'a>(&'a [u8]);
impl<'a> ser::Serialize for Literal<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.0.serialize(serializer)
    }
}
impl<'a, 'de> de::DeserializeSeed<'de> for Literal<'a> {
    type Value = ();
    fn deserialize<D>(mut self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let len = self.0.len();

        struct Visitor<'a, 'b>(&'a mut &'b [u8]);
        deserializer.deserialize_tuple(len, Visitor(&mut self.0))?;
        impl<'a, 'b, 'de> de::Visitor<'de> for Visitor<'a, 'b> {
            type Value = ();
            fn expecting(
                &self,
                f: &mut std::fmt::Formatter<'_>,
            ) -> std::result::Result<(), std::fmt::Error> {
                write!(f, "{} literal bytes", self.0.len())
            }

            fn visit_u8<E>(self, v: u8) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                match self.0.first().copied() {
                    Some(e) if v == e => Ok(()),
                    Some(_) => Err(de::Error::invalid_value(
                        de::Unexpected::Bytes(&[v]),
                        &format!("{:?}", self.0).as_str(),
                    )),
                    None => Err(de::Error::invalid_length(1, &"no more bytes")),
                }
            }
        }

        if self.0.is_empty() {
            Ok(())
        } else {
            Err(de::Error::invalid_length(
                len - self.0.len(),
                &format!("{} literal bytes", len).as_str(),
            ))
        }
    }
}
