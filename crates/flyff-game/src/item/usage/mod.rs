//! The item-use dispatcher core (Plan/19 §5.1-§5.3, WP-ITEM-05; ref/13 §10; src:WORLDSERVER/DPSrvr.cpp:2635-2683,
//! User.cpp:2983-3060, _Common/MoverSkill.cpp:1294-1748).
//!
//! `DoUseItem` pipeline, in order: the part and ownership checks (VR-010), the usable state (a trade partner
//! blocks the use of a bag item), the expired-item rule, the equipment toggle for equipables, then for
//! everything else the cast time, the cooldown group, the dispatch on **IK2** (the rows of this work package:
//! food, refreshers and potions), the dispatch on **IK3** (pets), the after-use effects, the consumption and the
//! `UpdateItem`.
//!
//! The rows owned by later work packages (system items, blink wings, buffs, SM modes, pets, tickets, text
//! items...) answer `TID_SBEVE_NOTUSEITEM` and consume nothing (E10): they plug into [`dispatch_ik2`] /
//! [`dispatch_ik3`] without touching the pipeline.

pub mod blink;
pub mod cast;
pub mod cooldown;
pub mod food;
pub mod giftbox;
pub mod pack;
pub mod system;
pub mod text;

use super::container::StorageId;
use super::env::ItemEnv;
use super::fx::Fx;
use super::handlers::{equip_facts, toggle_equip};
use super::model::Item;
use super::ops::{RemoveReason, UiParam, core as ops};
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::follow::Followup;
use crate::mover::Mover;
use crate::stats::equip::EquipFacts;
use cast::CastStep;
use cooldown::group_of;
use flyff_common::GameMs;
use flyff_common::defs::itemkind::{
    IK1_GOLD, IK2_BLINKWING, IK2_FOOD, IK2_POTION, IK2_REFRESHER, IK2_SYSTEM, IK2_WARP, IK3_EGG,
    IK3_LINK, IK3_PET,
};
use flyff_common::defs::text::{
    TID_GAME_ITEM_EXPIRED, TID_GAME_LIMITED_USE, TID_GAME_NOTOVERLAP_USE, TID_GAME_PET_DEAD,
    TID_GAME_TRADELIMITUSING, TID_SBEVE_NOTUSEITEM,
};
use flyff_common::rng::GameRng;
use flyff_data::props::item::ItemProp;
use flyff_proto::pb;
use flyff_rules::consts::dst::CHS_SETSTONE;
use flyff_rules::consts::{NULL_ID, STATE_BASEMOTION_MODE};

/// Facts about the player's other states that block or change the use of items. The trade, vendor and quiz
/// states of the economy and event work packages are set by their owners; the defaults allow everything.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UseFacts {
    /// A trade partner exists (`m_vtInfo.GetOther()`).
    pub trading: bool,
    /// The player has a personal shop open.
    pub vending: bool,
    /// The player is in the quiz world (`WI_WORLD_QUIZ`).
    pub quiz_world: bool,
    /// The player is a spectator of a guild-war match.
    pub guild_spectator: bool,
    /// The player is in the 1:1 guild combat (`g_GuildCombat1to1Mng.IsPossibleUser`; doc 21): no blink wings.
    pub gc1to1: bool,
    /// The player is inside a secret room (doc 22): no blink wings.
    pub secret_room: bool,
    /// The player is a rainbow-race entrant (doc 22): no blink wings, rings or teleports.
    pub rainbow_entrant: bool,
    /// `events.pk` is on (KQ-17): the angel buff items work (`run_use` fills it from the live configuration).
    pub pk_server: bool,
    /// The player stands in a guild-quest region (`prj.IsGuildQuestRegion`; the return scroll; `run_use` fills it).
    pub guild_quest_region: bool,
    /// The player is in an instance dungeon (doc 22): no disguise items.
    pub instance_dungeon: bool,
    /// The level of the player's party, `None` without a party (`run_use` fills it; the party level scroll).
    pub party_level: Option<i32>,
}

impl UseFacts {
    /// The facts the economy keeps on the player: any trade partner (an open NPC shop window included,
    /// `m_vtInfo.GetOther()`) and an open stall (19 §7.2).
    pub fn of(m: &Mover) -> UseFacts {
        let econ = m.player().map(|p| &p.econ);
        UseFacts {
            trading: econ.is_some_and(|e| e.has_partner()),
            vending: econ.is_some_and(|e| e.vending),
            // ---- rainbow race (doc 22 §14, task rainbow-race) ----
            rainbow_entrant: m.player().is_some_and(|p| p.misc.rainbow_entrant),
            // ---- secret room (doc 22 §11, task secret-room): `IsInTheSecretRoom` = worlds 152..=167 ----
            secret_room: crate::event::secret_room::is_room_world(m.core.world),
            ..UseFacts::default()
        }
    }
}

/// How a use ended (for the callers and the tests).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UseOutcome {
    /// Ignored without a message (foreign item, dead, a state that forbids it).
    Ignored,
    /// Refused; a text was queued.
    Refused,
    /// A cast started; the use will be re-issued.
    Deferred,
    /// The effect happened and the item was consumed (or its count shown again, if permanent).
    Used,
    /// The effect did not happen and nothing was consumed.
    NotConsumed,
    /// The item was equipped or unequipped.
    Equipped,
}

/// The result of one IK2 / IK3 row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowResult {
    /// Done: consume.
    Consume,
    /// Not done, nothing consumed, no further text (the row sent its own).
    Refused,
    /// A row of a later work package: `TID_SBEVE_NOTUSEITEM`, nothing consumed.
    NotYet,
    /// The row did everything itself, the item consumption included (blink wings, couple rings, ...): none of
    /// the post-processing of the pipeline follows.
    Handled,
    /// The original's result 1 of `DoUseItemSystem`: nothing happened, nothing consumed, no text; the gift box
    /// table is tried next.
    Fallthrough,
}

/// What the rows get besides the mover, the environment and the prop: the item being used, the request's part,
/// the game clock, the player's other states and the random stream (the gift box roll).
pub struct RowCx<'a> {
    /// The storage id of the item.
    pub id: StorageId,
    /// The `part` of the request (-1 / 0 / a part): the new face of the sex change.
    pub part: i32,
    /// The game clock of the tick.
    pub now: GameMs,
    /// The player's other states.
    pub facts: &'a UseFacts,
    /// The world's random stream (`xRandom`).
    pub rng: &'a mut dyn GameRng,
}

/// The IK2 rows (Plan/19 §5.3): food and the HP / MP / FP items (with the premium-mode variants of WP-ITEM-21),
/// the system items (WP-ITEM-09 / 21), blink wings and warps (WP-ITEM-10); the buff, text, skill and housing
/// kinds belong to later work packages. An unlisted kind without a `skill_time` consumes the item and does
/// nothing, as the original's `default` row.
pub fn dispatch_ik2(
    m: &mut Mover,
    env: &ItemEnv<'_>,
    prop: &ItemProp,
    ux: &mut RowCx<'_>,
    fx: &mut Fx,
) -> RowResult {
    // the buff-like kinds (BUFF, BUFF_TOGIFT, BUFF2, AIRFUEL, GMTEXT, SKILL; WP-ITEM-20)
    if let Some(r) = super::buffitems::dispatch_ik2(m, env, prop, ux, fx) {
        return r;
    }
    match prop.item_kind2 {
        IK2_REFRESHER | IK2_POTION if prop.circle_time != NULL_ID => {
            let sm = if prop.item_kind2 == IK2_REFRESHER {
                flyff_rules::consts::SM_MAINTAIN_MP
            } else {
                flyff_rules::consts::SM_MAINTAIN_FP
            };
            match crate::stats::sm::use_maintain(m, env, prop, sm, fx) {
                crate::stats::sm::SmUse::Used => RowResult::Consume,
                _ => RowResult::Refused,
            }
        }
        IK2_FOOD | IK2_REFRESHER | IK2_POTION => match food::use_food(m, env, prop, fx) {
            food::FoodResult::Used => RowResult::Consume,
            food::FoodResult::Refused => RowResult::Refused,
        },
        IK2_SYSTEM => match system::use_system(m, env, prop, ux, fx) {
            system::Sys::NotYet => RowResult::NotYet,
            system::Sys::Ret(0) => RowResult::Consume,
            system::Sys::Ret(1) => RowResult::Fallthrough,
            system::Sys::Ret(2) => {
                fx.text(TID_GAME_LIMITED_USE);
                RowResult::Refused
            }
            system::Sys::Ret(3) => {
                fx.text(TID_GAME_NOTOVERLAP_USE);
                RowResult::Refused
            }
            system::Sys::Ret(_) => RowResult::Refused,
        },
        IK2_BLINKWING => blink::use_blink_wing(m, env, prop, ux, fx),
        IK2_WARP => blink::use_warp(m, env, prop, ux, fx),
        // ---- quest text items (doc 19 §5.3, task quest-proof) ----
        flyff_common::defs::itemkind::IK2_TEXT => text::use_text(m, env, prop, ux, fx),
        flyff_common::defs::itemkind::IK2_BUFF
        | flyff_common::defs::itemkind::IK2_BUFF2
        | flyff_common::defs::itemkind::IK2_BUFF_TOGIFT
        | flyff_common::defs::itemkind::IK2_AIRFUEL
        | flyff_common::defs::itemkind::IK2_GMTEXT
        | flyff_common::defs::itemkind::IK2_SKILL
        | flyff_common::defs::itemkind::IK2_TOCASH => RowResult::NotYet,
        // ---- guild house (doc 22 §12.3, task guild-house): the list-up and the comeback scroll are follow-ups ----
        flyff_common::defs::itemkind::IK2_GUILDHOUSE_FURNITURE
        | flyff_common::defs::itemkind::IK2_GUILDHOUSE_NPC
        | flyff_common::defs::itemkind::IK2_GUILDHOUSE_PAPERING
        | flyff_common::defs::itemkind::IK2_GUILDHOUES_COMEBACK => {
            crate::event::guild_house::use_row(m, prop, ux, fx)
        }
        // ---- housing (doc 22 §13.2, task housing): the list-up is a follow-up, the row only queues it ----
        flyff_common::defs::itemkind::IK2_FURNITURE
        | flyff_common::defs::itemkind::IK2_PAPERING => crate::event::housing::use_row(m, ux, fx),
        _ if prop.skill_time != NULL_ID => super::buffitems::ik2::default_row(m, prop, fx),
        _ => RowResult::Consume,
    }
}

/// The IK3 rows (Plan/19 §5.1 step 5.4): an egg toggles the system pet (ITEM-16), a pet the eat pet
/// (ITEM-18). Runs after a consuming IK2 row only.
pub fn dispatch_ik3(
    m: &mut Mover,
    env: &ItemEnv<'_>,
    prop: &ItemProp,
    ux: &mut RowCx<'_>,
    fx: &mut Fx,
) -> RowResult {
    match prop.item_kind3 {
        IK3_EGG => {
            let cx = crate::buff::BuffCx::new(*env, ux.now);
            super::pets::system::use_egg(m, &cx, ux.id, fx)
        }
        IK3_PET => {
            let cx = crate::buff::BuffCx::new(*env, ux.now);
            super::pets::eat::use_eat_pet(m, &cx, ux.id, fx)
        }
        _ => RowResult::Consume,
    }
}

/// `DoUseItem(data, target, part)` for one player. Returns what happened. The rows that roll (the gift box)
/// draw from a fixed stream here; the game uses [`use_item_with`] and the world's.
#[allow(clippy::too_many_arguments)]
pub fn use_item(
    m: &mut Mover,
    env: &ItemEnv<'_>,
    id: StorageId,
    part: i32,
    now: GameMs,
    facts: &UseFacts,
    equip: &EquipFacts,
    fx: &mut Fx,
) -> UseOutcome {
    let mut rng = flyff_common::rng::Pcg32Rng::new(1, 1);
    use_item_with(m, env, id, part, now, facts, equip, &mut rng, fx)
}

/// [`use_item`] with the random stream of the rows.
#[allow(clippy::too_many_arguments)]
pub fn use_item_with(
    m: &mut Mover,
    env: &ItemEnv<'_>,
    id: StorageId,
    part: i32,
    now: GameMs,
    facts: &UseFacts,
    equip: &EquipFacts,
    rng: &mut dyn GameRng,
    fx: &mut Fx,
) -> UseOutcome {
    if part >= flyff_rules::consts::MAX_HUMAN_PARTS as i32 {
        return UseOutcome::Ignored;
    }
    let Some(p) = m.player() else {
        return UseOutcome::Ignored;
    };
    let inv = &p.items.inventory;
    let Some(item) = inv.get_at_id(id) else {
        return UseOutcome::Ignored;
    };
    // step 1: a worn item must be the one at `part`; a reserved item is unusable
    let worn = inv.is_equip(id);
    if part > 0 && worn && inv.equip_id(part as u32) != Some(id) {
        return UseOutcome::Ignored;
    }
    if !item.is_usable() {
        return UseOutcome::Ignored;
    }
    // step 2: the usable state (a trade partner / a stall block bag items)
    if !worn {
        if facts.trading {
            fx.text(TID_GAME_TRADELIMITUSING);
            return UseOutcome::Refused;
        }
        if facts.vending {
            return UseOutcome::Ignored;
        }
    }
    // step 3
    if facts.quiz_world {
        return UseOutcome::Ignored;
    }
    let item_id = item.item_id;
    let Some(own_prop) = env.data().item(item_id) else {
        return UseOutcome::Ignored;
    };
    // step 4(a): an expired item that is not equipment
    if own_prop.parts == NULL_ID && item.is_expired() {
        fx.text(if Item::is_pet(own_prop) {
            TID_GAME_PET_DEAD
        } else {
            TID_GAME_ITEM_EXPIRED
        });
        return UseOutcome::Refused;
    }
    // step 4(b): `CUser::DoUseItem` hands a vis charm, the vis key and the vis transform scroll to the vis pet code
    // before anything else (User.cpp:3015-3030; WP-ITEM-18)
    {
        let cx = crate::buff::BuffCx::new(*env, now);
        if let Some(out) = super::pets::vis::try_use(m, &cx, id, own_prop, fx) {
            return out;
        }
    }
    // step 4(c): a pack item opens before `DoUseItem` (no cast time, no cooldown)
    if let Some(pk) = env.data().pack_items.packs.get(&item_id) {
        let pk = pk.clone();
        return match pack::open(m, env, id, &pk, fx) {
            pack::PackResult::Opened => {
                // `DoUsePackItem` -> `OnAfterUseItem` (21 §13.3: the honor counter of the item)
                fx.then(Followup::ItemUsed {
                    player: m.id(),
                    item: own_prop.id,
                });
                UseOutcome::Used
            }
            pack::PackResult::LackSpace => UseOutcome::Refused,
        };
    }
    // step 5: `CMover::DoUseItem`
    if m.sim.act.state.is_die() {
        return UseOutcome::Ignored;
    }
    let mut prop = own_prop;
    if own_prop.item_kind3 == IK3_LINK {
        match env.data().item(own_prop.active_skill) {
            Some(linked) => prop = linked,
            None => return UseOutcome::Ignored,
        }
    }
    if facts.guild_spectator {
        return UseOutcome::Ignored;
    }
    if m.mv.dst.table.adj(flyff_rules::consts::dst::DST_CHRSTATE) & CHS_SETSTONE as i32 != 0 {
        return UseOutcome::Ignored;
    }
    // equipment: the toggle (refused while attacking; expiry never blocks taking an item off)
    if prop.parts != NULL_ID {
        if m.sim.act.state.is_act_attack() {
            return UseOutcome::Ignored;
        }
        return if toggle_equip(m, env, id, part, equip, now, fx) {
            UseOutcome::Equipped
        } else {
            UseOutcome::Refused
        };
    }
    // 5.1 cast time
    match cast::ready_check(m, env, id, prop, now, fx) {
        CastStep::Proceed => {}
        CastStep::Deferred => return UseOutcome::Deferred,
        CastStep::Refused => return UseOutcome::Refused,
    }
    // 5.2 cooldown group
    let group = group_of(prop);
    if let Some(g) = group
        && let Some(p) = m.player()
        && !p.items.cooldowns.can_use(g, now)
    {
        return UseOutcome::Ignored;
    }
    // 5.3 / 5.4 the dispatch
    let mut ux = RowCx {
        id,
        part,
        now,
        facts,
        rng,
    };
    let ik2 = dispatch_ik2(m, env, prop, &mut ux, fx);
    // the IK3 row acts (it has effects) only when the IK2 row consumed
    let ik3 = if ik2 == RowResult::Consume {
        dispatch_ik3(m, env, prop, &mut ux, fx)
    } else {
        RowResult::Consume
    };
    for row in [ik2, ik3] {
        match row {
            RowResult::Consume => {}
            RowResult::Refused => return UseOutcome::NotConsumed,
            RowResult::Handled => return UseOutcome::Used,
            RowResult::NotYet => {
                fx.text(TID_SBEVE_NOTUSEITEM);
                return UseOutcome::NotConsumed;
            }
            RowResult::Fallthrough => {
                // step 6: a gift box takes an unclaimed system item
                return match env.data().giftboxes.boxes.get(&own_prop.id) {
                    Some(b) => match giftbox::open(m, env, id, &b.clone(), ux.rng, fx) {
                        giftbox::BoxResult::Opened => {
                            // `DoUseGiftbox` -> `OnAfterUseItem`
                            fx.then(Followup::ItemUsed {
                                player: m.id(),
                                item: own_prop.id,
                            });
                            UseOutcome::Used
                        }
                        giftbox::BoxResult::LackSpace => UseOutcome::Refused,
                        giftbox::BoxResult::NotABox => UseOutcome::NotConsumed,
                    },
                    None => UseOutcome::NotConsumed,
                };
            }
        }
    }
    // 5.5 `OnAfterUseItem` (the SFX of the prop belongs to doc 15; the honor counter of the item, doc 21 §13.3)
    fx.then(Followup::ItemUsed {
        player: m.id(),
        item: prop.id,
    });
    // 5.6 / 5.7 consume one unless permanent, then the cooldown and the message
    let Some(p) = m.player() else {
        return UseOutcome::Ignored;
    };
    let item_now = p.items.inventory.get_at_id(id);
    let count = item_now.map_or(0, |it| it.count);
    // `bPermanence`: only TRUE (1, the gear, rides and eggs) is permanent; a consumable has `=` (loads as -1) -
    // 1 373 of the 5 283 real props, among them every food and potion
    let permanent = own_prop.permanence == 1;
    let new = if permanent {
        i32::from(count)
    } else {
        i32::from(count) - 1
    };
    let premium_line = (!permanent && new <= 0)
        .then(|| {
            item_now
                .filter(|it| it.is_charged())
                .map(super::fx::item_json)
        })
        .flatten();
    if let Some(item_json) = premium_line {
        // the audit line of a premium item (19 §21)
        fx.audit(
            flyff_db::audit::ev::ITEM_DESTROY,
            serde_json::json!({ "item": item_json, "count": 1, "reason": RemoveReason::Consume.tag() }),
        );
    }
    let param = if let Some(g) = group {
        if let Some(p) = m.player_mut() {
            p.items.cooldowns.set(g, now, prop.cool_time());
        }
        UiParam::CoolTime
    } else {
        UiParam::Num
    };
    ops::update_item(m, env, id, param, new as u32, 0, fx);
    UseOutcome::Used
}

/// `DoUseItem` (C2S 9): resolves the actor, the item id (`data >> 16`) and the facts, then [`use_item`].
pub fn on_do_use_item(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    msg: pb::DoUseItem,
) -> UseOutcome {
    let Ok(id) = StorageId::try_from(msg.data >> 16) else {
        return UseOutcome::Ignored;
    };
    run_use(svc, ctx, who, id, msg.part)
}

/// `StateModeReq` (C2S): only `STATE_BASEMOTION_MODE` with `STATEMODE_BASEMOTION_CANCEL` is acted on - the
/// client cancels a running cast (a blink wing, a return scroll); everything else is ignored (doc 16 §8,
/// `CDPSrvr::OnStateMode`).
pub fn on_state_mode(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    msg: pb::StateModeReq,
) {
    if msg.state_mode != STATE_BASEMOTION_MODE || msg.flag != cast::STATEMODE_CANCEL {
        return;
    }
    let mut fx = Fx::new();
    if let Some(m) = ctx.worlds.player_mut(who.0) {
        cast::cancel_cast(m, &mut fx);
    }
    fx.flush(svc, ctx, who);
}

/// Runs a use for `who` and flushes its effects (also the re-issue of a finished cast).
pub fn run_use(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    id: StorageId,
    part: i32,
) -> UseOutcome {
    let equip = equip_facts(ctx, who);
    let core = svc.items.handle();
    let env = core.env(ctx.clock.unix_ms.to_unix_time());
    let now = ctx.clock.game_ms;
    let mut fx = Fx::new();
    let Some(world) = ctx.worlds.core(who.0).map(|c| c.world) else {
        return UseOutcome::Ignored;
    };
    // the facts of the services that the mover-level rows need (WP-ITEM-20)
    let pk_server = ctx.live.event_state(flyff_common::config::EventId::Pk) != 0;
    let guild_quest_region = ctx
        .worlds
        .core(who.0)
        .is_some_and(|c| svc.skill.is_guild_quest_region(c.pos));
    let party_level = ctx
        .worlds
        .player(who.0)
        .and_then(|m| m.player())
        .and_then(|p| svc.social.party.party_of(p.ident.character_id))
        .and_then(|id| svc.social.party.get(id))
        .map(|p| p.level);
    // doc 21 §8 (task gc11): a participant of the 1:1 guild combat has no blink wings (and no ticket)
    let gc1to1 = crate::social::gc1to1::is_participant(svc, ctx, who.0);
    let Some(m) = ctx.worlds.player_mut(who.0) else {
        return UseOutcome::Ignored;
    };
    let mut facts = UseFacts::of(m);
    facts.gc1to1 = gc1to1;
    facts.pk_server = pk_server;
    facts.guild_quest_region = guild_quest_region;
    facts.party_level = party_level;
    // 23 §5.2 step 1a: no disguise scroll inside an instance dungeon (doc 22 installs the registry, WP-GM-19)
    facts.instance_dungeon = svc.gm.instances.is_instance_world(world);
    let out = use_item_with(
        m,
        &env,
        id,
        part,
        now,
        &facts,
        &equip,
        ctx.rng.world(world),
        &mut fx,
    );
    if matches!(out, UseOutcome::Used) {
        fx.mark_urgent();
    }
    fx.flush(svc, ctx, who);
    out
}

/// `true` for gold seeds (never an inventory item).
pub fn is_gold(prop: &ItemProp) -> bool {
    prop.item_kind1 == IK1_GOLD
}
