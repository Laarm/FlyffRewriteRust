//! The read-only environment and the working context of a damage call (Plan/17 §5; WP-COMBAT-04).

use super::effects::{Arena, Effects};
use super::hooks::CombatHooks;
use super::pvp::PvpEnv;
use crate::event::EventFactors;
use crate::stats::StatsCore;
use flyff_rules::combat::{AttributeTable, CombatConfig, CombatCx};

/// What the pipeline reads that is not on a mover: the rule tables and game data of the stats service, the
/// combat switches, the element table, the running events and the live settings.
pub struct DamageEnv<'a> {
    /// Rule tables, the stats configuration and the loaded game data.
    pub core: &'a StatsCore,
    /// The combat switches (Plan/17 §12).
    pub cfg: CombatConfig,
    /// The `ItemUpgrade.lua` element factors.
    pub attr: &'a AttributeTable,
    /// The Event.lua factors (flat ATK / DEF of players).
    pub events: &'a dyn EventFactors,
    /// Kill-steal protection is on: `events.steal` is **false** (the repack default is `true` = off).
    pub steal_protection: bool,
    /// `prj.m_fMonsterHitRate` (`rates.monster_hit`, 1.0 when unset).
    pub monster_hit_rate: f32,
    /// The PvP switches: `events.pk`, wars, `PKSetting.inc` (Plan/17 §8).
    pub pvp: PvpEnv,
}

impl DamageEnv<'_> {
    /// The context of the pure formulas.
    pub fn cx(&self) -> CombatCx<'_> {
        let mut cx = CombatCx::new(&self.cfg, self.attr);
        cx.event_attack_power = self.events.lua_attack();
        cx.event_defense_power = self.events.lua_defense();
        cx.monster_hit_rate = self.monster_hit_rate;
        cx
    }
}

/// Everything one damage call works with: the world (`w`), the collected [`Effects`] (`fx`), the environment and
/// the seams. A hook gets `&mut cx.w` and `&mut cx.fx` as separate borrows.
pub struct ArbiterCx<'a> {
    /// The mutable world.
    pub w: Arena<'a>,
    /// What the call wants done outside the movers.
    pub fx: Effects,
    /// The read-only environment.
    pub env: &'a DamageEnv<'a>,
    /// The seams.
    pub hooks: &'a mut dyn CombatHooks,
}

impl<'a> ArbiterCx<'a> {
    /// A context with no effects collected yet.
    pub fn new(
        w: Arena<'a>,
        env: &'a DamageEnv<'a>,
        hooks: &'a mut dyn CombatHooks,
    ) -> ArbiterCx<'a> {
        ArbiterCx {
            w,
            fx: Effects::default(),
            env,
            hooks,
        }
    }
}
