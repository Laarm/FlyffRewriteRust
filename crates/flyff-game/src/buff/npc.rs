//! NPC buffs and the class buff pang (Plan/17 §7.11, WP-SKILL-03; `src:WORLDSERVER/DPSrvr.cpp:11210-11575`).
//!
//! [`on_npc_buff`] is the **only** implementation of the rules of the `NpcBuff` message once the handler of
//! doc 20 has done its precondition (VR-012: *any* `MMI_NPC_BUFF` NPC within `gameplay.npc_menu_range`,
//! checked once by `NpcChecker::close_npc`; the handler drops the request silently otherwise and does not call
//! this function):
//!
//! 1. with `features.buff_pang_update` the job table [`BUFF_PANG`]: each entry is applied to the player at its
//!    level with a duration of 1 800 s, and announced with `DoApplyUseSkill` (`DoAddBuff2`);
//! 2. the official `NpcBuffSkill` entries of the NPC's `character.inc` row: the player's level inside
//!    `[min, max]`, the skill level inside `[expert_lv, expert_max]`, the cheer conflicts, the duration of the
//!    entry; out of the level range the player is told `TID_GAME_NPCBUFF_LEVELLIMIT`.
//!
//! The props are never mutated: the duration and the refer targets are an override view of the application.

use super::api::apply_skill;
use super::set::{BuffKey, BuffKind};
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::item::fx::Fx;
use crate::skill::apply::{Applied, announce, do_apply_skill};
use flyff_common::defs::job::*;
use flyff_common::defs::skill::*;
use flyff_common::defs::text::{TID_GAME_NPCBUFF_FAILED, TID_GAME_NPCBUFF_LEVELLIMIT};
use std::sync::Arc;

/// The duration of a class buff in ms (`DoAddBuff2(..., 1800, ...)`).
pub const BUFF_PANG_MS: u32 = 1_800_000;

/// The `__BUFF_PANG_UPDATE` table: per job the `(skill, level)` pairs applied in this order. Transcribed
/// mechanically from the original (27 jobs, 156 pairs); every other job gets nothing.
pub const BUFF_PANG: &[(u32, &[(u32, u32)])] = &[
    (
        JOB_MERCENARY,
        &[
            (SI_MER_SUP_IMPOWERWEAPON, 20),
            (SI_MER_SUP_SMITEAXE, 20),
            (SI_MER_SUP_AXEMASTER, 20),
            (SI_MER_SUP_SWORDMASTER, 20),
            (SI_MER_SUP_BLAZINGSWORD, 20),
            (SI_MER_SHIELD_PANBARRIER, 20),
            (SI_MER_SHIELD_PROTECTION, 20),
        ],
    ),
    (
        JOB_BLADE,
        &[
            (SI_MER_SUP_IMPOWERWEAPON, 20),
            (SI_MER_SUP_SMITEAXE, 20),
            (SI_MER_SUP_AXEMASTER, 20),
            (SI_MER_SUP_SWORDMASTER, 20),
            (SI_MER_SUP_BLAZINGSWORD, 20),
            (SI_BLD_SUP_BERSERK, 10),
        ],
    ),
    (
        JOB_BLADE_MASTER,
        &[
            (SI_MER_SUP_IMPOWERWEAPON, 20),
            (SI_MER_SUP_SMITEAXE, 20),
            (SI_MER_SUP_AXEMASTER, 20),
            (SI_MER_SUP_SWORDMASTER, 20),
            (SI_MER_SUP_BLAZINGSWORD, 20),
            (SI_BLD_SUP_BERSERK, 10),
            (SI_BLD_MASTER_ONEHANDMASTER, 5),
        ],
    ),
    (
        JOB_BLADE_HERO,
        &[
            (SI_MER_SUP_IMPOWERWEAPON, 20),
            (SI_MER_SUP_SMITEAXE, 20),
            (SI_MER_SUP_AXEMASTER, 20),
            (SI_MER_SUP_SWORDMASTER, 20),
            (SI_MER_SUP_BLAZINGSWORD, 20),
            (SI_BLD_SUP_BERSERK, 10),
            (SI_BLD_MASTER_ONEHANDMASTER, 5),
        ],
    ),
    (
        JOB_KNIGHT,
        &[
            (SI_MER_SUP_IMPOWERWEAPON, 20),
            (SI_MER_SUP_SMITEAXE, 20),
            (SI_MER_SUP_AXEMASTER, 20),
            (SI_MER_SUP_SWORDMASTER, 20),
            (SI_MER_SUP_BLAZINGSWORD, 20),
            (SI_MER_SHIELD_PANBARRIER, 20),
            (SI_MER_SHIELD_PROTECTION, 20),
            (SI_KNT_SUP_GUARD, 10),
            (SI_KNT_SUP_RAGE, 10),
            (SI_KNT_SUP_PAINREFLECTION, 10),
        ],
    ),
    (
        JOB_KNIGHT_MASTER,
        &[
            (SI_MER_SUP_IMPOWERWEAPON, 20),
            (SI_MER_SUP_SMITEAXE, 20),
            (SI_MER_SUP_AXEMASTER, 20),
            (SI_MER_SUP_SWORDMASTER, 20),
            (SI_MER_SUP_BLAZINGSWORD, 20),
            (SI_MER_SHIELD_PANBARRIER, 20),
            (SI_MER_SHIELD_PROTECTION, 20),
            (SI_KNT_SUP_GUARD, 10),
            (SI_KNT_SUP_RAGE, 10),
            (SI_KNT_SUP_PAINREFLECTION, 10),
            (SI_KNT_MASTER_TWOHANDMASTER, 5),
        ],
    ),
    (
        JOB_KNIGHT_HERO,
        &[
            (SI_MER_SUP_IMPOWERWEAPON, 20),
            (SI_MER_SUP_SMITEAXE, 20),
            (SI_MER_SUP_AXEMASTER, 20),
            (SI_MER_SUP_SWORDMASTER, 20),
            (SI_MER_SUP_BLAZINGSWORD, 20),
            (SI_MER_SHIELD_PANBARRIER, 20),
            (SI_MER_SHIELD_PROTECTION, 20),
            (SI_KNT_SUP_GUARD, 10),
            (SI_KNT_SUP_RAGE, 10),
            (SI_KNT_SUP_PAINREFLECTION, 10),
            (SI_KNT_MASTER_TWOHANDMASTER, 5),
        ],
    ),
    (
        JOB_ASSIST,
        &[(SI_ASS_CHEER_STONEHAND, 20), (SI_ASS_HEAL_PREVENTION, 20)],
    ),
    (
        JOB_RINGMASTER,
        &[
            (SI_ASS_CHEER_STONEHAND, 20),
            (SI_ASS_HEAL_PREVENTION, 20),
            (SI_RIN_SUP_PROTECT, 10),
            (SI_RIN_SUP_HOLYGUARD, 10),
            (SI_RIN_SUP_SPIRITUREFORTUNE, 10),
            (SI_RIN_SQU_GEBURAHTIPHRETH, 10),
        ],
    ),
    (
        JOB_RINGMASTER_MASTER,
        &[
            (SI_ASS_CHEER_STONEHAND, 20),
            (SI_ASS_HEAL_PREVENTION, 20),
            (SI_RIN_SUP_PROTECT, 10),
            (SI_RIN_SUP_HOLYGUARD, 10),
            (SI_RIN_SUP_SPIRITUREFORTUNE, 10),
            (SI_RIN_SQU_GEBURAHTIPHRETH, 10),
            (SI_RIG_MASTER_BLESSING, 5),
        ],
    ),
    (
        JOB_RINGMASTER_HERO,
        &[
            (SI_ASS_CHEER_STONEHAND, 20),
            (SI_ASS_HEAL_PREVENTION, 20),
            (SI_RIN_SUP_PROTECT, 10),
            (SI_RIN_SUP_HOLYGUARD, 10),
            (SI_RIN_SUP_SPIRITUREFORTUNE, 10),
            (SI_RIN_SQU_GEBURAHTIPHRETH, 10),
            (SI_RIG_MASTER_BLESSING, 5),
        ],
    ),
    (
        JOB_BILLPOSTER,
        &[
            (SI_ASS_CHEER_STONEHAND, 20),
            (SI_ASS_HEAL_PREVENTION, 20),
            (SI_BIL_PST_BARAQIJALESNA, 10),
            (SI_BIL_PST_ASMODEUS, 10),
        ],
    ),
    (
        JOB_BILLPOSTER_MASTER,
        &[
            (SI_ASS_CHEER_STONEHAND, 20),
            (SI_ASS_HEAL_PREVENTION, 20),
            (SI_BIL_PST_BARAQIJALESNA, 10),
            (SI_BIL_PST_ASMODEUS, 10),
            (SI_BIL_MASTER_KNUCKLEMASTER, 5),
        ],
    ),
    (
        JOB_BILLPOSTER_HERO,
        &[
            (SI_ASS_CHEER_STONEHAND, 20),
            (SI_ASS_HEAL_PREVENTION, 20),
            (SI_BIL_PST_BARAQIJALESNA, 10),
            (SI_BIL_PST_ASMODEUS, 10),
            (SI_BIL_MASTER_KNUCKLEMASTER, 5),
        ],
    ),
    (
        JOB_ACROBAT,
        &[
            (SI_ACR_SUP_YOYOMASTER, 20),
            (SI_ACR_SUP_BOWMASTER, 20),
            (SI_ACR_SUP_FASTWALKER, 20),
            (SI_ACR_YOYO_ABSOLUTEBLOCK, 20),
        ],
    ),
    (
        JOB_RANGER,
        &[
            (SI_ACR_SUP_BOWMASTER, 20),
            (SI_ACR_SUP_FASTWALKER, 20),
            (SI_ACR_YOYO_ABSOLUTEBLOCK, 20),
            (SI_RAG_SUP_FASTATTACK, 10),
            (SI_RAG_SUP_NATURE, 10),
        ],
    ),
    (
        JOB_RANGER_MASTER,
        &[
            (SI_ACR_SUP_BOWMASTER, 20),
            (SI_ACR_SUP_FASTWALKER, 20),
            (SI_ACR_YOYO_ABSOLUTEBLOCK, 20),
            (SI_RAG_SUP_FASTATTACK, 10),
            (SI_RAG_SUP_NATURE, 10),
            (SI_RAG_MASTER_BOWMASTER, 5),
        ],
    ),
    (
        JOB_RANGER_HERO,
        &[
            (SI_ACR_SUP_BOWMASTER, 20),
            (SI_ACR_SUP_FASTWALKER, 20),
            (SI_ACR_YOYO_ABSOLUTEBLOCK, 20),
            (SI_RAG_SUP_FASTATTACK, 10),
            (SI_RAG_SUP_NATURE, 10),
            (SI_RAG_MASTER_BOWMASTER, 5),
            (SI_RAG_HERO_HAWKEYE, 5),
        ],
    ),
    (
        JOB_JESTER,
        &[
            (SI_ACR_SUP_YOYOMASTER, 20),
            (SI_ACR_SUP_FASTWALKER, 20),
            (SI_ACR_YOYO_ABSOLUTEBLOCK, 20),
            (SI_JST_YOYO_CRITICALSWING, 10),
            (SI_JST_SUP_POISON, 10),
        ],
    ),
    (
        JOB_JESTER_MASTER,
        &[
            (SI_ACR_SUP_YOYOMASTER, 20),
            (SI_ACR_SUP_FASTWALKER, 20),
            (SI_ACR_YOYO_ABSOLUTEBLOCK, 20),
            (SI_JST_YOYO_CRITICALSWING, 10),
            (SI_JST_SUP_POISON, 10),
            (SI_JST_MASTER_YOYOMASTER, 5),
        ],
    ),
    (
        JOB_JESTER_HERO,
        &[
            (SI_ACR_SUP_YOYOMASTER, 20),
            (SI_ACR_SUP_FASTWALKER, 20),
            (SI_ACR_YOYO_ABSOLUTEBLOCK, 20),
            (SI_JST_YOYO_CRITICALSWING, 10),
            (SI_JST_SUP_POISON, 10),
            (SI_JST_MASTER_YOYOMASTER, 5),
        ],
    ),
    (
        JOB_ELEMENTOR,
        &[
            (SI_ELE_FIRE_FIREMASTER, 10),
            (SI_ELE_EARTH_EARTHMASTER, 10),
            (SI_ELE_WIND_WINDMASTER, 10),
            (SI_ELE_ELECTRICITY_LIGHTINGMASTER, 10),
            (SI_ELE_WATER_WATERMASTER, 10),
        ],
    ),
    (
        JOB_ELEMENTOR_MASTER,
        &[
            (SI_ELE_FIRE_FIREMASTER, 10),
            (SI_ELE_EARTH_EARTHMASTER, 10),
            (SI_ELE_WIND_WINDMASTER, 10),
            (SI_ELE_ELECTRICITY_LIGHTINGMASTER, 10),
            (SI_ELE_WATER_WATERMASTER, 10),
            (SI_ELE_MASTER_INTMASTER, 5),
        ],
    ),
    (
        JOB_ELEMENTOR_HERO,
        &[
            (SI_ELE_FIRE_FIREMASTER, 10),
            (SI_ELE_EARTH_EARTHMASTER, 10),
            (SI_ELE_WIND_WINDMASTER, 10),
            (SI_ELE_ELECTRICITY_LIGHTINGMASTER, 10),
            (SI_ELE_WATER_WATERMASTER, 10),
            (SI_ELE_MASTER_INTMASTER, 5),
        ],
    ),
    (JOB_PSYCHIKEEPER, &[(SI_PSY_NLG_CRUCIOSPELL, 10)]),
    (
        JOB_PSYCHIKEEPER_MASTER,
        &[(SI_PSY_NLG_CRUCIOSPELL, 10), (SI_PSY_MASTER_INTMASTER, 5)],
    ),
    (
        JOB_PSYCHIKEEPER_HERO,
        &[(SI_PSY_NLG_CRUCIOSPELL, 10), (SI_PSY_MASTER_INTMASTER, 5)],
    ),
];

/// The class buffs of `job`.
pub fn class_buffs(job: u32) -> &'static [(u32, u32)] {
    BUFF_PANG
        .iter()
        .find(|(j, _)| *j == job)
        .map_or(&[], |(_, list)| list)
}

/// The cheer buff that makes the event version of a haste skill pointless (`SI_ASS_CHEER_*`).
fn cheer_of(skill: u32) -> Option<u32> {
    match skill {
        SI_GEN_EVE_QUICKSTEP => Some(SI_ASS_CHEER_QUICKSTEP),
        SI_GEN_EVE_HASTE => Some(SI_ASS_CHEER_HASTE),
        SI_GEN_EVE_HEAPUP => Some(SI_ASS_CHEER_HEAPUP),
        SI_GEN_EVE_ACCURACY => Some(SI_ASS_CHEER_ACCURACY),
        _ => None,
    }
}

/// `CDPSrvr::OnNPCBuff` after the NPC check: the class buffs, then the official entries of `npc_key`.
pub fn on_npc_buff(svc: &mut Services, ctx: &mut GameCtx<'_>, player: PlayerRef, npc_key: &str) {
    let Some(p) = ctx.worlds.player(player.0) else {
        return;
    };
    let (job, level) = (p.mv.job, p.mv.level);
    let data = Arc::clone(ctx.data);
    // 1: the class buffs
    let pang = svc.items.core().cfg.buff_pang_update;
    if pang && let Ok(job) = u32::try_from(job) {
        for &(skill, lvl) in class_buffs(job) {
            let _ = apply_skill(
                svc,
                ctx,
                player.0,
                skill,
                lvl,
                player.0,
                Some(BUFF_PANG_MS),
                true,
            );
        }
    }
    // 2: the official entries
    let Some(def) = data.npc(npc_key) else {
        return;
    };
    for b in &def.buff_skills {
        let Some(prop) = data.skill(b.skill) else {
            continue;
        };
        if level >= b.min_player_lv && level <= b.max_player_lv {
            if b.level < prop.expert_lv || b.level > prop.expert_max {
                continue;
            }
            if let Some(cheer) = cheer_of(b.skill)
                && ctx
                    .worlds
                    .mover(player.0)
                    .is_some_and(|m| m.buffs.contains(BuffKey::new(BuffKind::Skill, cheer)))
            {
                let mut fx = Fx::new();
                fx.defined(TID_GAME_NPCBUFF_FAILED, vec![format!("\"{}\"", prop.name)]);
                fx.flush(svc, ctx, player);
                continue;
            }
            let Some(add) =
                data.add_skill_by_id(prop.sub_define.wrapping_add(b.level).wrapping_sub(1))
            else {
                continue;
            };
            let a = Applied {
                prop: std::borrow::Cow::Borrowed(prop),
                add: Some(std::borrow::Cow::Borrowed(add)),
            }
            .with_duration(b.time_ms)
            .without_refer_targets();
            do_apply_skill(svc, ctx, player.0, player.0, &a, false, 0, false);
            announce(svc, ctx, player.0, player.0, b.skill, b.level);
        } else {
            let mut fx = Fx::new();
            fx.defined(
                TID_GAME_NPCBUFF_LEVELLIMIT,
                vec![format!(
                    "{} {} \"{}\"",
                    b.min_player_lv, b.max_player_lv, prop.name
                )],
            );
            fx.flush(svc, ctx, player);
        }
    }
}
