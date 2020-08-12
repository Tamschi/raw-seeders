use std::marker::PhantomData;
use {
    serde::{
        de::{self, DeserializeSeed as _},
        ser,
    },
    serde_seeded::Seeder,
};

pub fn literal<'a>(literal: &'a [u8]) -> impl 'a + Seeder<()> {
    struct Literal<'a>(&'a [u8]);
    impl<'a: 's, 's> Seeder<'s, ()> for Literal<'a> {
        type Seed = Self;
        type Seeded = Self;
        fn seed(self) -> Self::Seed {
            self
        }
        fn seeded(self, _: &()) -> Self::Seeded {
            self
        }
    }
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

    Literal(literal)
}

pub struct LittleEndian;
impl<'s, T: 's + ByteOrdered> Seeder<'s, T> for LittleEndian {
    type Seed = LittleEndianSeed<T>;
    type Seeded = LittleEndianSeeded<'s, T>;
    fn seed(self) -> Self::Seed {
        LittleEndianSeed(PhantomData)
    }
    fn seeded(self, value: &'s T) -> Self::Seeded {
        LittleEndianSeeded(value)
    }
}

pub struct LittleEndianSeed<T>(PhantomData<T>);
impl<'de, T: ByteOrdered> de::DeserializeSeed<'de> for LittleEndianSeed<T> {
    type Value = T;
    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        T::deserialize_le(deserializer)
    }
}

pub struct LittleEndianSeeded<'a, T>(&'a T);
impl<'a, T: ByteOrdered> ser::Serialize for LittleEndianSeeded<'a, T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.0.serialize_le(serializer)
    }
}

pub trait ByteOrdered: Sized {
    fn deserialize_le<'de, D: de::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error>;
    fn serialize_le<S: ser::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error>;
}

impl ByteOrdered for u32 {
    fn deserialize_le<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Self::from_le_bytes(PhantomData.deserialize(deserializer)?))
    }
    fn serialize_le<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bytes(&self.to_le_bytes())
    }
}
