use arrayvec::{Array, ArrayVec};
use cast::{i32, u32, usize};
use encoding::{all::WINDOWS_1252, DecoderTrap, Encoding as _};
use log::{debug, trace};
use serde::{
	de::{self, DeserializeSeed as _},
	ser::{self, SerializeSeq as _, SerializeTuple as _},
};
use serde_seeded::{seed, seeded, DeSeeder, SerSeeder};
use std::{borrow::Borrow, fmt::Debug, iter, marker::PhantomData, ops::Deref};
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
impl<'ser> SerSeeder<'ser, (), Self> for Literal<'ser> {
	fn seeded(self, _: &()) -> Self {
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

/// Little-endian (least significant byte first) storage for integers.
#[derive(Debug, Copy, Clone, Default)]
pub struct LittleEndian;
impl<'de, T: ByteOrdered> DeSeeder<'de, T> for LittleEndian {
	type Seed = Self;
	fn seed(self) -> Self::Seed {
		self
	}
}
impl<'ser, T: 'ser + ByteOrdered> SerSeeder<'ser, T, LittleEndianSeeded<'ser, T>> for LittleEndian {
	fn seeded(self, value: &'ser T) -> LittleEndianSeeded<'ser, T> {
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
struct LittleEndianSeeded<'a, T>(&'a T);
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

impl ByteOrdered for i32 {
	fn deserialize_le<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
		Ok(Self::from_le_bytes(PhantomData.deserialize(deserializer)?))
	}
	fn serialize_le<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
		serializer.serialize_bytes(&self.to_le_bytes())
	}
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
impl<
		'ser,
		T: 'ser + IEEE754able,
		ReprSeeder: 'ser + SerSeeder<'ser, T::Repr, ReprSeeded>,
		ReprSeeded,
	> SerSeeder<'ser, T, IEEE754Seeded<'ser, T, ReprSeeder>> for IEEE754<ReprSeeder>
{
	type Seeded = IEEE754Seeded<'ser, T, ReprSeeder>;
	fn seeded(self, value: &'ser T) -> IEEE754Seeded<'ser, T, ReprSeeder> {
		Box::new(IEEE754Seeded(value, &self.0))
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
struct IEEE754Seeded<'a, T, ReprSeeder>(&'a T, &'a ReprSeeder);
impl<'ser, T: IEEE754able, ReprSeeder: SerSeeder<'ser, T::Repr, ReprSeeded>, ReprSeeded>
	ser::Serialize for IEEE754Seeded<'ser, T, ReprSeeder>
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
pub struct Tuple<ItemSeeder>(pub ItemSeeder);
impl<'de, T: DeTupleable, ItemSeeder: Clone + DeSeeder<'de, T::Item>> DeSeeder<'de, T>
	for Tuple<ItemSeeder>
{
	type Seed = TupleSeed<T, ItemSeeder>;
	fn seed(self) -> Self::Seed {
		TupleSeed(self.0, PhantomData)
	}
}
impl<
		'ser,
		T: SerTupleable<Item>,
		ItemSeeder: Clone + SerSeeder<'ser, Item, ItemSeeded>,
		Item,
		ItemSeeded,
	> SerSeeder<'ser, T, TupleSeeded<'ser, T, ItemSeeder>> for Tuple<ItemSeeder>
{
	fn seeded<'s>(&'s self, value: &'s T) -> TupleSeeded<'ser, T, ItemSeeder> {
		TupleSeeded(value, &self.0)
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
struct TupleSeeded<'a, T, ItemSeeder>(&'a T, &'a ItemSeeder);
impl<
		'ser,
		T: SerTupleable<Item>,
		ItemSeeder: SerSeeder<'ser, Item, ItemSeeded>,
		Item,
		ItemSeeded,
	> ser::Serialize for TupleSeeded<'ser, T, ItemSeeder>
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
pub trait SerTupleable<Item> {
	fn len(&self) -> usize;
	fn to<
		'ser,
		SerializeTuple: ser::SerializeTuple,
		ItemSeeder: SerSeeder<'ser, Item, ItemSeeded>,
		ItemSeeded,
	>(
		&self,
		serialize_tuple: &mut SerializeTuple,
		item_seeder: &ItemSeeder,
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
impl<T: AsRef<[Item]>, Item> SerTupleable<Item> for T {
	fn len(&self) -> usize {
		self.as_ref().len()
	}
	fn to<
		'ser,
		SerializeTuple: ser::SerializeTuple,
		ItemSeeder: SerSeeder<'ser, Item, ItemSeeded>,
		ItemSeeded,
	>(
		&self,
		serialize_tuple: &mut SerializeTuple,
		item_seeder: &ItemSeeder,
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
pub struct TupleN<Length, ItemSeeder>(pub Length, pub ItemSeeder);
impl<'de, T: DeTupleNable, Length: Borrow<usize>, ItemSeeder: Clone + DeSeeder<'de, T::Item>>
	DeSeeder<'de, T> for TupleN<Length, ItemSeeder>
{
	type Seed = TupleNSeed<T, ItemSeeder>;
	fn seed(self) -> Self::Seed {
		TupleNSeed(*self.0.borrow(), self.1, PhantomData)
	}
}
impl<
		'ser,
		T: SerTupleNable,
		Length: Borrow<usize>,
		ItemSeeder: SerSeeder<'ser, T::Item, ItemSeeded>,
		ItemSeeded,
	> SerSeeder<'ser, T, TupleNSeeded<'ser, T, ItemSeeder>> for TupleN<Length, ItemSeeder>
{
	fn seeded<'s>(&'s self, value: &'s T) -> TupleNSeeded<'ser, T, ItemSeeder> {
		Box::new(TupleNSeeded(value, *self.0.borrow(), &self.1))
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
				trace!(
					"Deserializing TupleN({}, {})...",
					self.0,
					std::any::type_name::<A>()
				);
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
				error?;
				if self.0 != vec.len() {
					return Err(de::Error::invalid_length(vec.len(), &self));
				}
				trace!("Done TupleN({}, {}).", self.0, std::any::type_name::<A>());
				Ok(vec)
			}
		}

		deserializer.deserialize_tuple(self.0, Visitor(self.0, self.1, PhantomData))
	}
}

#[doc(hidden)]
#[derive(Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
struct TupleNSeeded<'a, T, ItemSeeder>(&'a T, usize, &'a ItemSeeder);
impl<'ser, T: SerTupleNable, ItemSeeder: SerSeeder<'ser, T::Item, ItemSeeded>, ItemSeeded>
	ser::Serialize for TupleNSeeded<'ser, T, ItemSeeder>
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
		let mut serialize_seq = serializer.serialize_tuple(self.0.len())?;
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
pub trait SerTupleNable {
	type Item;
	fn len(&self) -> usize;
	fn to<
		'ser,
		SerializeTuple: ser::SerializeTuple,
		ItemSeeder: SerSeeder<'ser, Self::Item, ItemSeeded>,
		ItemSeeded,
	>(
		&self,
		serialize_tuple: &mut SerializeTuple,
		item_seeder: &ItemSeeder,
	) -> Result<(), SerializeTuple::Error>;

	fn is_empty(&self) -> bool {
		self.len() == 0
	}
}
//TODO: Likely remove this once calls are fully qualified.
impl<'a, T: SerTupleNable> SerTupleNable for &'a T {
	type Item = T::Item;
	fn len(&self) -> usize {
		T::len(self)
	}
	fn to<
		'ser,
		SerializeTuple: ser::SerializeTuple,
		ItemSeeder: SerSeeder<'ser, Self::Item, ItemSeeded>,
		ItemSeeded,
	>(
		&self,
		serialize_tuple: &mut SerializeTuple,
		item_seeder: &ItemSeeder,
	) -> Result<(), SerializeTuple::Error> {
		T::to(self, serialize_tuple, item_seeder)
	}

	fn is_empty(&self) -> bool {
		T::is_empty(self)
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
impl<T> SerTupleNable for Vec<T> {
	type Item = T;
	fn len(&self) -> usize {
		self.len()
	}
	fn to<
		'ser,
		SerializeTuple: ser::SerializeTuple,
		ItemSeeder: SerSeeder<'ser, Self::Item, ItemSeeded>,
		ItemSeeded,
	>(
		&self,
		serialize_tuple: &mut SerializeTuple,
		item_seeder: &ItemSeeder,
	) -> Result<(), SerializeTuple::Error> {
		for element in self.as_slice() {
			serialize_tuple.serialize_element(&item_seeder.seeded(element))?
		}
		Ok(())
	}
}
impl<Item> SerTupleNable for [Item] {
	type Item = Item;
	fn len(&self) -> usize {
		self.deref().len()
	}
	fn to<
		'ser,
		SerializeTuple: ser::SerializeTuple,
		ItemSeeder: SerSeeder<'ser, Self::Item, ItemSeeded>,
		ItemSeeded,
	>(
		&self,
		serialize_tuple: &mut SerializeTuple,
		item_seeder: &ItemSeeder,
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
impl<'ser, T: SerSeqable, ItemSeeder: Clone + SerSeeder<'ser, T::Item, ItemSeeded>, ItemSeeded>
	SerSeeder<'ser, T, SeqSeeded<'ser, T, ItemSeeder>> for Seq<ItemSeeder>
{
	fn seeded<'s>(&'s self, value: &'s T) -> SeqSeeded<'ser, T, ItemSeeder> {
		Box::new(SeqSeeded(value, &self.0))
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
struct SeqSeeded<'a, T, ItemSeeder>(&'a T, &'a ItemSeeder);
impl<'ser, T: SerSeqable, ItemSeeder: SerSeeder<'ser, T::Item, ItemSeeded>, ItemSeeded>
	ser::Serialize for SeqSeeded<'ser, T, ItemSeeder>
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
pub trait SerSeqable {
	type Item;
	fn len(&self) -> usize;
	fn to<
		'ser,
		SerializeSeq: ser::SerializeSeq,
		ItemSeeder: SerSeeder<'ser, Self::Item, ItemSeeded>,
		ItemSeeded,
	>(
		&self,
		serialize_seq: &mut SerializeSeq,
		item_seeder: &ItemSeeder,
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
impl<T> SerSeqable for Vec<T> {
	type Item = T;
	fn len(&self) -> usize {
		self.len()
	}
	fn to<
		'ser,
		SerializeSeq: ser::SerializeSeq,
		ItemSeeder: SerSeeder<'ser, Self::Item, ItemSeeded>,
		ItemSeeded,
	>(
		&self,
		serialize_seq: &mut SerializeSeq,
		item_seeder: &ItemSeeder,
	) -> Result<(), SerializeSeq::Error> {
		for element in self.as_slice() {
			serialize_seq.serialize_element(&item_seeder.seeded(element))?
		}
		Ok(())
	}
}
impl<Item> SerSeqable for [Item] {
	type Item = Item;
	fn len(&self) -> usize {
		self.deref().len()
	}
	fn to<
		'ser,
		SerializeSeq: ser::SerializeSeq,
		ItemSeeder: SerSeeder<'ser, Self::Item, ItemSeeded>,
		ItemSeeded,
	>(
		&self,
		serialize_seq: &mut SerializeSeq,
		item_seeder: &ItemSeeder,
	) -> Result<(), SerializeSeq::Error> {
		for element in self {
			serialize_seq.serialize_element(&item_seeder.seeded(element))?
		}
		Ok(())
	}
}

/// [`Vec<_>`] as length-prefixed tuple.  
/// (Usage: [`Tuple::of(length_seeder: --Seeder<usize>, item_seeder)`])
#[derive(Debug, Copy, Clone)]
pub struct LengthPrefixed<LengthSeeder, ItemSeeder>(pub LengthSeeder, pub ItemSeeder);

impl<'de, LengthSeeder: DeSeeder<'de, usize>, ItemSeeder: DeSeeder<'de, Item> + Clone, Item>
	DeSeeder<'de, Vec<Item>> for LengthPrefixed<LengthSeeder, ItemSeeder>
{
	type Seed = LengthPrefixedSeed<LengthSeeder, ItemSeeder, Item>;
	fn seed(self) -> Self::Seed {
		LengthPrefixedSeed(self.0, self.1, PhantomData)
	}
}

pub struct LengthPrefixedSeed<LengthSeeder, ItemSeeder, Item>(
	pub LengthSeeder,
	pub ItemSeeder,
	pub PhantomData<Item>,
);

impl<'de, LengthSeeder: DeSeeder<'de, usize>, ItemSeeder: DeSeeder<'de, Item> + Clone, Item>
	de::DeserializeSeed<'de> for LengthPrefixedSeed<LengthSeeder, ItemSeeder, Item>
{
	type Value = Vec<Item>;
	fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
	where
		D: serde::Deserializer<'de>,
	{
		#[derive(Debug, seed)]
		#[seed_generics_de('de, LengthSeeder: DeSeeder<'de, usize>, ItemSeeder: DeSeeder<'de, Item> + Clone)]
		#[seed_args(length_seeder: LengthSeeder, item_seeder: ItemSeeder)]
		struct LengthPrefixedLayout<Item> {
			#[seeded(length_seeder)]
			length: usize,

			#[seeded(TupleN(length, item_seeder))]
			data: Vec<Item>,
		}

		LengthPrefixedLayout::seed(self.0, self.1)
			.deserialize(deserializer)?
			.data
			.pipe(Ok)
	}
}

impl<
		'ser,
		LengthSeeder: SerSeeder<'ser, usize, LengthSeeded>,
		LengthSeeded,
		ItemSeeder: SerSeeder<'ser, Item, ItemSeeded>,
		Item,
		ItemSeeded,
	> SerSeeder<'ser, Vec<Item>, LengthPrefixedSeeded<'ser, LengthSeeder, ItemSeeder, Item>>
	for LengthPrefixed<LengthSeeder, ItemSeeder>
{
	fn seeded<'s>(
		&'s self,
		value: &'s Vec<Item>,
	) -> LengthPrefixedSeeded<'ser, LengthSeeder, ItemSeeder, Item> {
		Box::new(LengthPrefixedSeeded(&self.0, &self.1, value))
	}
}

struct LengthPrefixedSeeded<'a, LengthSeeder, ItemSeeder, Item>(
	&'a LengthSeeder,
	&'a ItemSeeder,
	&'a Vec<Item>,
);

impl<
		'ser,
		LengthSeeder: SerSeeder<'ser, usize, LengthSeeded>,
		LengthSeeded,
		ItemSeeder: SerSeeder<'ser, Item, ItemSeeded>,
		Item,
		ItemSeeded,
	> ser::Serialize for LengthPrefixedSeeded<'ser, LengthSeeder, ItemSeeder, Item>
{
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: serde::Serializer,
	{
		#[derive(Debug, seeded)]
		#[seed_generics(
			'ser,
			LengthSeeder: 'ser + SerSeeder<'ser, usize, LengthSeeded>,
			LengthSeeded,
			ItemSeeder: 'ser + SerSeeder<'ser, Item, ItemSeeded>,
			ItemSeeded,
		)]
		#[seed_args(length_seeder: &'ser LengthSeeder, item_seeder: &'ser ItemSeeder)]
		struct LengthPrefixedLayout<'a, Item> {
			#[seeded(length_seeder)]
			length: usize,

			#[seeded(TupleN(*length, item_seeder))]
			data: &'a Vec<Item>,
		}

		LengthPrefixedLayout {
			length: self.2.len(),
			data: self.2,
		}
		.seeded(self.0, self.1)
		.serialize(serializer)
	}
}

#[derive(Debug, Copy, Clone)]
pub struct SerdeLike;
impl<'de, T: de::Deserialize<'de>> DeSeeder<'de, T> for SerdeLike {
	type Seed = PhantomData<T>;
	fn seed(self) -> Self::Seed {
		PhantomData
	}
}
impl<'ser, T: ser::Serialize> SerSeeder<'ser, T, &'ser T> for SerdeLike {
	fn seeded(self, value: &'ser T) -> &'ser T {
		value
	}
}

/// Fallible u32-storage.  
/// (Parameters: u32 [`Seeder`])
#[derive(Debug, Copy, Clone, Default)]
pub struct TryAsU32<U32Seeder>(pub U32Seeder);
impl<'d, T: TryAsU32able, U32Seeder: DeSeeder<'d, u32>> DeSeeder<'d, T> for TryAsU32<U32Seeder> {
	type Seed = TryAsU32Seed<T, U32Seeder>;
	fn seed(self) -> Self::Seed {
		TryAsU32Seed(self.0, PhantomData)
	}
}
impl<'ser, T: TryAsU32able, U32Seeder: SerSeeder<'ser, u32, U32Seeded>, U32Seeded>
	SerSeeder<'ser, T, TryAsI32Seeded<'ser, T, U32Seeder>> for TryAsU32<U32Seeder>
{
	fn seeded(&self, value: &'ser T) -> TryAsI32Seeded<'ser, T, U32Seeder> {
		Box::new(TryAsU32Seeded(value, &self.0))
	}
}

#[doc(hidden)]
#[derive(Debug, Copy, Clone, Default)]
pub struct TryAsU32Seed<T, U32Seeder>(U32Seeder, PhantomData<T>);
impl<'de, T: TryAsU32able, U32Seeder: DeSeeder<'de, u32>> de::DeserializeSeed<'de>
	for TryAsU32Seed<T, U32Seeder>
{
	type Value = T;
	fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
	where
		D: serde::Deserializer<'de>,
	{
		self.0.seed().deserialize(deserializer)?.pipe(T::from)
	}
}

#[doc(hidden)]
#[derive(Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
struct TryAsU32Seeded<'a, T, U32Seeder>(&'a T, &'a U32Seeder);
impl<'ser, T: TryAsU32able, U32Seeder: SerSeeder<'ser, u32, U32Seeded>, U32Seeded> ser::Serialize
	for TryAsU32Seeded<'ser, T, U32Seeder>
{
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: serde::Serializer,
	{
		self.0
			.to()?
			.pipe(|repr| self.1.seeded(&repr).serialize(serializer))
	}
}

/// See [`TryAsU32`].
pub trait TryAsU32able: Sized {
	fn from<E: de::Error>(repr: u32) -> Result<Self, E>;
	fn to<E: ser::Error>(&self) -> Result<u32, E>;
}

impl TryAsU32able for usize {
	fn from<E: de::Error>(repr: u32) -> Result<Self, E> {
		usize(repr).pipe(Ok)
	}
	fn to<E: ser::Error>(&self) -> Result<u32, E> {
		u32(*self).map_err(ser::Error::custom)
	}
}

/// Fallible i32-storage.  
/// (Parameters: i32 [`Seeder`])
#[derive(Debug, Copy, Clone, Default)]
pub struct TryAsI32<I32Seeder>(pub I32Seeder);
impl<'de, T: TryAsI32able, I32Seeder: DeSeeder<'de, i32>> DeSeeder<'de, T> for TryAsI32<I32Seeder> {
	type Seed = TryAsI32Seed<T, I32Seeder>;
	fn seed(self) -> Self::Seed {
		TryAsI32Seed(self.0, PhantomData)
	}
}
impl<'ser, T: TryAsI32able, I32Seeder: SerSeeder<'ser, i32, I32Seeder>, I32Seeded>
	SerSeeder<'ser, T, TryAsI32Seeded<'ser, T, I32Seeder>> for TryAsI32<I32Seeder>
{
	fn seeded<'s>(&'s self, value: &'s T) -> TryAsI32Seeded<'ser, T, I32Seeder> {
		TryAsI32Seeded(value, &self.0)
	}
}

#[doc(hidden)]
#[derive(Debug, Copy, Clone, Default)]
pub struct TryAsI32Seed<T, I32Seeder>(I32Seeder, PhantomData<T>);
impl<'de, T: TryAsI32able, I32Seeder: DeSeeder<'de, i32>> de::DeserializeSeed<'de>
	for TryAsI32Seed<T, I32Seeder>
{
	type Value = T;
	fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
	where
		D: serde::Deserializer<'de>,
	{
		self.0.seed().deserialize(deserializer)?.pipe(T::from)
	}
}

#[doc(hidden)]
#[derive(Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
struct TryAsI32Seeded<'a, T, I32Seeder>(&'a T, &'a I32Seeder);
impl<'ser, T: TryAsI32able, I32Seeder: SerSeeder<'ser, i32, I32Seeded>, I32Seeded> ser::Serialize
	for TryAsI32Seeded<'ser, T, I32Seeder>
{
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: serde::Serializer,
	{
		self.0
			.to()?
			.pipe(|repr| self.1.seeded(&repr).serialize(serializer))
	}
}

/// See [`TryAsI32`].
pub trait TryAsI32able: Sized {
	fn from<E: de::Error>(repr: i32) -> Result<Self, E>;
	fn to<E: ser::Error>(&self) -> Result<i32, E>;
}

impl TryAsI32able for usize {
	fn from<E: de::Error>(repr: i32) -> Result<Self, E> {
		usize(repr).map_err(de::Error::custom)
	}
	fn to<E: ser::Error>(&self) -> Result<i32, E> {
		i32(*self).map_err(ser::Error::custom)
	}
}

/// String as Windows-1252 storage.  
/// (Parameters: Vec<u8> [`Seeder`])
#[derive(Debug, Copy, Clone, Default)]
pub struct Windows1252<BytesSeeder>(pub BytesSeeder);
impl<'de, T: DeWindows1252able<'de>, BytesSeeder: DeSeeder<'de, Vec<u8>>> DeSeeder<'de, T>
	for Windows1252<BytesSeeder>
{
	type Seed = Windows1252Seed<T, BytesSeeder>;
	fn seed(self) -> Self::Seed {
		Windows1252Seed(self.0, PhantomData)
	}
}
impl<
		'ser,
		T: SerWindows1252able,
		BytesSeeder: SerSeeder<'ser, Vec<u8>, BytesSeeded>,
		BytesSeeded,
	> SerSeeder<'ser, T, Windows1252Seeded<'ser, T, BytesSeeder>> for Windows1252<BytesSeeder>
{
	fn seeded(self, value: &'ser T) -> Windows1252Seeded<'ser, T, BytesSeeder> {
		Box::new(Windows1252Seeded(value, &self.0))
	}
}

#[doc(hidden)]
#[derive(Debug, Copy, Clone, Default)]
pub struct Windows1252Seed<T, BytesSeeder>(BytesSeeder, PhantomData<T>);
impl<'de, T: DeWindows1252able<'de>, BytesSeeder: DeSeeder<'de, Vec<u8>>> de::DeserializeSeed<'de>
	for Windows1252Seed<T, BytesSeeder>
{
	type Value = T;
	fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
	where
		D: serde::Deserializer<'de>,
	{
		let value = self.0.seed().deserialize(deserializer)?.pipe(T::from)?;
		debug!("Decoded Windows-1252: {:?}", value);
		Ok(value)
	}
}

#[doc(hidden)]
#[derive(Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
struct Windows1252Seeded<'a, T, BytesSeeder>(&'a T, &'a BytesSeeder);
impl<
		'ser,
		T: SerWindows1252able,
		BytesSeeder: SerSeeder<'ser, Vec<u8>, BytesSeeded>,
		BytesSeeded,
	> ser::Serialize for Windows1252Seeded<'ser, T, BytesSeeder>
{
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: serde::Serializer,
	{
		self.0
			.to()?
			.pipe(|repr| self.1.seeded(&repr).serialize(serializer))
	}
}

/// See [`Windows1252`].
pub trait DeWindows1252able<'de>: Sized + Debug {
	fn from<E: de::Error>(repr: Vec<u8>) -> Result<Self, E>;
}
/// See [`Windows1252`].
pub trait SerWindows1252able: Sized {
	fn to<E: ser::Error>(&self) -> Result<Vec<u8>, E>;
}

impl<'de> DeWindows1252able<'de> for String {
	fn from<E: de::Error>(repr: Vec<u8>) -> Result<Self, E> {
		WINDOWS_1252
			.decode(repr.as_ref(), DecoderTrap::Strict)
			.map_err(de::Error::custom)
	}
}
impl SerWindows1252able for String {
	fn to<E: ser::Error>(&self) -> Result<Vec<u8>, E> {
		WINDOWS_1252
			.encode(self, encoding::EncoderTrap::Strict)
			.map_err(ser::Error::custom)
	}
}

/// Serializes [`AsRef<[u8]>`] as bytes.  
/// Deserializes bytes as [`From<&'de [u8]> + From<Vec<u8>>`].
pub struct Buffer;
impl<'de, T: From<&'de [u8]> + From<Vec<u8>>> DeSeeder<'de, T> for Buffer {
	type Seed = BufferSeed<T>;
	fn seed(self) -> Self::Seed {
		BufferSeed(PhantomData)
	}
}
impl<'ser, T: AsRef<[u8]>> SerSeeder<'ser, T, BufferSeeded<'ser>> for Buffer {
	fn seeded<'s>(&self, value: &'s T) -> BufferSeeded<'ser> {
		Box::new(BufferSeeded(value.as_ref()))
	}
}

/// See [`Buffer`].
pub struct BufferSeed<T>(PhantomData<T>);
impl<'de, T: From<&'de [u8]> + From<Vec<u8>>> de::DeserializeSeed<'de> for BufferSeed<T> {
	type Value = T;

	fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
	where
		D: serde::Deserializer<'de>,
	{
		struct Visitor<T>(PhantomData<T>);
		impl<'de, T: From<&'de [u8]> + From<Vec<u8>>> de::Visitor<'de> for Visitor<T> {
			type Value = T;

			fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
				write!(formatter, "bytes")
			}

			fn visit_borrowed_bytes<E: de::Error>(self, v: &'de [u8]) -> Result<Self::Value, E> {
				Ok(v.into())
			}
			fn visit_byte_buf<E: de::Error>(self, v: Vec<u8>) -> Result<Self::Value, E> {
				Ok(v.into())
			}
			fn visit_bytes<E: de::Error>(self, v: &[u8]) -> Result<Self::Value, E> {
				self.visit_byte_buf(v.into())
			}
		}

		deserializer.deserialize_bytes(Visitor(self.0))
	}
}

struct BufferSeeded<'a>(&'a [u8]);
impl<'a> ser::Serialize for BufferSeeded<'a> {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: serde::Serializer,
	{
		serializer.serialize_bytes(self.0)
	}
}
