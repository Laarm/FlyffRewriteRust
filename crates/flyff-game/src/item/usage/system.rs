//! `use_system`: the `IK2_SYSTEM` rows of Plan/19 §5.4 that belong to WP-ITEM-09 (restat scrolls, the sex change,
//! the Perin, the instant bank) and WP-ITEM-21 (the premium-mode scrolls: shout, party skill, remote bank,
//! action points, attack up, the blessing, the velocity scroll); `DoUseItemSystem`, `MoverSkill.cpp:1897-2860`.
//!
//! The result codes are the original's: 0 consume, 1 do not consume (the pipeline then tries the gift box
//! table), 2 "already in use" (`TID_GAME_LIMITED_USE`), 3 "cannot overlap" (`TID_GAME_NOTOVERLAP_USE`), 99 do not
//! consume, the message is already sent. The ids owned by later work packages (pets, collector battery, buffs,
//! smelting, EXP scrolls, summons...) are [`Sys::NotYet`]: `TID_SBEVE_NOTUSEITEM`, nothing consumed.

use super::RowCx;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::follow::Followup;
use crate::item::env::ItemEnv;
use crate::item::fx::Fx;
use crate::mover::Mover;
use crate::stats::points::{restat_full, restat_one, restat_one_low};
use crate::stats::sm;
use flyff_common::defs::item::*;
use flyff_common::defs::itemkind::{IK3_BALLOON, IK3_CREATE_MONSTER, IK3_TICKET};
use flyff_common::defs::neuz::*;
use flyff_common::defs::text::*;
use flyff_data::props::item::ItemProp;
use flyff_proto::pb;
use flyff_rules::consts::{
    NULL_ID, SM_ACTPOINT, SM_ATTACK_UP, SM_ATTACK_UP1, SM_BANK001, SM_BANK15, SM_BANK30,
    SM_PARTYSKILL1, SM_PARTYSKILL15, SM_PARTYSKILL30, SM_REVIVAL, SM_SHOUT001, SM_SHOUT15,
    SM_SHOUT30,
};

/// What a `use_system` row answered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sys {
    /// The original's result code (0, 1, 2, 3 or 99).
    Ret(u8),
    /// A row of a later work package.
    NotYet,
}

/// The result codes as names.
pub const CONSUME: Sys = Sys::Ret(0);
/// Result 1: not consumed, the gift box table is tried.
pub const FALLTHROUGH: Sys = Sys::Ret(1);
/// Result 2.
pub const IN_USE: Sys = Sys::Ret(2);
/// Result 99: not consumed, the row sent its own message.
pub const REFUSED: Sys = Sys::Ret(99);

/// `CMover::GetSummonState()` (`MoverParam.cpp:313-350`): what keeps a player from using a Perin or a summon.
/// 0 free, 1 trading, 2 dead, 3 vending, 4 in combat, 5 flying, 6 in a duel.
pub fn summon_state(m: &Mover, facts: &super::UseFacts) -> u8 {
    if facts.trading {
        1
    } else if m.sim.act.state.is_die() || m.mv.hp == 0 {
        2
    } else if facts.vending {
        3
    } else if crate::stats::tick::is_attack_mode(m) {
        4
    } else if m.sim.act.flags.fly() {
        5
    } else if m.combat.duel.state != 0 {
        6
    } else {
        0
    }
}

/// The `II_SYS_SYS_SCR_*` ids that later work packages answer.
fn later(prop: &ItemProp) -> bool {
    matches!(
        prop.id,
        II_GEN_TOO_COL_NORMALBATTERY
            | II_SYS_SYS_SCR_SELPAR
            | II_SYS_SYS_SCR_HOLY
            | II_SYS_SYS_SCR_AMPES
            | II_SYS_SYS_SCR_AMPEM
            | II_SYS_SYS_SCR_SMELPROT
            | II_SYS_SYS_SCR_AWAKESAFE
            | II_SYS_SYS_SCR_PET_FEED_POCKET
            | II_SYS_SYS_SCR_GET01
            | II_SYS_SYS_SCR_GET02
            | II_SYS_SYS_SCR_RETURN
            | II_SYS_SYS_SCR_PARTYSUMMON
    ) || prop.item_kind3 == IK3_BALLOON
        // a monster-summon scroll is used with `CreateMonster{data, pos}` (WP-ITEM-22), never with `DoUseItem`
        || prop.item_kind3 == IK3_CREATE_MONSTER
        || is_ampes_tier(prop.id)
        || is_smelting_buff(prop.id)
}

/// The level-banded EXP scrolls (`AMPESA..E`, `AMPESS` and their tiers).
fn is_ampes_tier(id: u32) -> bool {
    [
        II_SYS_SYS_SCR_AMPESA,
        II_SYS_SYS_SCR_AMPESB,
        II_SYS_SYS_SCR_AMPESC,
        II_SYS_SYS_SCR_AMPESD,
        II_SYS_SYS_SCR_AMPESE,
        II_SYS_SYS_SCR_AMPESS,
    ]
    .iter()
    .any(|&base| (base..=base + 2).contains(&id))
}

/// The protection and smelting scrolls applied as buffs (WP-ITEM-20).
fn is_smelting_buff(id: u32) -> bool {
    [
        II_SYS_SYS_SCR_SMELPROT,
        II_SYS_SYS_SCR_SMELPROT3,
        II_SYS_SYS_SCR_SMELTING,
        II_SYS_SYS_SCR_SMELTING2,
        II_SYS_SYS_SCR_SUPERSMELTING,
        II_SYS_SYS_SCR_GLVCHEER,
        II_SYS_SYS_SCR_SUPERLEADERPARTY,
        II_SYS_SYS_SCR_FONTEDIT,
        II_SYS_SYS_SCR_VENDOR,
        II_SYS_SYS_SCR_SKLINSTRUCT,
        II_SYS_SYS_SCR_SMELPROT4,
    ]
    .contains(&id)
}

/// `DoUseItemSystem` for the rows of this work package. `part` is the `nPart` of the request (the new face of
/// the sex change).
pub fn use_system(
    m: &mut Mover,
    env: &ItemEnv<'_>,
    prop: &ItemProp,
    ux: &mut RowCx<'_>,
    fx: &mut Fx,
) -> Sys {
    // `__JEFF_9_20`: a level limit refuses with the limit in the text
    if prop.limit_level1 != NULL_ID && (m.mv.level as u32) < prop.limit_level1 {
        fx.defined(
            TID_GAME_ITEM_LEVEL,
            vec![format!("\"{}\"", prop.limit_level1)],
        );
        return REFUSED;
    }
    // `__AZRIA_1023`: an item used with a text (`DoUseItemInput`) goes to the consumers of the text
    if let Some(input) = m
        .player()
        .map(|p| p.items.input.clone())
        .filter(|t| !t.is_empty())
    {
        return use_with_input(m, env, prop, &input, ux, fx);
    }
    // the rows that apply buffs or depend on them (WP-ITEM-20)
    if let Some(r) = crate::item::buffitems::dispatch_system(m, env, prop, ux, fx) {
        return r;
    }
    let c = env.stats();
    let now_unix = env.now.0;
    match prop.id {
        // ---- the pet scrolls (WP-ITEM-16) ----
        II_SYS_SYS_SCR_PET_LIFE | II_SYS_SYS_SCR_PET_LIFE02 => {
            let cx = crate::buff::BuffCx::new(*env, ux.now);
            crate::item::pets::system::use_life_scroll(m, &cx, prop.id, &mut *ux.rng, fx)
        }
        II_SYS_SYS_SCR_PET_HATCH => {
            let cx = crate::buff::BuffCx::new(*env, ux.now);
            crate::item::pets::system::use_hatch_scroll(m, &cx, &mut *ux.rng, fx)
        }
        II_CHR_SYS_SCR_RESTATE => {
            let b = m.mv.base;
            if [b.str, b.sta, b.dex, b.int].iter().all(|&v| v == 15) {
                fx.text(TID_GAME_DONOTUSE_RESTATE);
                return FALLTHROUGH;
            }
            honor(m, fx);
            match restat_full(m, c, &mut fx.out) {
                Some(_) => CONSUME,
                None => FALLTHROUGH,
            }
        }
        II_CHR_SYS_SCR_RESTATE_STR => restat(m, c, 0, false, fx),
        II_CHR_SYS_SCR_RESTATE_STA => restat(m, c, 1, false, fx),
        II_CHR_SYS_SCR_RESTATE_DEX => restat(m, c, 2, false, fx),
        II_CHR_SYS_SCR_RESTATE_INT => restat(m, c, 3, false, fx),
        II_CHR_SYS_SCR_RESTATE_STR_LOW => restat(m, c, 0, true, fx),
        II_CHR_SYS_SCR_RESTATE_STA_LOW => restat(m, c, 1, true, fx),
        II_CHR_SYS_SCR_RESTATE_DEX_LOW => restat(m, c, 2, true, fx),
        II_CHR_SYS_SCR_RESTATE_INT_LOW => restat(m, c, 3, true, fx),
        II_CHR_SYS_SCR_TRANSY => {
            if sex_change(m, env, ux.part, fx) {
                CONSUME
            } else {
                FALLTHROUGH
            }
        }
        // the skill reset (`InitSkillExp`, 16 §12.3): all skills but the master and hero ones back to level 0, the
        // points refunded, `TaskBar` and `InitSkillPoint`; a missing skill prop leaves the scroll unused
        II_SYS_SYS_SCR_RECCURENCE => {
            if crate::stats::skills::init_skill_exp(m, c, &mut fx.out) {
                CONSUME
            } else {
                FALLTHROUGH
            }
        }
        II_SYS_SYS_SCR_PERIN => perin(m, env, ux, fx),
        II_SYS_SYS_SCR_CUSTODY2 => {
            fx.then(Followup::SpawnInstantBank { player: m.id() });
            CONSUME
        }
        // ---- the premium-mode scrolls (WP-ITEM-21) ----
        II_CHR_SYS_SCR_SHOUTFULL15 => {
            sm_scroll(m, prop, SM_SHOUT15, sm::is_shout_full(m), now_unix, fx)
        }
        II_CHR_SYS_SCR_SHOUTFULL30 => {
            sm_scroll(m, prop, SM_SHOUT30, sm::is_shout_full(m), now_unix, fx)
        }
        II_CHR_SYS_SCR_SHOUTFULL001 => {
            sm_scroll(m, prop, SM_SHOUT001, sm::is_shout_full(m), now_unix, fx)
        }
        II_CHR_SYS_SCR_PSKILLFULL1 => {
            sm_scroll(m, prop, SM_PARTYSKILL1, sm::is_party_skill(m), now_unix, fx)
        }
        II_CHR_SYS_SCR_PSKILLFULL15 => sm_scroll(
            m,
            prop,
            SM_PARTYSKILL15,
            sm::is_party_skill(m),
            now_unix,
            fx,
        ),
        II_CHR_SYS_SCR_PSKILLFULL30 => sm_scroll(
            m,
            prop,
            SM_PARTYSKILL30,
            sm::is_party_skill(m),
            now_unix,
            fx,
        ),
        II_CHR_SYS_SCR_COMMBANK15 => {
            sm_scroll(m, prop, SM_BANK15, sm::is_comm_bank(m), now_unix, fx)
        }
        II_CHR_SYS_SCR_COMMBANK30 => {
            sm_scroll(m, prop, SM_BANK30, sm::is_comm_bank(m), now_unix, fx)
        }
        II_CHR_SYS_SCR_COMMBANK001 => {
            sm_scroll(m, prop, SM_BANK001, sm::is_comm_bank(m), now_unix, fx)
        }
        II_CHR_SYS_SCR_ACTIVITION => {
            let busy = sm::sm_active(m, SM_ACTPOINT);
            sm_scroll(m, prop, SM_ACTPOINT, busy, now_unix, fx)
        }
        II_CHR_SYS_SCR_UPCUTSTONE01 => {
            let busy = sm::sm_active(m, SM_ATTACK_UP1) || sm::sm_active(m, SM_ATTACK_UP);
            sm_scroll(m, prop, SM_ATTACK_UP1, busy, now_unix, fx)
        }
        II_CHR_SYS_SCR_UPCUTSTONE => {
            let busy = sm::sm_active(m, SM_ATTACK_UP1) || sm::sm_active(m, SM_ATTACK_UP);
            sm_scroll(m, prop, SM_ATTACK_UP, busy, now_unix, fx)
        }
        II_SYS_SYS_SCR_BLESSING => {
            let busy = sm::sm_active(m, SM_REVIVAL);
            sm_scroll(m, prop, SM_REVIVAL, busy, now_unix, fx)
        }
        II_SYS_SYS_SCR_VELOCIJUMP => match sm::use_velocity_jump(m, env, prop, fx) {
            sm::SmUse::Used => CONSUME,
            _ => IN_USE,
        },
        // the friend summon (WP-SOCIAL-19, `DoUseItemSystem` result 99): the use opens the name dialog and never
        // consumes the scroll; the success of the summon does (the party summon scroll is used with `SummonParty`)
        II_SYS_SYS_SCR_FRIENDSUMMON_A | II_SYS_SYS_SCR_FRIENDSUMMON_B => {
            fx.then(Followup::FriendSummon {
                player: m.id(),
                slot: ux.id,
            });
            REFUSED
        }
        _ if later(prop) => Sys::NotYet,
        // `II_SYS_SYS_SCR_CHANAM` (the rename goes through its own message), the seals (PL-144) and every
        // other id: not consumed; a gift box takes it from here
        _ => FALLTHROUGH,
    }
}

/// `CUser::DoUseItemInput` (`User.cpp:8595-8621`): the consumers of the text of a `DoUseItemInput` - the tickets
/// (ITEM-24) and the pet naming scroll; any other system item is not used (result 1).
fn use_with_input(
    m: &mut Mover,
    env: &ItemEnv<'_>,
    prop: &ItemProp,
    input: &str,
    ux: &RowCx<'_>,
    fx: &mut Fx,
) -> Sys {
    if prop.item_kind3 == IK3_TICKET {
        return crate::item::ticket::use_ticket(m, env, prop, input, ux, fx);
    }
    match prop.id {
        II_SYS_SYS_SCR_PET_NAMING => {
            let cx = crate::buff::BuffCx::new(*env, ux.now);
            crate::item::pets::system::name_pet(m, &cx, input, fx)
        }
        _ => FALLTHROUGH,
    }
}

/// The honor re-check after a restat (the item router of the original, 16 §7.3): a follow-up.
fn honor(m: &Mover, fx: &mut Fx) {
    fx.then(Followup::HonorRecheck { player: m.id() });
}

/// `ReStateOne(kind)` / `ReStateOneLow(kind)` of the restat scrolls.
fn restat(m: &mut Mover, c: &crate::stats::StatsCore, kind: u8, low: bool, fx: &mut Fx) -> Sys {
    let done = if low {
        restat_one_low(m, c, kind, &mut fx.out)
    } else {
        restat_one(m, c, kind, &mut fx.out)
    };
    match done {
        Some(_) => {
            honor(m, fx);
            CONSUME
        }
        None => FALLTHROUGH,
    }
}

/// A premium-mode scroll: `busy` -> result 2; else the mode starts for `circle_time` seconds and the
/// scroll is used up. A party-skill mode also tells the social services (queued by `sm::set`).
fn sm_scroll(
    m: &mut Mover,
    prop: &ItemProp,
    slot: usize,
    busy: bool,
    now_unix: i64,
    fx: &mut Fx,
) -> Sys {
    if busy {
        return IN_USE;
    }
    sm::set(m, slot, prop.circle_time, now_unix, &mut fx.out);
    CONSUME
}

/// `DoUseItemSexChange(face)` (`MoverSkill.cpp:3161-3195`): refused with `TID_GAME_CHECK_EQUIP` while an armour
/// or fashion part is worn (head, hair, weapons, shield, ride and the jewelry and ammunition slots are
/// allowed); else the sex toggles, the mover index becomes `MI_MALE` / `MI_FEMALE`, the scale and the stats of
/// the worn items are redone (silently), `SexChange` and `ChangeFaceNtf` go to everybody around, and the
/// disguise buffs end.
pub fn sex_change(m: &mut Mover, env: &ItemEnv<'_>, face: i32, fx: &mut Fx) -> bool {
    let Some(p) = m.player() else { return false };
    for part in 0..MAX_HUMAN_PARTS {
        if matches!(
            part,
            PARTS_HEAD | PARTS_HAIR | PARTS_LWEAPON | PARTS_RWEAPON | PARTS_SHIELD | PARTS_RIDE
        ) || (PARTS_NECKLACE1..=PARTS_BULLET).contains(&part)
        {
            continue;
        }
        if p.items.inventory.get_equip(part).is_some() {
            fx.text(TID_GAME_CHECK_EQUIP);
            return false;
        }
    }
    let was_male = p.appearance.sex == 0;
    let new_index = if was_male {
        flyff_common::defs::obj::MI_FEMALE
    } else {
        flyff_common::defs::obj::MI_MALE
    };
    if let Some(p) = m.player_mut() {
        p.appearance.sex = u8::from(was_male);
        p.appearance.head_mesh = u8::try_from(face.clamp(0, 255)).unwrap_or(0);
    }
    m.core.index = new_index;
    m.core.scale = crate::mover::model_scale(env.data(), new_index);
    m.sim.params.scale = m.core.scale;
    let mut quiet = crate::stats::out::Out::new();
    crate::stats::equip::redo_equip(m, env, false, &mut quiet);
    fx.out.near(pb::SexChange {
        mover_index: new_index,
    });
    let id_player = m.player().map_or(0, |p| p.ident.character_id.0);
    fx.out.near(pb::ChangeFaceNtf {
        id_player,
        face: face.max(0) as u32,
    });
    // the disguise buffs end with the old body
    let cx = crate::buff::BuffCx::new(*env, flyff_common::GameMs(0));
    crate::buff::core::remove_buffs(
        m,
        &cx,
        crate::buff::RemoveFlags::IK3,
        flyff_common::defs::itemkind::IK3_TEXT_DISGUISE,
        fx,
    );
    true
}

/// `II_SYS_SYS_SCR_PERIN` (`MoverSkill.cpp:2692-2760`): 100 000 000 penya, unless the player is trading,
/// dead, vending, fighting or flying (`TID_GAME_STATE_NOTUSE` with the reason, result 99) or already holds
/// 2 000 000 000 (`TID_GAME_TOOMANYMONEY_USE_PERIN` as a diagnostic line, result 99).
fn perin(m: &mut Mover, env: &ItemEnv<'_>, ux: &RowCx<'_>, fx: &mut Fx) -> Sys {
    let state = summon_state(m, ux.facts);
    if state != 0 {
        let why = match state {
            1 => TID_GAME_TRADE_NOTUSE,
            2 => TID_GAME_DIE_NOTUSE,
            3 => TID_GAME_VENDOR_NOTUSE,
            4 => TID_GAME_ATTACK_NOTUSE,
            5 => TID_GAME_FLY_NOTUSE,
            _ => 0,
        };
        let reason = env.data().texts.get(why).to_owned();
        fx.defined(TID_GAME_STATE_NOTUSE, vec![format!("\"{reason}\"")]);
        return REFUSED;
    }
    let gold = crate::item::gold::core::gold(m);
    if !(0..2_000_000_000).contains(&gold) {
        let text = env
            .data()
            .texts
            .get(TID_GAME_TOOMANYMONEY_USE_PERIN)
            .to_owned();
        fx.out.to_self(pb::Text {
            kind: flyff_rules::chat::TEXT_DIAG,
            text,
        });
        return REFUSED;
    }
    crate::item::gold::core::add_gold(m, env, 100_000_000, true, fx);
    CONSUME
}

/// `II_SYS_SYS_SCR_CUSTODY2`: the instant-bank NPC `MaFl_InstantBank` (a penguin) appears at the player and
/// disappears after 180 s (`TIMEWAIT_INSTANT_BANKER`, `ProcInstantBanker`).
pub fn spawn_instant_bank(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    use flyff_common::defs::obj::MI_INFO_PENG;
    const KEY: &str = "MaFl_InstantBank";
    let data = std::sync::Arc::clone(ctx.data);
    let Some(prop) = data.mover(MI_INFO_PENG) else {
        return;
    };
    let Some((world, layer, pos)) = ctx
        .worlds
        .player(who.0)
        .map(|m| (m.core.world, m.core.layer, m.core.pos))
    else {
        return;
    };
    let (core, sim) = crate::mover::npc_parts(&data, prop, pos, 0.0);
    let world_rng = ctx.rng.world(world);
    let mut mover = Mover::new_npc(core, sim, prop, data.npc(KEY), &data, world_rng);
    if let crate::mover::MoverKind::Npc(n) = &mut mover.kind {
        n.character_key = KEY.to_owned();
    }
    match ctx.worlds.spawn_mover(world, layer, mover) {
        Ok(id) => {
            let due = flyff_common::GameMs(ctx.clock.game_ms.0 + INSTANT_BANKER_MS);
            svc.items.track_instant_bank(due, id);
        }
        Err(e) => tracing::warn!("instant bank: {e}"),
    }
}

/// `TIMEWAIT_INSTANT_BANKER`.
pub const INSTANT_BANKER_MS: u64 = 180_000;
