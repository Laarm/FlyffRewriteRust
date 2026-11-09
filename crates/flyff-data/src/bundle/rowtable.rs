//! `RowTable<T>`: the columnar encoding of large homogeneous tables (Plan/14 §10.2).
//!
//! ```jsonc
//! { "columns": ["id", "name", "num", /* T's fields in declaration order */],
//!   "rows":    [[6, "Wooden Sword", 1, /* ... */], /* ascending id */] }
//! ```
//!
//! A per-object encoding of the item table would be about 15 MB; this one is about 3 MB. Rows are
//! serialised **by position**, decoded **by column name**, so adding a column is non-breaking: an unknown
//! column is ignored and a missing one takes the type's default (`#[serde(default)]` at the container, or
//! an `Option` field). `columns` is written first (decoding needs it before `rows`).
//!
//! `T` must serialise as a struct with the same fields in every row (no `flatten`, no
//! `skip_serializing_if`: a skipped field is written as `null` to keep the columns aligned).

use serde::de::{self, DeserializeSeed, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde::ser::{self, Impossible, SerializeSeq, SerializeStruct};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::marker::PhantomData;

/// A columnar table of rows of type `T`.
#[derive(Clone, Debug, PartialEq)]
pub struct RowTable<T> {
    /// The written columns: the field names of `T` in declaration order, minus the excluded ones.
    pub columns: Vec<String>,
    /// The rows in the table's order.
    pub rows: Vec<T>,
}

impl<T> Default for RowTable<T> {
    fn default() -> Self {
        Self {
            columns: Vec::new(),
            rows: Vec::new(),
        }
    }
}

impl<T> RowTable<T> {
    /// The number of rows.
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// `true` when the table has no row.
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

impl<T: Serialize> RowTable<T> {
    /// A table over `rows` with every field of `T` as a column.
    pub fn new(rows: Vec<T>) -> Self {
        Self::without(rows, &[])
    }

    /// A table over `rows` without the fields named in `exclude` (the bundle never carries IDS keys or
    /// server-only blocks, Plan/14 §10.1). An empty table has no columns.
    pub fn without(rows: Vec<T>, exclude: &[&str]) -> Self {
        let columns = rows
            .first()
            .and_then(|r| field_names(r).ok())
            .unwrap_or_default()
            .into_iter()
            .filter(|n| !exclude.contains(&n.as_str()))
            .collect();
        Self { columns, rows }
    }
}

/// The serialised field names of a struct value, in declaration order.
pub fn field_names<T: Serialize + ?Sized>(value: &T) -> Result<Vec<String>, String> {
    let mut names = Vec::new();
    value
        .serialize(ProbeSer { names: &mut names })
        .map_err(|e| e.0)?;
    Ok(names)
}

/// The serde field names of `T` in declaration order, from `deserialize_struct` (no instance needed).
/// `None` when `T` does not deserialize as a struct.
pub fn deserialize_fields<'de, T: Deserialize<'de>>() -> Option<&'static [&'static str]> {
    struct Probe;
    #[derive(Debug)]
    struct Found(&'static [&'static str]);
    impl fmt::Display for Found {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("probe")
        }
    }
    impl std::error::Error for Found {}
    impl de::Error for Found {
        fn custom<M: fmt::Display>(_: M) -> Self {
            Found(&[])
        }
    }
    impl<'de> Deserializer<'de> for Probe {
        type Error = Found;
        fn deserialize_any<V: Visitor<'de>>(self, _: V) -> Result<V::Value, Found> {
            Err(Found(&[]))
        }
        fn deserialize_struct<V: Visitor<'de>>(
            self,
            _name: &'static str,
            fields: &'static [&'static str],
            _v: V,
        ) -> Result<V::Value, Found> {
            Err(Found(fields))
        }
        serde::forward_to_deserialize_any! {
            bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string bytes byte_buf option unit
            unit_struct newtype_struct seq tuple tuple_struct map enum identifier ignored_any
        }
    }
    match T::deserialize(Probe) {
        Err(Found(f)) if !f.is_empty() => Some(f),
        _ => None,
    }
}

// ---- serialisation ----------------------------------------------------------------------------

#[derive(Debug)]
struct SerErr(String);
impl fmt::Display for SerErr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for SerErr {}
impl ser::Error for SerErr {
    fn custom<M: fmt::Display>(m: M) -> Self {
        SerErr(m.to_string())
    }
}

fn not_a_struct<E: ser::Error>() -> E {
    E::custom("a RowTable row must serialise as a struct")
}

/// Generates the `Serializer` methods that a row never uses.
macro_rules! unsupported_methods {
    ($ok:ty, $err:ty) => {
        fn serialize_bool(self, _: bool) -> Result<$ok, $err> {
            Err(not_a_struct())
        }
        fn serialize_i8(self, _: i8) -> Result<$ok, $err> {
            Err(not_a_struct())
        }
        fn serialize_i16(self, _: i16) -> Result<$ok, $err> {
            Err(not_a_struct())
        }
        fn serialize_i32(self, _: i32) -> Result<$ok, $err> {
            Err(not_a_struct())
        }
        fn serialize_i64(self, _: i64) -> Result<$ok, $err> {
            Err(not_a_struct())
        }
        fn serialize_u8(self, _: u8) -> Result<$ok, $err> {
            Err(not_a_struct())
        }
        fn serialize_u16(self, _: u16) -> Result<$ok, $err> {
            Err(not_a_struct())
        }
        fn serialize_u32(self, _: u32) -> Result<$ok, $err> {
            Err(not_a_struct())
        }
        fn serialize_u64(self, _: u64) -> Result<$ok, $err> {
            Err(not_a_struct())
        }
        fn serialize_f32(self, _: f32) -> Result<$ok, $err> {
            Err(not_a_struct())
        }
        fn serialize_f64(self, _: f64) -> Result<$ok, $err> {
            Err(not_a_struct())
        }
        fn serialize_char(self, _: char) -> Result<$ok, $err> {
            Err(not_a_struct())
        }
        fn serialize_str(self, _: &str) -> Result<$ok, $err> {
            Err(not_a_struct())
        }
        fn serialize_bytes(self, _: &[u8]) -> Result<$ok, $err> {
            Err(not_a_struct())
        }
        fn serialize_none(self) -> Result<$ok, $err> {
            Err(not_a_struct())
        }
        fn serialize_some<V: ?Sized + Serialize>(self, _: &V) -> Result<$ok, $err> {
            Err(not_a_struct())
        }
        fn serialize_unit(self) -> Result<$ok, $err> {
            Err(not_a_struct())
        }
        fn serialize_unit_struct(self, _: &'static str) -> Result<$ok, $err> {
            Err(not_a_struct())
        }
        fn serialize_unit_variant(
            self,
            _: &'static str,
            _: u32,
            _: &'static str,
        ) -> Result<$ok, $err> {
            Err(not_a_struct())
        }
        fn serialize_newtype_struct<V: ?Sized + Serialize>(
            self,
            _: &'static str,
            _: &V,
        ) -> Result<$ok, $err> {
            Err(not_a_struct())
        }
        fn serialize_newtype_variant<V: ?Sized + Serialize>(
            self,
            _: &'static str,
            _: u32,
            _: &'static str,
            _: &V,
        ) -> Result<$ok, $err> {
            Err(not_a_struct())
        }
        fn serialize_seq(self, _: Option<usize>) -> Result<Self::SerializeSeq, $err> {
            Err(not_a_struct())
        }
        fn serialize_tuple(self, _: usize) -> Result<Self::SerializeTuple, $err> {
            Err(not_a_struct())
        }
        fn serialize_tuple_struct(
            self,
            _: &'static str,
            _: usize,
        ) -> Result<Self::SerializeTupleStruct, $err> {
            Err(not_a_struct())
        }
        fn serialize_tuple_variant(
            self,
            _: &'static str,
            _: u32,
            _: &'static str,
            _: usize,
        ) -> Result<Self::SerializeTupleVariant, $err> {
            Err(not_a_struct())
        }
        fn serialize_map(self, _: Option<usize>) -> Result<Self::SerializeMap, $err> {
            Err(not_a_struct())
        }
        fn serialize_struct_variant(
            self,
            _: &'static str,
            _: u32,
            _: &'static str,
            _: usize,
        ) -> Result<Self::SerializeStructVariant, $err> {
            Err(not_a_struct())
        }
    };
}

/// A serializer that records the field names of a struct and ignores the values.
struct ProbeSer<'a> {
    names: &'a mut Vec<String>,
}

struct ProbeStruct<'a> {
    names: &'a mut Vec<String>,
}

impl SerializeStruct for ProbeStruct<'_> {
    type Ok = ();
    type Error = SerErr;
    fn serialize_field<V: ?Sized + Serialize>(
        &mut self,
        key: &'static str,
        _: &V,
    ) -> Result<(), SerErr> {
        self.names.push(key.to_owned());
        Ok(())
    }
    fn skip_field(&mut self, key: &'static str) -> Result<(), SerErr> {
        self.names.push(key.to_owned());
        Ok(())
    }
    fn end(self) -> Result<(), SerErr> {
        Ok(())
    }
}

impl<'a> Serializer for ProbeSer<'a> {
    type Ok = ();
    type Error = SerErr;
    type SerializeSeq = Impossible<(), SerErr>;
    type SerializeTuple = Impossible<(), SerErr>;
    type SerializeTupleStruct = Impossible<(), SerErr>;
    type SerializeTupleVariant = Impossible<(), SerErr>;
    type SerializeMap = Impossible<(), SerErr>;
    type SerializeStruct = ProbeStruct<'a>;
    type SerializeStructVariant = Impossible<(), SerErr>;
    fn serialize_struct(self, _: &'static str, _: usize) -> Result<ProbeStruct<'a>, SerErr> {
        Ok(ProbeStruct { names: self.names })
    }
    unsupported_methods!((), SerErr);
}

/// Serialises a struct value as a JSON-style array of its (masked) field values.
struct RowSer<'m, S: Serializer> {
    inner: S,
    mask: &'m [bool],
}

struct RowStruct<'m, Q: SerializeSeq> {
    seq: Q,
    mask: &'m [bool],
    index: usize,
}

impl<Q: SerializeSeq> SerializeStruct for RowStruct<'_, Q> {
    type Ok = Q::Ok;
    type Error = Q::Error;
    fn serialize_field<V: ?Sized + Serialize>(
        &mut self,
        _: &'static str,
        value: &V,
    ) -> Result<(), Q::Error> {
        if self.mask.get(self.index).copied().unwrap_or(false) {
            self.seq.serialize_element(value)?;
        }
        self.index += 1;
        Ok(())
    }
    fn skip_field(&mut self, _: &'static str) -> Result<(), Q::Error> {
        if self.mask.get(self.index).copied().unwrap_or(false) {
            self.seq.serialize_element(&Option::<()>::None)?;
        }
        self.index += 1;
        Ok(())
    }
    fn end(self) -> Result<Q::Ok, Q::Error> {
        self.seq.end()
    }
}

impl<'m, S: Serializer> Serializer for RowSer<'m, S> {
    type Ok = S::Ok;
    type Error = S::Error;
    type SerializeSeq = Impossible<S::Ok, S::Error>;
    type SerializeTuple = Impossible<S::Ok, S::Error>;
    type SerializeTupleStruct = Impossible<S::Ok, S::Error>;
    type SerializeTupleVariant = Impossible<S::Ok, S::Error>;
    type SerializeMap = Impossible<S::Ok, S::Error>;
    type SerializeStruct = RowStruct<'m, S::SerializeSeq>;
    type SerializeStructVariant = Impossible<S::Ok, S::Error>;
    fn serialize_struct(
        self,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeStruct, S::Error> {
        let kept = self.mask.iter().filter(|k| **k).count();
        Ok(RowStruct {
            seq: self.inner.serialize_seq(Some(kept))?,
            mask: self.mask,
            index: 0,
        })
    }
    unsupported_methods!(S::Ok, S::Error);
}

/// One row as an array.
struct RowValue<'a, T> {
    row: &'a T,
    mask: &'a [bool],
}

impl<T: Serialize> Serialize for RowValue<'_, T> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.row.serialize(RowSer {
            inner: s,
            mask: self.mask,
        })
    }
}

impl<T: Serialize> Serialize for RowTable<T> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        // the keep-mask: which declared fields are columns (every row has the same layout)
        let mask: Vec<bool> = match self.rows.first() {
            None => Vec::new(),
            Some(first) => field_names(first)
                .map_err(<S::Error as ser::Error>::custom)?
                .iter()
                .map(|n| self.columns.iter().any(|c| c == n))
                .collect(),
        };
        let mut st = s.serialize_struct("RowTable", 2)?;
        st.serialize_field("columns", &self.columns)?;
        st.serialize_field(
            "rows",
            &Rows {
                rows: &self.rows,
                mask: &mask,
            },
        )?;
        st.end()
    }
}

struct Rows<'a, T> {
    rows: &'a [T],
    mask: &'a [bool],
}

impl<T: Serialize> Serialize for Rows<'_, T> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut seq = s.serialize_seq(Some(self.rows.len()))?;
        for row in self.rows {
            seq.serialize_element(&RowValue {
                row,
                mask: self.mask,
            })?;
        }
        seq.end()
    }
}

// ---- deserialisation --------------------------------------------------------------------------

/// A `MapAccess` that pairs the column names with the cells of one row (a sequence).
struct ColumnMap<'c, A> {
    columns: std::slice::Iter<'c, String>,
    seq: A,
    pending: bool,
}

impl<'de, A: SeqAccess<'de>> MapAccess<'de> for ColumnMap<'_, A> {
    type Error = A::Error;
    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, A::Error> {
        match self.columns.next() {
            None => Ok(None),
            Some(name) => {
                self.pending = true;
                seed.deserialize(de::value::StrDeserializer::<A::Error>::new(name))
                    .map(Some)
            }
        }
    }
    fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value, A::Error> {
        if !self.pending {
            return Err(de::Error::custom("value without a key"));
        }
        self.pending = false;
        match self.seq.next_element_seed(seed)? {
            Some(v) => Ok(v),
            None => Err(de::Error::custom("row is shorter than its columns")),
        }
    }
}

/// The `Deserializer` that hands a row to `T::deserialize` as a map.
struct RowDe<'c, A> {
    columns: &'c [String],
    seq: A,
}

impl<'de, A: SeqAccess<'de>> Deserializer<'de> for RowDe<'_, A> {
    type Error = A::Error;
    fn deserialize_any<V: Visitor<'de>>(self, v: V) -> Result<V::Value, A::Error> {
        v.visit_map(ColumnMap {
            columns: self.columns.iter(),
            seq: self.seq,
            pending: false,
        })
    }
    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string bytes byte_buf option unit
        unit_struct newtype_struct seq tuple tuple_struct map struct enum identifier ignored_any
    }
}

struct RowSeed<'c, T> {
    columns: &'c [String],
    marker: PhantomData<T>,
}

impl<'de, T: Deserialize<'de>> DeserializeSeed<'de> for RowSeed<'_, T> {
    type Value = T;
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<T, D::Error> {
        struct V<'c, T> {
            columns: &'c [String],
            marker: PhantomData<T>,
        }
        impl<'de, T: Deserialize<'de>> Visitor<'de> for V<'_, T> {
            type Value = T;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a row array")
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<T, A::Error> {
                // `RowDe` borrows the sequence mutably through a wrapper so that cells beyond the
                // known columns can be drained afterwards
                let value = T::deserialize(RowDe {
                    columns: self.columns,
                    seq: &mut seq,
                })?;
                while seq.next_element::<IgnoredAny>()?.is_some() {}
                Ok(value)
            }
        }
        d.deserialize_seq(V {
            columns: self.columns,
            marker: PhantomData,
        })
    }
}

struct RowsSeed<'c, T> {
    columns: &'c [String],
    marker: PhantomData<T>,
}

impl<'de, T: Deserialize<'de>> DeserializeSeed<'de> for RowsSeed<'_, T> {
    type Value = Vec<T>;
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<Vec<T>, D::Error> {
        struct V<'c, T> {
            columns: &'c [String],
            marker: PhantomData<T>,
        }
        impl<'de, T: Deserialize<'de>> Visitor<'de> for V<'_, T> {
            type Value = Vec<T>;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("an array of rows")
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Vec<T>, A::Error> {
                let mut out = Vec::with_capacity(seq.size_hint().unwrap_or(0).min(1 << 20));
                while let Some(row) = seq.next_element_seed(RowSeed {
                    columns: self.columns,
                    marker: PhantomData,
                })? {
                    out.push(row);
                }
                Ok(out)
            }
        }
        d.deserialize_seq(V {
            columns: self.columns,
            marker: PhantomData,
        })
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for RowTable<T> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V<T>(PhantomData<T>);
        impl<'de, T: Deserialize<'de>> Visitor<'de> for V<T> {
            type Value = RowTable<T>;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a RowTable {columns, rows}")
            }
            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<RowTable<T>, M::Error> {
                let mut columns: Option<Vec<String>> = None;
                let mut rows: Option<Vec<T>> = None;
                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "columns" => columns = Some(map.next_value()?),
                        "rows" => {
                            let cols = columns.as_deref().ok_or_else(|| {
                                de::Error::custom("RowTable: `columns` must come before `rows`")
                            })?;
                            rows = Some(map.next_value_seed(RowsSeed {
                                columns: cols,
                                marker: PhantomData,
                            })?);
                        }
                        _ => {
                            map.next_value::<IgnoredAny>()?;
                        }
                    }
                }
                Ok(RowTable {
                    columns: columns.ok_or_else(|| de::Error::missing_field("columns"))?,
                    rows: rows.ok_or_else(|| de::Error::missing_field("rows"))?,
                })
            }
        }
        d.deserialize_struct("RowTable", &["columns", "rows"], V(PhantomData))
    }
}
