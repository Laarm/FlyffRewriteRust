//! The DPS meter (Plan/17 §10; WP-COMBAT-14; `__DPS_METER`, `src:WORLDSERVER/AttackArbiter.cpp:234-270, 853-865`,
//! `src:_Common/Mover.cpp:4030-4050`, `src:WORLDSERVER/User.cpp:9137-9166`).
//!
//! A tracked monster (`combat.dps` is `Some`) is a `RANK_BOSS` or `RANK_SUPER` monster, or one listed in
//! `DPSProp.txt` (`GameData.dps_monsters`, empty here). The mark is set when the respawner or an instance spawn
//! creates it ([`mark_if_tracked`], [`DpsMarker`]) and only with `features.dps_meter`.
//!
//! * a **hit** by a player ([`record_hit`], the damage pipeline's step 9): the player is listed if new (insertion
//!   order), every listed online player gets `DpsUpdate{mode 2, show = true}`, the attacker's damage grows;
//! * **every tick** of the monster ([`tick_mover`], the mover tail of the combat consumers): players that left
//!   the world or are 64 m or more away are removed (the latter get `DpsUpdate{mode 2, show = false}`); when the
//!   list or a value changed the remaining players get `DpsUpdate{mode 0, list}` (the original sent it every
//!   tick; mode 1 is never sent);
//! * the **death** ([`close`], step 1 of `OnDied`): every listed player gets `show = false`.

use super::effects::{Arena, Effects};
use super::hooks::KillCtx;
use super::state::{DpsEntry, DpsTrack, is_live};
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::mover::Mover;
use crate::world::object::MoverKey;
use crate::world::phases::FrameCtx;
use crate::world::respawn::RespawnHooks;
use flyff_common::ObjId;
use flyff_common::defs::attribute::{RANK_BOSS, RANK_SUPER};
use flyff_data::GameData;
use flyff_proto::{EncodedMsg, pb};

/// The meter lists only players within this distance of the monster (`IsValidArea( pUser, 64.0f )`, strict).
pub const DPS_RANGE: f32 = 64.0;
/// The one `RANK_SUPER` monster an instance dungeon does not track (`dwID != 1627`).
pub const INSTANCE_UNTRACKED: u32 = 1627;

/// Whether a monster of class `class` and prop id `mover_id` is tracked: a boss, a super monster, or one listed in
/// `dps_monsters`.
pub fn is_tracked(class: u32, mover_id: u32, dps_monsters: &[u32]) -> bool {
    class == RANK_BOSS || class == RANK_SUPER || dps_monsters.contains(&mover_id)
}

/// `combat::dps::mark_if_tracked` (17 §10) for a freshly created monster of the respawner (class boss or super,
/// or listed): starts its meter. Instance spawns use [`mark_instance_spawn`].
pub fn mark_if_tracked(m: &mut Mover, dps_monsters: &[u32]) {
    let Some(class) = m.monster().map(|e| e.class) else {
        return;
    };
    if is_tracked(class, m.prop_id(), dps_monsters) && m.combat.dps.is_none() {
        m.combat.dps = Some(DpsTrack::default());
    }
}

/// The instance-dungeon rule of `CInstanceDungeonBase::CreateMonster`: a listed monster, a boss, or a super
/// monster other than mover 1627.
pub fn mark_instance_spawn(m: &mut Mover, dps_monsters: &[u32]) {
    let Some(class) = m.monster().map(|e| e.class) else {
        return;
    };
    let id = m.prop_id();
    if (dps_monsters.contains(&id)
        || class == RANK_BOSS
        || (class == RANK_SUPER && id != INSTANCE_UNTRACKED))
        && m.combat.dps.is_none()
    {
        m.combat.dps = Some(DpsTrack::default());
    }
}

/// The respawner's marker (`RespawnHooks::mark_if_tracked`): installed by `Game::new` with the monsters of
/// `DPSProp.txt` and `features.dps_meter`.
#[derive(Clone, Debug, Default)]
pub struct DpsMarker {
    enabled: bool,
    listed: Vec<u32>,
}

impl DpsMarker {
    /// The marker for `data` and the switch `features.dps_meter`.
    pub fn new(data: &GameData, enabled: bool) -> DpsMarker {
        DpsMarker {
            enabled,
            listed: data.dps_monsters.clone(),
        }
    }
}

impl RespawnHooks for DpsMarker {
    fn mark_if_tracked(&mut self, m: &mut Mover) {
        if self.enabled {
            mark_if_tracked(m, &self.listed);
        }
    }
}

/// The wire message of mode 2.
fn show_msg(show: bool) -> pb::DpsUpdate {
    pb::DpsUpdate {
        sub: Some(pb::dps_update::Sub::Show(pb::dps_update::DpsShow { show })),
    }
}

/// The wire message of mode 0: `(id, name, nDPS)` of every listed player in insertion order.
fn list_msg(t: &DpsTrack) -> pb::DpsUpdate {
    pb::DpsUpdate {
        sub: Some(pb::dps_update::Sub::Dps(pb::dps_update::DpsList {
            entries: t
                .players
                .iter()
                .map(|e| pb::dps_update::DpsEntry {
                    id_player: e.id_player,
                    name: e.name.clone(),
                    value: e.dps,
                })
                .collect(),
        })),
    }
}

/// A live player of the store.
fn valid_player(store: &crate::world::ObjectStore, o: ObjId) -> bool {
    store
        .mover(o)
        .is_some_and(|m| m.is_player() && !m.core.deleted)
}

/// A player hit a tracked monster (step 9 of the damage pipeline): lists the attacker if new, shows the meter to
/// every listed online player and adds `damage` to the attacker. Nothing happens for an untracked victim or an
/// attacker that is not a player.
pub fn record_hit(
    w: &mut Arena<'_>,
    fx: &mut Effects,
    attacker: ObjId,
    victim: ObjId,
    damage: i32,
) {
    let Some((id, name)) = w
        .store
        .mover(attacker)
        .and_then(|m| m.player())
        .map(|p| (p.ident.character_id.0, p.ident.name.clone()))
    else {
        return;
    };
    let Some(track) = w
        .store
        .mover_mut(victim)
        .and_then(|m| m.combat.dps.as_mut())
    else {
        return;
    };
    if !track.players.iter().any(|e| e.id_player == id) {
        track.players.push(DpsEntry {
            id_player: id,
            obj: attacker,
            name,
            dps: 0,
            total: 0,
        });
    }
    if let Some(e) = track.players.iter_mut().find(|e| e.id_player == id) {
        e.dps = e.dps.wrapping_add(damage);
        e.total = e.total.wrapping_add(damage);
    }
    track.dirty = true;
    let listed: Vec<ObjId> = track.players.iter().map(|e| e.obj).collect();
    for o in listed {
        if valid_player(w.store, o) {
            fx.to_self(o, show_msg(true));
        }
    }
}

/// The tick tail of a mover (17 §10): for a live tracked monster, prunes the list and sends the damage list. A
/// listed player that left the world is dropped silently; one 64 m or more away gets `show = false` and is dropped.
pub fn tick_mover(ctx: &mut FrameCtx<'_>, key: MoverKey) {
    let Some(m) = ctx.store.movers.get(key) else {
        return;
    };
    let Some(track) = m.combat.dps.as_ref() else {
        return;
    };
    if track.players.is_empty() || !is_live(m) {
        return;
    }
    let (world, layer, pos) = (m.core.world, m.core.layer, m.core.pos);
    // (listed obj, still a player, within range)
    let verdicts: Vec<(ObjId, bool, bool)> = track
        .players
        .iter()
        .map(|e| {
            let p = ctx
                .store
                .mover(e.obj)
                .filter(|p| p.is_player() && !p.core.deleted);
            let near = p.is_some_and(|p| {
                p.core.world == world
                    && p.core.layer == layer
                    && (p.core.pos - pos).length_squared() < DPS_RANGE * DPS_RANGE
            });
            (e.obj, p.is_some(), near)
        })
        .collect();
    let Some(track) = ctx
        .store
        .movers
        .get_mut(key)
        .and_then(|m| m.combat.dps.as_mut())
    else {
        return;
    };
    let before = track.players.len();
    let mut i = 0;
    for (_, valid, near) in &verdicts {
        if *valid && *near {
            i += 1;
        } else {
            track.players.remove(i);
        }
    }
    if track.players.len() != before {
        track.dirty = true;
    }
    let send_list = std::mem::take(&mut track.dirty);
    let list = send_list.then(|| list_msg(track));
    let remaining: Vec<ObjId> = track.players.iter().map(|e| e.obj).collect();
    for (o, valid, near) in verdicts {
        if valid && !near {
            ctx.sink.relay(o, &EncodedMsg::new(0, show_msg(false)));
        }
    }
    if let Some(l) = list {
        let enc = EncodedMsg::new(0, l);
        for o in remaining {
            ctx.sink.relay(o, &enc);
        }
    }
}

/// Step 1 of `OnDied` (`DeathHooks::dps_close`): the meter of the dead monster closes - every listed player
/// still in the world gets `show = false`.
pub fn close(_svc: &mut Services, ctx: &mut GameCtx<'_>, k: &KillCtx) {
    let Some(track) = ctx
        .worlds
        .mover_mut(k.victim)
        .and_then(|m| m.combat.dps.take())
    else {
        return;
    };
    for e in track.players {
        if ctx.worlds.player(e.obj).is_some() {
            crate::chat::send_to(ctx, PlayerRef(e.obj), show_msg(false));
        }
    }
}
