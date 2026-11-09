//! `UseSkillReq` and `CMover::DoUseSkill` (Plan/17 §6.3, §6.4; WP-SKILL-04;
//! `src:_Common/MoverSkill.cpp:171-1074`, `src:WORLDSERVER/DPSrvrLux.cpp:32-100`).
//!
//! [`on_use_skill_req`] is the handler: the VR-004 checks of the request, then [`do_use_skill`]; a refusal is
//! answered with `ClearUseSkill`, a successful `SUT_QUEUESTART` arms the action-slot cursor.
//!
//! [`use_skill_direct`] is the second `DoUseSkill(skill, level, target, sut, control, cast)` of the original with
//! its **22 ordered steps**; it takes the cast time as an argument (the slot-based entry computes it) and works
//! for players and for movers without a job table. Every refusal reports its step number ([`Refusal::step`]) and,
//! where the original tells the player, the text (`DefinedText`). It runs on an [`ActionCx`], so it can be used by
//! a client handler and by the world phase (the action-slot queue starts its next skill at the barrier of the frame
//! the previous skill ends).
//!
//! Costs: a `KT_SKILL` pays FP (then MP) the moment the state starts; a `KT_MAGIC` pays at the hit frame
//! ([`super::cast`]), except the "use all" sentinel (`req == 9 999 999`), which empties MP and FP now.
//! Cool-downs are set at the hit ([`super::cast`]); this module only reads them (step 16).

use super::{REQ_ALL, SLOTS, SkillRuntime};
use crate::buff::set::{BuffKey, BuffKind};
use crate::combat::action::{face_target, send_plain_msg};
use crate::combat::context::ActionCx;
use crate::combat::state::{ActParams, ObjAct, is_die, set_atk_cnt};
use crate::combat::target::set_jjim;
use crate::combat::view::{active_hand_prop, combatant, worn_item};
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::mover::Mover;
use crate::stats::StatsCore;
use crate::stats::env::{max_fp, max_mp, point_param};
use crate::stats::out::Out;
use crate::stats::pool::{inc_fp, inc_mp};
use flyff_common::defs::attribute::{HD_DUAL, KT_MAGIC, KT_SKILL};
use flyff_common::defs::define::{EXT_SELFCHGPARAMET, WUI_NOW};
use flyff_common::defs::itemkind::{
    IK3_BOW, IK3_ENCHANTWEAPON, IK3_MAGICBOTH, IK3_SHIELD, IK3_STAFF, IK3_WAND, IK3_YOBO, IK3_YOYO,
};
use flyff_common::defs::neuz::{PARTS_BULLET, PARTS_SHIELD};
use flyff_common::defs::skill::{
    SI_ACR_SUP_DARKILLUSION, SI_ACR_YOYO_SNITCH, SI_ASS_CHEER_ACCURACY, SI_ASS_CHEER_HASTE,
    SI_ASS_CHEER_HEAPUP, SI_ASS_CHEER_QUICKSTEP, SI_ASS_HEAL_HEALING, SI_ASS_HEAL_RESURRECTION,
    SI_BIL_PST_ASALRAALAIKUM, SI_BLD_SUP_BERSERK, SI_GEN_ATK_COUNTER, SI_GEN_EVE_ACCURACY,
    SI_GEN_EVE_HASTE, SI_GEN_EVE_HEAPUP, SI_GEN_EVE_QUICKSTEP, SI_JST_YOYO_VATALSTAB,
    SI_MAG_MAG_BLINKPOOL, SI_RIG_HERO_RETURN,
};
use flyff_common::defs::text::{
    TID_GAME_DONOTUSEBUFF, TID_GAME_GUILDCOMBAT_STANDS_NOTUSESKILL, TID_GAME_NEVERKILLSTOP,
    TID_GAME_NPCBUFF_FAILED, TID_GAME_RAINBOWRACE_NOTELEPORT, TID_GAME_REQ_DARK, TID_GAME_REQFP,
    TID_GAME_REQMP, TID_GAME_SKILLWAITTIME, TID_GAME_WRONGITEM, TID_GMAE_PK_NOT_AREA,
    TID_SKILL_NOTELEWEAPON, TID_TIP_NEEDSATTACKITEM, TID_TIP_NEEDSKILLITEM,
};
use flyff_common::defs::world::{WI_WORLD_ARENA, WI_WORLD_GUILDWAR, WI_WORLD_QUIZ};
use flyff_common::{GameMs, ObjId};
use flyff_data::props::item::ItemProp;
use flyff_data::props::skill_add::AddSkillProp;
use flyff_proto::pb;
use flyff_rules::chat::TEXT_GENERAL;
use flyff_rules::combat::{CombatantView, Hand, HitType};
use flyff_rules::consts::dst::{CHS_SILENT, DST_CHRSTATE, DST_FP, DST_MP, DST_SKILL_LEVEL};
use flyff_rules::consts::parts::{PARTS_LWEAPON, PARTS_RWEAPON};
use flyff_rules::consts::{NO_ATTACK_MODE, NULL_ID};
use flyff_rules::jobs::{is_hero, is_master};
use flyff_rules::skill::{casting_time, req_fp, req_mp};
use flyff_sim::ActMsg;
use flyff_sim::mover::SkillCtx;
use flyff_sim::types::{
    ACT_DROP, ACT_OK, MVRF_NOATTACK, MVRF_SKILL, OBJACT_USESKILL, OBJMSG_MAGICCASTING,
    OBJMSG_MAGICSKILL, OBJMSG_MELEESKILL, OBJMSG_STOP,
};

/// `SKILLUSETYPE`: how a skill is started.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkillUseType {
    /// `SUT_NORMAL`: an ordinary use.
    Normal = 0,
    /// `SUT_QUEUESTART`: the first skill of an action-slot chain (the cast time covers the whole queue).
    QueueStart = 1,
    /// `SUT_QUEUEING`: a later skill of the chain (no cast animation).
    Queueing = 2,
}

impl SkillUseType {
    /// The wire value (`SKILLUSETYPE`).
    pub fn wire(self) -> i32 {
        self as i32
    }

    /// VR-004: `0..=2`, else `None`.
    pub fn from_wire(v: i32) -> Option<SkillUseType> {
        match v {
            0 => Some(SkillUseType::Normal),
            1 => Some(SkillUseType::QueueStart),
            2 => Some(SkillUseType::Queueing),
            _ => None,
        }
    }
}

/// A refused skill start: the number of the step of Plan/17 §6.3 that said no (1..=20).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Refusal {
    /// The step (see the module documentation of [`use_skill_direct`]).
    pub step: u8,
}

/// What a successful `DoUseSkill` did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Started {
    /// The action state started (the sim answered `1`): costs paid, `UseSkillNtf` sent.
    Started,
    /// The sim answered `0`, `-1` or `-3`: nothing started and nothing was paid, yet the original returns `TRUE`
    /// (KEEP-QUIRK: the request is not answered with `ClearUseSkill`).
    NotStarted,
}

fn fail(step: u8) -> Result<Started, Refusal> {
    Err(Refusal { step })
}

// ---- levels, props, queue time --------------------------------------------------------------------------------

/// `CMover::GetSkillLevel(slot)` (`__SKILL0517`): `min(param(DST_SKILL_LEVEL, level), expert_max)`; 0 for an empty
/// slot, a skill the data does not know and a mover without a job table. The `(DWORD)` conversion is the
/// original's: a negative `DST_SKILL_LEVEL` sum wraps to a huge number and is capped to `expert_max` (KEEP).
pub fn effective_level(m: &Mover, core: &StatsCore, slot: usize) -> u32 {
    let Some(s) = m
        .player()
        .and_then(|p| p.progression.job_skills.get(slot))
        .filter(|s| !s.is_empty())
    else {
        return 0;
    };
    let Some(prop) = core.data.skill(s.skill_id) else {
        return 0;
    };
    let lv =
        m.mv.dst
            .table
            .param(DST_SKILL_LEVEL, i32::try_from(s.level).unwrap_or(i32::MAX)) as u32;
    lv.min(prop.expert_max)
}

/// `GetSkill(skill)` + `GetSkillLevel`: the effective level of `skill` when the player has it in its job table
/// (possibly 0), else `default`.
pub fn known_level_or(m: &Mover, core: &StatsCore, skill: u32, default: u32) -> u32 {
    slot_of(m, skill).map_or(default, |slot| effective_level(m, core, slot))
}

/// The slot of `skill` in the job table (`GetSkillIdx`), `None` when the mover does not have it.
pub fn slot_of(m: &Mover, skill: u32) -> Option<usize> {
    m.player()?
        .progression
        .job_skills
        .iter()
        .position(|s| s.skill_id == skill)
}

/// The rows of `skill` at `level` (`GetSkillProp`): the skill row and the level row.
pub fn props(core: &StatsCore, skill: u32, level: u32) -> Option<(&ItemProp, &AddSkillProp)> {
    let prop = core.data.skill(skill)?;
    let add = core.data.add_skill(prop.sub_define, level)?;
    Some((prop, add))
}

/// `CMover::GetQueueCastingTime` (`MoverSkill.cpp:97-150`): the casting times of every non-empty slot of the
/// action slot, each at the effective level of its skill, summed, cut to 70 %, converted to ticks and reduced by
/// `DST_SPELL_RATE`. A queue entry whose skill or level row is missing makes the legacy function return 0 (an
/// error is logged); the same here.
pub fn queue_casting_ticks(m: &Mover, core: &StatsCore) -> i32 {
    let Some(p) = m.player() else {
        return 0;
    };
    let mut total: i32 = 0;
    for q in &p.taskbar.queue {
        if q.shortcut.kind == 0 {
            continue;
        }
        let slot = q.shortcut.id as usize;
        let skill = p
            .progression
            .job_skills
            .get(slot)
            .map_or(NULL_ID, |s| s.skill_id);
        let level = effective_level(m, core, slot);
        let Some((_, add)) = props(core, skill, level) else {
            return 0;
        };
        total = total.wrapping_add(add.casting_time as i32);
    }
    let total = (total as f32 * 0.7_f32) as i32;
    let ticks = ((total as f32 / 1000.0_f32) * 15.0_f32) as i32;
    casting_time(&m.mv.dst.table, ticks)
}

/// The animation speed a started skill carries on the wire (`UseSkillNtf.ani_speed`, PL-087): the casting
/// speed for a cast and, with `features.speed_cast`, a melee skill; a player only (the sim plays NPCs at 1.0).
fn skill_ani_speed(m: &Mover, skill_type: u32, speed_cast: bool) -> f32 {
    if !m.is_player() {
        return 1.0;
    }
    if skill_type == KT_MAGIC || speed_cast {
        m.sim.params.casting_ani_speed
    } else {
        1.0
    }
}

// ---- the client handler ---------------------------------------------------------------------------------------

/// `UseSkillReq` (`OnUseSkill`, `DPSrvrLux.cpp:32-100`): `slot >= 45`, a vending player, an empty slot, an
/// effective level of 0 or above `expert_max` and a `use_type` above 2 (VR-004) are refused with `ClearUseSkill`
/// (the original silently ignored them). Then [`do_use_skill`]; a refusal sends `ClearUseSkill` to the player, a
/// success of `SUT_QUEUESTART` arms the action-slot cursor (`m_nUsedSkillQueue = 0`).
pub fn on_use_skill_req(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    req: pb::UseSkillReq,
) {
    let me = who.0;
    let core = svc.stats.handle();
    let sut = SkillUseType::from_wire(req.use_type);
    let slot = req.slot as usize;
    let ok = (|| {
        let sut = sut?;
        if slot >= SLOTS {
            return None;
        }
        let m = ctx.worlds.player(me)?;
        if m.player()?.econ.vending {
            return None;
        }
        let s = m.player()?.progression.job_skills.get(slot)?;
        if s.is_empty() {
            return None;
        }
        let prop = core.data.skill(s.skill_id)?;
        let level = effective_level(m, &core, slot);
        (level != 0 && level <= prop.expert_max).then_some(sut)
    })();
    let Some(sut) = ok else {
        refuse(svc, ctx, me);
        return;
    };
    let target = ObjId(req.target);
    let started = crate::combat::with_action(svc, ctx, me, |cx| {
        do_use_skill(cx, me, slot, target, sut, req.control)
    });
    match started {
        Some(Ok(_)) => {
            if sut == SkillUseType::QueueStart
                && let Some(p) = ctx.worlds.player_mut(me).and_then(Mover::player_mut)
            {
                p.skills_rt.queue_cursor = Some(0);
            }
        }
        _ => refuse(svc, ctx, me),
    }
}

/// `SNAPSHOTTYPE_CLEAR_USESKILL` to the player.
fn refuse(_svc: &mut Services, ctx: &mut GameCtx<'_>, me: ObjId) {
    let mut out = Out::new();
    out.to_self(pb::ClearUseSkill {});
    out.flush(ctx, me);
}

/// `CMover::DoUseSkill(type, idx, target, sut, control)` (`MoverSkill.cpp:171-283`): the slot-based entry. It
/// resolves the skill and its effective level, computes the cast time in ticks and runs [`use_skill_direct`].
pub fn do_use_skill(
    cx: &mut ActionCx<'_>,
    caster: ObjId,
    slot: usize,
    target: ObjId,
    sut: SkillUseType,
    control: bool,
) -> Result<Started, Refusal> {
    let core = cx.arb.env.core;
    let Some(m) = cx.mover(caster) else {
        return fail(0);
    };
    if !m.is_player() {
        return fail(0);
    }
    let Some(s) = m
        .player()
        .and_then(|p| p.progression.job_skills.get(slot))
        .filter(|s| !s.is_empty())
    else {
        return fail(0);
    };
    let skill = s.skill_id;
    let level = effective_level(m, core, slot);
    if level == 0 {
        return fail(0);
    }
    let Some((prop, add)) = props(core, skill, level) else {
        return fail(0);
    };
    let cast = match prop.skill_type {
        KT_MAGIC => match sut {
            SkillUseType::QueueStart => queue_casting_ticks(m, core),
            SkillUseType::Queueing => 0,
            SkillUseType::Normal => {
                flyff_rules::skill::casting_ticks(&m.mv.dst.table, add.casting_time)
            }
        },
        KT_SKILL => 1,
        _ => 0,
    };
    use_skill_direct(cx, caster, skill, level, target, sut, control, cast)
}

/// Reads whether the player `m` holds a CHAT-relevant state: silenced.
fn silenced(m: &Mover) -> bool {
    (m.mv.dst.table.adj(DST_CHRSTATE) as u32) & CHS_SILENT != 0
}

/// A refusal as the read-only checks see it: the step, what to tell whom, and whether the caster already turned
/// toward the target (step 8 turns before the later steps can still say no).
struct Reject {
    step: u8,
    say: Vec<(ObjId, pb::server_msg::Msg)>,
    turned: bool,
}

/// What the checks of steps 1-16 hand on.
struct Checked {
    /// Step 8 turned the caster toward this target.
    turn_to: Option<ObjId>,
}

/// Everything the read-only checks look at.
struct CheckCx<'a> {
    store: &'a crate::world::object::ObjectStore,
    hooks: &'a dyn crate::combat::CombatHooks,
    core: &'a StatsCore,
    act: crate::combat::ActionConfig,
    steal_protection: bool,
    /// The PvP switches (`events.pk`, wars, `PKSetting.inc`) for `GetHitType2` and the region rule.
    pvp: crate::combat::pvp::PvpEnv,
    now: GameMs,
}

fn say(tid: u32) -> pb::server_msg::Msg {
    defined(tid).into()
}

/// Steps 1-16 of `DoUseSkill`, without touching anything (the caller applies the turn of step 8 and the texts).
#[allow(clippy::too_many_arguments)]
fn pre_checks(
    c: &CheckCx<'_>,
    caster: ObjId,
    target_id: ObjId,
    prop: &ItemProp,
    add: &AddSkillProp,
    level: u32,
) -> Result<Checked, Reject> {
    let rej = |step: u8, say: Vec<(ObjId, pb::server_msg::Msg)>, turned: bool| Reject {
        step,
        say,
        turned,
    };
    let act = c.act;
    let core = c.core;
    let valid = |id: ObjId| c.store.mover(id).filter(|m| !m.core.deleted);
    let Some(m) = valid(caster) else {
        return Err(rej(0, vec![], false));
    };
    let player = m.is_player();
    let tell = |tid: u32| -> Vec<(ObjId, pb::server_msg::Msg)> {
        if player {
            vec![(caster, say(tid))]
        } else {
            vec![]
        }
    };

    // 1: the caster
    if is_die(m) || m.sim.act.flags.fly() || m.sim.mvrf.has(MVRF_NOATTACK) {
        return Err(rej(1, vec![], false));
    }
    let world = m.core.world.0;
    if world == WI_WORLD_QUIZ {
        return Err(rej(1, vec![], false));
    }
    if world == WI_WORLD_GUILDWAR && m.player().is_some_and(|p| p.social.guild_combat_state == 0) {
        return Err(rej(1, tell(TID_GAME_GUILDCOMBAT_STANDS_NOTUSESKILL), false));
    }
    if m.mv.mode & NO_ATTACK_MODE != 0 {
        return Err(rej(1, vec![], false));
    }

    // 2: the target
    let same_layer = valid(target_id)
        .is_some_and(|t| t.core.world == m.core.world && t.core.layer == m.core.layer);
    let Some(t) = valid(target_id).filter(|_| same_layer) else {
        return Err(rej(2, vec![], false));
    };
    if prop.id == SI_ASS_HEAL_HEALING && is_die(t) {
        return Err(rej(2, vec![], false));
    }
    let others = target_id != caster;

    // 3: F-177, hostile skills need a PvP-capable ground
    if prop.evildoing < 0
        && player
        && t.is_player()
        && others
        && crate::combat::pvp::skill_region_refused(m, t, &c.pvp, &core.rules, c.hooks)
    {
        return Err(rej(3, tell(TID_GMAE_PK_NOT_AREA), false));
    }

    // 4: Berserk forbids skills in the vanilla build
    if !act.better_berserk
        && m.buffs
            .contains(BuffKey::new(BuffKind::Skill, SI_BLD_SUP_BERSERK))
    {
        return Err(rej(4, vec![], false));
    }

    // 5: the Cheers do not stack on the NPC buffs of the same name
    let eve = match prop.id {
        SI_ASS_CHEER_QUICKSTEP => Some(SI_GEN_EVE_QUICKSTEP),
        SI_ASS_CHEER_HASTE => Some(SI_GEN_EVE_HASTE),
        SI_ASS_CHEER_HEAPUP => Some(SI_GEN_EVE_HEAPUP),
        SI_ASS_CHEER_ACCURACY => Some(SI_GEN_EVE_ACCURACY),
        _ => None,
    };
    if let Some(e) = eve
        && t.buffs.contains(BuffKey::new(BuffKind::Skill, e))
    {
        let say = if t.is_player() {
            vec![(
                target_id,
                pb::DefinedText {
                    tid: TID_GAME_NPCBUFF_FAILED as i32,
                    args: vec![format!("\"{}\"", prop.name)],
                    display: pb::defined_text::Display::Message as i32,
                }
                .into(),
            )]
        } else {
            vec![]
        };
        return Err(rej(5, say, false));
    }

    // 6: Snitch and Vital Stab are Dark Illusion skills
    if matches!(prop.id, SI_ACR_YOYO_SNITCH | SI_JST_YOYO_VATALSTAB)
        && !m
            .buffs
            .contains(BuffKey::new(BuffKind::Skill, SI_ACR_SUP_DARKILLUSION))
    {
        return Err(rej(6, tell(TID_GAME_REQ_DARK), false));
    }

    // 7: Hero Return and the rainbow race
    if !act.rm_hero_skill
        && prop.id == SI_RIG_HERO_RETURN
        && player
        && c.hooks.rainbow_race_entry(m)
    {
        return Err(rej(7, tell(TID_GAME_RAINBOWRACE_NOTELEPORT), false));
    }

    // 8: skills on somebody else
    let mut turn_to = None;
    if others {
        if prop.use_chance == WUI_NOW || prop.exe_target == EXT_SELFCHGPARAMET {
            return Err(rej(8, vec![], false));
        }
        if !t.is_player() && crate::combat::is_peaceful(t) {
            return Err(rej(8, vec![], false));
        }
        if c.steal_protection
            && prop.evildoing < 0
            && crate::combat::arbiter::steal_check(c.store, &core.data, caster, target_id)
        {
            // `IsSteal` tells the attacker
            return Err(rej(
                8,
                tell(flyff_common::defs::text::TID_GAME_PRIORITYMOB),
                false,
            ));
        }
        if world != WI_WORLD_ARENA
            && prop.evildoing > 0
            && crate::combat::pvp::hit_type2(m, t, &c.pvp, c.hooks, true) != HitType::Fail
        {
            return Err(rej(8, tell(TID_GAME_NEVERKILLSTOP), false));
        }
        turn_to = Some(target_id);
    }
    let turned = turn_to.is_some();

    // 9: the guild war and the school event
    if world == WI_WORLD_GUILDWAR {
        if prop.id == SI_MAG_MAG_BLINKPOOL || (!act.rm_hero_skill && prop.id == SI_RIG_HERO_RETURN)
        {
            return Err(rej(9, vec![], turned));
        }
        if !c.hooks.guild_combat_war_state() && prop.id == SI_ASS_HEAL_RESURRECTION {
            return Err(rej(9, vec![], turned));
        }
    }
    if act.school_event && prop.id == SI_ASS_HEAL_RESURRECTION {
        return Err(rej(9, vec![], turned));
    }

    // 10: the weapon the skill needs
    let hand_prop = if prop.link_kind == IK3_SHIELD {
        worn_item(m, PARTS_SHIELD).and_then(|w| core.data.item(w.item_id))
    } else {
        active_hand_prop(m, core)
    };
    let kind3 = hand_prop.map_or(NULL_ID, |p| p.item_kind3);
    match prop.link_kind {
        IK3_ENCHANTWEAPON => {
            if player {
                let right = worn_item(m, PARTS_RWEAPON);
                let left = worn_item(m, PARTS_LWEAPON);
                let bad = match (right, left) {
                    (Some(r), Some(l)) => r.element == 0 && l.element == 0,
                    (Some(r), None) => r.element == 0,
                    (None, None) => true,
                    // KEEP-QUIRK: a left-hand weapon alone is accepted
                    (None, Some(_)) => false,
                };
                if bad {
                    return Err(rej(10, tell(TID_SKILL_NOTELEWEAPON), turned));
                }
            }
        }
        IK3_MAGICBOTH => {
            if kind3 != IK3_WAND && kind3 != IK3_STAFF {
                return Err(rej(10, tell(TID_GAME_WRONGITEM), turned));
            }
        }
        IK3_YOBO => {
            if kind3 != IK3_YOYO && kind3 != IK3_BOW {
                return Err(rej(10, tell(TID_GAME_WRONGITEM), turned));
            }
        }
        other => {
            if other != NULL_ID && other != kind3 {
                return Err(rej(10, tell(TID_GAME_WRONGITEM), turned));
            }
        }
    }

    // 11: a buff of a higher level is on the target (`BUFF_SKILL == KT_MAGIC`)
    if prop.skill_type == KT_MAGIC
        && t.buffs
            .get(BuffKey::new(BuffKind::Skill, prop.id))
            .is_some_and(|b| b.level > level)
    {
        return Err(rej(11, tell(TID_GAME_DONOTUSEBUFF), turned));
    }

    // 12: one- / two-handed, dual
    if prop.handed != NULL_ID
        && let Some(hp) = hand_prop
        && player
    {
        let wrong = if prop.handed == HD_DUAL {
            !combatant(m, core, Hand::Right, None).is_dual_weapon()
        } else {
            hp.handed != prop.handed
        };
        if wrong {
            return Err(rej(12, tell(TID_GAME_WRONGITEM), turned));
        }
    }

    // 13: the ammunition (`IsBullet`)
    if prop.link_kind_bullet != NULL_ID {
        let bullet = if player {
            worn_item(m, PARTS_BULLET)
                .and_then(|w| core.data.item(w.item_id))
                .or(core.rules.hand.as_ref())
        } else {
            core.data.item(m.mv.virt_item)
        };
        if bullet.is_none_or(|b| b.item_kind2 != prop.link_kind_bullet) {
            let tid = if prop.item_kind3 == IK3_BOW || prop.link_kind == IK3_BOW {
                TID_TIP_NEEDSATTACKITEM
            } else {
                TID_TIP_NEEDSKILLITEM
            };
            return Err(rej(13, tell(tid), turned));
        }
    }

    // 14: the level and the prerequisites of a player
    if player {
        let legend = is_master(&core.rules, m.mv.job) || is_hero(&core.rules, m.mv.job);
        if prop.req_dis_lv != NULL_ID
            && i64::from(m.mv.level) < i64::from(prop.req_dis_lv)
            && !legend
        {
            return Err(rej(14, vec![], turned));
        }
        let table = m.player().map(|p| &p.progression.job_skills);
        for (need, need_level, text) in [
            (prop.re_skill1, prop.re_skill_level1, "SkillLevel1 "),
            (prop.re_skill2, prop.re_skill_level2, "SkillLevel2 "),
        ] {
            if need == NULL_ID {
                continue;
            }
            // a prerequisite the character does not have is only logged by the original
            let have = table.and_then(|t| t.iter().find(|s| s.skill_id == need));
            if let Some(s) = have
                && s.level < need_level
            {
                let text = pb::Text {
                    kind: TEXT_GENERAL,
                    text: text.to_owned(),
                };
                return Err(rej(14, vec![(caster, text.into())], turned));
            }
        }
    }

    // 15: Asalraalaikum needs its FP up front
    if prop.id == SI_BIL_PST_ASALRAALAIKUM
        && player
        && point_param(m, core, DST_FP) < req_fp(&m.mv.dst.table, add.req_fp)
    {
        return Err(rej(15, tell(TID_GAME_REQFP), turned));
    }

    // 16: the cool-down
    if let (Some(slot), Some(p)) = (slot_of(m, prop.id), m.player())
        && p.skills_rt.reuse_delay(slot, c.now) > 0
    {
        return Err(rej(16, tell(TID_GAME_SKILLWAITTIME), turned));
    }
    Ok(Checked { turn_to })
}

/// `DoUseSkill(skill, level, target, sut, control, cast)` (`MoverSkill.cpp:288-1074`): the 22 steps of Plan/17
/// §6.3. `cast` is the cast time in ticks. A refusal carries the step number; the steps that tell the player
/// do so through the effects of `cx`.
///
/// | step | refusal |
/// |---|---|
/// | 1 | caster dead, flying, `MVRF_NOATTACK`, in the quiz world; a guild-war spectator (text); `NO_ATTACK_MODE` |
/// | 2 | no target; Healing on a dead target |
/// | 3 | F-177 hostile skill between players in a zone that forbids it (text) |
/// | 4 | under Berserk without `better_berserk` |
/// | 5 | a Cheer on a target that holds the NPC buff of the same name (text to the target) |
/// | 6 | Snitch / Vital Stab without Dark Illusion (text) |
/// | 7 | Hero Return in the rainbow race without `rm_hero_skill` (text) |
/// | 8 | target rules (`WUI_NOW`, self-only skill on another, peaceful NPC, kill-steal, helping an enemy: text) |
/// | 9 | guild-war and school-event restrictions |
/// | 10 | the weapon the skill needs (text) |
/// | 11 | a higher level of the buff is on the target (text) |
/// | 12 | one- / two-handed / dual (text) |
/// | 13 | the ammunition (text) |
/// | 14 | character level, prerequisite skills (text) |
/// | 15 | Asalraalaikum without the FP (text) |
/// | 16 | cool-down (text) |
/// | 18 | MP / FP (text), silence |
/// | 20 | the sim refused the action (`-2`) |
///
/// Steps 17, 19, 21 and 22 cannot fail. Step 0 is "nothing to start" (no such skill row or mover).
#[allow(clippy::too_many_arguments)]
pub fn use_skill_direct(
    cx: &mut ActionCx<'_>,
    caster: ObjId,
    skill: u32,
    level: u32,
    target_id: ObjId,
    sut: SkillUseType,
    control: bool,
    cast: i32,
) -> Result<Started, Refusal> {
    let core = cx.arb.env.core;
    let act = cx.act;
    let now = cx.arb.w.now;
    let Some((prop, add)) = props(core, skill, level) else {
        return fail(0);
    };
    // steps 1-16: nothing is changed by the checks
    let checked = {
        let c = CheckCx {
            store: &*cx.arb.w.store,
            hooks: &*cx.arb.hooks,
            core,
            act,
            steal_protection: cx.arb.env.steal_protection,
            pvp: cx.arb.env.pvp,
            now,
        };
        pre_checks(&c, caster, target_id, prop, add, level)
    };
    let checked = match checked {
        Ok(c) => c,
        Err(rj) => {
            for (to, body) in rj.say {
                cx.arb.fx.to_self(to, body);
            }
            if rj.turned {
                face_target(cx, caster, target_id);
            }
            return fail(rj.step);
        }
    };
    if let Some(t) = checked.turn_to {
        face_target(cx, caster, t);
    }
    let caster_is_player = cx.mover(caster).is_some_and(Mover::is_player);

    // 17: stop and forget the destination
    send_plain_msg(cx, caster, ActMsg::new(OBJMSG_STOP));
    if let Some(m) = cx.mover_mut(caster) {
        m.sim.clear_dest();
    }

    // 18: the resources
    let Some(m) = cx.mover(caster) else {
        return fail(18);
    };
    let dst = &m.mv.dst.table;
    let (mp, fp) = (point_param(m, core, DST_MP), point_param(m, core, DST_FP));
    let mut motion = prop.use_motion as i32;
    let mut lacking: Option<u32> = None;
    let msg = match prop.skill_type {
        KT_SKILL => {
            if req_fp(dst, add.req_fp) == REQ_ALL {
                if mp != max_mp(m, core) {
                    lacking = Some(TID_GAME_REQMP);
                } else if fp != max_fp(m, core) {
                    lacking = Some(TID_GAME_REQFP);
                }
            } else if fp.wrapping_sub(req_fp(dst, add.req_fp)) < 0 {
                lacking = Some(TID_GAME_REQFP);
            } else if add.req_mp > 0 && mp.wrapping_sub(req_mp(dst, add.req_mp)) < 0 {
                lacking = Some(TID_GAME_REQMP);
            }
            OBJMSG_MELEESKILL
        }
        KT_MAGIC => {
            if silenced(m) {
                return fail(18);
            }
            if req_mp(dst, add.req_mp) == REQ_ALL {
                if mp != max_mp(m, core) {
                    lacking = Some(TID_GAME_REQMP);
                } else if fp != max_fp(m, core) {
                    lacking = Some(TID_GAME_REQFP);
                }
            } else if mp.wrapping_sub(req_mp(dst, add.req_mp)) < 0 {
                lacking = Some(TID_GAME_REQMP);
            }
            match sut {
                SkillUseType::Queueing => {
                    motion = motion.wrapping_add(2);
                    OBJMSG_MAGICSKILL
                }
                SkillUseType::QueueStart | SkillUseType::Normal => OBJMSG_MAGICCASTING,
            }
        }
        _ => return fail(18),
    };
    if let Some(tid) = lacking {
        if caster_is_player {
            cx.arb.fx.to_self(caster, defined(tid));
        }
        return fail(18);
    }

    // 19: no cast, no cast animation
    if sut != SkillUseType::Queueing && cast == 0 && prop.id != SI_GEN_ATK_COUNTER {
        motion = motion.wrapping_add(2);
    }

    // 20: the action state; the sim reads the skill context at the start (loop motions, the casting end)
    let prev_ctx = cx.mover(caster).map(|m| m.sim.skill);
    if let Some(m) = cx.mover_mut(caster) {
        let keep = m.sim.skill;
        flyff_sim::sync::set_skill(
            &mut m.sim,
            SkillCtx {
                skill,
                level,
                target: target_id,
                skill_count: add.skill_count,
                act: OBJACT_USESKILL,
                ..keep
            },
        );
    }
    let am = ActMsg::with(msg, [motion, target_id.0 as i32, cast, skill as i32, 0]);
    let mut rules = crate::combat::StartRules::plain();
    let ret = cx
        .sim_call(caster, &[target_id], &mut rules, |sim, c| {
            flyff_sim::act::send_act_msg_cx(sim, c, am)
        })
        .unwrap_or(ACT_DROP);
    if ret != ACT_OK
        && let (Some(prev), Some(m)) = (prev_ctx, cx.mover_mut(caster))
    {
        // the action did not start: a running skill keeps its context
        m.sim.skill = prev;
    }
    let started = if ret == ACT_OK {
        start_effects(cx, caster, target_id, prop, add, level, sut, control, cast);
        Started::Started
    } else if ret == ACT_DROP {
        return fail(20);
    } else {
        Started::NotStarted
    };

    // 21: a skill command is running
    if let Some(m) = cx.mover_mut(caster) {
        m.sim.mvrf.add(MVRF_SKILL);
    }

    // 22: helping a pink or chaotic player makes the helper pink (`__S8_PK`, WP-COMBAT-12's rule)
    let pvp = cx.arb.env.pvp;
    if pvp.pk
        && let Some(t) = cx.mover(target_id).cloned()
        && let Some(c) = cx.mover_mut(caster)
        && let Some(rel) = crate::combat::pvp::help_pink(&pvp, c, &t, prop.evildoing, now)
    {
        cx.arb.fx.near(caster, rel);
    }
    Ok(started)
}

/// `AddDefinedText(tid)` as a message body.
fn defined(tid: u32) -> pb::DefinedText {
    crate::chat::ingress::notice::defined_text(tid, Vec::new())
}

/// What `nRet == 1` does after the sim started the state (the tail of step 20).
#[allow(clippy::too_many_arguments)]
fn start_effects(
    cx: &mut ActionCx<'_>,
    caster: ObjId,
    target_id: ObjId,
    prop: &ItemProp,
    add: &AddSkillProp,
    level: u32,
    sut: SkillUseType,
    control: bool,
    cast: i32,
) {
    let core = cx.arb.env.core;
    let act = cx.act;
    // `SetActParam(OBJACT_USESKILL, skill, target)`, `[3] = level`, `[4] = control`
    if let Some(m) = cx.mover_mut(caster) {
        m.combat.act = ActParams {
            act: ObjAct::UseSkill,
            p: [
                i64::from(prop.id),
                i64::from(target_id.0),
                0,
                i64::from(level),
                i64::from(control),
            ],
            projectile_id: 0,
        };
    }
    // the costs of a melee skill, and the "use everything" sentinel of a magic skill, are paid now
    let mut out = Out::new();
    if let Some(m) = cx.arb.w.store.mover_mut(caster) {
        let dst = m.mv.dst.table.clone();
        if prop.skill_type == KT_SKILL {
            if add.req_fp > 0 {
                inc_fp(m, core, req_fp(&dst, add.req_fp).wrapping_neg(), &mut out);
            }
            let mp = req_mp(&dst, add.req_mp);
            if mp > 0 {
                inc_mp(m, core, mp.wrapping_neg(), &mut out);
            }
        }
        if prop.skill_type == KT_MAGIC && req_mp(&dst, add.req_mp) == REQ_ALL {
            let (fp, mp) = (m.mv.fp, m.mv.mp);
            inc_fp(m, core, fp.wrapping_neg(), &mut out);
            inc_mp(m, core, mp.wrapping_neg(), &mut out);
        }
    }
    cx.arb.fx.extend_out(caster, out);
    // a bullet is used up only without `features.no_arrows_poster`
    if prop.link_kind_bullet != NULL_ID
        && !act.no_arrows_poster
        && cx
            .mover(caster)
            .is_some_and(|m| worn_item(m, PARTS_BULLET).is_some())
    {
        let crate::combat::ArbiterCx { w, fx, hooks, .. } = &mut cx.arb;
        hooks.arrow_down(w, fx, caster);
    }
    // a hostile skill claims the target and starts the attack counter
    if prop.evildoing < 0 {
        let data = std::sync::Arc::clone(&core.data);
        set_jjim(cx.arb.w.store, &data, caster, target_id);
        if let Some(m) = cx.mover_mut(caster) {
            set_atk_cnt(m, 1);
        }
    }
    let (frame, speed) = (
        cx.sc.frame.0,
        cx.mover(caster)
            .map_or(1.0, |m| skill_ani_speed(m, prop.skill_type, act.speed_cast)),
    );
    cx.arb.fx.near(
        caster,
        pb::UseSkillNtf {
            skill_id: prop.id,
            level,
            target: target_id.0,
            use_type: sut.wire(),
            casting_time: cast,
            server_frame: frame,
            ani_speed: speed,
        },
    );
}

/// The skill runtime of a player (read helper for the other modules).
pub fn runtime(m: &Mover) -> Option<&SkillRuntime> {
    m.player().map(|p| &p.skills_rt)
}
