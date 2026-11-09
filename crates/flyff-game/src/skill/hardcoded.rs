//! The hard-coded skills (`CCtrl::ApplySkillHardCoding` and the special cases around `ApplySkill`, Plan/17 §6.10;
//! WP-SKILL-08; `src:_Common/Ctrl.cpp:787-815, 1237-1349`).
//!
//! Most rows of the catalogue are rules of other steps; this module holds the ones that belong to the
//! application of a skill to one target and the index of all of them:
//!
//! | skill | rule | where |
//! |---|---|---|
//! | `SI_ASS_HEAL_RESURRECTION` | a dead player target not yet offered gets a `ResurrectionOffer` and `ResurrectionMessage`; the rest of the application is skipped; an already resurrecting player cancels it | [`apply_hardcoded`] (calls [`resurrect::offer`]) |
//! | `SI_ACR_YOYO_SNITCH` | steals penya from the target (any roll result) | [`on_applied`] / [`snitch`]; the Dark Illusion requirement is step 6 of `do_use_skill` |
//! | `SI_ACR_YOYO_PULLING` | pulls the target toward the caster | [`after_params`] |
//! | any skill with `dmg_shift` (`features.no_yoyo_knockback = false`) | pushes the target 0.85 away from the caster | [`after_params`] |
//! | `SI_KNT_HERO_DRAWING` | the target is set at the caster's position and pushed | `cast::hero_drawing`; cancelled by a damage motion or a released target (`cast::on_cast_cancelled`) |
//! | `SI_RIG_HERO_RETURN` | `features.rm_hero_skill`: the 16 assist buffs at their maximum level; else a teleport to the revival point | `cast::hero_return` |
//! | `SI_RIG_MASTER_BLESSING` | `features.new_skill_attribute`: the `DST_IGNORE_DMG` shield; its release resets the *remaining* shield; the arbiter spends it and removes the buff | `buff::core::release`, `combat::arbiter` (`shield_reduce`, `remove_blessing`) |
//! | `SI_GEN_FLASH` | `CommonSkill{skill, level}` to a player target (the blind effect of the client) | `apply::apply_effect` step 9 |
//! | `SI_GEN_BURN` (buff) | on expiry `x_random_range(ability_min, ability_max)` `AF_FORCE` damage from the buff's attacker and `XI_SKILL_ELE_FIRE_BURINGFIELD02` | `buff::core::on_expire` |
//! | `SI_ELE_FIRE_BURINGFIELD` | never a buff: the ground DoT | `apply::apply_effect`, [`ctrl`](super::ctrl) |
//! | `SI_PSY_PSY_PSYCHICWALL` | a wall ctrl, not an application | [`area::do_apply_skill_ex`](super::area::do_apply_skill_ex), [`ctrl`](super::ctrl) |
//! | `SI_MAG_MAG_BLINKPOOL` | refused by the queue; `TeleSkill` moves the caster | `tele`, `queue` |
//! | `SI_ACR_SUP_DARKILLUSION` | `CHS_INVISIBILITY`; ended by any other skill the caster uses and when it is attacked / attacks | [`area::do_apply_skill_ex`](super::area::do_apply_skill_ex), `combat::arbiter` (`remove_invisible`) |
//! | `SI_GEN_ATK_COUNTER` | the counter-attack cast of a victim in `CHS_ATK_COUNTERATTACK` / `CHS_DMG_COUNTERATTACK`; no cast-motion skip | `combat::arbiter` (`apply_post`), `cast::SkillJob::Counter` |
//! | `SI_MER_ONE_REFLEXHIT` | accumulates `reflex_dmg` while in its state | `combat::action` |
//! | `SI_BLD_DOUBLESW_SILENTSTRIKE`, `SI_BIL_KNU_BELIALSMESHING`, `SI_ACR_YOYO_COUNTER` | motion rules | the sim (`flyff_sim::act`), the end of the counter buff aborts the attack (`buff::core::release`) |
//!
//! `apply_skill` calls [`apply_hardcoded`] for a **new** buff before the DST slots, [`after_params`] after them
//! and [`on_applied`] at its very end.

use crate::ctx::{GameCtx, Services};
use crate::item::fx::Fx;
use crate::item::ops::CtxSerials;
use crate::movement::intents;
use crate::skill::resurrect::{self, Offered};
use crate::stats::env::prop_of;
use flyff_common::ObjId;
use flyff_common::defs::attribute::{RANK_MIDBOSS, RANK_SUPER};
use flyff_common::defs::skill::{
    SI_ACR_YOYO_PULLING, SI_ACR_YOYO_SNITCH, SI_ASS_HEAL_RESURRECTION,
};
use flyff_data::props::item::ItemProp;
use flyff_data::props::skill_add::AddSkillProp;
use flyff_proto::pb;
use flyff_rules::consts::NULL_ID;
use flyff_rules::consts::dst::{DST_GOLD, DST_HAWKEYE_RATE};
use flyff_sim::bounds::get_attack_range;
use flyff_sim::num::get_degree;
use flyff_sim::types::AR_HRANGE;

/// What a hard-coded skill gets to see.
pub struct HardCodeCx<'a> {
    /// The caster.
    pub src: ObjId,
    /// The target the skill was applied to.
    pub target: ObjId,
    /// The skill row.
    pub prop: &'a ItemProp,
    /// The level row.
    pub add: Option<&'a AddSkillProp>,
}

/// `ApplySkillHardCoding(...)`: `false` stops the application. Only the Resurrection skill has a rule here: on a
/// dead player it stores the offer and sends the confirmation (the application then ends in `apply_skill`
/// because the offer is pending); a player already resurrecting stops it.
pub fn apply_hardcoded(svc: &mut Services, ctx: &mut GameCtx<'_>, cx: &HardCodeCx<'_>) -> bool {
    if cx.prop.id == SI_ASS_HEAL_RESURRECTION {
        let level = cx.add.map_or(0, |a| a.skill_lvl);
        let o = resurrect::offer(svc, ctx, cx.src, cx.target, cx.prop.id, level);
        return o != Offered::Resurrecting;
    }
    true
}

/// The pushes of step 8 after `ApplyParam`: the `dmg_shift` knockback (unless `features.no_yoyo_knockback`, the
/// repack default) and the pull of `SI_ACR_YOYO_PULLING`. Both act on the target (a mover).
pub fn after_params(svc: &mut Services, ctx: &mut GameCtx<'_>, cx: &HardCodeCx<'_>) {
    let core = svc.stats.handle();
    let (Some(src), Some(t)) = (ctx.worlds.mover(cx.src), ctx.worlds.mover(cx.target)) else {
        return;
    };
    let (src_pos, t_pos) = (src.core.pos, t.core.pos);
    let class = prop_of(t, &core).class;
    let hawkeye = t.mv.dst.table.adj(DST_HAWKEYE_RATE);
    // the angle from the caster to the target (`GetDegree(pTarget->GetPos(), pSrc->GetPos())`)
    let angle = get_degree(t_pos, src_pos);
    let mut pushes: Vec<f32> = Vec::new();
    let shift = cx.add.map_or(cx.prop.dmg_shift, |a| a.dmg_shift);
    if !ctx.cfg.rules.features.no_yoyo_knockback
        && shift != NULL_ID
        && class != RANK_SUPER
        && class != RANK_MIDBOSS
    {
        pushes.push(0.85);
    }
    if cx.prop.id == SI_ACR_YOYO_PULLING
        && let Some(add) = cx.add
        && add.dest_data1[2] != -1
    {
        // the pull shrinks with the distance: `len * min(1, dist^2 * 0.1 / AR_HRANGE)` metres toward the caster
        let pulling = add.dest_data1[2] as f32 * 0.001;
        let d = t_pos - src_pos;
        let total = (d.x * d.x + d.y * d.y + d.z * d.z) * 0.1;
        let reach = get_attack_range(AR_HRANGE, hawkeye);
        let total = if total > reach { reach } else { total };
        let ratio = total / reach;
        pushes.push(pulling * -ratio);
    }
    for power in pushes {
        if let Some(m) = ctx.worlds.mover_mut(cx.target) {
            intents::push(m, angle, power);
        }
    }
}

/// The end of `CCtrl::ApplySkill` (`Ctrl.cpp:1306-1349`), reached whatever the roll said: the penya steal of
/// `SI_ACR_YOYO_SNITCH`.
pub fn on_applied(svc: &mut Services, ctx: &mut GameCtx<'_>, cx: &HardCodeCx<'_>) {
    if cx.prop.id == SI_ACR_YOYO_SNITCH {
        snitch(svc, ctx, cx.src, cx.target);
    }
}

/// The penya `SI_ACR_YOYO_SNITCH` takes: `level(target) * 20 - level(caster) * 10`, times 1.0 (a caster up to level
/// 20), 0.7 (30), 0.5 (40) or 0.3 (above), at most the target's wallet; nothing unless the result is positive and
/// the target owns something.
pub fn snitch_amount(caster_level: i32, target_level: i32, wallet: i64) -> i64 {
    if wallet <= 0 {
        return 0;
    }
    let base = target_level
        .wrapping_mul(20)
        .wrapping_sub(caster_level.wrapping_mul(10));
    let n = match caster_level {
        ..=20 => base,
        21..=30 => (base as f32 * 0.7_f32) as i32,
        31..=40 => (base as f32 * 0.5_f32) as i32,
        _ => (base as f32 * 0.3_f32) as i32,
    };
    if n <= 0 { 0 } else { i64::from(n).min(wallet) }
}

/// `SI_ACR_YOYO_SNITCH` (see [`snitch_amount`]): the amount moves from the target (a player's wallet, or the gold a
/// monster carries) to the caster, who gets the gain text.
pub fn snitch(svc: &mut Services, ctx: &mut GameCtx<'_>, src: ObjId, target: ObjId) {
    let (Some(caster), Some(t)) = (ctx.worlds.player(src), ctx.worlds.mover(target)) else {
        return;
    };
    let wallet = |m: &crate::mover::Mover| -> i64 {
        match (m.player(), m.monster()) {
            (Some(p), _) => i64::from(p.items.gold),
            (_, Some(e)) => i64::from(e.gold),
            _ => 0,
        }
    };
    let n = snitch_amount(caster.mv.level, t.mv.level, wallet(t));
    if n <= 0 {
        return;
    }
    let items = svc.items.handle();
    let env = items.env(ctx.clock.unix_ms.to_unix_time());
    // the caster gets the penya and the gain text, the target pays
    let mut fx = Fx::new();
    {
        let mut serials = CtxSerials::new(&mut svc.serials, ctx.persist, ctx.tickets);
        if let Some(m) = ctx.worlds.player_mut(src) {
            crate::item::gold::core::add_gold(m, &env, n, true, &mut fx);
            crate::item::gold::core::gold_text(m, &env, &mut serials, n, &mut fx);
        }
    }
    let mut pay = Fx::new();
    if let Some(m) = ctx.worlds.mover_mut(target) {
        if m.is_player() {
            crate::item::gold::core::add_gold(m, &env, -n, true, &mut pay);
        } else if let Some(e) = m.monster_mut() {
            e.gold = (i64::from(e.gold) - n).clamp(0, i64::from(i32::MAX)) as i32;
            pay.out.near(pb::SetPointParam {
                dst: DST_GOLD as i32,
                value: e.gold,
            });
        }
    }
    fx.flush_for(svc, ctx, src);
    pay.flush_for(svc, ctx, target);
}
