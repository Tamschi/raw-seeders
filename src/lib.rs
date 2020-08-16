use arrayvec::{Array, ArrayVec};
use serde::{
	de::{self, DeserializeSeed as _},
	ser::{self, SerializeSeq as _, SerializeTuple as _},
};
use serde_seeded::{seed, seeded, DeSeeder, SerSeeder};
use std::{iter, marker::PhantomData, ops::Deref};
use wyz::Pipe as _;

/// Stores a binary slice instead of a `()`.  
/// (Parameters: A `&[u8]` specifying the data to store or check against.)
#[derive(Debug, Clone, Copy, PartialEq, Ord, PartialOrd, Eq)]
pub struct Literal<'a>(pub &'a [u8]);
impl<'a, 'de> DeSeeder<'de, ()> for Literal<'a> {
	type Seed = Self;
	fn seed(self) -> Self::Seed {
		self
	}
}
impl<'s> SerSeeder<'s, ()> for Literal<'s> {
	type Seeded = Self;
	fn seeded(&'s self, _: &()) -> Self::Seeded {
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
impl<'de, T: ByteOrdered> DeSeeder<'de, T> for LittleEndian {
	type Seed = LittleEndianSeed<T>;
	fn seed(self) -> Self::Seed {
		LittleEndianSeed(PhantomData)
	}
}
impl<'s, T: 's + ByteOrdered> SerSeeder<'s, T> for LittleEndian {
	type Seeded = LittleEndianSeeded<'s, T>;
	fn seeded(&'s self, value: &'s T) -> Self::Seeded {
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
impl<'d, T: IEEE754able, ReprSeeder: DeSeeder<'d, T::Repr>> DeSeeder<'d, T>
	for IEEE754<ReprSeeder>
{
	type Seed = IEEE754Seed<T, ReprSeeder>;
	fn seed(self) -> Self::Seed {
		IEEE754Seed(self.0, PhantomData)
	}
}
impl<'s, T: 's + IEEE754able, ReprSeeder: for<'repr> SerSeeder<'repr, T::Repr> + 's>
	SerSeeder<'s, T> for IEEE754<ReprSeeder>
{
	type Seeded = IEEE754Seeded<'s, T, ReprSeeder>;
	fn seeded(&'s self, value: &'s T) -> Self::Seeded {
		IEEE754Seeded(value, &self.0)
	}
}

#[doc(hidden)]
#[derive(Debug, Copy, Clone, Default)]
pub struct IEEE754Seed<T, ReprSeeder>(ReprSeeder, PhantomData<T>);
impl<'de, T: IEEE754able, ReprSeeder: DeSeeder<'de, T::Repr>> de::DeserializeSeed<'de>
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
impl<'s, T: IEEE754able, ReprSeeder: for<'repr> SerSeeder<'repr, T::Repr>> ser::Serialize
	for IEEE754Seeded<'s, T, ReprSeeder>
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

/// Fixed length containers as tuple.  
/// (Usage: [`Tuple::of(item_seeder)`])
#[derive(Debug, Copy, Clone, Default)]
pub struct Tuple<ItemSeeder, Item>(ItemSeeder, PhantomData<Item>);
impl<ItemSeeder, Item> Tuple<ItemSeeder, Item> {
	pub fn of(item_seeder: ItemSeeder) -> Self {
		Self(item_seeder, PhantomData)
	}
}

impl<'de, T: DeTupleable, ItemSeeder: Clone + DeSeeder<'de, T::Item>> DeSeeder<'de, T>
	for Tuple<ItemSeeder, T::Item>
{
	type Seed = TupleSeed<T, ItemSeeder>;
	fn seed(self) -> Self::Seed {
		TupleSeed(self.0, PhantomData)
	}
}
impl<
		's,
		T: 's + SerTupleable<'s, Item>,
		ItemSeeder: Clone + SerSeeder<'s, Item> + 's,
		Item: 's,
	> SerSeeder<'s, T> for Tuple<ItemSeeder, Item>
{
	type Seeded = TupleSeeded<'s, T, ItemSeeder, Item>;
	fn seeded(&'s self, value: &'s T) -> Self::Seeded {
		TupleSeeded(value, &self.0, self.1)
	}
}

#[doc(hidden)]
#[derive(Debug, Copy, Clone, Default)]
pub struct TupleSeed<T, ItemSeeder>(ItemSeeder, PhantomData<T>);
impl<'de, T: DeTupleable, ItemSeeder: Clone + DeSeeder<'de, T::Item>> de::DeserializeSeed<'de>
	for TupleSeed<T, ItemSeeder>
{
	type Value = T;
	fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
	where
		D: serde::Deserializer<'de>,
	{
		struct Visitor<T, ItemSeeder>(ItemSeeder, PhantomData<T>);
		impl<'de, T: DeTupleable, ItemSeeder: Clone + DeSeeder<'de, T::Item>> de::Visitor<'de>
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
pub struct TupleSeeded<'s, T, ItemSeeder, Item>(&'s T, &'s ItemSeeder, PhantomData<Item>);
impl<'s, T: SerTupleable<'s, Item>, ItemSeeder: SerSeeder<'s, Item>, Item> ser::Serialize
	for TupleSeeded<'s, T, ItemSeeder, Item>
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
pub trait SerTupleable<'s, Item> {
	fn len(&self) -> usize;
	fn to<SerializeTuple: ser::SerializeTuple, ItemSeeder: SerSeeder<'s, Item>>(
		&'s self,
		serialize_tuple: &mut SerializeTuple,
		item_seeder: &'s ItemSeeder,
	) -> Result<(), SerializeTuple::Error>;

	fn is_empty(&self) -> bool {
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
impl<'s, T: AsRef<[Item]>, Item: 's> SerTupleable<'s, Item> for T {
	fn len(&self) -> usize {
		self.as_ref().len()
	}
	fn to<SerializeTuple: ser::SerializeTuple, ItemSeeder: SerSeeder<'s, Item>>(
		&'s self,
		serialize_tuple: &mut SerializeTuple,
		item_seeder: &'s ItemSeeder,
	) -> Result<(), SerializeTuple::Error> {
		for element in self.as_ref() {
			serialize_tuple.serialize_element(&item_seeder.seeded(element))?
		}
		Ok(())
	}
}

/// Vec as tuple.
/// (Usage: [`TupleN(length, item_seeder)`])
#[derive(Debug, Copy, Clone, Default)]
pub struct TupleN<ItemSeeder>(pub usize, pub ItemSeeder);
impl<'de, T: DeTupleNable, ItemSeeder: Clone + DeSeeder<'de, T::Item>> DeSeeder<'de, T>
	for TupleN<ItemSeeder>
{
	type Seed = TupleNSeed<T, ItemSeeder>;
	fn seed(self) -> Self::Seed {
		TupleNSeed(self.0, self.1, PhantomData)
	}
}
impl<'s, T: 's + SerTupleNable<'s>, ItemSeeder: 's + Clone + SerSeeder<'s, T::Item>>
	SerSeeder<'s, T> for TupleN<ItemSeeder>
{
	type Seeded = TupleNSeeded<'s, T, ItemSeeder>;
	fn seeded(&'s self, value: &'s T) -> Self::Seeded {
		TupleNSeeded(value, self.0, &self.1)
	}
}

#[doc(hidden)]
#[derive(Debug, Copy, Clone, Default)]
pub struct TupleNSeed<T, ItemSeeder>(usize, ItemSeeder, PhantomData<T>);
impl<'de, T: DeTupleNable, ItemSeeder: Clone + DeSeeder<'de, T::Item>> de::DeserializeSeed<'de>
	for TupleNSeed<T, ItemSeeder>
{
	type Value = T;
	fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
	where
		D: serde::Deserializer<'de>,
	{
		struct Visitor<T, ItemSeeder>(usize, ItemSeeder, PhantomData<T>);
		impl<'de, T: DeTupleNable, ItemSeeder: Clone + DeSeeder<'de, T::Item>> de::Visitor<'de>
			for Visitor<T, ItemSeeder>
		{
			type Value = T;
			fn expecting(
				&self,
				f: &mut std::fmt::Formatter<'_>,
			) -> std::result::Result<(), std::fmt::Error> {
				write!(f, "TupleN({}, _)", self.0)
			}

			fn visit_seq<A: de::SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
				let mut error = Ok(());
				let vec = T::from(
					iter::from_fn(|| match seq.next_element_seed(self.1.clone().seed()) {
						Ok(next) => next,
						Err(e) => {
							error = Err(e);
							None
						}
					})
					.take(self.0),
				)?;
				if self.0 != vec.len() {
					return Err(de::Error::invalid_length(vec.len(), &self));
				}
				Ok(vec)
			}
		}

		deserializer.deserialize_tuple(self.0, Visitor(self.0, self.1, PhantomData))
	}
}

#[doc(hidden)]
#[derive(Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct TupleNSeeded<'s, T, ItemSeeder>(&'s T, usize, &'s ItemSeeder);
impl<'s, T: SerTupleNable<'s>, ItemSeeder: SerSeeder<'s, T::Item>> ser::Serialize
	for TupleNSeeded<'s, T, ItemSeeder>
{
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: serde::Serializer,
	{
		if self.1 != self.0.len() {
			return Err(ser::Error::custom(format_args!(
				"Tried to serialise SeqN({}, _) from a .len = {}",
				self.1,
				self.0.len()
			)));
		}
		let mut serialize_seq = serializer.serialize_tuple(self.0.len().into())?;
		self.0.to(&mut serialize_seq, self.2)?;
		serialize_seq.end()
	}
}

/// See [`TupleN`].
pub trait DeTupleNable: Sized {
	type Item;
	fn len(&self) -> usize;
	fn from<I: IntoIterator<Item = Self::Item>, E: de::Error>(items: I) -> Result<Self, E>;

	fn is_empty(&self) -> bool {
		self.len() == 0
	}
}
/// See [`TupleN`].
pub trait SerTupleNable<'s> {
	type Item;
	fn len(&self) -> usize;
	fn to<SerializeTuple: ser::SerializeTuple, ItemSeeder: SerSeeder<'s, Self::Item>>(
		&'s self,
		serialize_tuple: &mut SerializeTuple,
		item_seeder: &'s ItemSeeder,
	) -> Result<(), SerializeTuple::Error>;

	fn is_empty(&self) -> bool {
		self.len() == 0
	}
}

impl<T> DeTupleNable for Vec<T> {
	type Item = T;
	fn len(&self) -> usize {
		self.len()
	}
	fn from<I: IntoIterator<Item = Self::Item>, E: de::Error>(items: I) -> Result<Self, E> {
		Ok(items.into_iter().collect())
	}
}
impl<'s, T> SerTupleNable<'s> for Vec<T> {
	type Item = T;
	fn len(&self) -> usize {
		self.len()
	}
	fn to<SerializeTuple: ser::SerializeTuple, ItemSeeder: SerSeeder<'s, Self::Item>>(
		&'s self,
		serialize_tuple: &mut SerializeTuple,
		item_seeder: &'s ItemSeeder,
	) -> Result<(), SerializeTuple::Error> {
		for element in self.as_slice() {
			serialize_tuple.serialize_element(&item_seeder.seeded(element))?
		}
		Ok(())
	}
}
impl<'s, Item> SerTupleNable<'s> for [Item] {
	type Item = Item;
	fn len(&self) -> usize {
		self.deref().len()
	}
	fn to<SerializeTuple: ser::SerializeTuple, ItemSeeder: SerSeeder<'s, Self::Item>>(
		&'s self,
		serialize_tuple: &mut SerializeTuple,
		item_seeder: &'s ItemSeeder,
	) -> Result<(), SerializeTuple::Error> {
		for element in self {
			serialize_tuple.serialize_element(&item_seeder.seeded(element))?
		}
		Ok(())
	}
}

/// Vec as seq.
/// (Usage: [`Seq(item_seeder)`])
#[derive(Debug, Copy, Clone, Default)]
pub struct Seq<ItemSeeder>(pub ItemSeeder);
impl<'de, T: DeSeqable, ItemSeeder: Clone + DeSeeder<'de, T::Item>> DeSeeder<'de, T>
	for Seq<ItemSeeder>
{
	type Seed = SeqSeed<T, ItemSeeder>;
	fn seed(self) -> Self::Seed {
		SeqSeed(self.0, PhantomData)
	}
}
impl<'s, T: 's + SerSeqable<'s>, ItemSeeder: 's + Clone + SerSeeder<'s, T::Item>> SerSeeder<'s, T>
	for Seq<ItemSeeder>
{
	type Seeded = SeqSeeded<'s, T, ItemSeeder>;
	fn seeded(&'s self, value: &'s T) -> Self::Seeded {
		SeqSeeded(value, &self.0)
	}
}

#[doc(hidden)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SeqSeed<T, ItemSeeder>(ItemSeeder, PhantomData<T>);
impl<'de, T: DeSeqable, ItemSeeder: Clone + DeSeeder<'de, T::Item>> de::DeserializeSeed<'de>
	for SeqSeed<T, ItemSeeder>
{
	type Value = T;
	fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
	where
		D: serde::Deserializer<'de>,
	{
		struct Visitor<T, ItemSeeder>(ItemSeeder, PhantomData<T>);
		impl<'de, T: DeSeqable, ItemSeeder: Clone + DeSeeder<'de, T::Item>> de::Visitor<'de>
			for Visitor<T, ItemSeeder>
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
pub struct SeqSeeded<'s, T, ItemSeeder>(&'s T, &'s ItemSeeder);
impl<'s, T: SerSeqable<'s>, ItemSeeder: SerSeeder<'s, T::Item>> ser::Serialize
	for SeqSeeded<'s, T, ItemSeeder>
{
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: serde::Serializer,
	{
		let mut serialize_seq = serializer.serialize_seq(self.0.len().into())?;
		self.0.to(&mut serialize_seq, self.1)?;
		serialize_seq.end()
	}
}

/// See [`Seq`].
pub trait DeSeqable: Sized {
	type Item;
	fn from<I: IntoIterator<Item = Self::Item>, E: de::Error>(items: I) -> Result<Self, E>;
}
/// See [`Seq`].
pub trait SerSeqable<'s> {
	type Item;
	fn len(&self) -> usize;
	fn to<SerializeSeq: ser::SerializeSeq, ItemSeeder: SerSeeder<'s, Self::Item>>(
		&'s self,
		serialize_seq: &mut SerializeSeq,
		item_seeder: &'s ItemSeeder,
	) -> Result<(), SerializeSeq::Error>;

	fn is_empty(&self) -> bool {
		self.len() == 0
	}
}

impl<T> DeSeqable for Vec<T> {
	type Item = T;
	fn from<I: IntoIterator<Item = Self::Item>, E: de::Error>(items: I) -> Result<Self, E> {
		Ok(items.into_iter().collect())
	}
}
impl<'s, T> SerSeqable<'s> for Vec<T> {
	type Item = T;
	fn len(&self) -> usize {
		self.len()
	}
	fn to<SerializeSeq: ser::SerializeSeq, ItemSeeder: SerSeeder<'s, Self::Item>>(
		&'s self,
		serialize_seq: &mut SerializeSeq,
		item_seeder: &'s ItemSeeder,
	) -> Result<(), SerializeSeq::Error> {
		for element in self.as_slice() {
			serialize_seq.serialize_element(&item_seeder.seeded(element))?
		}
		Ok(())
	}
}
impl<'s, Item> SerSeqable<'s> for [Item] {
	type Item = Item;
	fn len(&self) -> usize {
		self.deref().len()
	}
	fn to<SerializeSeq: ser::SerializeSeq, ItemSeeder: SerSeeder<'s, Self::Item>>(
		&'s self,
		serialize_seq: &mut SerializeSeq,
		item_seeder: &'s ItemSeeder,
	) -> Result<(), SerializeSeq::Error> {
		for element in self {
			serialize_seq.serialize_element(&item_seeder.seeded(element))?
		}
		Ok(())
	}
}

#[derive(Debug, Copy, Clone)]
pub struct LengthPrefixed<Length, LengthSeeder, ItemSeeder>(
	pub PhantomData<Length>,
	pub LengthSeeder,
	pub ItemSeeder,
);

impl<'de, Length, LengthSeeder, ItemSeeder: DeSeeder<'de, Item>, Item> DeSeeder<'de, Vec<Item>>
	for LengthPrefixed<Length, LengthSeeder, ItemSeeder>
{
	type Seed = LengthPrefixedSeed<Length, LengthSeeder, ItemSeeder, Item>;
	fn seed(self) -> Self::Seed {
		LengthPrefixedSeed(self.0, self.1, self.2, PhantomData)
	}
}

pub struct LengthPrefixedSeed<Length, LengthSeeder, ItemSeeder, Item>(
	pub PhantomData<Length>,
	pub LengthSeeder,
	pub ItemSeeder,
	pub PhantomData<Item>,
);

impl<'de, Length, LengthSeeder, ItemSeeder: DeSeeder<'de, Item>, Item> de::DeserializeSeed<'de>
	for LengthPrefixedSeed<Length, LengthSeeder, ItemSeeder, Item>
{
	type Value = Vec<Item>;
	fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
	where
		D: serde::Deserializer<'de>,
	{
		LengthPrefixedLayout::seed(self.1, self.2)
			.deserialize(deserializer)?
			.data
			.pipe(Ok)
	}
}

impl<'s, Length, LengthSeeder, ItemSeeder: SerSeeder<'s, Item>, Item> SerSeeder<'s, Vec<Item>>
	for LengthPrefixed<Length, LengthSeeder, ItemSeeder>
{
	type Seeded = LengthPrefixedSeeded<'s, Length, LengthSeeder, ItemSeeder, Item>;
	fn seeded(&'s self, value: &'s Vec<Item>) -> Self::Seeded {
		LengthPrefixedSeeded(self.0, self.1, self.2, value)
	}
}

struct LengthPrefixedSeeded<'s, Length, LengthSeeder, ItemSeeder, Item>(
	PhantomData<Length>,
	LengthSeeder,
	ItemSeeder,
	&'s Vec<Item>,
);

impl<'s, Length, LengthSeeder, ItemSeeder: SerSeeder<'s, Item>, Item> ser::Serialize
	for LengthPrefixedSeeded<'s, Length, LengthSeeder, ItemSeeder, Item>
{
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: serde::Serializer,
	{
		LengthPrefixedLayout {
			length: self.3.len() as Length,
			data: self.3,
		}
		.seeded(self.1, self.2)
		.serialize(serializer)
	}
}

#[derive(Debug, seed)]
#[seed_generics_de('de, LengthSeeder: DeSeeder<'de, Length>, ItemSeeder: DeSeeder<'de, Item>)]
#[seed_generics_ser(LengthSeeder: SerSeeder<'s, Length>, ItemSeeder: SerSeeder<'s, Item>)]
#[seed_args(length_seeder: LengthSeeder, item_seeder: ItemSeeder)]
struct LengthPrefixedLayout<Length, Item> {
	#[seeded(length_seeder)]
	length: Length,

	#[seeded_de(TupleN(self.length as usize, item_seeder))]
	#[seeded_ser(TupleN(self.length as usize, item_seeder))]
	// #[seeded_ser(TupleN(self.length as usize, item_seeder))]
	data: Vec<Item>,
}

pub struct SerdeLike;
impl<'s, T: 's + ser::Serialize> SerSeeder<'s, T> for SerdeLike {
	type Seeded = &'s T;
	fn seeded(&'s self, value: &'s T) -> Self::Seeded {
		value
	}
}
impl<'de, T: de::Deserialize<'de>> DeSeeder<'de, T> for SerdeLike {
	type Seed = PhantomData<T>;
	fn seed(self) -> Self::Seed {
		PhantomData
	}
}
