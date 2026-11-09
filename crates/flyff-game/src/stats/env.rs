//! The read-only environment of the stat functions and the glue between a [`Mover`] and the pure formulas of
//! `flyff-rules` (Plan/16 §5, §18): [`StatsCore`] holds the rule tables, the switches and the game data; the
//! `inputs` / getter functions build a `StatInputs` from a mover and answer the questions the other documents ask
//! (`max_hp`, the clamped pool getters, the three recoveries, the DST base of the rate composites).

use crate::mover::Mover;
use flyff_common::defs::obj::MI_MALE;
use flyff_data::GameData;
use flyff_data::props::mover::MoverProp;
use flyff_rules::consts::{HEAL_TICKS, HEAL_TICKS_SLOW, SM_ATTACK_UP, SM_ATTACK_UP1, dst::*};
use flyff_rules::dst::OriginMax;
use flyff_rules::stats::{self, StatInputs, WeaponView};
use flyff_rules::{RulesConfig, RulesData};
use std::sync::Arc;

/// Everything the stat functions read that is not on the mover (16 §18.2, §5.2): the rule tables, the
/// projection of the configuration, the game data (mover props for NPC numbers) and the switches of this area.
#[derive(Debug)]
pub struct StatsCore {
    /// The tables of the formulas (`RulesData::from_game_data`).
    pub rules: RulesData,
    /// The configuration projection (`features.*`, `gameplay.set_max_*`).
    pub cfg: RulesConfig,
    /// The loaded game data (empty in tests without data).
    pub data: Arc<GameData>,
    /// The mover prop used when a mover's own prop is missing (tests without data): neutral, speed 0.1.
    pub default_prop: MoverProp,
    /// `fidelity.gold_update_self_only` (PL-075): `SetPointParam{DST_GOLD}` goes to the owner only.
    pub gold_self_only: bool,
    /// `GameData.guide_max_seq`: the highest tutorial step the server accepts (VR-004). `Guide.inc` is client
    /// data and not part of `GameData` yet; the default is generous until the data crate provides it.
    pub guide_max_seq: i32,
    /// The `DST_HEAL` interval in ticks (`features.heal_over_time`: 45, else 90).
    pub heal_ticks: i16,
    /// The switches of the EXP system (`features.exp_stop`, `features.better_clockwork_alpha`,
    /// `features.party_exp_solo`, `fidelity.party_exp_loop_fix`; WP-MOVER-08/09).
    pub exp: ExpSwitches,
    /// `features.hide_fashion`: `HideCoatReq` works (16 §13.7).
    pub hide_fashion: bool,
    /// `features.fast_jobchange`: the `UpdateJob` message works (16 §11.4).
    pub fast_jobchange: bool,
}

/// The configuration switches the EXP system reads (16 §8.5, §9).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExpSwitches {
    /// `features.exp_stop`: `MODE_EXPUP_STOP` works and the `ExpStop` message toggles it.
    pub exp_stop: bool,
    /// `features.better_clockwork_alpha`: `MI_LOADCLOCKWORK` splits its EXP evenly like the other bosses.
    pub better_clockwork_alpha: bool,
    /// `features.party_exp_solo`: a solo kill by a party member still adds party points.
    pub party_exp_solo: bool,
    /// `fidelity.party_exp_loop_fix` (PL-081): each user of an even split is capped independently, and the party
    /// walk continues where the original aborted.
    pub party_exp_loop_fix: bool,
}

impl ExpSwitches {
    /// The switches of the configuration registry.
    pub fn from_config(cfg: &flyff_common::config::RulesConfig) -> ExpSwitches {
        ExpSwitches {
            exp_stop: cfg.features.exp_stop,
            better_clockwork_alpha: cfg.features.better_clockwork_alpha,
            party_exp_solo: cfg.features.party_exp_solo,
            party_exp_loop_fix: cfg.fidelity.party_exp_loop_fix,
        }
    }
}

/// The tutorial range used while `GameData` has no `guide_max_seq` (the real `Guide.inc` ends far below it).
pub const DEFAULT_GUIDE_MAX_SEQ: i32 = 1000;

impl Default for StatsCore {
    fn default() -> Self {
        StatsCore::new(
            Arc::default(),
            &flyff_common::config::RulesConfig::default(),
        )
    }
}

impl StatsCore {
    /// Builds the environment from the loaded data and the configuration (called once at start).
    pub fn new(data: Arc<GameData>, cfg: &flyff_common::config::RulesConfig) -> StatsCore {
        let rules_cfg = RulesConfig::from_config(cfg);
        StatsCore {
            rules: RulesData::from_game_data(&data),
            data,
            default_prop: MoverProp {
                id: MI_MALE,
                speed: stats::PLAYER_BASE_SPEED,
                level: 1,
                ..MoverProp::default()
            },
            gold_self_only: cfg.fidelity.gold_update_self_only,
            guide_max_seq: DEFAULT_GUIDE_MAX_SEQ,
            heal_ticks: if rules_cfg.heal_over_time {
                HEAL_TICKS
            } else {
                HEAL_TICKS_SLOW
            },
            exp: ExpSwitches::from_config(cfg),
            hide_fashion: cfg.features.hide_fashion,
            fast_jobchange: cfg.features.fast_jobchange,
            cfg: rules_cfg,
        }
    }

    /// The same environment over hand-built tables (tests): the configuration defaults of the repack.
    pub fn with_rules(rules: RulesData) -> StatsCore {
        StatsCore {
            rules,
            ..StatsCore::default()
        }
    }
}

/// The stats environment as a game service (`Services.stats`): a cheap handle to the shared [`StatsCore`].
#[derive(Clone, Debug, Default)]
pub struct StatsService {
    core: Arc<StatsCore>,
}

impl StatsService {
    /// A service over `core`.
    pub fn new(core: Arc<StatsCore>) -> StatsService {
        StatsService { core }
    }

    /// The environment.
    pub fn core(&self) -> &StatsCore {
        &self.core
    }

    /// A shared handle to the environment (the world hooks keep one).
    pub fn handle(&self) -> Arc<StatsCore> {
        Arc::clone(&self.core)
    }
}

/// The mover prop the formulas read for `m`: the prop of its `core.index` (`MI_MALE` / `MI_FEMALE` for a
/// player), else the neutral default.
pub fn prop_of<'a>(m: &Mover, c: &'a StatsCore) -> &'a MoverProp {
    c.data.mover(m.prop_id()).unwrap_or(&c.default_prop)
}

/// `SM_ATTACK_UP | SM_ATTACK_UP1` active (`IsSMMode`: the timer is positive).
pub fn sm_attack_up(m: &Mover) -> bool {
    m.player()
        .is_some_and(|p| p.sm.timers[SM_ATTACK_UP] > 0 || p.sm.timers[SM_ATTACK_UP1] > 0)
}

/// `IsSMMode(sm)`: the timer or flag is positive (16 §14.1).
pub fn sm_active(m: &Mover, sm: usize) -> bool {
    m.player()
        .and_then(|p| p.sm.timers.get(sm))
        .is_some_and(|t| *t > 0)
}

/// The inputs of the formulas for `m` with the given weapon views (bare hands: `None`). The equipment
/// (WP-MOVER-11) supplies the views; every pool, regeneration and stat getter below needs none.
pub fn inputs<'a>(
    m: &'a Mover,
    c: &'a StatsCore,
    weapon: Option<WeaponView<'a>>,
    left_weapon: Option<WeaponView<'a>>,
) -> StatInputs<'a> {
    StatInputs {
        is_player: m.is_player(),
        level: m.mv.level,
        job: m.mv.job,
        base: m.mv.base,
        dst: &m.mv.dst.table,
        equip_cache: m.mv.equip_cache,
        weapon,
        left_weapon,
        mover_prop: prop_of(m, c),
        rates: m.mv.rates,
        sm_attack_up: sm_attack_up(m),
    }
}

/// [`inputs`] without weapons.
pub fn bare_inputs<'a>(m: &'a Mover, c: &'a StatsCore) -> StatInputs<'a> {
    inputs(m, c, None, None)
}

/// `GetMaxHitPoint()`.
pub fn max_hp(m: &Mover, c: &StatsCore) -> i32 {
    stats::max_hp(&bare_inputs(m, c), &c.rules)
}

/// `GetMaxManaPoint()`.
pub fn max_mp(m: &Mover, c: &StatsCore) -> i32 {
    stats::max_mp(&bare_inputs(m, c), &c.rules)
}

/// `GetMaxFatiguePoint()`.
pub fn max_fp(m: &Mover, c: &StatsCore) -> i32 {
    stats::max_fp(&bare_inputs(m, c), &c.rules)
}

/// `GetMaxPoint(dst)` for `DST_HP` / `DST_MP` / `DST_FP`, else 0.
pub fn max_of(m: &Mover, c: &StatsCore, dst: u32) -> i32 {
    match dst {
        DST_HP => max_hp(m, c),
        DST_MP => max_mp(m, c),
        DST_FP => max_fp(m, c),
        _ => 0,
    }
}

/// `GetHitPoint()` / `GetManaPoint()` / `GetFatiguePoint()` (`MoverParam.cpp:2796-2840`): `param(dst, stored)`,
/// at most the maximum with `features.fix_get_hp_mp_fp_point`. The stored value is not reduced when the maximum
/// drops (KEEP). `DST_GOLD` is the owner's gold; any other dst gives 0.
pub fn point_param(m: &Mover, c: &StatsCore, dst: u32) -> i32 {
    let stored = match dst {
        DST_HP => m.mv.hp,
        DST_MP => m.mv.mp,
        DST_FP => m.mv.fp,
        DST_GOLD => return m.player().map_or(0, |p| p.items.gold),
        _ => return 0,
    };
    let v = m.mv.dst.table.param(dst, stored);
    if c.cfg.fix_get_hp_mp_fp_point {
        v.min(max_of(m, c, dst))
    } else {
        v
    }
}

/// The three recoveries (`GetHPRecovery`, `GetMPRecovery`, `GetFPRecovery`).
pub fn recoveries(m: &Mover, c: &StatsCore) -> (i32, i32, i32) {
    let i = bare_inputs(m, c);
    (
        stats::hp_recovery(&i, &c.rules),
        stats::mp_recovery(&i, &c.rules),
        stats::fp_recovery(&i, &c.rules),
    )
}

/// `GetMaxOrigin*Point(TRUE)`: the base the rate composites are computed from at apply time (16 §4.4).
pub fn origin_max(m: &Mover, c: &StatsCore) -> OriginMax {
    let i = bare_inputs(m, c);
    OriginMax {
        hp: stats::max_hp_origin(&i, &c.rules, true),
        mp: stats::max_mp_origin(&i, &c.rules, true),
        fp: stats::max_fp_origin(&i, &c.rules, true),
    }
}

/// Writes the DST-derived and stat-derived inputs of the shared sim back into `m.sim.params` (31 §5.4):
/// `speed_adj_pct`, `speed_chg_set`, `jumping`, `chr_state`, `hawkeye_rate` from the aggregate,
/// `attack_speed` / `casting_ani_speed` from the formulas, `level`, `hp_positive` and `mode`. Call it after
/// every DST change, level change and weapon change (the DST functions of this module do).
pub fn refresh_sim_params(m: &mut Mover, c: &StatsCore) {
    let (attack_speed, casting_ani_speed) = {
        // the worn weapons decide the attack speed (doc 16 §13: `stats::equip::weapon_views`)
        let (right, left) = super::equip::weapon_views(m, &c.data);
        let i = inputs(m, c, right, left);
        (
            stats::attack_speed(&i, &c.rules),
            stats::casting_ani_speed(&m.mv.dst.table),
        )
    };
    let t = &m.mv.dst.table;
    let p = &mut m.sim.params;
    p.speed_adj_pct = t.adj(DST_SPEED);
    p.speed_chg_set = t.chg(DST_SPEED) != flyff_rules::consts::NULL_CHGPARAM;
    p.jumping = t.param(DST_JUMPING, 0);
    p.chr_state = t.adj(DST_CHRSTATE) as u32;
    p.hawkeye_rate = t.param(DST_HAWKEYE_RATE, 0);
    p.attack_speed = attack_speed;
    p.casting_ani_speed = casting_ani_speed;
    p.level = u32::try_from(m.mv.level).unwrap_or(1);
    p.hp_positive = m.mv.hp > 0;
    p.mode = m.mv.mode;
}
