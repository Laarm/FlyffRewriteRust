//! Visibility: who sees whom (Plan/15 §6.2-§6.3, WP-WORLD-03).
//!
//! Ports of `CLinkMap::AddItToView` and `ModifyView` (`src:WORLDSERVER/LinkMap.cpp:205-646`) and
//! `CCtrl::RemoveItFromView` (`_Common/Ctrl.cpp:166-201`) over the 2-D clamped windows of [`super::grid`]:
//!
//! * `core.viewers` (legacy `m_2pc`) holds the players that have the object spawned; for a linked player it
//!   includes the player itself;
//! * `player.view.seen` (legacy `m_2npc`) holds the non-player objects the player has spawned.
//!
//! Visibility is purely cell based (no distance test), symmetric between players and per layer. Invariants
//! V1-V5 (15 §6.2) hold after every `_add`, `_modifylink` and `_delete` phase.
//!
//! Messages are not built here: every viewer link that is created calls [`SpawnSink::spawn`] and every one
//! that is removed [`SpawnSink::despawn`] (WP-WORLD-06 implements the sink over the `Outbox`; tests use the
//! [`RecordingSink`]). The message order inside a call follows the original: **players** (enter, then leave),
//! **then the other objects** (enter, then leave); cells in scan order (rows `z` ascending, then `x`), inside a
//! cell the legacy link-list order.

use super::grid::{CellRect, Grid};
use super::object::{CellIdx, LinkState, ObjectStore};
use flyff_common::ObjId;

/// The phases of a layer in a tick (15 §5.3), also used by the spawn cache and the recording hooks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Phase {
    /// `_add`.
    Add,
    /// `_process`.
    Process,
    /// `_modifylink`.
    ModifyLink,
    /// `_OnDie`.
    OnDie,
    /// `_replace`.
    Replace,
    /// `_delete`.
    Delete,
}

/// Receives the spawn and despawn of every viewer link (15 §6.3).
pub trait SpawnSink {
    /// `Spawn(x) -> p`: append the spawn message of object `x` to the session of player `p`. `store` is the
    /// state after the link was recorded; the sink must not assume any other object's links.
    fn spawn(&mut self, store: &ObjectStore, x: ObjId, p: ObjId);

    /// `DelObj(x) -> p`.
    fn despawn(&mut self, x: ObjId, p: ObjId);

    /// A phase of a layer starts; the encode-once cache of one phase and layer begins here (15 §7.3).
    fn phase_begin(&mut self, _phase: Phase) {}

    /// The phase ends; the cache is cleared.
    fn phase_end(&mut self, _phase: Phase) {}

    /// The fixed urgent-flush point right after each layer's `_replace` (15 §8.1, 10 §4.4).
    fn urgent_flush(&mut self) {}

    /// A message of the per-frame pipeline (movement intents, relays, keyframes: Plan/18 §3.3) for player `to`;
    /// `msg` is encoded once for all recipients. The default drops it.
    fn relay(&mut self, _to: ObjId, _msg: &flyff_proto::EncodedMsg) {}

    /// `SpawnSelf` of player `p` for `p` itself (15 §7.1, §10.4 step 7: a world change; the join and the
    /// re-attach write theirs in phase 1). The default drops it.
    fn spawn_self(&mut self, _store: &ObjectStore, _p: ObjId) {}

    /// Marks the session of player `p` urgent: its queue leaves at the next urgent flush point (`SetPos` to
    /// self, `Replace`; 15 §10.3-§10.4). The default does nothing.
    fn urgent(&mut self, _p: ObjId) {}
}

/// One event of the [`RecordingSink`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpawnEvent {
    /// `Spawn(x) -> p`.
    Spawn {
        /// The spawned object.
        x: ObjId,
        /// The receiving player.
        p: ObjId,
    },
    /// `DelObj(x) -> p`.
    Despawn {
        /// The removed object.
        x: ObjId,
        /// The receiving player.
        p: ObjId,
    },
    /// `Replace` to `p`: the client dropped everything it knew (a world or layer change, 15 §10.4).
    Reset {
        /// The player that changed world or layer.
        p: ObjId,
    },
}

/// A [`SpawnSink`] that records the events (unit tests of the grid and the visibility, doc tests).
#[derive(Clone, Debug, Default)]
pub struct RecordingSink {
    /// The events in call order.
    pub events: Vec<SpawnEvent>,
}

impl RecordingSink {
    /// Takes the recorded events.
    pub fn take(&mut self) -> Vec<SpawnEvent> {
        std::mem::take(&mut self.events)
    }
}

impl SpawnSink for RecordingSink {
    fn spawn(&mut self, _store: &ObjectStore, x: ObjId, p: ObjId) {
        self.events.push(SpawnEvent::Spawn { x, p });
    }

    fn despawn(&mut self, x: ObjId, p: ObjId) {
        self.events.push(SpawnEvent::Despawn { x, p });
    }

    fn relay(&mut self, to: ObjId, msg: &flyff_proto::EncodedMsg) {
        // a `Replace` wipes the player's client: recorded so that tests can follow the visible sets
        if let Ok(b) = flyff_proto::decode_batch(msg.as_bytes())
            && b.msgs
                .iter()
                .any(|m| matches!(m.msg, Some(flyff_proto::pb::server_msg::Msg::Replace(_))))
        {
            self.events.push(SpawnEvent::Reset { p: to });
        }
    }
}

/// The linked cell of an object (`None` when it is not linked).
pub(crate) fn linked_cell(store: &ObjectStore, id: ObjId) -> Option<CellIdx> {
    match store.core(id)?.link {
        LinkState::Linked { cell } => Some(cell),
        _ => None,
    }
}

fn deleted(store: &ObjectStore, id: ObjId) -> bool {
    store.core(id).is_none_or(|c| c.deleted)
}

/// Adds `viewer` to `x.viewers`; `true` when it was not there.
fn viewers_insert(store: &mut ObjectStore, x: ObjId, viewer: ObjId) -> bool {
    store.core_mut(x).is_some_and(|c| c.viewers.insert(viewer))
}

/// Removes `viewer` from `x.viewers`; `true` when it was there.
fn viewers_remove(store: &mut ObjectStore, x: ObjId, viewer: ObjId) -> bool {
    store
        .core_mut(x)
        .is_some_and(|c| c.viewers.shift_remove(&viewer))
}

/// Adds `x` to `player.view.seen`.
fn seen_insert(store: &mut ObjectStore, player: ObjId, x: ObjId) -> bool {
    store
        .mover_mut(player)
        .and_then(|m| m.player_mut())
        .is_some_and(|p| p.view.seen.insert(x))
}

/// Removes `x` from `player.view.seen`.
fn seen_remove(store: &mut ObjectStore, player: ObjId, x: ObjId) -> bool {
    store
        .mover_mut(player)
        .and_then(|m| m.player_mut())
        .is_some_and(|p| p.view.seen.shift_remove(&x))
}

/// The link of two players: `U.viewers += P; Spawn(P) -> U; P.viewers += U; Spawn(U) -> P`.
fn link_players(store: &mut ObjectStore, sink: &mut dyn SpawnSink, p: ObjId, u: ObjId) {
    if viewers_insert(store, u, p) {
        sink.spawn(store, p, u);
    }
    if viewers_insert(store, p, u) {
        sink.spawn(store, u, p);
    }
}

/// The unlink of two players (both directions, `DelObj` each way).
fn unlink_players(store: &mut ObjectStore, sink: &mut dyn SpawnSink, p: ObjId, u: ObjId) {
    if viewers_remove(store, p, u) {
        sink.despawn(u, p);
    }
    if viewers_remove(store, u, p) {
        sink.despawn(p, u);
    }
}

/// Collects the players / others of the cells of `rect` that are not in `skip` (scan order).
fn collect(grid: &Grid, rect: CellRect, skip: Option<CellRect>, players: bool) -> Vec<ObjId> {
    let mut out = Vec::new();
    grid.for_each_cell(rect, |c, cell| {
        if skip.is_some_and(|s| s.contains(c.x, c.z)) {
            return;
        }
        if players {
            out.extend(cell.players.iter().copied());
        } else {
            out.extend(cell.others.iter().copied());
        }
    });
    out
}

/// `AddItToView`: the object was just linked into the grid (15 §6.3).
///
/// A player and every player of its window see each other (a player never receives its own spawn), and the
/// player sees every non-player object of the window; a non-player object is shown to every player of its
/// window. Objects flagged for deletion are skipped (their own destroy handles them).
pub fn add_to_view(grid: &Grid, store: &mut ObjectStore, sink: &mut dyn SpawnSink, id: ObjId) {
    let Some(cell) = linked_cell(store, id) else {
        return;
    };
    let rect = grid.window(cell);
    if store.is_player(id) {
        for u in collect(grid, rect, None, true) {
            if u != id && !deleted(store, u) {
                link_players(store, sink, id, u);
            }
        }
        for c in collect(grid, rect, None, false) {
            if !deleted(store, c) {
                viewers_insert(store, c, id);
                if seen_insert(store, id, c) {
                    sink.spawn(store, c, id);
                }
            }
        }
    } else {
        for u in collect(grid, rect, None, true) {
            if !deleted(store, u) {
                seen_insert(store, u, id);
                if viewers_insert(store, id, u) {
                    sink.spawn(store, id, u);
                }
            }
        }
    }
}

/// `ModifyView`: the object moved from cell `old` to cell `new` (already re-linked in the grid; `old != new`).
///
/// Players: the players of the cells that entered the window are linked both ways, then the players of the
/// cells that left are unlinked both ways, then the same for the other objects. Non-player objects: the
/// players of the cells that entered the window get the spawn, those of the cells that left the `DelObj`.
pub fn modify_view(
    grid: &Grid,
    store: &mut ObjectStore,
    sink: &mut dyn SpawnSink,
    id: ObjId,
    old: CellIdx,
    new: CellIdx,
) {
    if old == new {
        return;
    }
    let (old_r, new_r) = (grid.window(old), grid.window(new));
    if store.is_player(id) {
        for u in collect(grid, new_r, Some(old_r), true) {
            if u != id && !deleted(store, u) {
                link_players(store, sink, id, u);
            }
        }
        for u in collect(grid, old_r, Some(new_r), true) {
            if u != id && !deleted(store, u) {
                unlink_players(store, sink, id, u);
            }
        }
        for c in collect(grid, new_r, Some(old_r), false) {
            if !deleted(store, c) {
                viewers_insert(store, c, id);
                if seen_insert(store, id, c) {
                    sink.spawn(store, c, id);
                }
            }
        }
        for c in collect(grid, old_r, Some(new_r), false) {
            if !deleted(store, c) {
                viewers_remove(store, c, id);
                if seen_remove(store, id, c) {
                    sink.despawn(c, id);
                }
            }
        }
    } else {
        for u in collect(grid, new_r, Some(old_r), true) {
            if !deleted(store, u) {
                seen_insert(store, u, id);
                if viewers_insert(store, id, u) {
                    sink.spawn(store, id, u);
                }
            }
        }
        for u in collect(grid, old_r, Some(new_r), true) {
            if !deleted(store, u) {
                seen_remove(store, u, id);
                if viewers_remove(store, id, u) {
                    sink.despawn(id, u);
                }
            }
        }
    }
}

/// `RemoveItFromView` (+ `RemoveItFromView2` for players): called when the object is destroyed, after it left
/// the grid.
///
/// A player sends `DelObj` of itself to every other viewer and leaves the `seen` set of everything it saw
/// (no message); another object sends its `DelObj` to every viewer. Both sets are emptied.
pub fn remove_from_view(store: &mut ObjectStore, sink: &mut dyn SpawnSink, id: ObjId) {
    let is_player = store.is_player(id);
    let viewers: Vec<ObjId> = store
        .core(id)
        .map(|c| c.viewers.iter().copied().collect())
        .unwrap_or_default();
    if is_player {
        for u in viewers {
            if u != id && viewers_remove(store, u, id) {
                sink.despawn(id, u);
            }
        }
        let seen: Vec<ObjId> = store
            .mover(id)
            .and_then(|m| m.player())
            .map(|p| p.view.seen.iter().copied().collect())
            .unwrap_or_default();
        for c in seen {
            viewers_remove(store, c, id);
        }
        if let Some(p) = store.mover_mut(id).and_then(|m| m.player_mut()) {
            p.view.seen.clear();
        }
    } else {
        for u in viewers {
            seen_remove(store, u, id);
            sink.despawn(id, u);
        }
    }
    if let Some(c) = store.core_mut(id) {
        c.viewers.clear();
    }
}

/// `resend_view` (re-attach, 15 §9.3): rebuilds a fresh client from the server-side sets: the spawn of every
/// other viewer, then of everything the player has seen. The sets do not change.
pub fn resend_view(store: &ObjectStore, sink: &mut dyn SpawnSink, player: ObjId) {
    let viewers: Vec<ObjId> = store
        .core(player)
        .map(|c| c.viewers.iter().copied().collect())
        .unwrap_or_default();
    for u in viewers {
        if u != player {
            sink.spawn(store, u, player);
        }
    }
    let seen: Vec<ObjId> = store
        .mover(player)
        .and_then(|m| m.player())
        .map(|p| p.view.seen.iter().copied().collect())
        .unwrap_or_default();
    for c in seen {
        sink.spawn(store, c, player);
    }
}
