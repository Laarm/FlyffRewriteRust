//! PvP hit types, region inspection and the PK system (Plan/17 §8.1-§8.5, §8.8; WP-COMBAT-12;
//! `src:_Common/MoverAttack.cpp:1950-2126`, `Mover.cpp:5679-5960, 6577-7030`).
//!
//! | module | content |
//! |---|---|
//! | [`mod@hit`] | `GetHitType`'s player-versus-player branch, `GetHitType2`, `IsPVPTarget` / `IsWarTarget` / `IsArenaTarget` / `IsGuildCombatTarget` |
//! | [`inspect`] | `IsPKPVPInspectionBase`, `IsPVPInspectionBase`, `IsPVPInspection` (solo), `GetSummonState`, the PK / PvP skill-region rule (F-177) |
//! | [`pk`] | pink, `PkRelation`, propensity, the chaotic restrictions, the PK EXP hook |
//! | [`kill`] | `SubPVP` (`GetPVPCase`, `SubPK`, `SubWar`), the PvP fame, `m_bLastPK` |
//! | [`mod@drop`] | `DoDropItemRandom` (the PK cost) |
//!
//! Every rule is a function over `&Mover`s and a [`PvpEnv`] (the live switches and `PKSetting.inc`), so the damage
//! pipeline can run them without services. The duels (WP-COMBAT-13) are [`super::duel`]; with the default
//! `events.pk = false` none of the PK branches runs, but all of them are implemented and tested.

pub mod drop;
pub mod hit;
pub mod inspect;
pub mod kill;
pub mod pk;

pub use hit::{
    hit_type_pvp, hit_type2, is_arena_target, is_guild_combat_target, is_pvp_target,
    is_school_target, is_war_target, region_class,
};
pub use inspect::{
    InspectError, pk_pvp_inspection_base, pvp_inspection_base, pvp_inspection_solo,
    skill_region_refused, summon_state,
};
pub use kill::{PvpCase, get_pvp_case, pk_flags, pvp_fame, sub_pvp};
pub use pk::{
    Restriction, chaotic_restriction, expire_pink, help_pink, is_pink, next_pk_propensity,
    pink_relation, pk_value_relation, propensity_penalty, propensity_relation, set_pink,
    set_pk_propensity, set_pk_value,
};

use crate::ctx::{GameCtx, Services};
use crate::live::LiveConfig;
use flyff_common::config::{EventId, RulesConfig};
use flyff_data::rules::pk_setting::PkSetting;

/// `SBS_START2`: the value of `EVE_SCHOOL_BATTLE` that makes everybody a school target (a dead event).
pub const SBS_START2: u8 = 3;

/// The switches and tables the PvP rules read: a small `Copy` snapshot so that the world phase (no services) can
/// use it. Built from the live configuration and `PKSetting.inc` ([`PvpEnv::new`]); [`PvpEnv::default`] is the
/// repack (PK off, wars on).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PvpEnv {
    /// `g_eLocal.GetState(EVE_18) == 1` (`events.eve_18`): the PvP case of a death is computed at all.
    pub eve_18: bool,
    /// `EVE_PK` (`events.pk`): player kills outside duels are possible.
    pub pk: bool,
    /// `EVE_PKCOST` (`events.pk_cost`): a chaotic victim drops items, a PK death costs EXP.
    pub pk_cost: bool,
    /// `EVE_GUILDWAR` (`events.guild_war`).
    pub guild_war: bool,
    /// `EVE_SCHOOL_BATTLE == SBS_START2` (a dead event).
    pub school_battle: bool,
    /// `PKSetting.LIMITE_LEVEL`: players up to this level cannot PK or be PKed.
    pub limit_level: i32,
    /// `PKSetting.GENERAL_ATTACK_TIME`: seconds pink after attacking a normal player.
    pub general_attack_s: u32,
    /// `PKSetting.READY_ATTACK_TIME`: seconds pink after attacking a pink player / helping.
    pub ready_attack_s: u32,
    /// `features.pvp_points_per_kill`: +1 fame per PvP kill.
    pub pvp_points_per_kill: bool,
}

impl Default for PvpEnv {
    fn default() -> Self {
        PvpEnv {
            eve_18: true,
            pk: false,
            pk_cost: false,
            guild_war: true,
            school_battle: false,
            limit_level: 20,
            general_attack_s: 16,
            ready_attack_s: 8,
            pvp_points_per_kill: true,
        }
    }
}

impl PvpEnv {
    /// The snapshot of the live event flags, the `[features]` switches and `PKSetting.inc`.
    pub fn new(live: &LiveConfig, rules: &RulesConfig, pk: &PkSetting) -> PvpEnv {
        let on = |e| live.event_state(e) != 0;
        PvpEnv {
            eve_18: live.event_state(EventId::Eve18) == 1,
            pk: on(EventId::Pk),
            pk_cost: on(EventId::PkCost),
            guild_war: live.event_state(EventId::GuildWar) == 1,
            school_battle: live.event_state(EventId::SchoolBattle) == SBS_START2,
            limit_level: pk.limit_level,
            general_attack_s: u32::try_from(pk.general_attack_time).unwrap_or(0),
            ready_attack_s: u32::try_from(pk.ready_attack_time).unwrap_or(0),
            pvp_points_per_kill: rules.features.pvp_points_per_kill,
        }
    }

    /// The snapshot of a service-context caller (`PKSetting` from the stats environment).
    pub fn from_services(s: &Services, ctx: &GameCtx<'_>) -> PvpEnv {
        PvpEnv::new(ctx.live, &ctx.cfg.rules, &s.stats.core().data.pk)
    }

    /// The snapshot of a caller that has no services (`PKSetting` from the game data of the context).
    pub fn from_ctx(ctx: &GameCtx<'_>) -> PvpEnv {
        PvpEnv::new(ctx.live, &ctx.cfg.rules, &ctx.data.pk)
    }
}
