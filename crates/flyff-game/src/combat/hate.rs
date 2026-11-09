//! The hate list `m_idEnemies` (Plan/17 §5.16; `Mover.cpp:5105-5245`): the **single** enemy list of a mover,
//! read by the kill EXP (doc 16), the AI (doc 18) and the drop owner (doc 19).
//!
//! Legacy `std::map<OBJID, HIT_INFO>` iterates in **ascending `ObjId`** order, never in insertion order, and
//! the sum of hits wraps like the C `int`.

use super::effects::{Arena, Effects, SimFx};
use crate::ai::AiMsg;
use flyff_common::{GameMs, ObjId};
use std::collections::BTreeMap;

/// `HIT_INFO{int nHit, DWORD dwTick}`: what one enemy did to (or, for the attacker's mirror entry, received
/// from) a mover.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HateEntry {
    /// `nHit`: the summed damage (wraps).
    pub hits: i32,
    /// `dwTick`: when the last hit landed. Stored for parity with the `__VER < 8` PK sessions, which are not
    /// compiled in V15; never used for a decision.
    pub last_hit: GameMs,
}

/// The stand-in name of WP-MOVER-02.
pub type HitInfo = HateEntry;

/// `m_idEnemies` (see the module documentation).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HateList {
    map: BTreeMap<ObjId, HateEntry>,
}

impl HateList {
    /// `AddEnemy(obj, dmg)`: a new enemy is stored as `{dmg, now}` and the answer is 0; an existing one gets
    /// `last_hit = now` and `hits = hits.wrapping_add(dmg)` and the answer is the previous `last_hit`.
    pub fn add(&mut self, obj: ObjId, dmg: i32, now: GameMs) -> GameMs {
        match self.map.get_mut(&obj) {
            Some(e) => {
                let old = e.last_hit;
                e.last_hit = now;
                e.hits = e.hits.wrapping_add(dmg);
                old
            }
            None => {
                self.map.insert(
                    obj,
                    HateEntry {
                        hits: dmg,
                        last_hit: now,
                    },
                );
                GameMs(0)
            }
        }
    }

    /// The stand-in call of WP-MOVER-02 (one hit of 1 damage), kept for the callers that predate the arbiter.
    pub fn add_hit(&mut self, attacker: ObjId, now: GameMs) {
        self.add(attacker, 1, now);
    }

    /// `GetEnemyHit(obj)`: the summed damage of `obj`, 0 when absent.
    pub fn hits(&self, obj: ObjId) -> i32 {
        self.map.get(&obj).map_or(0, |e| e.hits)
    }

    /// The entry of `obj`.
    pub fn get(&self, obj: ObjId) -> Option<&HateEntry> {
        self.map.get(&obj)
    }

    /// `true` when `obj` is in the list.
    pub fn contains(&self, obj: ObjId) -> bool {
        self.map.contains_key(&obj)
    }

    /// `GetEnemyCount()`.
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// `true` when nobody is listed.
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// `GetMaxEnemyHitID()`: scanned in ascending `ObjId`, an entry replaces the best only if `best_hits <
    /// hits` (strict), so a tie keeps the lowest id and entries with `hits <= 0` (the attacker's own mirror
    /// entries) are never chosen; `None` (legacy `NULL_ID`) when nothing qualifies.
    pub fn max_hate(&self) -> Option<ObjId> {
        let mut best = None;
        let mut best_hits = 0_i32;
        for (id, e) in &self.map {
            if best_hits < e.hits {
                best = Some(*id);
                best_hits = e.hits;
            }
        }
        best
    }

    /// The enemies in ascending `ObjId` order.
    pub fn iter(&self) -> impl Iterator<Item = (ObjId, &HateEntry)> {
        self.map.iter().map(|(id, e)| (*id, e))
    }

    /// Forgets everybody.
    pub fn clear(&mut self) {
        self.map.clear();
    }

    /// Removes one entry; `true` when it was there.
    pub fn remove(&mut self, obj: ObjId) -> bool {
        self.map.remove(&obj).is_some()
    }

    /// The ids in ascending order (a copy).
    pub fn ids(&self) -> Vec<ObjId> {
        self.map.keys().copied().collect()
    }
}

/// `CMover::RemoveEnemy(obj)` (`Mover.cpp:5170-5204`) on mover `m`: if `obj` is in `m`'s list, `m`'s melee hit
/// target is cleared when it was `obj`, `m`'s AI receives `DstDie{dead: obj}` (queued through the AI mailbox),
/// `m`'s destination object is cleared (and `OBJMSG_STAND` sent) when it was `obj`, and the entry is erased.
pub fn remove_enemy(w: &mut Arena<'_>, fx: &mut Effects, m: ObjId, obj: ObjId) {
    let Some(mv) = w.store.mover_mut(m) else {
        return;
    };
    if !mv.combat.hate.contains(obj) {
        return;
    }
    if mv.sim.act.objid_hit == obj {
        mv.sim.act.objid_hit = ObjId::NONE;
    }
    fx.mail.push((m, AiMsg::DstDie { dead: obj }));
    if mv.sim.dest.obj == obj {
        mv.sim.clear_dest_obj();
        fx.sim.push((m, SimFx::Stand));
    }
    mv.combat.hate.remove(obj);
}

/// `CMover::RemoveAllEnemies()` (`Mover.cpp:5206-5222`) on mover `m`: the list is cleared, then every enemy
/// that is still valid drops `m` through [`remove_enemy`] — walking the old keys **from the back (descending
/// `ObjId`)**, so the `DstDie` messages arrive in descending order (KEEP).
pub fn remove_all_enemies(w: &mut Arena<'_>, fx: &mut Effects, m: ObjId) {
    let Some(mv) = w.store.mover_mut(m) else {
        return;
    };
    let enemies = mv.combat.hate.ids();
    mv.combat.hate.clear();
    for enemy in enemies.into_iter().rev() {
        if w.store.mover(enemy).is_some_and(|e| !e.core.deleted) {
            remove_enemy(w, fx, enemy, m);
        }
    }
}
