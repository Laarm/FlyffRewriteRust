//! Buffs and the database (Plan/17 §7.9, 13 §6.9, §17.3-§17.4; `buff.cpp:94-160`): the snapshot into the
//! `buffs` document and the load, which caps the remaining time of common buffs at 60 minutes, drops the
//! buffs of rows that no longer exist, strips the angel buffs of a PK-less server (KQ-17) and leaves every
//! effect for the first process (`inst = None`).
//!
//! `BUFF_ITEM2` is the only kind with a wall-clock expiry: its document record carries the absolute time, the
//! entry's `level` holds it (unix seconds). Every other duration pauses while the player is offline.

use super::core::{BuffCx, capped_remaining, process, remove_real};
use super::set::{BuffEntry, BuffKey, BuffKind, BuffSet};
use crate::item::fx::Fx;
use crate::mover::Mover;
use flyff_common::GameMs;
use flyff_common::defs::itemkind::IK3_ANGEL_BUFF;
use flyff_data::GameData;
use flyff_db::BuffsDoc;
use flyff_db::model::docs::BuffRecord;

/// The `buffs` document of a mover: every buff except `BUFF_EQUIP` and the housing buffs (13 §17.3 rule 5), a
/// buff that is only waiting for its erasure, with `remaining_ms` as of `now`.
pub fn snapshot(m: &Mover, now: GameMs) -> BuffsDoc {
    BuffsDoc {
        buffs: m
            .buffs
            .iter()
            .filter(|(_, e)| !e.volatile && !e.removed)
            .map(|(key, e)| {
                let item2 = key.kind == BuffKind::Item2;
                BuffRecord {
                    kind: key.kind.as_u8(),
                    id: key.id,
                    level: if item2 { 0 } else { e.level },
                    remaining_ms: e.snapshot_ms(now),
                    expires_at: item2.then_some(i64::from(e.level)),
                }
            })
            .collect(),
        ..BuffsDoc::default()
    }
}

/// Why a stored record was not loaded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dropped {
    /// The kind is not one of the four stored kinds.
    UnknownKind,
    /// The skill or item row (or the add-skill row of the level) no longer exists.
    NoProp,
}

/// What loading a document did, for the audit line and the tests.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LoadReport {
    /// Records dropped, with the reason.
    pub dropped: Vec<(BuffKey, Dropped)>,
    /// Records whose remaining time was capped at 60 minutes.
    pub capped: Vec<BuffKey>,
}

/// The buff set of a stored document (`CBuffMgr::Serialize` load): every entry has `inst = None`.
/// An empty skill or item table (a build without game data) keeps every record.
pub fn load(doc: &BuffsDoc, data: &GameData) -> (BuffSet, LoadReport) {
    let mut set = BuffSet::new();
    let mut report = LoadReport::default();
    let check_items = !data.items.is_empty();
    let check_skills = !data.skills.is_empty();
    for r in &doc.buffs {
        let Some(kind) = BuffKind::from_u8(r.kind) else {
            report.dropped.push((
                BuffKey::new(BuffKind::Item, u32::from(r.id)),
                Dropped::UnknownKind,
            ));
            continue;
        };
        if kind == BuffKind::Equip {
            report
                .dropped
                .push((BuffKey::new(kind, u32::from(r.id)), Dropped::UnknownKind));
            continue;
        }
        let key = BuffKey::new(kind, u32::from(r.id));
        let known = match kind {
            BuffKind::Skill => {
                !check_skills || super::core::skill_props(data, u32::from(r.id), r.level).is_some()
            }
            BuffKind::Item | BuffKind::Item2 => {
                !check_items || data.item(u32::from(r.id)).is_some()
            }
            BuffKind::Pet | BuffKind::Equip => true,
        };
        if !known {
            report.dropped.push((key, Dropped::NoProp));
            continue;
        }
        let total = capped_remaining(data, key, r.remaining_ms);
        if total != r.remaining_ms {
            report.capped.push(key);
        }
        let level = if kind == BuffKind::Item2 {
            u32::try_from(r.expires_at.unwrap_or(0).max(0)).unwrap_or(u32::MAX)
        } else {
            r.level
        };
        let housing = kind == BuffKind::Item
            && data
                .item(u32::from(r.id))
                .is_some_and(|p| p.item_kind1 == flyff_common::defs::itemkind::IK1_HOUSING);
        set.insert_raw(
            key,
            BuffEntry {
                level,
                total_ms: total,
                inst: None,
                attacker: None,
                removed: false,
                volatile: housing,
            },
        );
    }
    (set, report)
}

/// KQ-17: with `events.pk = false` the angel-buff items are refused, and the buffs of such items are stripped
/// at login. Returns the removed keys.
pub fn strip_angel_buffs(m: &mut Mover, cx: &BuffCx<'_>, fx: &mut Fx) -> Vec<BuffKey> {
    let hits: Vec<BuffKey> = m
        .buffs
        .iter()
        .filter(|(key, _)| {
            key.kind == BuffKind::Item
                && cx
                    .data()
                    .item(u32::from(key.id))
                    .is_some_and(|p| p.item_kind3 == IK3_ANGEL_BUFF)
        })
        .map(|(k, _)| k)
        .collect();
    for k in &hits {
        remove_real(m, cx, *k, fx);
    }
    hits
}

/// The buff part of a join, before the mover is spawned: the KQ-17 strip when `events.pk` is off, then the
/// first process, which applies the effects of every loaded buff **without messages** (the spawn snapshot
/// carries the DSTs) and removes the ones whose time ran out offline (`BUFF_ITEM2`). The messages of the
/// returned [`Fx`] are not wanted (nobody sees the mover yet); its follow-ups are (`BUFF_TOGIFT` of an expired
/// gift buff): run them after the join batch.
pub fn apply_at_join(m: &mut Mover, cx: &BuffCx<'_>, events_pk: bool) -> Fx {
    let mut fx = Fx::new();
    if !events_pk {
        strip_angel_buffs(m, cx, &mut fx);
    }
    process(m, cx, &mut fx);
    fx
}
