//! `BuffSet`: the buffs of one mover (`CBuffMgr` / `IBuff`, Plan/17 §7.1-§7.2; `src:_Common/buff.cpp:662-828`).
//!
//! This file is the pure container: the key order of the original map, `remain`, and the add / refresh / replace /
//! 28-cap rules of `AddBuff` as a planner ([`BuffSet::plan_add`]) plus the mutations that carry the plan out. The
//! effects of a buff (DST application and exact revert, messages, expiry actions) belong to [`super::core`]; the
//! container knows nothing of props, movers or messages, so the property tests of the cap and the eviction can
//! run on it alone.

use crate::buff::{BUFF_EQUIP, BUFF_ITEM, BUFF_ITEM2, BUFF_PET, BUFF_SKILL};
use flyff_common::{GameMs, ObjId, UnixTime};
use flyff_rules::buff::MAX_SKILL_BUFFS;
use std::collections::BTreeMap;

/// `BUFF_*`: the kind of a buff.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum BuffKind {
    /// `BUFF_ITEM`: an item prop applied as a timed effect.
    Item = 0,
    /// `BUFF_SKILL`: a skill at a level.
    Skill = 1,
    /// `BUFF_PET`: the pet stat bonus, `level = MAKELONG(adj, dst)`.
    Pet = 2,
    /// `BUFF_ITEM2`: a time-limited cash item, `level` = the absolute expiry in unix seconds.
    Item2 = 3,
    /// `BUFF_EQUIP`: the display buff of a gift-box item (`DST_GIFTBOX`), never stored.
    Equip = 4,
}

impl BuffKind {
    /// The wire / document value.
    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    /// The kind of a wire / document value.
    pub fn from_u8(v: u8) -> Option<BuffKind> {
        match v {
            0 => Some(BuffKind::Item),
            1 => Some(BuffKind::Skill),
            2 => Some(BuffKind::Pet),
            3 => Some(BuffKind::Item2),
            4 => Some(BuffKind::Equip),
            _ => None,
        }
    }

    /// The kind of a legacy `WORD` (see the `BUFF_*` constants).
    pub fn from_u16(v: u16) -> Option<BuffKind> {
        u8::try_from(v).ok().and_then(BuffKind::from_u8)
    }
}

/// The map key of a buff: `MAKELONG(id, type)`, so the order is kind first, then id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BuffKey {
    /// The kind.
    pub kind: BuffKind,
    /// Skill id or item id (a `WORD` in the original; larger ids truncate like the cast did).
    pub id: u16,
}

impl BuffKey {
    /// A key; `id` is truncated to 16 bits like `(WORD)` in the original.
    pub fn new(kind: BuffKind, id: u32) -> BuffKey {
        BuffKey {
            kind,
            id: id as u16,
        }
    }

    /// The legacy key `(type << 16) | id`.
    pub fn legacy(self) -> u32 {
        (u32::from(self.kind.as_u8()) << 16) | u32::from(self.id)
    }
}

/// One buff (`IBuff`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuffEntry {
    /// Skill level; `Pet`: `MAKELONG(adj, dst)`; `Item2`: the absolute expiry (unix s); `Item`: 0.
    pub level: u32,
    /// `tmTotal`: the duration in ms.
    pub total_ms: u32,
    /// `tmInst`: when it started; `None` = loaded from the database, effects not yet applied.
    pub inst: Option<GameMs>,
    /// The caster (`m_oiAttacker`).
    pub attacker: Option<ObjId>,
    /// A "fake" removal: the effects are reverted, the entry is erased by the next process.
    pub removed: bool,
    /// Never stored: `BUFF_EQUIP` and the buffs of housing items (13 §6.9).
    pub volatile: bool,
}

impl BuffEntry {
    /// `GetRemain()`: `inst` unset reads 0; an elapsed time beyond the total reads 0.
    pub fn remain_ms(&self, now: GameMs) -> u32 {
        let Some(inst) = self.inst else { return 0 };
        let elapsed = now.0.saturating_sub(inst.0);
        if elapsed > u64::from(self.total_ms) {
            0
        } else {
            self.total_ms - elapsed as u32
        }
    }

    /// The `tmTotal` a snapshot stores (`IBuff::Serialize`): the remaining time, the whole total while the
    /// effects are not applied yet, 0 for a zero total.
    pub fn snapshot_ms(&self, now: GameMs) -> u32 {
        if self.total_ms == 0 {
            return 0;
        }
        match self.inst {
            Some(_) => self.remain_ms(now),
            None => self.total_ms,
        }
    }
}

/// A buff to add (`AddBuff(type, id, level, total, attacker)`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AddRequest {
    /// The key.
    pub key: BuffKey,
    /// The level (see [`BuffEntry::level`]).
    pub level: u32,
    /// The requested duration in ms.
    pub total_ms: u32,
    /// The caster.
    pub attacker: Option<ObjId>,
    /// The entry is never stored (housing items; `BUFF_EQUIP` always is).
    pub volatile: bool,
}

/// What [`BuffSet::plan_add`] decided.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AddPlan {
    /// `IsAddable` said no (a negative total, or a hostile skill on a player in its invulnerability window).
    Refused,
    /// An equal-level buff exists: its total becomes `max(new, remain)` (`AddTotal`), `inst = now`, the effects
    /// stay; `SetSkillState` goes out with the new values; the call answers "not new".
    Refresh,
    /// A buff of a higher level exists: nothing changes, `SetSkillState` goes out with the new values; "not new".
    IgnoreLower,
    /// The buff is inserted: first the lower-level buff of the same key (real removal) and the evicted
    /// beneficial skill buff, if any, are removed for real, in this order.
    Insert {
        /// The lower-level buff of the same key (`Overwrite`: `RemoveBuff(old, FALSE)`).
        replace: Option<BuffKey>,
        /// The lowest-key beneficial skill buff when 28 skill buffs are present (`PrepareBS`).
        evict: Option<BuffKey>,
    },
}

/// The buffs of one mover, ordered by key.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BuffSet {
    map: BTreeMap<BuffKey, BuffEntry>,
}

impl BuffSet {
    /// An empty set.
    pub fn new() -> BuffSet {
        BuffSet::default()
    }

    /// Number of entries (fake-removed ones included, like `m_mapBuffs.size()`).
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// `true` without entries.
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// The entry of `key`.
    pub fn get(&self, key: BuffKey) -> Option<&BuffEntry> {
        self.map.get(&key)
    }

    /// The entry of `key`, mutably.
    pub fn get_mut(&mut self, key: BuffKey) -> Option<&mut BuffEntry> {
        self.map.get_mut(&key)
    }

    /// `HasBuff(type, id)`: queries do not filter fake-removed entries (17 §7.13).
    pub fn contains(&self, key: BuffKey) -> bool {
        self.map.contains_key(&key)
    }

    /// `HasBuff(wType, wId)` with the legacy integer types (`BUFF_*` constants); an unknown kind is never held.
    pub fn has(&self, kind: u16, id: u32) -> bool {
        BuffKind::from_u16(kind).is_some_and(|k| self.contains(BuffKey::new(k, id)))
    }

    /// The entries in key order.
    pub fn iter(&self) -> impl Iterator<Item = (BuffKey, &BuffEntry)> + '_ {
        self.map.iter().map(|(k, e)| (*k, e))
    }

    /// The keys in order (a snapshot, so the caller may mutate while walking).
    pub fn keys(&self) -> Vec<BuffKey> {
        self.map.keys().copied().collect()
    }

    /// The number of skill buffs (fake-removed ones included: `IsBSFull` counts the map).
    pub fn skill_count(&self) -> usize {
        self.map
            .keys()
            .filter(|k| k.kind == BuffKind::Skill)
            .count()
    }

    /// `GetRemain()` of `key`; 0 for a missing buff.
    pub fn remain_ms(&self, key: BuffKey, now: GameMs) -> u32 {
        self.get(key).map_or(0, |e| e.remain_ms(now))
    }

    /// Drops every entry (the disconnect path and tests; no effects are reverted here).
    pub fn clear(&mut self) {
        self.map.clear();
    }

    /// Inserts a loaded entry as it stood (`CBuffMgr::Serialize` load, fixtures): `inst = None` means "apply
    /// on the first process". A key that exists is replaced.
    pub fn insert_raw(&mut self, key: BuffKey, entry: BuffEntry) {
        self.map.insert(key, entry);
    }

    /// Inserts an entry started `started` with the legacy integer types (fixtures of other modules' tests and
    /// the spawn snapshots). `expire_at` is `BUFF_ITEM2`'s absolute expiry.
    pub fn push(&mut self, b: Buff) {
        let Some(kind) = BuffKind::from_u16(b.kind) else {
            return;
        };
        let level = match (kind, b.expire_at) {
            (BuffKind::Item2, Some(t)) => u32::try_from(t.0.max(0)).unwrap_or(u32::MAX),
            _ => b.level,
        };
        self.map.insert(
            BuffKey::new(kind, b.id),
            BuffEntry {
                level,
                total_ms: b.total_ms,
                inst: Some(b.started),
                attacker: None,
                removed: false,
                volatile: kind == BuffKind::Equip,
            },
        );
    }

    /// `CBuffMgr::AddBuff(IBuff*)` as a plan: `addable` is `IsAddable` (decided with the props), `is_good(key)`
    /// answers "beneficial skill buff" (`evildoing >= 0`, for `GetFirstBS`).
    pub fn plan_add(
        &self,
        req: &AddRequest,
        addable: bool,
        is_good: impl Fn(BuffKey) -> bool,
    ) -> AddPlan {
        if !addable || i32::try_from(req.total_ms).is_err() {
            return AddPlan::Refused;
        }
        let mut replace = None;
        if let Some(old) = self.map.get(&req.key) {
            match old.level.cmp(&req.level) {
                std::cmp::Ordering::Equal => return AddPlan::Refresh,
                std::cmp::Ordering::Greater => return AddPlan::IgnoreLower,
                std::cmp::Ordering::Less => replace = Some(req.key),
            }
        }
        // PrepareBS: the 28 cap counts the map as it is after the replaced buff went away
        let mut evict = None;
        if req.key.kind == BuffKind::Skill {
            let count = self
                .map
                .keys()
                .filter(|k| k.kind == BuffKind::Skill && Some(**k) != replace)
                .count();
            if count >= MAX_SKILL_BUFFS {
                evict = self
                    .map
                    .keys()
                    .find(|k| k.kind == BuffKind::Skill && Some(**k) != replace && is_good(**k))
                    .copied();
            }
        }
        AddPlan::Insert { replace, evict }
    }

    /// `AddTotal` of an existing buff: `max(total, remain)`, or `total + remain` for the ids that accumulate
    /// (`add_remaining`: the pet feed pocket 2 and the pet tonics), then `inst = now`. The effects are not
    /// re-applied.
    pub fn refresh(&mut self, key: BuffKey, total_ms: u32, add_remaining: bool, now: GameMs) {
        let Some(e) = self.map.get_mut(&key) else {
            return;
        };
        let remain = e.remain_ms(now);
        let wanted = if add_remaining {
            total_ms.wrapping_add(remain)
        } else {
            total_ms
        };
        e.total_ms = wanted.max(remain);
        e.inst = Some(now);
    }

    /// Inserts the new buff (`inst = now`).
    pub fn insert(&mut self, req: &AddRequest, now: GameMs) {
        self.map.insert(
            req.key,
            BuffEntry {
                level: req.level,
                total_ms: req.total_ms,
                inst: Some(now),
                attacker: req.attacker,
                removed: false,
                volatile: req.volatile || req.key.kind == BuffKind::Equip,
            },
        );
    }

    /// Erases an entry (the real removal's last step).
    pub fn erase(&mut self, key: BuffKey) -> Option<BuffEntry> {
        self.map.remove(&key)
    }

    /// Marks an entry removed (a "fake" removal); `true` when it was not marked before.
    pub fn mark_removed(&mut self, key: BuffKey) -> bool {
        match self.map.get_mut(&key) {
            Some(e) if !e.removed => {
                e.removed = true;
                true
            }
            _ => false,
        }
    }
}

/// A buff as the older code and the fixtures wrote it: legacy integer kinds, the start time, the absolute
/// expiry of `BUFF_ITEM2`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Buff {
    /// `BUFF_*` as a `u16` (see [`BUFF_ITEM`], [`BUFF_SKILL`], [`BUFF_PET`], [`BUFF_ITEM2`], [`BUFF_EQUIP`]).
    pub kind: u16,
    /// Skill id or item id.
    pub id: u32,
    /// Skill level (ignored for `BUFF_ITEM2`).
    pub level: u32,
    /// When the buff started.
    pub started: GameMs,
    /// Total duration in ms.
    pub total_ms: u32,
    /// `BUFF_ITEM2` only: the absolute expiry (unix seconds).
    pub expire_at: Option<UnixTime>,
}

impl Buff {
    /// The remaining time at `now` (`total - (now - started)`, clamped to `>= 0`).
    pub fn remain_ms(&self, now: GameMs) -> u32 {
        let elapsed = now.0.saturating_sub(self.started.0);
        u64::from(self.total_ms).saturating_sub(elapsed) as u32
    }
}

const _: () = {
    // the legacy constants and the enum agree
    assert!(BUFF_ITEM == BuffKind::Item as u16);
    assert!(BUFF_SKILL == BuffKind::Skill as u16);
    assert!(BUFF_PET == BuffKind::Pet as u16);
    assert!(BUFF_ITEM2 == BuffKind::Item2 as u16);
    assert!(BUFF_EQUIP == BuffKind::Equip as u16);
};
