//! Item-summoned monsters (`CCreateMonster`, Plan/18 §14, WP-AI-16; ref/12 §12.5, ref/07b §8.16;
//! src:_Common/CreateMonster.cpp).
//!
//! **Single owner.** This module is the one implementation of every summon rule: the global limit, the item
//! listing and its keep time, the monster roll, the distance and area tests, the spawn, the registry, the
//! owner-only attack rule and the timed removal. The `CreateMonster` message handler of doc 19
//! (`item::create_monster::on_request`, WP-ITEM-22) only checks the inventory item and calls [`try_create`];
//! it consumes one item on `Ok` and nothing on `Err`.
//!
//! | step | rule | text on failure |
//! |---|---|---|
//! | 1 | the registry holds `max_create_num` summons | `TID_GAME_CREATEMON_LIMIT` |
//! | 2 | the item is listed in `CreateMonster.lua` | silent |
//! | 3 | the roll `x_random(100)` walks the monsters in **ascending mover id** (the original is a `std::map`) | silent |
//! | 4 | the target is more than 15 m from the user (3-D) | `TID_GAME_CREATEMON_F_15` |
//! | 5 | the target cell is NOWALK / NOMOVE, the user stands in a safety region, or the world is not Madrigal | `TID_GAME_CREATEMON_F_AREA` |
//! | 6 | spawn at the client's position in the user's layer, passive; register `{owner, end_at, 'N'}`; `TID_GAME_CREATEMON_S` | |
//!
//! **Who may attack.** [`is_attack_able`] is true for every monster that is no summon; a summon only for its
//! owner and the owner's party members while the owner is online. The damage pipeline asks it through
//! `CombatHooks::create_monster_attackable` (player hits and the `AIMSG_DAMAGE` gate); [`priority_text`] is the
//! `TID_GAME_PRIORITYSUMMONMOB` the refused attacker gets. The rule needs only the two movers: the monster carries
//! a [`SummonTag`] (its owner), the attacker's party link lists the owner's avatar while the owner is online.
//!
//! **Lifetime.** [`expire`] is item P3.2 of the roll-call job (10 §5.3, after P3.1): every summon with
//! `now >= end_at` is deleted and unregistered on its first run. [`on_died`] unregisters a killed summon at once
//! (the original keeps the entry until the corpse is destroyed, state `'D'`). A summon that vanished any other
//! way (a GM delete, the unload of its layer, a transformation) is dropped lazily by [`CreateMonsterRegistry::prune`],
//! which every [`try_create`] and every [`expire`] run first, so the limit counts live summons only.
//!
//! **Deviations.** A missing `CreateMonster.lua` makes every call `NotListed` (the original refuses to start); a
//! distance that is not a finite number is `TooFar` (the original's `15 < NaN` is false); a position outside the
//! world, which the original would have handed to `ADDOBJ`, is refused with the area text.

use crate::chat::ingress::notice::{defined, defined_text};
use crate::combat::state::is_live;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::mover::Mover;
use crate::world::Worlds;
use crate::world::region::{is_region_attr, region_attr_of};
use crate::world::terrain::HeightAttr;
use flyff_common::defs::define::RA_SAFETY;
use flyff_common::defs::text::{
    TID_GAME_CREATEMON_F_15, TID_GAME_CREATEMON_F_AREA, TID_GAME_CREATEMON_LIMIT,
    TID_GAME_CREATEMON_S, TID_GAME_PRIORITYSUMMONMOB,
};
use flyff_common::defs::world::WI_WORLD_MADRIGAL;
use flyff_common::math::Vec3;
use flyff_common::rng::GameRng;
use flyff_common::{CharacterId, GameMs, ObjId};
use flyff_data::lua_config::CreateMonsterEntry;
use flyff_data::types::MoverId;
use flyff_db::AuditEvent;
use flyff_db::audit::ev;
use flyff_proto::pb;
use serde_json::json;
use std::collections::BTreeMap;

/// The summon is alive (`'N'`).
pub const STATE_ALIVE: u8 = b'N';
/// The summon was removed by the expiry (`'R'`).
pub const STATE_REMOVED: u8 = b'R';
/// The summon was killed (`'D'`).
pub const STATE_DIED: u8 = b'D';

/// The farthest a summon may be placed from the user (3-D, metres).
pub const MAX_SUMMON_DISTANCE: f32 = 15.0;

/// Why [`try_create`] summoned nothing (18 §14). The first, fourth and fifth variants have a text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CreateMonsterError {
    /// The registry is full (`TID_GAME_CREATEMON_LIMIT`).
    Limit,
    /// The item is not listed (silent).
    NotListed,
    /// The roll hit no monster, or the rolled monster has no prop (silent).
    NoMonsterRolled,
    /// More than 15 m away (`TID_GAME_CREATEMON_F_15`).
    TooFar,
    /// The cell, the region or the world forbids it (`TID_GAME_CREATEMON_F_AREA`).
    BadArea,
}

/// `CREATE_MONSTER_INFO`: one registered summon.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreateMonsterInfo {
    /// `dwOwnerId`: the summoning character.
    pub owner: CharacterId,
    /// `dwEndTick`: the game time from which [`expire`] removes it.
    pub end_at: GameMs,
    /// `chState`: [`STATE_ALIVE`], [`STATE_REMOVED`] or [`STATE_DIED`].
    pub state: u8,
}

/// The owner a summoned monster carries (`MonsterExt.summon`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SummonTag {
    /// The summoning character.
    pub owner: CharacterId,
    /// The owner's name when the item was used (the text of a refused attacker names it).
    pub owner_name: String,
}

/// The global registry of the item-summoned monsters, all worlds (`m_mapCreateMonsterInfo`).
#[derive(Clone, Debug, Default)]
pub struct CreateMonsterRegistry {
    /// The summons by object id, ascending (deterministic iteration).
    pub entries: BTreeMap<ObjId, CreateMonsterInfo>,
}

impl CreateMonsterRegistry {
    /// How many summons are registered.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// `true` when none is.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The registration of a monster.
    pub fn get(&self, id: ObjId) -> Option<&CreateMonsterInfo> {
        self.entries.get(&id)
    }

    /// Registers a summon (a second registration of an id replaces the first).
    pub fn register(&mut self, id: ObjId, info: CreateMonsterInfo) {
        self.entries.insert(id, info);
    }

    /// Drops a registration (`RemoveInfo`).
    pub fn remove(&mut self, id: ObjId) -> Option<CreateMonsterInfo> {
        self.entries.remove(&id)
    }

    /// Drops the registrations whose monster no longer exists or is flagged deleted, audited with their last
    /// state. Returns how many were dropped.
    pub fn prune(&mut self, worlds: &Worlds, audit: &flyff_db::AuditLog) -> usize {
        let gone: Vec<ObjId> = self
            .entries
            .keys()
            .copied()
            .filter(|&id| worlds.mover(id).is_none_or(|m| m.core.deleted))
            .collect();
        for id in &gone {
            if let Some(info) = self.entries.remove(id) {
                audit_removed(audit, *id, &info, info.state, None);
            }
        }
        gone.len()
    }
}

/// The monsters of an item sorted like the original's `map<DWORD, int>`: ascending mover id, the first
/// `AddMonster` of an id wins.
pub fn sorted_monsters(entries: &[CreateMonsterEntry]) -> Vec<(MoverId, i32)> {
    let mut out: Vec<(MoverId, i32)> = Vec::with_capacity(entries.len());
    for e in entries {
        if !out.iter().any(|(m, _)| *m == e.mover) {
            out.push((e.mover, e.probability));
        }
    }
    out.sort_by_key(|(m, _)| *m);
    out
}

/// `CREATE_MONSTER_PROP::GetRandomMonsterId`: walks the sorted monsters with `roll = x_random(100)`: the first
/// one whose probability exceeds what is left of the roll, `None` when the probabilities sum to less.
pub fn pick_monster(sorted: &[(MoverId, i32)], roll: i32) -> Option<MoverId> {
    let mut left = roll;
    for &(mover, prob) in sorted {
        if left < prob {
            return Some(mover);
        }
        left -= prob;
    }
    None
}

/// `CCreateMonster::CreateMonster` without the inventory part (steps 1-6): the user `who` uses an item listed
/// as `item_id` at `pos`. Sends every failure text and the success text itself; consumes nothing.
pub fn try_create(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    item_id: u32,
    pos: Vec3,
) -> Result<ObjId, CreateMonsterError> {
    let data = std::sync::Arc::clone(ctx.data);
    let Some(cfg) = data.lua.create_monster.as_ref() else {
        return Err(CreateMonsterError::NotListed);
    };
    svc.summons.prune(&*ctx.worlds, ctx.audit);
    // 1. the global limit
    if i64::try_from(svc.summons.len()).unwrap_or(i64::MAX) >= i64::from(cfg.max_create_num) {
        defined(ctx, who, TID_GAME_CREATEMON_LIMIT, Vec::new());
        return Err(CreateMonsterError::Limit);
    }
    // 2. the item is listed (a repeated AddItem of one id: the first wins, like `map::insert`)
    let Some(listing) = cfg.items.iter().find(|i| i.item == item_id) else {
        return Err(CreateMonsterError::NotListed);
    };
    let Some((world, layer, user_pos, in_safety, owner, owner_name)) =
        ctx.player(who).and_then(|m| {
            let p = m.player()?;
            Some((
                m.core.world,
                m.core.layer,
                m.core.pos,
                is_region_attr(region_attr_of(m), RA_SAFETY),
                p.ident.character_id,
                p.ident.name.clone(),
            ))
        })
    else {
        return Err(CreateMonsterError::BadArea);
    };
    // 3. the monster roll (the world's stream, drawn before the distance test like the original)
    let sorted = sorted_monsters(&listing.monsters);
    let roll = ctx.rng.world(world).random_below(100) as i32;
    let Some(mover) = pick_monster(&sorted, roll) else {
        return Err(CreateMonsterError::NoMonsterRolled);
    };
    let Some(prop) = data.mover(mover) else {
        return Err(CreateMonsterError::NoMonsterRolled);
    };
    // 4. within 15 m of the user
    let dist = (user_pos - pos).length();
    if dist.is_nan() || dist > MAX_SUMMON_DISTANCE {
        defined(ctx, who, TID_GAME_CREATEMON_F_15, Vec::new());
        return Err(CreateMonsterError::TooFar);
    }
    // 5. the area
    let attr = ctx
        .worlds
        .statics_of(who.0)
        .map(|s| s.height_attr(pos.x, pos.z));
    if matches!(attr, Some(HeightAttr::NoWalk | HeightAttr::NoMove))
        || in_safety
        || world.0 != WI_WORLD_MADRIGAL
    {
        defined(ctx, who, TID_GAME_CREATEMON_F_AREA, Vec::new());
        return Err(CreateMonsterError::BadArea);
    }
    // 6. spawn: passive, in the user's layer, at the position the client chose
    let spawn = super::MonsterSpawn {
        mover,
        world,
        layer,
        pos,
        active_attack: false,
        gold: None,
        owner_ai: None,
    };
    let Some(id) = super::spawn::commit(ctx.worlds, &data, ctx.rng.world(world), &spawn) else {
        defined(ctx, who, TID_GAME_CREATEMON_F_AREA, Vec::new());
        return Err(CreateMonsterError::BadArea);
    };
    if let Some(ext) = ctx.worlds.mover_mut(id).and_then(Mover::monster_mut) {
        ext.summon = Some(SummonTag { owner, owner_name });
    }
    let end_at = GameMs(ctx.clock.game_ms.0 + u64::from(listing.keep_time_ms));
    svc.summons.register(
        id,
        CreateMonsterInfo {
            owner,
            end_at,
            state: STATE_ALIVE,
        },
    );
    defined(
        ctx,
        who,
        TID_GAME_CREATEMON_S,
        vec![format!("\"{}\"", prop.name)],
    );
    crate::audit::emit_for(
        ctx,
        who,
        ev::ITEM_CREATE_MONSTER,
        json!({
            "state": "C",
            "item": item_id,
            "monster": mover,
            "name": prop.name,
            "obj_id": id.0,
            "owner": owner.0,
            "keep_ms": listing.keep_time_ms,
        }),
    );
    Ok(id)
}

/// `CCreateMonster::IsAttackAble(user, target)` without the text: `true` when `target` is no summon, or
/// `att` is its owner, or the owner is online and a member of `att`'s party. An attacker that is no player is
/// not gated.
pub fn is_attack_able(att: &Mover, target: &Mover) -> bool {
    let Some(tag) = target.monster().and_then(|e| e.summon.as_ref()) else {
        return true;
    };
    let Some(p) = att.player() else {
        return true;
    };
    if p.ident.character_id == tag.owner {
        return true;
    }
    p.social.party_link.as_ref().is_some_and(|l| {
        l.members
            .iter()
            .any(|m| m.id == tag.owner && m.obj.is_some())
    })
}

/// The `TID_GAME_PRIORITYSUMMONMOB` text of a refused attacker (`IsAttackAble(.., bTextOut = TRUE)`): the
/// owner's name in quotes. `None` when `target` is no summon.
pub fn priority_text(target: &Mover) -> Option<pb::DefinedText> {
    let tag = target.monster().and_then(|e| e.summon.as_ref())?;
    Some(defined_text(
        TID_GAME_PRIORITYSUMMONMOB,
        vec![format!("\"{}\"", tag.owner_name)],
    ))
}

/// `CCreateMonster::ProcessRemoveMonster` (item P3.2 of the roll-call job): deletes every registered summon
/// whose time is up (`now >= end_at`) on this run and unregisters it. A registration whose monster is gone or
/// dead is dropped as well.
pub fn expire(svc: &mut Services, ctx: &mut GameCtx<'_>) {
    if svc.summons.is_empty() {
        return;
    }
    svc.summons.prune(&*ctx.worlds, ctx.audit);
    let now = ctx.clock.game_ms;
    let due: Vec<ObjId> = svc
        .summons
        .entries
        .iter()
        .filter(|(_, info)| now >= info.end_at)
        .map(|(id, _)| *id)
        .collect();
    for id in due {
        let live = ctx
            .worlds
            .mover(id)
            .is_some_and(|m| !m.core.deleted && is_live(m));
        let Some(info) = svc.summons.remove(id) else {
            continue;
        };
        let state = if live { STATE_REMOVED } else { STATE_DIED };
        if live {
            ctx.worlds.delete(id);
        }
        audit_removed(ctx.audit, id, &info, state, Some(ctx.worlds));
    }
}

/// `SetState('D')` + `RemoveInfo` of a killed mover: a summon is unregistered at once.
pub fn on_died(svc: &mut Services, ctx: &mut GameCtx<'_>, victim: ObjId) {
    if let Some(info) = svc.summons.remove(victim) {
        audit_removed(ctx.audit, victim, &info, STATE_DIED, Some(ctx.worlds));
    }
}

/// `RemoveInfo`'s log line (`M_<state>_<name>`) as an audit event.
fn audit_removed(
    audit: &flyff_db::AuditLog,
    id: ObjId,
    info: &CreateMonsterInfo,
    state: u8,
    worlds: Option<&Worlds>,
) {
    let name = worlds
        .and_then(|w| w.mover(id))
        .map(|m| m.prop_id())
        .unwrap_or_default();
    audit.emit(AuditEvent::new(
        ev::ITEM_CREATE_MONSTER,
        json!({
            "state": char::from(state).to_string(),
            "obj_id": id.0,
            "monster": name,
            "owner": info.owner.0,
        }),
    ));
}
