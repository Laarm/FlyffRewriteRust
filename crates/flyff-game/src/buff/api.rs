//! The public buff API (Plan/17 §7.13): what docs 15, 16, 18-23 call. Other modules never touch a
//! [`BuffSet`](super::set::BuffSet) directly.
//!
//! The functions take the split-borrow pair `(&mut Services, &mut GameCtx)` of the game thread (the plan's
//! `g: &mut Game`) and a mover by `ObjId`; all are no-ops on an id that is not a mover. Removal is the legacy
//! **fake** removal: the effects revert at once, the entry is erased - and `RemoveSkillInfluence` sent - by the
//! mover's next buff process; queries do not filter fake-removed entries.

use super::core::{self, BuffCx, RemoveFlags, find_ik3 as find_ik3_key};
use super::set::{BuffKey, BuffKind};
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::item::fx::Fx;
use crate::mover::Mover;
use crate::skill::apply::{Applied, announce, do_apply_skill};
use flyff_common::ObjId;
use std::sync::Arc;

/// Why an `apply_*` call did not apply.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuffRefusal {
    /// The target is not a mover.
    TargetInvalid,
    /// The item or skill row (or the level row) does not exist.
    NoProp,
    /// A hostile skill buff on a player in its invulnerability window.
    NotAddable,
    /// A buff of the same key and a higher level exists: `SetSkillState` was re-sent, nothing applied (17 §7.2).
    LowerLevel,
}

/// Runs `f` on mover `id` with a buff context and a fresh [`Fx`], then flushes the effects (messages, audit,
/// follow-ups). `None` when `id` is not a mover.
pub fn with_mover<R>(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    id: ObjId,
    f: impl FnOnce(&mut Mover, &BuffCx<'_>, &mut Fx) -> R,
) -> Option<R> {
    let items = svc.items.handle();
    let env = items.env(ctx.clock.unix_ms.to_unix_time());
    let cx = BuffCx::new(env, ctx.clock.game_ms);
    let mut fx = Fx::new();
    let r = {
        let m = ctx.worlds.mover_mut(id)?;
        f(m, &cx, &mut fx)
    };
    fx.flush_for(svc, ctx, id);
    Some(r)
}

// ---- apply (WP-SKILL-02) ---------------------------------------------------------------------------------------

/// Checks what [`BuffRefusal`] can say before the application starts.
fn pre_check(
    ctx: &GameCtx<'_>,
    target: ObjId,
    kind: BuffKind,
    id: u32,
    level: u32,
    hostile: bool,
) -> Result<(), BuffRefusal> {
    let Some(t) = ctx.worlds.mover(target) else {
        return Err(BuffRefusal::TargetInvalid);
    };
    if kind == BuffKind::Skill && hostile && t.is_player() && t.combat.dead_ticks > 0 {
        return Err(BuffRefusal::NotAddable);
    }
    if kind == BuffKind::Skill
        && t.buffs
            .get(BuffKey::new(kind, id))
            .is_some_and(|e| e.level > level)
    {
        return Err(BuffRefusal::LowerLevel);
    }
    Ok(())
}

/// Item buff = legacy `DoApplySkill(target, item_prop, NULL)`: `BUFF_ITEM` with `prop.skill_time` and the
/// item's DST slots (17 §6.7, §6.8 with `add = None`). The exclusivity of 19 §5.5 is the **caller's**. `src` is
/// the user (`None` = the target itself).
pub fn apply_item(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    target: ObjId,
    item: u32,
    src: Option<ObjId>,
) -> Result<(), BuffRefusal> {
    let data = Arc::clone(ctx.data);
    let Some(a) = Applied::item(&data, item) else {
        return if ctx.worlds.mover(target).is_none() {
            Err(BuffRefusal::TargetInvalid)
        } else {
            Err(BuffRefusal::NoProp)
        };
    };
    pre_check(ctx, target, BuffKind::Item, item, 0, a.prop.evildoing < 0)?;
    let src = src.unwrap_or(target);
    do_apply_skill(svc, ctx, src, target, &a, false, 0, false);
    Ok(())
}

/// A skill effect at a level = `DoApplySkill(target, skill_prop, add_skill(skill, level))`. `duration_ms`
/// overrides `add.skill_time` through the override view (NPC buffs, lord skills; the tables are never
/// mutated, 17 §6.15); both refer targets are disabled with it, as `DoAddBuff2` and the NPC buffer do. With
/// `broadcast`, `DoApplyUseSkill` goes to everybody around the caster.
#[allow(clippy::too_many_arguments)]
pub fn apply_skill(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    target: ObjId,
    skill: u32,
    level: u32,
    src: ObjId,
    duration_ms: Option<u32>,
    broadcast: bool,
) -> Result<(), BuffRefusal> {
    let data = Arc::clone(ctx.data);
    let Some(mut a) = Applied::skill(&data, skill, level) else {
        return if ctx.worlds.mover(target).is_none() {
            Err(BuffRefusal::TargetInvalid)
        } else {
            Err(BuffRefusal::NoProp)
        };
    };
    pre_check(
        ctx,
        target,
        BuffKind::Skill,
        skill,
        level,
        a.prop.evildoing < 0,
    )?;
    if let Some(ms) = duration_ms {
        a = a.with_duration(ms).without_refer_targets();
    }
    do_apply_skill(svc, ctx, src, target, &a, false, 0, false);
    if broadcast {
        announce(svc, ctx, src, target, skill, level);
    }
    Ok(())
}

// ---- raw add, queries, removal (WP-SKILL-01) ------------------------------------------------------------------------

/// `CMover::AddBuff`: `BUFF_PET`, `BUFF_EQUIP`, `BUFF_ITEM2` and the internal callers. `true` for a new entry
/// (the caller applies effects where 17 §7.1 says so).
#[allow(clippy::too_many_arguments)]
pub fn add(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    m: ObjId,
    kind: BuffKind,
    id: u16,
    level: u32,
    total_ms: u32,
    attacker: Option<ObjId>,
) -> bool {
    with_mover(svc, ctx, m, |mv, cx, fx| {
        core::add(mv, cx, kind, u32::from(id), level, total_ms, attacker, fx)
    })
    .unwrap_or(false)
}

/// `HasBuff(type, id)`.
pub fn has(ctx: &GameCtx<'_>, m: ObjId, kind: BuffKind, id: u16) -> bool {
    ctx.worlds
        .mover(m)
        .is_some_and(|mv| mv.buffs.contains(BuffKey { kind, id }))
}

/// `HasBuff(BUFF_ITEM, item)`.
pub fn has_item(ctx: &GameCtx<'_>, m: ObjId, item: u32) -> bool {
    ctx.worlds
        .mover(m)
        .is_some_and(|mv| mv.buffs.contains(BuffKey::new(BuffKind::Item, item)))
}

/// `HasBuff(BUFF_SKILL, skill)`.
pub fn has_skill(ctx: &GameCtx<'_>, m: ObjId, skill: u32) -> bool {
    ctx.worlds
        .mover(m)
        .is_some_and(|mv| mv.buffs.contains(BuffKey::new(BuffKind::Skill, skill)))
}

/// `HasBuffByIk3`: `BUFF_ITEM` entries only (`BUFF_ITEM2` never matches).
pub fn has_ik3(ctx: &GameCtx<'_>, m: ObjId, ik3: u32) -> bool {
    find_ik3(ctx, m, ik3).is_some()
}

/// `GetBuffByIk3`: the item id of the first match in key order (the angel EXP of doc 16 reads it).
pub fn find_ik3(ctx: &GameCtx<'_>, m: ObjId, ik3: u32) -> Option<u32> {
    let mv = ctx.worlds.mover(m)?;
    find_ik3_key(mv, ctx.data, ik3).map(|k| u32::from(k.id))
}

/// `RemoveBuff(type, id)` (fake removal).
pub fn remove(svc: &mut Services, ctx: &mut GameCtx<'_>, m: ObjId, kind: BuffKind, id: u16) {
    with_mover(svc, ctx, m, |mv, cx, fx| {
        core::remove_fake(mv, cx, BuffKey { kind, id }, fx);
    });
}

/// `RemoveBuff(BUFF_ITEM, item)`.
pub fn remove_item(svc: &mut Services, ctx: &mut GameCtx<'_>, m: ObjId, item: u32) {
    with_mover(svc, ctx, m, |mv, cx, fx| {
        core::remove_fake(mv, cx, BuffKey::new(BuffKind::Item, item), fx);
    });
}

/// `RemoveBuff(BUFF_SKILL, skill)`.
pub fn remove_skill(svc: &mut Services, ctx: &mut GameCtx<'_>, m: ObjId, skill: u32) {
    with_mover(svc, ctx, m, |mv, cx, fx| {
        core::remove_fake(mv, cx, BuffKey::new(BuffKind::Skill, skill), fx);
    });
}

/// `RemoveBuffs(flags, param)` (17 §7.5).
pub fn remove_flags(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    m: ObjId,
    flags: RemoveFlags,
    param: u32,
) {
    with_mover(svc, ctx, m, |mv, cx, fx| {
        core::remove_buffs(mv, cx, flags, param, fx);
    });
}

/// `RemoveIk3Buffs` = `RemoveBuffs(IK3, ik3)`.
pub fn remove_by_ik3(svc: &mut Services, ctx: &mut GameCtx<'_>, m: ObjId, ik3: u32) {
    remove_flags(svc, ctx, m, RemoveFlags::IK3, ik3);
}

/// `RemoveCommonBuffs` = `RemoveBuffs(COMMON)`: everything except the charged item buffs.
pub fn remove_common(svc: &mut Services, ctx: &mut GameCtx<'_>, m: ObjId) {
    remove_flags(svc, ctx, m, RemoveFlags::COMMON, 0);
}

/// `RemoveDebufBuffs` = `RemoveBuffs(DEBUF)` (the holy scroll).
pub fn remove_debufs(svc: &mut Services, ctx: &mut GameCtx<'_>, m: ObjId) {
    remove_flags(svc, ctx, m, RemoveFlags::DEBUF, 0);
}

/// `RemoveAllBuff` = `RemoveBuffs(UNCONDITIONAL)` (GM): everything but the housing buffs.
pub fn remove_all(svc: &mut Services, ctx: &mut GameCtx<'_>, m: ObjId) {
    remove_flags(svc, ctx, m, RemoveFlags::UNCONDITIONAL, 0);
}

// ---- death ------------------------------------------------------------------------------------------------------

/// `DoDie` step 8: `RemoveCommonBuffs()` of the dying mover (the caller sends `RemoveAllSkillInfluence`).
pub fn on_death_remove_buffs(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    k: &crate::combat::KillCtx,
) {
    remove_common(svc, ctx, k.victim);
}

/// `DoDie` step 3: a dying player loses its stun, poison, darkness and bleeding (the counters set directly;
/// the ones a buff carries went with the buff).
pub fn on_death_clear_status(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    k: &crate::combat::KillCtx,
) {
    use flyff_rules::consts::dst::{CHS_BLEEDING, CHS_DARK, CHS_POISON, CHS_STUN};
    with_mover(svc, ctx, k.victim, |m, cx, fx| {
        super::status::clear_direct(m, cx, CHS_STUN | CHS_POISON | CHS_DARK | CHS_BLEEDING, fx);
    });
}

// ---- the item-release command seam -------------------------------------------------------------------------------

/// The revert command of a text-command item (`ParsingCommand(text, mover, TRUE)`): `text` runs through the
/// command registry with the issuer `Issuer::Item` (23 §5; the whitelist and `fidelity.item_gm_commands` are
/// the registry's rules, PL-114). The executor is `gm::item_text::run` (WP-GM-19).
pub fn run_item_command(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    item: u32,
    text: &str,
) {
    crate::gm::item_text::run(svc, ctx, who, item, text);
}
