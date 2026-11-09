//! The world queries (Plan/25 §3.9, §3.10, §3.14, §4.4, §11.5; WP-ADMIN-06): `inspect_object`, `layer_list`,
//! `online_siblings` and `find`, plus the builders the subscriptions share ([`entity_detail`], [`layer_infos`],
//! [`roster_entries`]). Everything here takes `&Game`: nothing can change, and nothing calls a visibility or wake
//! function (a monster that nobody sees stays asleep however often the console looks at it).

use super::AdminQueryResult;
use crate::admin::events::{EntityDetail, HateEntry, LayerInfo, RosterEntry};
use crate::admin::map::{EntityKind, MapFlags};
use crate::ai::MonsterState;
use crate::mover::Mover;
use crate::tick::Game;
use crate::world::{LayerKind, LayerState, ObjKind};
use flyff_common::{AccountId, LayerId, ObjId, WorldId};
use flyff_rules::consts::defs::neuz::AII_PET;
use std::collections::BTreeSet;

/// At most this many hits answer a `Find`.
pub const FIND_LIMIT: usize = 50;
/// The hate list shows the top entries only.
const HATE_TOP: usize = 5;
/// Ticks per second (the respawn countdown is in seconds).
const TICKS_PER_S: u64 = 15;
/// The loot priority of a ground item's owner lasts this long after the drop (19 §6.5).
const LOOT_PRIORITY_MS: u64 = 7_000;

/// One hit of a `Find` (25 §3.9).
#[derive(Clone, Debug, PartialEq)]
pub struct FindHit {
    /// What it is (players, monsters, NPCs).
    pub kind: EntityKind,
    /// The object.
    pub obj_id: u32,
    /// The character id of a player, `0` otherwise.
    pub character_id: u32,
    /// The prop id.
    pub prop: u32,
    /// The display name.
    pub name: String,
    /// World id.
    pub world: u32,
    /// Layer id.
    pub layer: i32,
    /// Position x.
    pub x: f32,
    /// Position z.
    pub z: f32,
    /// Level.
    pub level: u32,
}

/// An online character of an account (`OnlineSiblings`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sibling {
    /// The character.
    pub character_id: u32,
    /// Its avatar.
    pub obj_id: u32,
    /// Its name.
    pub name: String,
    /// `true` while a socket is bound (a lingering avatar has none).
    pub connected: bool,
}

// ---- InspectObject ------------------------------------------------------------------------------------------------

/// `InspectObject`: the [`EntityDetail`] of one object.
pub(super) fn inspect_object(g: &Game, id: ObjId) -> AdminQueryResult {
    match entity_detail(g, id) {
        Some(d) => AdminQueryResult::Object(Box::new(d)),
        None => AdminQueryResult::not_found(format!("object {id} does not exist")),
    }
}

fn clamp_u32(v: i32) -> u32 {
    u32::try_from(v.max(0)).unwrap_or(0)
}

/// The detail of the object `id` (`None` when it does not exist). Shared by `InspectObject` and `EntityWatch`.
pub fn entity_detail(g: &Game, id: ObjId) -> Option<EntityDetail> {
    let store = g.worlds.store();
    let core = store.core(id)?;
    if core.deleted {
        return None;
    }
    let mut d = EntityDetail {
        obj_id: id.0,
        kind: EntityKind::Ctrl,
        prop: core.index,
        name: String::new(),
        world: core.world.0,
        layer: core.layer.0,
        x: core.pos.x,
        y: core.pos.y,
        z: core.pos.z,
        level: 0,
        hp: 0,
        hp_max: 0,
        mp: 0,
        mp_max: 0,
        fp: 0,
        fp_max: 0,
        flags: 0,
        ai_state: String::new(),
        target: 0,
        hate: Vec::new(),
        spawn_region: core.respawn.map_or(0, |r| u32::from(r.gm_index) + 1),
        respawn_tick: respawn_tick(g, core),
        character_id: 0,
        owner: 0,
        mode_flags: 0,
        closed: false,
    };
    match core.kind {
        ObjKind::Mover => {
            let m = store.mover(id)?;
            fill_mover(g, m, &mut d);
        }
        ObjKind::Item => {
            let it = store.item(id)?;
            d.kind = EntityKind::Item;
            d.name = g
                .data
                .item(it.core.index)
                .map(|p| p.name.clone())
                .unwrap_or_default();
            let now = g.clock.game_ms.0;
            let priority = it.meta.owner.is_some()
                && now.saturating_sub(it.meta.drop_time.0) < LOOT_PRIORITY_MS;
            d.owner = it.meta.owner.map_or(0, |o| o.0);
            d.flags = u32::from(
                MapFlags::default()
                    .with(MapFlags::OWNER_ONLY, priority || it.meta.holder.is_some())
                    .0,
            );
        }
        ObjKind::Ctrl => {
            let c = store.ctrl(id)?;
            d.kind = EntityKind::Ctrl;
            d.name = g
                .data
                .ctrls
                .get(c.core.index)
                .map(|p| p.name.clone())
                .unwrap_or_default();
        }
    }
    Some(d)
}

/// The tick of the next refill check of the respawn line an object came from (`0` = it did not come from one).
fn respawn_tick(g: &Game, core: &crate::world::ObjCore) -> u64 {
    let Some(origin) = core.respawn else {
        return 0;
    };
    let Some(state) = g
        .worlds
        .layer(core.world, core.layer)
        .and_then(|l| l.respawner.states.get(usize::from(origin.gm_index)))
    else {
        return 0;
    };
    let secs = u64::try_from(state.countdown.max(0)).unwrap_or(0);
    g.clock
        .tick
        .0
        .saturating_add(secs.saturating_mul(TICKS_PER_S))
}

fn fill_mover(g: &Game, m: &Mover, d: &mut EntityDetail) {
    let stats = g.services.stats.core();
    let flags = MapFlags::of_mover(m);
    d.flags = u32::from(flags.0);
    d.level = clamp_u32(m.mv.level);
    d.hp = clamp_u32(m.mv.hp);
    d.mp = clamp_u32(m.mv.mp);
    d.fp = clamp_u32(m.mv.fp);
    d.hp_max = clamp_u32(crate::stats::env::max_hp(m, stats));
    d.mp_max = clamp_u32(crate::stats::env::max_mp(m, stats));
    d.fp_max = clamp_u32(crate::stats::env::max_fp(m, stats));
    d.mode_flags = m.mv.mode;
    let prop_name = || {
        g.data
            .mover(m.prop_id())
            .map(|p| p.name.clone())
            .unwrap_or_default()
    };
    if let Some(p) = m.player() {
        d.kind = EntityKind::Player;
        d.name.clone_from(&p.ident.name);
        d.character_id = p.ident.character_id.0;
    } else if let Some(n) = m.npc() {
        d.kind = EntityKind::Npc;
        d.name = prop_name();
        if d.name.is_empty() {
            d.name.clone_from(&n.character_key);
        }
    } else if let Some(e) = m.monster() {
        d.kind = if e.ai.kind == AII_PET {
            EntityKind::Pet
        } else {
            EntityKind::Monster
        };
        d.name = prop_name();
        d.owner = e.owner.map_or(0, |o| o.0);
        if let Some(b) = e.ai.monster() {
            d.target = b.target.map_or(0, |t| t.0);
            d.ai_state = ai_state_text(flags, b).to_owned();
        }
        let mut hate: Vec<(ObjId, i32)> = m
            .combat
            .hate
            .iter()
            .map(|(id, entry)| (id, entry.hits))
            .collect();
        // most damage first, ties by the lower object id (the list iterates ascending, the sort is stable)
        hate.sort_by_key(|(_, hits)| std::cmp::Reverse(*hits));
        d.hate = hate
            .into_iter()
            .filter(|(_, hits)| *hits > 0)
            .take(HATE_TOP)
            .map(|(id, hits)| HateEntry {
                obj_id: id.0,
                name: object_name(g, id),
                hate: clamp_u32(hits),
            })
            .collect();
    }
}

/// The AI state as the inspector shows it (25 §3.10).
fn ai_state_text(flags: MapFlags, brain: &crate::ai::MonsterAi) -> &'static str {
    if flags.has(MapFlags::DEAD) {
        return "dead";
    }
    if flags.has(MapFlags::ASLEEP) {
        return "asleep";
    }
    if brain.returning {
        return "return";
    }
    match brain.state {
        MonsterState::Init => "init",
        MonsterState::Idle => "idle",
        MonsterState::Wander => "wander",
        MonsterState::Pursue | MonsterState::RagePatrol => "chase",
        MonsterState::Evade => "evade",
        MonsterState::Runaway => "runaway",
        MonsterState::Rage if brain.go_target => "chase",
        MonsterState::Rage => "attack",
        MonsterState::Stand => "stand",
        MonsterState::Patrol => "patrol",
    }
}

/// A short display name of any object (hate lists, targets).
fn object_name(g: &Game, id: ObjId) -> String {
    let Some(m) = g.worlds.store().mover(id) else {
        return String::new();
    };
    if let Some(p) = m.player() {
        return p.ident.name.clone();
    }
    g.data
        .mover(m.prop_id())
        .map(|p| p.name.clone())
        .unwrap_or_default()
}

// ---- LayerList ----------------------------------------------------------------------------------------------------

/// `LayerList`.
pub(super) fn layer_list(g: &Game, world: Option<WorldId>) -> AdminQueryResult {
    AdminQueryResult::Layers(layer_infos(g, world))
}

/// Every live layer other than layer 0, of `world` or of all worlds, in ascending world and layer order (25 §3.14).
/// The cost is the member count of those layers, nothing for a server without instances.
pub fn layer_infos(g: &Game, world: Option<WorldId>) -> Vec<LayerInfo> {
    layer_infos_counted(g, world).0
}

/// [`layer_infos`] and the number of members visited (what a subscription charges to the visit budget).
pub fn layer_infos_counted(g: &Game, world: Option<WorldId>) -> (Vec<LayerInfo>, u32) {
    let store = g.worlds.store();
    let mut out = Vec::new();
    let mut visited = 0_u32;
    for w in g.worlds.worlds_iter() {
        if world.is_some_and(|only| only != w.id) {
            continue;
        }
        for (lid, layer) in &w.layers {
            if *lid == LayerId(0) || layer.state != LayerState::Live {
                continue;
            }
            let (mut members, mut monsters) = (0_u32, 0_u32);
            for id in layer.members.iter() {
                visited = visited.saturating_add(1);
                let Some(m) = store.mover(*id) else { continue };
                if m.is_player() {
                    members += 1;
                } else if m.monster().is_some_and(|e| e.ai.kind != AII_PET) {
                    monsters += 1;
                }
            }
            let (kind, owner) = match layer.kind {
                LayerKind::Normal => ("normal", String::new()),
                LayerKind::Ticket => ("ticket", String::new()),
                LayerKind::Instance => ("instance", format!("dungeon {}", lid.0)),
                LayerKind::MiniRoom => (
                    "mini_room",
                    u32::try_from(lid.0)
                        .ok()
                        .and_then(|c| g.services.presence.get(flyff_common::CharacterId(c)))
                        .map_or_else(|| format!("character {}", lid.0), |d| d.name.clone()),
                ),
                LayerKind::GuildHouse => ("guild_house", format!("guild {}", lid.0)),
            };
            out.push(LayerInfo {
                world: w.id.0,
                layer: lid.0,
                kind: kind.to_owned(),
                owner,
                members,
                monsters,
                created_tick: 0,
                seconds_left: 0,
            });
        }
    }
    (out, visited)
}

// ---- OnlineSiblings -----------------------------------------------------------------------------------------------

/// `OnlineSiblings`: the characters of the account that are in the world (at most one while invariant I3 holds).
pub(super) fn online_siblings(g: &Game, account: AccountId) -> AdminQueryResult {
    let mut out = Vec::new();
    if let Some(p) = g.sessions.player_of_account(account)
        && let Some(e) = g.sessions.get(p)
    {
        let name = g
            .worlds
            .player(p)
            .and_then(Mover::player)
            .map(|pl| pl.ident.name.clone())
            .or_else(|| g.services.presence.get(e.character).map(|d| d.name.clone()))
            .unwrap_or_else(|| format!("#{}", e.character.0));
        out.push(Sibling {
            character_id: e.character.0,
            obj_id: e.player.0,
            name,
            connected: e.session.is_some(),
        });
    }
    AdminQueryResult::Siblings(out)
}

// ---- Roster ---------------------------------------------------------------------------------------------------------

/// The roster rows of every avatar in the world, ascending by character id (25 §3.16 `RosterEntry`).
pub fn roster_entries(g: &Game) -> Vec<RosterEntry> {
    let mut rows: Vec<RosterEntry> = g
        .sessions
        .iter()
        .map(|e| {
            let dir = g.services.presence.get(e.character);
            let mover = g.worlds.player(e.player);
            let guild = mover
                .and_then(Mover::player)
                .and_then(|p| p.social.guild)
                .map_or(0, |gid| gid.0);
            RosterEntry {
                character_id: e.character.0,
                obj_id: e.player.0,
                // the avatar knows its name even when the directory has not been told about a fresh character
                name: mover
                    .and_then(Mover::player)
                    .map(|p| p.ident.name.clone())
                    .or_else(|| dir.map(|d| d.name.clone()))
                    .unwrap_or_else(|| format!("#{}", e.character.0)),
                level: mover.map_or_else(
                    || dir.map_or(0, |d| u32::from(d.level)),
                    |m| clamp_u32(m.mv.level),
                ),
                job: mover.map_or_else(
                    || dir.map_or(0, |d| u32::from(d.job)),
                    |m| clamp_u32(m.mv.job),
                ),
                world: mover.map_or(0, |m| m.core.world.0),
                layer: mover.map_or(0, |m| m.core.layer.0),
                x: mover.map_or(0.0, |m| m.core.pos.x),
                z: mover.map_or(0.0, |m| m.core.pos.z),
                guild_id: guild,
                authority: u32::from(e.authority.0),
            }
        })
        .collect();
    rows.sort_by_key(|r| r.character_id);
    rows
}

// ---- Find ---------------------------------------------------------------------------------------------------------

/// `Find`: online players by name or character id, monsters and NPCs by name or prop id (25 §3.9). The region and
/// location names of the same box are the server's static data (WP-ADMIN-08).
pub(super) fn find(g: &Game, text: &str) -> AdminQueryResult {
    AdminQueryResult::Find(find_hits(g, text))
}

/// A hit's sort rank: exact name, prefix, the rest.
fn match_rank(name_lower: &str, q: &str) -> Option<u8> {
    if name_lower == q {
        Some(0)
    } else if name_lower.starts_with(q) {
        Some(1)
    } else if name_lower.contains(q) {
        Some(2)
    } else {
        None
    }
}

fn find_hits(g: &Game, text: &str) -> Vec<FindHit> {
    let q = text.trim().to_lowercase();
    if q.is_empty() {
        return Vec::new();
    }
    let number = q.parse::<u32>().ok();
    let store = g.worlds.store();
    // players: name or character id, best match first, then by name
    let mut players: Vec<(u8, String, FindHit)> = Vec::new();
    for e in g.sessions.iter() {
        let Some(m) = store.mover(e.player) else {
            continue;
        };
        let Some(p) = m.player() else { continue };
        let name = p.ident.name.clone();
        let rank = match_rank(&name.to_lowercase(), &q)
            .or_else(|| (number == Some(e.character.0)).then_some(0));
        let Some(rank) = rank else { continue };
        players.push((
            rank,
            name.to_lowercase(),
            FindHit {
                kind: EntityKind::Player,
                obj_id: e.player.0,
                character_id: e.character.0,
                prop: m.prop_id(),
                name,
                world: m.core.world.0,
                layer: m.core.layer.0,
                x: m.core.pos.x,
                z: m.core.pos.z,
                level: clamp_u32(m.mv.level),
            },
        ));
    }
    players.sort_by(|a, b| (a.0, &a.1, a.2.obj_id).cmp(&(b.0, &b.1, b.2.obj_id)));
    let mut out: Vec<FindHit> = players.into_iter().map(|(_, _, h)| h).collect();
    out.truncate(FIND_LIMIT);
    if out.len() >= FIND_LIMIT {
        return out;
    }
    // monsters and NPCs: the props whose name matches (or the prop id), then the live movers of those props
    let props: BTreeSet<u32> = g
        .data
        .movers
        .iter()
        .filter(|(id, p)| number == Some(*id) || match_rank(&p.name.to_lowercase(), &q).is_some())
        .map(|(id, _)| id)
        .collect();
    // an NPC is also found by its `character.inc` key
    let by_key = |m: &Mover| {
        m.npc()
            .is_some_and(|n| !n.character_key.is_empty() && n.character_key.to_lowercase() == q)
    };
    for id in store.ids() {
        if out.len() >= FIND_LIMIT {
            break;
        }
        let Some(m) = store.mover(id) else { continue };
        if m.is_player() || (!props.contains(&m.prop_id()) && !by_key(m)) {
            continue;
        }
        let kind = if m.npc().is_some() {
            EntityKind::Npc
        } else if m.monster().is_some_and(|e| e.ai.kind == AII_PET) {
            EntityKind::Pet
        } else {
            EntityKind::Monster
        };
        out.push(FindHit {
            kind,
            obj_id: id.0,
            character_id: 0,
            prop: m.prop_id(),
            name: object_name(g, id),
            world: m.core.world.0,
            layer: m.core.layer.0,
            x: m.core.pos.x,
            z: m.core.pos.z,
            level: clamp_u32(m.mv.level),
        });
    }
    out
}
