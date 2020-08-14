use std::marker::PhantomData;
use {
    serde::{
        de::{self, DeserializeSeed as _},
        ser,
    },
    serde_seeded::Seeder,
    wyz::Pipe as _,
};

#[derive(Debug, Clone, Copy, PartialEq, Ord, PartialOrd, Eq)]
pub struct Literal<'a>(pub &'a [u8]);
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
    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let len = self.0.len();

        struct Visitor<'a>(&'a [u8]);
        impl<'a, 'de> de::Visitor<'de> for Visitor<'a> {
            type Value = ();
            fn expecting(
                &self,
                f: &mut std::fmt::Formatter<'_>,
            ) -> std::result::Result<(), std::fmt::Error> {
                write!(f, "{} literal bytes", self.0.len())
            }

            fn visit_seq<A: de::SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                for (i, expected) in self.0.iter().copied().enumerate() {
                    let received: u8 = seq
                        .next_element()?
                        .ok_or_else(|| de::Error::invalid_length(i, &self))?;
                    if expected != received {
                        return Err(de::Error::invalid_value(
                            de::Unexpected::Unsigned(received as u64),
                            &format!("{} in {:?}", expected, self.0).as_str(),
                        ));
                    }
                }
                Ok(())
            }
        }

        deserializer.deserialize_tuple(len, Visitor(self.0))
    }
}

#[derive(Debug, Copy, Clone, Default)]
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

#[derive(Debug, Copy, Clone, Default)]
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

#[derive(Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
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

#[derive(Debug, Copy, Clone, Default)]
pub struct IEEE754<ReprSeeder>(pub ReprSeeder);
impl<'s, T: 's + IEEE754able, ReprSeeder: Clone + for<'repr> Seeder<'repr, T::Repr> + 's>
    Seeder<'s, T> for IEEE754<ReprSeeder>
{
    type Seed = IEEE754Seed<T, ReprSeeder>;
    type Seeded = IEEE754Seeded<'s, T, ReprSeeder>;
    fn seed(self) -> Self::Seed {
        IEEE754Seed(self.0, PhantomData)
    }
    fn seeded(self, value: &'s T) -> Self::Seeded {
        IEEE754Seeded(value, self.0)
    }
}

#[derive(Debug, Copy, Clone, Default)]
pub struct IEEE754Seed<T, ReprSeeder>(ReprSeeder, PhantomData<T>);
impl<'de, T: IEEE754able, ReprSeeder: for<'d> Seeder<'d, T::Repr>> de::DeserializeSeed<'de>
    for IEEE754Seed<T, ReprSeeder>
{
    type Value = T;
    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        self.0.seed().deserialize(deserializer).map(T::from)
    }
}

#[derive(Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct IEEE754Seeded<'a, T, ReprSeeder>(&'a T, ReprSeeder);
impl<'a, T: IEEE754able, ReprSeeder: Clone + for<'b> Seeder<'b, T::Repr>> ser::Serialize
    for IEEE754Seeded<'a, T, ReprSeeder>
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.0
            .to()
            .pipe(|repr| self.1.clone().seeded(&repr).serialize(serializer))
    }
}

pub trait IEEE754able {
    type Repr;
    fn from(repr: Self::Repr) -> Self;
    fn to(&self) -> Self::Repr;
}

impl IEEE754able for f32 {
    type Repr = u32;
    fn from(repr: Self::Repr) -> Self {
        f32::from_bits(repr)
    }
    fn to(&self) -> Self::Repr {
        self.to_bits()
    }
}

impl IEEE754able for f64 {
    type Repr = u64;
    fn from(repr: Self::Repr) -> Self {
        f64::from_bits(repr)
    }
    fn to(&self) -> Self::Repr {
        self.to_bits()
    }
}
