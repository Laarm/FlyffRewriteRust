//! The application of a skill or item effect (Plan/17 §6.6-§6.9, §6.11, §7.8; WP-SKILL-02;
//! `src:_Common/Ctrl.cpp:1041-1352, 1450-1707`, `MoverActEvent.cpp:205-437`).
//!
//! * [`apply_param`] is `CMover::ApplyParam`: the **two** DST slots of a prop (a skill's add-skill row or an
//!   item's own columns) applied to one mover as ledger operations under a [`DstSource`], so the release of the
//!   buff reverts exactly them. It works on one `&mut Mover` and queues the cross-mover consequences (the
//!   caster's HP gain, the over-heal mark, forced damage) as [`Followup`]s.
//! * [`apply_skill`] is `CCtrl::ApplySkill` (steps 2-10 of §6.8): damage through the arbiter, the immunity of
//!   mid-bosses, the hit-type refusals, the probability roll, the buff, the hard-coded skills, the DSTs.
//! * [`do_apply_skill`] / [`do_active_skill`] are `DoApplySkill` / `DoActiveSkill` for the DIRECT region (the
//!   area regions are WP-SKILL-06; here they apply to the single target).
//!
//! A prop whose duration or refer targets must change for one application (NPC buffs, lord skills) goes
//! through an **override view** ([`Applied::with_duration`], [`Applied::without_refer_targets`]): the data
//! tables are never mutated, which the original did and restored.

use crate::buff::core::{BuffCx, add as add_buff, remove_buffs, skill_props, source_of};
use crate::buff::set::{BuffKey, BuffKind};
use crate::buff::{RemoveFlags, status};
use crate::combat::{DamageRequest, send_damage, send_damage_force};
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::follow::Followup;
use crate::item::fx::Fx;
use crate::mover::Mover;
use crate::skill::hardcoded::{self, HardCodeCx, apply_hardcoded};
use crate::stats::dst::DstSource;
use crate::stats::env::{bare_inputs, max_hp, point_param, prop_of};
use crate::stats::source::apply_dst;
use flyff_common::defs::attribute::{
    CT_FINISH, KT_MAGIC, RANK_MIDBOSS, RANK_SUPER, RT_HEAL, RT_TIME, SRO_AROUND, SRO_REGION,
};
use flyff_common::defs::define::{
    EXT_MAGICATK, EXT_MAGICATKSHOT, EXT_MAGICSHOT, EXT_MELEEATK, EXT_SELFCHGPARAMET,
};
use flyff_common::defs::skill::{
    SI_ELE_FIRE_BURINGFIELD, SI_GEN_FLASH, SI_MAG_FIRE_HOTAIR, SI_PSY_HERO_STONE,
};
use flyff_common::{GameMs, ObjId};
use flyff_data::GameData;
use flyff_data::props::item::ItemProp;
use flyff_data::props::skill_add::AddSkillProp;
use flyff_proto::pb;
use flyff_rules::buff::{ReferStats, refer_heal, refer_time};
use flyff_rules::combat::{AtkFlags, DamageParam, HitType, x_random};
use flyff_rules::consts::dst::*;
use flyff_rules::consts::{NULL_CHGPARAM, NULL_ID};
use flyff_rules::stats::{get_dex, get_int, get_sta};
use std::borrow::Cow;
use std::sync::Arc;

// ---- the caster ---------------------------------------------------------------------------------------------------

/// What `ApplyParam` and the buff duration read of the caster (`pSrc`), copied out so that the target can be
/// borrowed mutably.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CasterFacts {
    /// The caster's object id.
    pub id: ObjId,
    /// The caster is a player.
    pub is_player: bool,
    /// Its STA, DEX and INT (`GetSta()` ...).
    pub stats: ReferStats,
}

impl CasterFacts {
    /// The facts of mover `m`.
    pub fn of(m: &Mover, cx: &BuffCx<'_>) -> CasterFacts {
        let inp = bare_inputs(m, cx.stats());
        CasterFacts {
            id: m.id(),
            is_player: m.is_player(),
            stats: ReferStats {
                sta: get_sta(&inp),
                dex: get_dex(&inp),
                int: get_int(&inp),
            },
        }
    }
}

// ---- the thing being applied ------------------------------------------------------------------------------------

/// A skill row at a level, or an item prop, with the level row as a copy-on-write view: overriding the
/// duration (`DoAddBuff2`, NPC buffs) never touches the data tables.
#[derive(Clone, Debug)]
pub struct Applied<'a> {
    /// The skill row or the item prop.
    pub prop: Cow<'a, ItemProp>,
    /// The add-skill row of the level; `None` for an item.
    pub add: Option<Cow<'a, AddSkillProp>>,
}

impl<'a> Applied<'a> {
    /// A skill at `level` (`GetSkillProp`); `None` when the row or the level is missing.
    pub fn skill(data: &'a GameData, skill: u32, level: u32) -> Option<Applied<'a>> {
        let (prop, add) = skill_props(data, skill, level)?;
        Some(Applied {
            prop: Cow::Borrowed(prop),
            add: Some(Cow::Borrowed(add)),
        })
    }

    /// An item prop applied as an effect (`DoApplySkill(target, item_prop, NULL)`).
    pub fn item(data: &'a GameData, item: u32) -> Option<Applied<'a>> {
        Some(Applied {
            prop: Cow::Borrowed(data.item(item)?),
            add: None,
        })
    }

    /// The skill level (0 for an item).
    pub fn level(&self) -> u32 {
        self.add.as_ref().map_or(0, |a| a.skill_lvl)
    }

    /// The same application with another duration (`skill_time` of the level row, or of the item prop).
    pub fn with_duration(mut self, ms: u32) -> Applied<'a> {
        match &mut self.add {
            Some(a) => a.to_mut().skill_time = ms,
            None => self.prop.to_mut().skill_time = ms,
        }
        self
    }

    /// The same application with both refer targets disabled (NPC buffs: no `RT_TIME` bonus, no `RT_HEAL`).
    pub fn without_refer_targets(mut self) -> Applied<'a> {
        let p = self.prop.to_mut();
        p.refer_target1 = NULL_ID;
        p.refer_target2 = NULL_ID;
        self
    }
}

// ---- ApplyParam -------------------------------------------------------------------------------------------------

/// `CMover::ApplyParam(pSrc, prop, add, bSend, nDamage)` on `target`: the two DST slots. `source` is the ledger
/// source of everything applied (the buff key, so the buff's release reverts it). `false` means a state refused
/// the mover (immunity): the second slot is skipped.
#[allow(clippy::too_many_arguments)]
pub fn apply_param(
    target: &mut Mover,
    cx: &BuffCx<'_>,
    source: DstSource,
    caster: &CasterFacts,
    prop: &ItemProp,
    add: Option<&AddSkillProp>,
    send: bool,
    damage: i32,
    fx: &mut Fx,
) -> bool {
    let stats = cx.stats();
    let pvp = caster.is_player && target.is_player();
    let class = prop_of(target, stats).class;
    for i in 0..2 {
        let (dst, skill_time, pain_time, adj, chg, data1) = match add {
            Some(a) => (
                a.dest_param[i],
                a.skill_time,
                a.pain_time,
                a.adj_param_val[i] as i32,
                a.chg_param_val[i] as i32,
                a.dest_data1,
            ),
            None => (
                prop.dest_param[i],
                prop.skill_time,
                prop.continuous_pain,
                prop.adj_param_val[i],
                prop.chg_param_val[i] as i32,
                prop.dest_data1,
            ),
        };
        let _ = pvp; // `dwAbilityMin` of the PvP row is read by the original and not used again
        match dst {
            NULL_ID => {}
            DST_CHRSTATE => {
                let bits = adj as u32;
                if bits & CHS_INVISIBILITY != 0 {
                    if !status::set_dark_cover(target, cx, source, skill_time, fx) {
                        return false;
                    }
                    target.status.dark_cover_ticks = 0;
                } else if bits & CHS_STUN != 0 {
                    if !status::set_stun(target, cx, source, skill_time, fx) {
                        return false;
                    }
                    target.status.stun_ticks = 0;
                } else if bits & CHS_POISON != 0 {
                    if !status::set_poison(
                        target,
                        cx,
                        source,
                        Some(caster.id),
                        skill_time,
                        pain_time,
                        data1[0] as i16 as i32,
                        fx,
                    ) {
                        return false;
                    }
                    if let Some(d) = &mut target.status.poison {
                        d.ticks_left = 0;
                    }
                } else if bits & CHS_DARK != 0 {
                    if !status::set_dark(target, cx, source, skill_time, data1[1], fx) {
                        return false;
                    }
                    if let Some(d) = &mut target.status.dark {
                        d.1 = 0;
                    }
                } else if bits & CHS_BLEEDING != 0 {
                    let each = if data1[0] < 0 { damage / 10 } else { data1[0] };
                    if !status::set_bleeding(
                        target,
                        cx,
                        source,
                        Some(caster.id),
                        skill_time,
                        pain_time,
                        each as i16 as i32,
                        fx,
                    ) {
                        return false;
                    }
                    if let Some(d) = &mut target.status.bleeding {
                        d.ticks_left = 0;
                    }
                } else {
                    // the other states are set as they are; a mid-boss is immune to slowing
                    let able = !(bits & CHS_SLOW != 0 && class == RANK_MIDBOSS);
                    if able {
                        apply_dst(
                            target,
                            stats,
                            source,
                            (dst, adj, NULL_CHGPARAM),
                            true,
                            &mut fx.out,
                        );
                    }
                }
            }
            DST_CHR_STEALHP => {
                // the share of the damage the caster drinks
                let steal = (damage as f32 * (adj as f32 / 100.0_f32)) as i32;
                fx.then(Followup::AddHp {
                    mover: caster.id,
                    amount: steal,
                });
            }
            DST_CHR_STEALHP_IMM => {
                let hp = point_param(target, stats, DST_HP);
                let steal = adj.min(hp);
                fx.then(Followup::ForceDamage {
                    victim: target.id(),
                    attacker: Some(caster.id),
                    amount: steal,
                });
                fx.then(Followup::AddHp {
                    mover: caster.id,
                    amount: steal,
                });
            }
            DST_CURECHR => remove_debuff(target, cx, adj as u32, fx),
            DST_CLEARBUFF => {
                if adj as u32 == NULL_ID {
                    remove_buffs(target, cx, RemoveFlags::GOODSKILL, 0, fx);
                } else {
                    for _ in 0..adj.max(0) {
                        remove_buffs(
                            target,
                            cx,
                            RemoveFlags::ONCE | RemoveFlags::GOODSKILL,
                            0,
                            fx,
                        );
                    }
                }
            }
            DST_CHR_CHANCESTUN
            | DST_CHR_CHANCEPOISON
            | DST_CHR_CHANCEDARK
            | DST_CHR_CHANCEBLEEDING
            | DST_CHR_CHANCESTEALHP => {
                apply_dst(target, stats, source, (dst, adj, chg), true, &mut fx.out);
            }
            DST_HP => {
                // healing: only a level row with an `RT_HEAL` refer target does anything
                if let Some(a) = add
                    && (prop.refer_target1 == RT_HEAL || prop.refer_target2 == RT_HEAL)
                {
                    let refer = refer_heal(prop, a.skill_lvl, Some(caster.stats));
                    let inc = (a.adj_param_val[0] as i32).wrapping_add(refer as i32);
                    if (a.dest_param[0] == DST_HP || a.dest_param[1] == DST_HP)
                        && inc > 0
                        && caster.is_player
                        && max_hp(target, stats)
                            > point_param(target, stats, DST_HP).wrapping_add(inc)
                    {
                        fx.then(Followup::OverHeal { mover: caster.id });
                    }
                    apply_dst(
                        target,
                        stats,
                        source,
                        (a.dest_param[0], inc, a.chg_param_val[0] as i32),
                        true,
                        &mut fx.out,
                    );
                }
            }
            DST_SPEED => {
                if class != RANK_MIDBOSS && class != RANK_SUPER {
                    apply_dst(target, stats, source, (dst, adj, chg), send, &mut fx.out);
                }
            }
            _ => apply_dst(target, stats, source, (dst, adj, chg), send, &mut fx.out),
        }
    }
    true
}

/// `CMover::RemoveDebuff(state)` (`MoverParam.cpp:2166-2200`): the buffs that carry the states go, then the
/// counters that were set directly; `CHS_DEBUFFALL` also removes the hostile speed and chance buffs. (The
/// legacy closes with `SetDestParam(DST_CURECHR)`, a blanket `ResetDestParam(CHRSTATE, state)`: with
/// the per-source ledger every holder of a state bit has already been removed above.)
pub fn remove_debuff(m: &mut Mover, cx: &BuffCx<'_>, state: u32, fx: &mut Fx) {
    remove_buffs(m, cx, RemoveFlags::CHRSTATE, state, fx);
    status::clear_direct(m, cx, state, fx);
    if state & CHS_DEBUFFALL != 0 {
        for dst in [
            DST_SPEED,
            DST_CHR_CHANCEPOISON,
            DST_CHR_CHANCESTUN,
            DST_CHR_CHANCEBLEEDING,
        ] {
            remove_buffs(m, cx, RemoveFlags::DSTPARAM, dst, fx);
        }
    }
}

// ---- ApplySkill -------------------------------------------------------------------------------------------------

/// The arguments of `CCtrl::ApplySkill` that are not the thing applied.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ApplyArgs {
    /// The caster (`pSrc`).
    pub src: ObjId,
    /// `bIgnoreProb`: the probability is treated as `NULL_ID` (always).
    pub ignore_prob: bool,
    /// `nParam` (only `SI_MAG_FIRE_HOTAIR` passes it on to the damage).
    pub param: u32,
    /// `bOnlyDmg`: stop after the damage (ground DoT ticks).
    pub only_dmg: bool,
    /// `bTarget`: the target is the primary target of the skill, not a side target of an area.
    pub is_target: bool,
}

impl ApplyArgs {
    /// A plain application by `src`: probability rolled, damage and effects, primary target.
    pub fn by(src: ObjId) -> ApplyArgs {
        ApplyArgs {
            src,
            ignore_prob: false,
            param: 0,
            only_dmg: false,
            is_target: true,
        }
    }
}

/// What an application did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ApplyOutcome {
    /// The damage (`nDamage`).
    pub damage: i32,
    /// The probability roll passed (or there was none) and the target is a mover.
    pub rolled: bool,
    /// A new buff was created, or the effect needed none (the DSTs were applied).
    pub new: bool,
}

/// `GetHitType2(target, is_target, good)` (Plan/17 §8.1, `MoverAttack.cpp:1950-2010`): is `target` an enemy of
/// `att` for a skill? The PvP rules are [`crate::combat::pvp::hit_type2`] (WP-COMBAT-12): a `Pk` answer is
/// refused for a beneficial skill (`good`), a just revived or just finished duelist answers `Generic`.
pub fn hit_type2(
    ctx: &GameCtx<'_>,
    att: &Mover,
    def: &Mover,
    _is_target: bool,
    good: bool,
) -> HitType {
    let env = crate::combat::pvp::PvpEnv::from_ctx(ctx);
    crate::combat::pvp::hit_type2(att, def, &env, &*ctx.worlds.movement.combat.hooks, good)
}

/// `CCtrl::ApplyDamage` (`Ctrl.cpp:1001-1029`): the damage part of an application.
fn apply_damage(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    target: ObjId,
    a: &Applied<'_>,
    args: &ApplyArgs,
) -> i32 {
    let p = &*a.prop;
    if !matches!(p.exe_target, EXT_MELEEATK | EXT_MAGICATK | EXT_MAGICATKSHOT) {
        return 0;
    }
    let mut req = if p.id == SI_MAG_FIRE_HOTAIR {
        DamageRequest::new(
            target,
            args.src,
            AtkFlags::MAGICSKILL,
            DamageParam(args.param),
        )
    } else {
        let flags = if p.skill_type == KT_MAGIC {
            AtkFlags::MAGICSKILL
        } else {
            AtkFlags::MELEESKILL
        };
        DamageRequest::new(
            target,
            args.src,
            flags,
            DamageParam::skill_attack(p.id, a.level()),
        )
    };
    if !args.is_target {
        req = req.side_target();
    }
    if p.id != SI_MAG_FIRE_HOTAIR && p.combo_style == CT_FINISH {
        send_damage_force(svc, ctx, req)
    } else {
        send_damage(svc, ctx, req)
    }
}

/// `CCtrl::ApplySkill(src, prop, add, ignore_prob, param, only_dmg, is_target)` on a mover `target`
/// (`Ctrl.cpp:1041-1352`, Plan/17 §6.8 steps 2-10).
pub fn apply_skill(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    target: ObjId,
    a: &Applied<'_>,
    args: &ApplyArgs,
) -> ApplyOutcome {
    let mut out = ApplyOutcome::default();
    if ctx.worlds.mover(target).is_none() {
        return out;
    }
    let p = &*a.prop;
    let add = a.add.as_deref();
    let core = svc.items.handle();
    let env = core.env(ctx.clock.unix_ms.to_unix_time());
    let cx = BuffCx::new(env, ctx.clock.game_ms);
    let same = args.src == target;
    // 1: `__PK_PVP_SKILL_REGION`: a hostile skill between two players is skipped outside the PK-capable regions
    // unless they are duel / war / guild-combat / arena partners (KQ-08)
    if !same
        && p.evildoing < 0
        && p.skill_type != NULL_ID
        && let (Some(src), Some(t)) = (ctx.worlds.mover(args.src), ctx.worlds.mover(target))
        && crate::combat::pvp::skill_region_refused(
            src,
            t,
            &crate::combat::pvp::PvpEnv::from_services(svc, ctx),
            &svc.stats.core().rules,
            &*ctx.worlds.movement.combat.hooks,
        )
    {
        return out;
    }
    // 2: the damage
    out.damage = apply_damage(svc, ctx, target, a, args);
    // 3
    if args.only_dmg {
        return out;
    }
    // 4: mid-bosses are immune to everything but damage
    if ctx
        .worlds
        .mover(target)
        .is_none_or(|t| prop_of(t, cx.stats()).class == RANK_MIDBOSS)
    {
        return out;
    }
    // 5: the hit-type refusals (mover against mover only)
    if !same && let (Some(src), Some(t)) = (ctx.worlds.mover(args.src), ctx.worlds.mover(target)) {
        if !crate::combat::is_die(t) {
            if p.evildoing < 0 && hit_type2(ctx, src, t, args.is_target, false) == HitType::Fail {
                return out;
            }
            if p.evildoing > 0 && hit_type2(ctx, src, t, args.is_target, true) != HitType::Fail {
                return out;
            }
        } else {
            let mut fx = Fx::new();
            if p.exe_target != EXT_MAGICATKSHOT && p.sfx_obj[1] != NULL_ID {
                fx.out.near(sfx(p.sfx_obj[1]));
            }
            fx.flush_for(svc, ctx, target);
            if p.evildoing < 0 {
                return out;
            }
        }
    }
    // 6: the probability
    let both_players = ctx.worlds.mover(args.src).is_some_and(Mover::is_player)
        && ctx.worlds.mover(target).is_some_and(Mover::is_player);
    let mut prob = match add {
        Some(a) if both_players => a.probability_pvp,
        Some(a) => a.probability,
        None => p.probability as u32,
    };
    if args.ignore_prob {
        prob = NULL_ID;
    }
    let world = ctx.worlds.core(target).map(|c| c.world);
    let passes = prob == NULL_ID
        || world.is_some_and(|w| (x_random(ctx.rng.world(w), 100) as i32) < prob as i32);
    if passes {
        out.rolled = true;
        if ctx.worlds.mover(target).is_some()
            && !apply_effect(svc, ctx, target, a, args, &cx, &mut out)
        {
            return out;
        }
    } else if p.id == SI_PSY_HERO_STONE {
        let mut fx = Fx::new();
        fx.out.near(pb::RemoveSfxObj {
            sfx_id: p.sfx_obj[3],
            x: 0.0,
            y: 0.0,
            z: 0.0,
            flag: false,
        });
        fx.flush_for(svc, ctx, target);
    }
    // 10: the SFX of an area target (also after a failed roll), then the penya steal of the yo-yo
    if matches!(p.spell_region, SRO_AROUND | SRO_REGION)
        && p.exe_target != EXT_MAGICATKSHOT
        && p.sfx_obj[1] != NULL_ID
    {
        let mut fx = Fx::new();
        fx.out.near(sfx(p.sfx_obj[1]));
        fx.flush_for(svc, ctx, target);
    }
    hardcoded::on_applied(
        svc,
        ctx,
        &HardCodeCx {
            src: args.src,
            target,
            prop: p,
            add,
        },
    );
    out
}

/// Steps 7-9 of `CCtrl::ApplySkill` for a mover target whose roll passed: the buff, the hard-coded skills, the
/// DSTs, the push and the pull, the blinding flash. `false` ends the whole application (a pending resurrection,
/// the skill refused by its hard-coded step): the SFX and the penya steal are skipped as in the original
/// `return`s.
fn apply_effect(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    target: ObjId,
    a: &Applied<'_>,
    args: &ApplyArgs,
    cx: &BuffCx<'_>,
    out: &mut ApplyOutcome,
) -> bool {
    let p = &*a.prop;
    let add = a.add.as_deref();
    // 7: the buff
    let caster = ctx.worlds.mover(args.src).map(|m| CasterFacts::of(m, cx));
    let skill_time = add.map_or(p.skill_time, |a| a.skill_time) as i32;
    let mut is_new = true;
    let mut source = DstSource::Script { tag: p.id };
    let mut tfx = Fx::new();
    if skill_time >= 0 {
        let (t1, t2) = match &caster {
            Some(c) if p.refer_target1 == RT_TIME || p.refer_target2 == RT_TIME => {
                refer_time(p, a.level(), c.stats)
            }
            _ => (0, 0),
        };
        let kind = if p.skill_type == NULL_ID {
            BuffKind::Item
        } else {
            BuffKind::Skill
        };
        if p.id == SI_ELE_FIRE_BURINGFIELD {
            is_new = false;
        } else if let Some(t) = ctx.worlds.mover_mut(target) {
            let total = (skill_time.wrapping_add(t1 as i32).wrapping_add(t2 as i32)) as u32;
            is_new = add_buff(
                t,
                cx,
                kind,
                p.id,
                a.level(),
                total,
                Some(args.src),
                &mut tfx,
            );
            source = source_of(BuffKey::new(kind, p.id));
        }
    }
    out.new = is_new;
    tfx.flush_for(svc, ctx, target);
    if is_new {
        // 8: the hard-coded skills, a pending resurrection, then the DSTs
        let hc = HardCodeCx {
            src: args.src,
            target,
            prop: p,
            add,
        };
        if !apply_hardcoded(svc, ctx, &hc) {
            return false;
        }
        if ctx
            .worlds
            .mover(target)
            .is_some_and(|t| t.combat.resurrection.is_some())
        {
            return false;
        }
        let param_target = if p.exe_target == EXT_SELFCHGPARAMET {
            args.src
        } else {
            target
        };
        if let Some(c) = caster {
            let mut fx = Fx::new();
            if let Some(m) = ctx.worlds.mover_mut(param_target) {
                apply_param(m, cx, source, &c, p, add, true, out.damage, &mut fx);
            }
            fx.flush_for(svc, ctx, param_target);
        }
        hardcoded::after_params(svc, ctx, &hc);
    }
    // 9: the blinding flash is a client effect
    if p.id == SI_GEN_FLASH && ctx.worlds.player(target).is_some() {
        let mut fx = Fx::new();
        fx.out.to_self(pb::CommonSkill {
            skill_id: p.id,
            level: a.level(),
        });
        fx.flush_for(svc, ctx, target);
    }
    true
}

fn sfx(id: u32) -> pb::CreateSfxObj {
    pb::CreateSfxObj {
        sfx_id: id,
        x: 0.0,
        y: 0.0,
        z: 0.0,
        flag: false,
    }
}

// ---- DoApplySkill / DoActiveSkill -------------------------------------------------------------------------------

/// `CCtrl::DoApplySkill(target, prop, add, ignore_prob, param, only_dmg)` without `bControl`: see
/// [`area::do_apply_skill`](super::area::do_apply_skill), which resolves the region of the skill (around, region,
/// line, troupe, the ground DoT, the Psychic Wall), removes the caster's Dark Illusion, enforces the exclusive
/// Jester buffs and fires the secondary skill. Returns `nResult` (1, or 0 for a troupe skill without a party and
/// a Psychic Wall without a free slot).
#[allow(clippy::too_many_arguments)]
pub fn do_apply_skill(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    src: ObjId,
    target: ObjId,
    a: &Applied<'_>,
    ignore_prob: bool,
    param: u32,
    only_dmg: bool,
) -> i32 {
    super::area::do_apply_skill(
        svc,
        ctx,
        src,
        target,
        a,
        ignore_prob,
        param,
        only_dmg,
        false,
    )
}

/// `CCtrl::DoActiveSkill(skill, level, target, ignore_prob)` (`Ctrl.cpp:1683-1707`): a shot-type secondary
/// skill has **no server effect and no message** (the legacy `ShootSkill` is client-only, KQ-07); any other
/// is applied and announced with `DoApplyUseSkill` (the caster is the header).
pub fn do_active_skill(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    src: ObjId,
    skill: u32,
    level: u32,
    target: ObjId,
    ignore_prob: bool,
) {
    do_active_skill_ex(svc, ctx, src, skill, level, target, ignore_prob, false);
}

/// [`do_active_skill`] with `bControl` (the secondary skill of a skill cast with control).
#[allow(clippy::too_many_arguments)]
pub fn do_active_skill_ex(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    src: ObjId,
    skill: u32,
    level: u32,
    target: ObjId,
    ignore_prob: bool,
    control: bool,
) {
    let data = Arc::clone(ctx.data);
    let Some(a) = Applied::skill(&data, skill, level) else {
        return;
    };
    if matches!(a.prop.exe_target, EXT_MAGICATKSHOT | EXT_MAGICSHOT) {
        return;
    }
    super::area::do_apply_skill(svc, ctx, src, target, &a, ignore_prob, 0, false, control);
    announce(svc, ctx, src, target, skill, level);
}

/// `AddDoApplySkill(src, target, skill, level)`: `DoApplyUseSkill` to everybody around the caster.
pub fn announce(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    src: ObjId,
    target: ObjId,
    skill: u32,
    level: u32,
) {
    let mut fx = Fx::new();
    fx.out.near(pb::DoApplyUseSkill {
        target: target.0,
        skill_id: skill,
        level,
    });
    fx.flush_for(svc, ctx, src);
}

/// `CUser::AddCommonSkill` helper kept for callers: the blind effect of a flash.
pub fn common_skill(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    player: PlayerRef,
    skill: u32,
    level: u32,
) {
    let mut fx = Fx::new();
    fx.out.to_self(pb::CommonSkill {
        skill_id: skill,
        level,
    });
    fx.flush(svc, ctx, player);
}

/// The wall-clock-free time source for tests that call [`apply_param`] directly.
pub fn cx_at<'a>(env: crate::item::env::ItemEnv<'a>, now: GameMs) -> BuffCx<'a> {
    BuffCx::new(env, now)
}
