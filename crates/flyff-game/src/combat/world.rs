//! The combat runtime state the world phases and the client handlers share (Plan/17 §3, §4, §9; WP-COMBAT-05 ..
//! 09).
//!
//! The hit events of the movement phase (the barrier of a frame) have no [`crate::ctx::Services`]: they hold the
//! object store, the world's random stream and a message sink. The handlers have the services but run in
//! phase 1. Everything both sides need therefore lives in [`CombatWorld`], a field of
//! [`crate::movement::MovementState`] (`Worlds.movement.combat`), which both `FrameCtx.movement` and
//! `GameCtx.worlds.movement` reach:
//!
//! * [`CombatWorld::hooks`]: the seams of the damage pipeline (`CombatHooks`), installed by the owners of the
//!   later work packages;
//! * [`CombatWorld::skill`]: the seam of the skill hit handlers (WP-SKILL-05);
//! * [`CombatWorld::queues`]: the deaths waiting for the service-context pipeline ([`super::death`]), the server
//!   projectiles of every layer and the projectile id counter;
//! * [`CombatWorld::env`]: the per-tick snapshot of everything the pipeline reads from the services (rule
//!   tables, event factors, live settings), refreshed by [`super::tick::prepare`] right before the world phase.
//!
//! The deaths wait because their consumers (kill EXP, drops, quests, ...) need the services; the damage that
//! kills a mover only leaves it at 0 HP (the HP commit of the arbiter) and queues a [`KillCtx`].

use super::env::DamageEnv;
use super::hooks::{CombatHooks, KillCtx, NoHooks};
use super::projectile::ProjectileManager;
use super::pvp::PvpEnv;
use super::skillhit::SkillHitHooks;
use crate::event::EventFactors;
use crate::stats::StatsCore;
use flyff_common::config::RulesConfig;
use flyff_rules::combat::{AttributeTable, CombatConfig};
use std::sync::Arc;

/// The Event.lua flat ATK / DEF bonuses of the tick (`EventLua.GetAttackPower()` / `GetDefensePower()`), the only
/// event factors the damage pipeline reads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FixedFactors {
    /// `EventLua.GetAttackPower()`.
    pub attack: i32,
    /// `EventLua.GetDefensePower()`.
    pub defense: i32,
}

impl EventFactors for FixedFactors {
    fn lua_attack(&self) -> i32 {
        self.attack
    }
    fn lua_defense(&self) -> i32 {
        self.defense
    }
}

/// The switches the action layer reads (Plan/17 §0.5, §12): the formulas' switches are
/// [`CombatConfig`]'s.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActionConfig {
    /// `features.faster_projectiles` (`__FASTER_PROJECTILES`): the projectile speeds 0.4 / 0.45 instead of 0.2 / 0.25.
    pub faster_projectiles: bool,
    /// `features.no_arrows_poster` (`__NO_ARROWS_POSTER`): a bow shot consumes no arrow.
    pub no_arrows_poster: bool,
    /// `features.no_yoyo_knockback`: yo-yo hits never push.
    pub no_yoyo_knockback: bool,
    /// `features.splash_autoattacks`: yo-yo autoattacks hit everything within 5 m of the target.
    pub splash_autoattacks: bool,
    /// `features.wand_autoattacks`: the wand charge multiplier is 1.0.
    pub wand_autoattacks: bool,
    /// `features.better_clockwork_alpha`.
    pub better_clockwork_alpha: bool,
    /// `fidelity.player_melee_can_miss` (KQ-03).
    pub player_melee_can_miss: bool,
    /// `features.better_berserk` (`__BETTER_BERSERK`): skills stay usable under Berserk (WP-SKILL-04 step 4).
    pub better_berserk: bool,
    /// `features.rm_hero_skill` (`__RM_HERO_SKILL`): Hero Return casts the assist buffs (WP-SKILL-04 steps 7 / 9,
    /// WP-SKILL-05).
    pub rm_hero_skill: bool,
    /// `features.speed_cast`: `DST_SPELL_RATE` also speeds melee-skill animations (the notification's `ani_speed`).
    pub speed_cast: bool,
    /// The `EVE_SCHOOL` flag (dead event; no Resurrection skill while it runs, step 9).
    pub school_event: bool,
}

impl ActionConfig {
    /// The projection of the configuration registry.
    pub fn from_config(cfg: &RulesConfig) -> ActionConfig {
        let f = &cfg.features;
        ActionConfig {
            faster_projectiles: f.faster_projectiles,
            no_arrows_poster: f.no_arrows_poster,
            no_yoyo_knockback: f.no_yoyo_knockback,
            splash_autoattacks: f.splash_autoattacks,
            wand_autoattacks: f.wand_autoattacks,
            better_clockwork_alpha: f.better_clockwork_alpha,
            player_melee_can_miss: cfg.fidelity.player_melee_can_miss,
            better_berserk: f.better_berserk,
            rm_hero_skill: f.rm_hero_skill,
            speed_cast: f.speed_cast,
            school_event: false,
        }
    }

    /// The same switches with the running events of `live` (the school event).
    pub fn with_events(mut self, live: &crate::live::LiveConfig) -> ActionConfig {
        use flyff_common::config::EventId;
        self.school_event = live.event_state(EventId::School) != 0;
        self
    }
}

impl Default for ActionConfig {
    fn default() -> Self {
        ActionConfig::from_config(&RulesConfig::default())
    }
}

/// What the damage pipeline and the action layer read that is not on a mover, as the world phases see it: a
/// snapshot refreshed once per tick ([`super::tick::prepare`]).
#[derive(Clone, Debug)]
pub struct CombatEnv {
    /// Rule tables, the stats configuration and the loaded game data.
    pub core: Arc<StatsCore>,
    /// The formula switches.
    pub cfg: CombatConfig,
    /// The action-layer switches.
    pub act: ActionConfig,
    /// The `ItemUpgrade.lua` element factors.
    pub attr: Arc<AttributeTable>,
    /// The Event.lua flat bonuses of the tick.
    pub factors: FixedFactors,
    /// Kill-steal protection is on: `events.steal` is **false** (the repack default is `true` = off).
    pub steal_protection: bool,
    /// `rates.monster_hit` (1.0 when unset).
    pub monster_hit_rate: f32,
    /// The PvP switches of the tick.
    pub pvp: PvpEnv,
}

impl CombatEnv {
    /// The read-only environment of the damage pipeline.
    pub fn damage_env(&self) -> DamageEnv<'_> {
        DamageEnv {
            core: &self.core,
            cfg: self.cfg,
            attr: &self.attr,
            events: &self.factors,
            steal_protection: self.steal_protection,
            monster_hit_rate: self.monster_hit_rate,
            pvp: self.pvp,
        }
    }
}

/// Counters of the combat runtime (tests and metrics).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CombatStats {
    /// `AttrHit` events handled.
    pub hit_events: u64,
    /// Server projectiles spawned.
    pub projectiles_spawned: u64,
    /// Projectiles that ended with an impact.
    pub projectiles_hit: u64,
    /// Projectiles that ended without one (target gone, timeout).
    pub projectiles_expired: u64,
    /// Deaths processed by the service-context pipeline.
    pub deaths: u64,
    /// Attack requests dropped by the VR-004 enum checks.
    pub requests_dropped: u64,
}

/// The queues and tables of the runtime (see the module documentation).
#[derive(Debug, Default)]
pub struct CombatQueues {
    /// Movers that reached 0 HP and wait for [`super::death::process_pending`], in the order they died.
    pub deaths: Vec<KillCtx>,
    /// The live server projectiles of every layer.
    pub projectiles: ProjectileManager,
    /// The counters.
    pub stats: CombatStats,
    /// The skill hits whose effect needs the services (WP-SKILL-05): they run right after the world phase
    /// ([`crate::skill::cast::process_pending`]).
    pub skill_jobs: Vec<crate::skill::cast::SkillJob>,
    /// Non-zero while [`crate::skill::cast::process_pending`] runs jobs: a nested
    /// [`crate::skill::cast::drain_jobs`] (a job's own damage call) then leaves its jobs for the running loop.
    pub skill_depth: u8,
}

/// The combat runtime of the worlds (`Worlds.movement.combat`).
pub struct CombatWorld {
    /// The seams of the damage pipeline ([`NoHooks`] until an owner installs its implementation).
    pub hooks: Box<dyn CombatHooks>,
    /// The skill hit handlers ([`crate::skill::SkillHits`]; a test may install [`super::skillhit::NoSkillHits`]).
    pub skill: Box<dyn SkillHitHooks>,
    /// The queues and tables.
    pub queues: CombatQueues,
    /// The snapshot of the services' state; `None` until the first [`super::tick::prepare`] (the hit handlers
    /// of the phase then do nothing).
    pub env: Option<CombatEnv>,
}

impl Default for CombatWorld {
    fn default() -> Self {
        CombatWorld {
            hooks: Box::new(NoHooks),
            skill: Box::new(crate::skill::SkillHits),
            queues: CombatQueues::default(),
            env: None,
        }
    }
}

impl std::fmt::Debug for CombatWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CombatWorld")
            .field("queues", &self.queues)
            .field("env", &self.env.is_some())
            .finish_non_exhaustive()
    }
}
