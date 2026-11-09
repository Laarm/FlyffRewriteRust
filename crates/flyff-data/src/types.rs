//! Id aliases, `NULL_ID` and `IdTable<T>` (Plan/14 §6.1).
//!
//! Ids are plain `u32` aliases. The values are the define values, which are canonical everywhere
//! (protocol, DB, bundles; ref/17 A.5). Field values are stored raw as the C++ read them, so an `=`
//! cell keeps [`NULL_ID`].

use serde::de::{MapAccess, Visitor};
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::marker::PhantomData;

/// `0xFFFFFFFF`: the value of an `=` cell (and of `(DWORD)-1`), Plan/14 §6.1.
pub use flyff_script::NULL_ID;

/// Item id (`II_`, `SI_` skills share the item loader's `ItemProp`, `ST_` party skills).
pub type ItemId = u32;
/// Mover id (`MI_`).
pub type MoverId = u32;
/// Skill id (`SI_`).
pub type SkillId = u32;
/// Add-skill row id (`SA_`).
pub type AddSkillId = u32;
/// Ctrl id (`CI_`).
pub type CtrlId = u32;
/// World id (`WI_`). Plain `u32` here (the contract newtype of `flyff-common` is for runtime ids).
pub type WorldId = u32;
/// Text id of `textClient.inc` (`TID_`).
pub type TextId = u32;
/// Job id (`JOB_`).
pub type JobId = u32;
/// Motion id (`MTI_`).
pub type MotionId = u32;
/// Emote id (`MOT_`).
pub type EmoteId = u32;
/// SFX id (`XI_`).
pub type SfxId = u32;
/// Sound id (`SND_`).
pub type SoundId = u32;
/// Music id (`BGM_`).
pub type MusicId = u32;
/// Destination parameter id (`DST_`).
pub type DstId = u32;
/// Quest id (`QUEST_`).
pub type QuestId = u32;
/// Honor title id (`HONOR_`).
pub type HonorId = u32;
/// Menu id (`MMI_`).
pub type MenuId = u32;
/// Item kind (`IK1_`/`IK2_`/`IK3_`).
pub type ItemKind = u32;

/// A sparse table indexed by id: the replacement of the original's `CFixedArray` (Plan/14 §6.1).
///
/// Insertion overwrites (**last wins**), holes are `None`, iteration is in ascending id order, and
/// [`IdTable::size`] is the original's `GetSize()` (highest id + 1). Lookups are O(1). To keep a
/// corrupt id like `0xFFFFFFFF` from allocating gigabytes, ids above [`IdTable::MAX_ID`] are
/// refused by [`IdTable::insert`].
#[derive(Clone, Debug)]
pub struct IdTable<T> {
    slots: Vec<Option<Box<T>>>,
    ids: Vec<u32>,
}

impl<T> Default for IdTable<T> {
    fn default() -> Self {
        Self {
            slots: Vec::new(),
            ids: Vec::new(),
        }
    }
}

impl<T: PartialEq> PartialEq for IdTable<T> {
    fn eq(&self, other: &Self) -> bool {
        self.ids == other.ids && self.iter().zip(other.iter()).all(|(a, b)| a == b)
    }
}

impl<T> IdTable<T> {
    /// The largest accepted id (4,194,303): 32 MiB of slots at worst. The real data peaks at 224,897.
    pub const MAX_ID: u32 = (1 << 22) - 1;

    /// An empty table.
    pub fn new() -> Self {
        Self::default()
    }

    /// The entry of `id`, `None` for holes and out-of-range ids.
    pub fn get(&self, id: u32) -> Option<&T> {
        self.slots.get(usize::try_from(id).ok()?)?.as_deref()
    }

    /// `true` when `id` has an entry.
    pub fn contains(&self, id: u32) -> bool {
        self.get(id).is_some()
    }

    /// Mutable access. Intended for the load phase (fix-ups that mutate a stored row).
    pub fn get_mut(&mut self, id: u32) -> Option<&mut T> {
        self.slots
            .get_mut(usize::try_from(id).ok()?)?
            .as_deref_mut()
    }

    /// `(id, entry)` in ascending id order.
    pub fn iter(&self) -> impl Iterator<Item = (u32, &T)> + '_ {
        self.ids
            .iter()
            .filter_map(|&id| self.get(id).map(|v| (id, v)))
    }

    /// The ids in ascending order.
    pub fn ids(&self) -> &[u32] {
        &self.ids
    }

    /// The entries in ascending id order.
    pub fn values(&self) -> impl Iterator<Item = &T> + '_ {
        self.iter().map(|(_, v)| v)
    }

    /// Number of entries.
    pub fn len(&self) -> usize {
        self.ids.len()
    }

    /// `true` when there are no entries.
    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    /// The original's `GetSize()`: highest stored id + 1 (0 when empty).
    pub fn size(&self) -> u32 {
        self.ids.last().map_or(0, |m| m.saturating_add(1))
    }

    /// The largest id, if any.
    pub fn max_id(&self) -> Option<u32> {
        self.ids.last().copied()
    }

    /// Inserts `v` at `id` (**last wins**) and returns the previous entry. An id above
    /// [`Self::MAX_ID`] is refused: the value is handed back as `Err`.
    pub fn insert(&mut self, id: u32, v: T) -> Result<Option<T>, T> {
        if id > Self::MAX_ID {
            return Err(v);
        }
        let idx = id as usize;
        if self.slots.len() <= idx {
            self.slots.resize_with(idx + 1, || None);
        }
        let Some(slot) = self.slots.get_mut(idx) else {
            return Err(v);
        };
        let old = slot.replace(Box::new(v));
        if old.is_none() {
            match self.ids.last() {
                Some(&last) if last >= id => {
                    if let Err(pos) = self.ids.binary_search(&id) {
                        self.ids.insert(pos, id);
                    }
                }
                _ => self.ids.push(id),
            }
        }
        Ok(old.map(|b| *b))
    }
}

impl<T: Serialize> Serialize for IdTable<T> {
    /// A map `id -> entry` in ascending id order.
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut m = s.serialize_map(Some(self.len()))?;
        for (id, v) in self.iter() {
            m.serialize_entry(&id, v)?;
        }
        m.end()
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for IdTable<T> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V<T>(PhantomData<T>);
        impl<'de, T: Deserialize<'de>> Visitor<'de> for V<T> {
            type Value = IdTable<T>;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a map from id to entry")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<Self::Value, A::Error> {
                let mut t = IdTable::new();
                while let Some((id, v)) = a.next_entry::<u32, T>()? {
                    if t.insert(id, v).is_err() {
                        return Err(serde::de::Error::custom(format!(
                            "id {id} is above IdTable::MAX_ID"
                        )));
                    }
                }
                Ok(t)
            }
        }
        d.deserialize_map(V(PhantomData))
    }
}
