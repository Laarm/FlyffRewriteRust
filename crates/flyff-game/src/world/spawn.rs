//! Spawn serialization and the encode-once cache (Plan/15 §7, WP-WORLD-06).
//!
//! Message selection (15 §7.1): a player gets `SpawnSelf` of itself (only the join, the world change and the
//! re-attach produce it, never `add_to_view`), `SpawnPlayer` of every other player, `SpawnNpc` of NPCs, monsters
//! and pets, `SpawnItem` of ground items, `SpawnCtrl` of ctrls; `DelObj` is the header alone. `ServerMsg.obj_id`
//! is the object's `ObjId` in every case.
//!
//! The "others" variant of an object is the same for every receiver, so a [`SpawnCache`] keeps the encoded
//! message of one phase of one layer (cleared at the phase boundaries, 15 §7.3): ten players entering an NPC's
//! window in one `_modifylink` cause one encode. [`SpawnWriter`] implements the [`SpawnSink`] of the visibility
//! code over the `Outbox`.
//!
//! Fields of later milestones (vendor title, casting item, duel, cheer, murderer, ...) carry their documented
//! defaults until their owners land (`mover::wire`).

use super::object::{ObjKind, ObjRef, ObjectStore};
use super::visibility::{Phase, SpawnSink};
use crate::mover::wire::{
    add_game_join, mover_common, npc_part, obj_common, player_other, player_public, player_self,
    spawn_buffs,
};
use crate::outbox::Outbox;
use crate::session::SessionTable;
use flyff_common::det::DetMap;
use flyff_common::{GameMs, ObjId, SimFrame};
use flyff_proto::{EncodedMsg, pb};

/// Counters of the encode-once cache (15 §7.3, tests and `/metrics`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SpawnCacheStats {
    /// Spawn messages encoded (cache misses).
    pub encodes: u64,
    /// Spawns served from the cache.
    pub hits: u64,
    /// `DelObj` messages sent.
    pub despawns: u64,
}

/// `ObjId -> EncodedMsg` of the "others" spawn of one phase of one layer.
#[derive(Debug, Default)]
pub struct SpawnCache {
    map: DetMap<ObjId, EncodedMsg>,
}

impl SpawnCache {
    /// Forgets every entry (the end of a phase).
    pub fn clear(&mut self) {
        self.map.clear();
    }

    /// The cached message of `id`, built by `build` on a miss. `None` when `build` yields nothing.
    pub fn get_or_encode(
        &mut self,
        id: ObjId,
        stats: &mut SpawnCacheStats,
        build: impl FnOnce() -> Option<EncodedMsg>,
    ) -> Option<EncodedMsg> {
        if let Some(m) = self.map.get(&id) {
            stats.hits += 1;
            return Some(m.clone());
        }
        let m = build()?;
        stats.encodes += 1;
        self.map.insert(id, m.clone());
        Some(m)
    }

    /// Number of cached messages.
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// `true` when nothing is cached.
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

// ---- message builders ---------------------------------------------------------------------------------------

/// `SpawnSelf` of a player for itself: the common header, the public and the private part and the buffs
/// (15 §7.1, ref/08 §7.5). `None` when `player` is not a player.
pub fn spawn_self(store: &ObjectStore, player: ObjId, now: GameMs) -> Option<pb::SpawnSelf> {
    let m = store.mover(player).filter(|m| m.is_player())?;
    Some(pb::SpawnSelf {
        obj: Some(obj_common(&m.core)),
        mover: Some(mover_common(m)),
        player: player_public(m, now),
        self_: player_self(m, now),
        buffs: spawn_buffs(m, now),
    })
}

/// `SpawnPlayer` of a player for every other player (ref/08 §7.5.4).
pub fn spawn_player(store: &ObjectStore, player: ObjId, now: GameMs) -> Option<pb::SpawnPlayer> {
    let m = store.mover(player).filter(|m| m.is_player())?;
    Some(pb::SpawnPlayer {
        obj: Some(obj_common(&m.core)),
        mover: Some(mover_common(m)),
        player: player_public(m, now),
        other: player_other(m),
        buffs: spawn_buffs(m, now),
    })
}

/// `SpawnNpc` of an NPC, monster or pet (ref/08 §7.5.5).
pub fn spawn_npc(store: &ObjectStore, id: ObjId, now: GameMs) -> Option<pb::SpawnNpc> {
    let m = store.mover(id).filter(|m| !m.is_player())?;
    Some(pb::SpawnNpc {
        obj: Some(obj_common(&m.core)),
        mover: Some(mover_common(m)),
        npc: npc_part(m),
        buffs: spawn_buffs(m, now),
    })
}

/// `SpawnItem` of a ground item (ref/08 §7.4; loot rights are not sent, `item.obj_id` is 0).
pub fn spawn_item(store: &ObjectStore, id: ObjId) -> Option<pb::SpawnItem> {
    let d = store.item(id)?;
    let mut item = crate::item::wire::item_elem(&d.item);
    item.obj_id = 0;
    if let Some(gold) = d.meta.gold {
        // a ground gold seed (19 §2.1): the amount travels as `hit_point`, the count is 1 (SIMPLIFY: the original
        // wrote the amount into the `short` count as well, which wrapped above 32 767 and was never displayed)
        item.hit_point = gold;
        item.num = 1;
    }
    Some(pb::SpawnItem {
        obj: Some(obj_common(&d.core)),
        item: Some(item),
    })
}

/// `SpawnCtrl` of a ctrl (the `CObj` prefix and the id, ref/08 §7.3).
pub fn spawn_ctrl(store: &ObjectStore, id: ObjId) -> Option<pb::SpawnCtrl> {
    let c = store.ctrl(id)?;
    Some(pb::SpawnCtrl {
        obj: Some(obj_common(&c.core)),
    })
}

/// The encoded "others" spawn of object `x` as it goes to every viewer that is not the object itself.
pub fn encode_others(store: &ObjectStore, x: ObjId, now: GameMs) -> Option<EncodedMsg> {
    match store.lookup(x)? {
        ObjRef::Mover(_) if store.is_player(x) => {
            spawn_player(store, x, now).map(|m| EncodedMsg::new(x.0, m))
        }
        ObjRef::Mover(_) => spawn_npc(store, x, now).map(|m| EncodedMsg::new(x.0, m)),
        ObjRef::Item(_) => spawn_item(store, x).map(|m| EncodedMsg::new(x.0, m)),
        ObjRef::Ctrl(_) => spawn_ctrl(store, x).map(|m| EncodedMsg::new(x.0, m)),
    }
}

/// `AddGameJoin` of a player (join batch step 10), re-exported next to the other join builders.
pub fn join_add_game_join(store: &ObjectStore, player: ObjId) -> Option<pb::AddGameJoin> {
    Some(add_game_join(store.mover(player)?.player()?))
}

/// The object kind check used by the sink (`SpawnPlayer` for players, `SpawnNpc` for the other movers).
pub fn kind_of(store: &ObjectStore, x: ObjId) -> Option<ObjKind> {
    store.core(x).map(|c| c.kind)
}

// ---- the sink over the Outbox ----------------------------------------------------------------------------------

/// The [`SpawnSink`] of the game thread: pushes the cached "others" spawns and the `DelObj`s into the sessions
/// of the receiving players through the `Outbox` (15 §7.3, §8.1). A player without a bound session (lingering,
/// not yet bound) receives nothing; the outbox drops pushes for slots without a sink.
pub struct SpawnWriter<'a> {
    outbox: &'a mut Outbox,
    sessions: &'a SessionTable,
    cache: SpawnCache,
    stats: &'a mut SpawnCacheStats,
    now: GameMs,
    flush_frame: SimFrame,
    /// The frame an effect right now takes (`4n` before the frames, `4n + 4` after): the stamp of the spawn
    /// replay (18 §5.4).
    replay_frame: SimFrame,
    max_batch: usize,
    report: crate::outbox::FlushReport,
}

impl<'a> SpawnWriter<'a> {
    /// A writer for one tick: `now` stamps the buffs' remaining time, `flush_frame` (`4n + 4`) and `max_batch`
    /// parameterise the urgent flush after each `_replace`.
    pub fn new(
        outbox: &'a mut Outbox,
        sessions: &'a SessionTable,
        stats: &'a mut SpawnCacheStats,
        now: GameMs,
        flush_frame: SimFrame,
        max_batch: usize,
    ) -> SpawnWriter<'a> {
        SpawnWriter {
            outbox,
            sessions,
            cache: SpawnCache::default(),
            stats,
            now,
            flush_frame,
            replay_frame: SimFrame(flush_frame.0.saturating_sub(4)),
            max_batch,
            report: crate::outbox::FlushReport::default(),
        }
    }

    /// What the urgent flushes did so far (the runtime hands it to `on_flush_report`).
    pub fn take_report(&mut self) -> crate::outbox::FlushReport {
        std::mem::take(&mut self.report)
    }

    fn session_of(&self, p: ObjId) -> Option<flyff_common::SessionId> {
        self.sessions.get(p).and_then(|e| e.session)
    }
}

impl SpawnSink for SpawnWriter<'_> {
    fn spawn(&mut self, store: &ObjectStore, x: ObjId, p: ObjId) {
        debug_assert!(x != p, "add_to_view never sends a player its own spawn");
        let Some(sid) = self.session_of(p) else {
            return;
        };
        let now = self.now;
        let msg = self
            .cache
            .get_or_encode(x, self.stats, || encode_others(store, x, now));
        if let Some(m) = msg {
            self.outbox.push_encoded(sid, &m);
        }
        // what the spawned NPC is doing right now (18 §5.4, 31 §16.3 rule 8): in the same batch, only for `p`
        if let Some(mv) = store.mover(x) {
            for r in crate::movement::intents::replay_messages(mv, self.replay_frame) {
                self.outbox.push_encoded(sid, &r);
            }
        }
    }

    fn despawn(&mut self, x: ObjId, p: ObjId) {
        let Some(sid) = self.session_of(p) else {
            return;
        };
        self.stats.despawns += 1;
        self.outbox.to_session(sid, x.0, pb::DelObj {});
    }

    fn phase_begin(&mut self, phase: Phase) {
        self.cache.clear();
        // `_add` runs before the frames of the tick (effects are `4n`), every later phase after them (`4n + 4`)
        let before_frames = matches!(phase, Phase::Add | Phase::Process);
        self.replay_frame = if before_frames {
            SimFrame(self.flush_frame.0.saturating_sub(4))
        } else {
            self.flush_frame
        };
    }

    fn phase_end(&mut self, _phase: Phase) {
        self.cache.clear();
    }

    fn urgent_flush(&mut self) {
        let r = self.outbox.flush_urgent(self.flush_frame, self.max_batch);
        self.report.merge(r);
    }

    fn relay(&mut self, to: ObjId, msg: &EncodedMsg) {
        if let Some(sid) = self.session_of(to) {
            self.outbox.push_encoded(sid, msg);
        }
    }

    fn spawn_self(&mut self, store: &ObjectStore, p: ObjId) {
        let Some(sid) = self.session_of(p) else {
            return;
        };
        if let Some(m) = spawn_self(store, p, self.now) {
            self.outbox.to_session(sid, p.0, m);
        }
    }

    fn urgent(&mut self, p: ObjId) {
        if let Some(sid) = self.session_of(p) {
            self.outbox.mark_urgent(sid);
        }
    }
}
