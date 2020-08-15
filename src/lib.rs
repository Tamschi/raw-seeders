use iter::FromIterator;
use {
    arrayvec::{Array, ArrayVec},
    serde::{
        de::{self, DeserializeSeed as _},
        ser::{self, SerializeSeq as _, SerializeTuple as _},
    },
    serde_seeded::{DeSeeder, SerSeeder},
    std::{iter, marker::PhantomData},
    wyz::Pipe as _,
};

/// Stores a binary slice instead of a `()`.  
/// (Parameters: A `&[u8]` specifying the data to store or check against.)
#[derive(Debug, Clone, Copy, PartialEq, Ord, PartialOrd, Eq)]
pub struct Literal<'a>(pub &'a [u8]);
impl<'a> DeSeeder<()> for Literal<'a> {
    type Seed = Self;
    fn seed(self) -> Self::Seed {
        self
    }
}
impl<'seeder: 'value, 'value> SerSeeder<'seeder, 'value, ()> for Literal<'seeder> {
    type Seeded = Self;
    fn seeded(&'seeder self, _: &()) -> Self::Seeded {
        *self
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

/// Little-endian (least significant byte first) storage for integers.
#[derive(Debug, Copy, Clone, Default)]
pub struct LittleEndian;
impl<T: ByteOrdered> DeSeeder<T> for LittleEndian {
    type Seed = LittleEndianSeed<T>;
    fn seed(self) -> Self::Seed {
        LittleEndianSeed(PhantomData)
    }
}
impl<'seeder: 'value, 'value, T: 'value + ByteOrdered> SerSeeder<'seeder, 'value, T>
    for LittleEndian
{
    type Seeded = LittleEndianSeeded<'value, T>;
    fn seeded(&'seeder self, value: &'value T) -> Self::Seeded {
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

#[doc(hidden)]
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

/// See [`BigEndian`] and [`LittleEndian`].
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

/// IEEE 754-storage for floating point numbers.  
/// (Parameters: unsigned integer [`Seeder`])
#[derive(Debug, Copy, Clone, Default)]
pub struct IEEE754<ReprSeeder>(pub ReprSeeder);
impl<T: IEEE754ableDe, ReprSeeder: DeSeeder<T::Repr>> DeSeeder<T> for IEEE754<ReprSeeder> {
    type Seed = IEEE754Seed<T, ReprSeeder>;
    fn seed(self) -> Self::Seed {
        IEEE754Seed(self.0, PhantomData)
    }
}
impl<
        'seeder: 'value,
        'value,
        T: 'value + IEEE754ableSer,
        ReprSeeder: for<'repr> SerSeeder<'value, 'repr, T::Repr> + 'seeder,
    > SerSeeder<'seeder, 'value, T> for IEEE754<ReprSeeder>
{
    type Seeded = IEEE754Seeded<'value, T, ReprSeeder>;
    fn seeded(&'seeder self, value: &'value T) -> Self::Seeded {
        IEEE754Seeded(value, &self.0)
    }
}

#[doc(hidden)]
#[derive(Debug, Copy, Clone, Default)]
pub struct IEEE754Seed<T, ReprSeeder>(ReprSeeder, PhantomData<T>);
impl<'de, T: IEEE754ableDe, ReprSeeder: DeSeeder<T::Repr>> de::DeserializeSeed<'de>
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

#[doc(hidden)]
#[derive(Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct IEEE754Seeded<'a, T, ReprSeeder>(&'a T, &'a ReprSeeder);
impl<'seeder, T: IEEE754ableSer, ReprSeeder: for<'repr> SerSeeder<'seeder, 'repr, T::Repr>>
    ser::Serialize for IEEE754Seeded<'seeder, T, ReprSeeder>
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.0
            .to()
            .pipe(|repr| self.1.seeded(&repr).serialize(serializer))
    }
}

/// See [`IEEE754`].
pub trait IEEE754ableDe {
    type Repr;
    fn from(repr: Self::Repr) -> Self;
}
/// See [`IEEE754`].
pub trait IEEE754ableSer {
    type Repr;
    fn to(&self) -> Self::Repr;
}
impl<T: IEEE754ableSer> IEEE754ableSer for &T {
    type Repr = T::Repr;
    fn to(&self) -> Self::Repr {
        T::to(self)
    }
}

impl IEEE754ableDe for f32 {
    type Repr = u32;
    fn from(repr: Self::Repr) -> Self {
        f32::from_bits(repr)
    }
}
impl IEEE754ableSer for f32 {
    type Repr = u32;
    fn to(&self) -> Self::Repr {
        self.to_bits()
    }
}

impl IEEE754ableDe for f64 {
    type Repr = u64;
    fn from(repr: Self::Repr) -> Self {
        f64::from_bits(repr)
    }
}
impl IEEE754ableSer for f64 {
    type Repr = u64;
    fn to(&self) -> Self::Repr {
        self.to_bits()
    }
}

/// Containers as tuple. Serialization-only of variable-length ones.  
/// (Usage: [`Tuple::of(item_seeder)`])
#[derive(Debug, Copy, Clone, Default)]
pub struct Tuple<ItemSeeder>(pub ItemSeeder);
impl<T: DeTupleable, ItemSeeder: Clone + DeSeeder<T::Item>> DeSeeder<T> for Tuple<ItemSeeder> {
    type Seed = TupleSeed<T, ItemSeeder>;
    fn seed(self) -> Self::Seed {
        TupleSeed(self.0, PhantomData)
    }
}
impl<
        'seeder: 'value,
        'value,
        T: 'value + SerTupleable<'seeder, 'value>,
        ItemSeeder: Clone + for<'item> SerSeeder<'seeder, 'item, T::Item> + 'seeder,
    > SerSeeder<'seeder, 'value, T> for Tuple<ItemSeeder>
{
    type Seeded = TupleSeeded<'seeder, 'value, T, ItemSeeder>;
    fn seeded(&'seeder self, value: &'value T) -> Self::Seeded {
        TupleSeeded(value, &self.0)
    }
}

#[doc(hidden)]
#[derive(Debug, Copy, Clone, Default)]
pub struct TupleSeed<T, ItemSeeder>(ItemSeeder, PhantomData<T>);
impl<'de, T: DeTupleable, ItemSeeder: Clone + DeSeeder<T::Item>> de::DeserializeSeed<'de>
    for TupleSeed<T, ItemSeeder>
{
    type Value = T;
    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct Visitor<T, ItemSeeder>(ItemSeeder, PhantomData<T>);
        impl<'de, T: DeTupleable, ItemSeeder: Clone + DeSeeder<T::Item>> de::Visitor<'de>
            for Visitor<T, ItemSeeder>
        {
            type Value = T;
            fn expecting(
                &self,
                f: &mut std::fmt::Formatter<'_>,
            ) -> std::result::Result<(), std::fmt::Error> {
                write!(f, "Tuple with lenth {}", T::len())
            }

            fn visit_seq<A: de::SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let mut error = Ok(());
                let array = T::from(
                    iter::from_fn(|| match seq.next_element_seed(self.0.clone().seed()) {
                        Ok(next) => next,
                        Err(e) => {
                            error = Err(e);
                            None
                        }
                    })
                    .take(T::len()),
                )?;
                Ok(array)
            }
        }

        deserializer.deserialize_tuple(T::len(), Visitor(self.0, PhantomData))
    }
}

#[doc(hidden)]
#[derive(Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct TupleSeeded<'seeder, 'value, T, ItemSeeder>(&'value T, &'seeder ItemSeeder);
impl<
        'seeder,
        'value,
        T: SerTupleable<'seeder, 'value>,
        ItemSeeder: SerSeeder<'seeder, 'value, T::Item>,
    > ser::Serialize for TupleSeeded<'seeder, 'value, T, ItemSeeder>
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut serialize_tuple = serializer.serialize_tuple(self.0.len())?;
        self.0.to(&mut serialize_tuple, self.1)?;
        serialize_tuple.end()
    }
}

/// See [`Tuple`].
pub trait DeTupleable: Sized {
    type Item;
    fn len() -> usize;
    fn from<I: IntoIterator<Item = Self::Item>, E: de::Error>(items: I) -> Result<Self, E>;
}
/// See [`Tuple`].
pub trait SerTupleable<'seeder, 'value> {
    type Item;
    fn len(&'value self) -> usize;
    fn to<SerializeTuple: ser::SerializeTuple, ItemSeeder: SerSeeder<'seeder, 'value, Self::Item>>(
        &'value self,
        serialize_tuple: &mut SerializeTuple,
        item_seeder: &'seeder ItemSeeder,
    ) -> Result<(), SerializeTuple::Error>;

    fn is_empty(&'value self) -> bool {
        self.len() == 0
    }
}

impl<T: Array> DeTupleable for T {
    type Item = T::Item;
    fn len() -> usize {
        T::CAPACITY
    }
    fn from<I: IntoIterator<Item = Self::Item>, E: de::Error>(items: I) -> Result<Self, E> {
        let mut items = items.into_iter();
        let mut vec = ArrayVec::new();
        while !vec.is_full() {
            vec.push(items.next().ok_or_else(|| {
                de::Error::invalid_length(
                    vec.len(),
                    &format!("Tuple of {}", <Self as DeTupleable>::len()).as_ref(),
                )
            })?)
        }
        let array = vec.into_inner().map_err(|_| unreachable!())?;
        Ok(array)
    }
}
impl<'seeder, 'value, T: 'value> SerTupleable<'seeder, 'value> for T
where
    &'value T: IntoIterator,
    <&'value T as IntoIterator>::IntoIter: ExactSizeIterator,
{
    type Item = <&'value T as IntoIterator>::Item;
    fn len(&'value self) -> usize {
        self.into_iter().len()
    }
    fn to<
        SerializeTuple: ser::SerializeTuple,
        ItemSeeder: SerSeeder<'seeder, 'value, Self::Item>,
    >(
        &'value self,
        serialize_tuple: &mut SerializeTuple,
        item_seeder: &'seeder ItemSeeder,
    ) -> Result<(), SerializeTuple::Error> {
        for element in self.into_iter() {
            serialize_tuple.serialize_element(&item_seeder.seeded(&element))?
        }
        Ok(())
    }
}

/// Containers as seq. Serialization-only of variable-length ones.  
/// (Usage: [`Seq::of(item_seeder)`])
#[derive(Debug, Copy, Clone, Default)]
pub struct Seq<ItemSeeder, Item>(ItemSeeder, PhantomData<Item>);
impl<ItemSeeder, Item> Seq<ItemSeeder, Item> {
    pub fn of(item_seeder: ItemSeeder) -> Self {
        Self(item_seeder, PhantomData)
    }
}

impl<T: DeSeqable<Item>, ItemSeeder: Clone + DeSeeder<Item>, Item> DeSeeder<T>
    for Seq<ItemSeeder, Item>
{
    type Seed = SeqSeed<T, ItemSeeder, Item>;
    fn seed(self) -> Self::Seed {
        SeqSeed(self.0, PhantomData)
    }
}
impl<
        'seeder: 'value,
        'value,
        T: 'value + SerSeqable<'seeder, 'value>,
        ItemSeeder: Clone + SerSeeder<'seeder, 'value, T::Item> + 'seeder,
    > SerSeeder<'seeder, 'value, T> for Seq<ItemSeeder, T::Item>
{
    type Seeded = SeqSeeded<'seeder, 'value, T, ItemSeeder>;
    fn seeded(&'seeder self, value: &'value T) -> Self::Seeded {
        SeqSeeded(value, &self.0)
    }
}

#[doc(hidden)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SeqSeed<T, ItemSeeder, Item>(ItemSeeder, PhantomData<(T, Item)>);
impl<'de, T: DeSeqable<Item>, ItemSeeder: Clone + DeSeeder<Item>, Item> de::DeserializeSeed<'de>
    for SeqSeed<T, ItemSeeder, Item>
{
    type Value = T;
    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct Visitor<T, ItemSeeder, Item>(ItemSeeder, PhantomData<(T, Item)>);
        impl<'de, T: DeSeqable<Item>, ItemSeeder: Clone + DeSeeder<Item>, Item> de::Visitor<'de>
            for Visitor<T, ItemSeeder, Item>
        {
            type Value = T;
            fn expecting(
                &self,
                f: &mut std::fmt::Formatter<'_>,
            ) -> std::result::Result<(), std::fmt::Error> {
                write!(f, "Seq")
            }

            fn visit_seq<A: de::SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let mut error = Ok(());
                let array = T::from(iter::from_fn(|| {
                    match seq.next_element_seed(self.0.clone().seed()) {
                        Ok(next) => next,
                        Err(e) => {
                            error = Err(e);
                            None
                        }
                    }
                }))?;
                Ok(array)
            }
        }

        deserializer.deserialize_seq(Visitor(self.0, PhantomData))
    }
}

#[doc(hidden)]
#[derive(Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct SeqSeeded<'seeder, 'value, T, ItemSeeder>(&'value T, &'seeder ItemSeeder);
impl<
        'seeder,
        'value,
        T: SerSeqable<'seeder, 'value>,
        ItemSeeder: SerSeeder<'seeder, 'value, T::Item>,
    > ser::Serialize for SeqSeeded<'seeder, 'value, T, ItemSeeder>
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut serialize_seq = serializer.serialize_seq(self.0.len())?;
        self.0.to(&mut serialize_seq, self.1)?;
        serialize_seq.end()
    }
}

/// See [`Seq`].
pub trait DeSeqable<Item>: Sized {
    fn from<I: IntoIterator<Item = Item>, E: de::Error>(items: I) -> Result<Self, E>;
}
/// See [`Seq`].
pub trait SerSeqable<'seeder, 'value> {
    type Item;
    fn len(&'value self) -> Option<usize>;
    fn to<SerializeSeq: ser::SerializeSeq, ItemSeeder: SerSeeder<'seeder, 'value, Self::Item>>(
        &'value self,
        serialize_seq: &mut SerializeSeq,
        item_seeder: &'seeder ItemSeeder,
    ) -> Result<(), SerializeSeq::Error>;

    fn is_empty(&'value self) -> Option<bool> {
        self.len().map(|len| len == 0)
    }
}

impl<T: FromIterator<Item>, Item> DeSeqable<Item> for T {
    fn from<I: IntoIterator<Item = Item>, E: de::Error>(items: I) -> Result<Self, E> {
        Ok(items.into_iter().collect())
    }
}
impl<'seeder, 'value, T: 'value> SerSeqable<'seeder, 'value> for T
where
    &'value T: IntoIterator,
    <&'value T as IntoIterator>::Item: 'value,
{
    type Item = <&'value T as IntoIterator>::Item;
    fn len(&'value self) -> Option<usize> {
        let hint = self.into_iter().size_hint();
        if let Some(max) = hint.1 {
            if hint.0 == max {
                return Some(max);
            }
        }
        None
    }
    fn to<
        SerializeSeq: ser::SerializeSeq,
        ItemSeeder: SerSeeder<'seeder, 'value, <&'value T as IntoIterator>::Item>,
    >(
        &'value self,
        serialize_seq: &mut SerializeSeq,
        item_seeder: &'seeder ItemSeeder,
    ) -> Result<(), SerializeSeq::Error> {
        for element in self.into_iter() {
            serialize_seq.serialize_element(&item_seeder.seeded(&element))?
        }
        Ok(())
    }
}
