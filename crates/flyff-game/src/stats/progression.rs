//! Experience, level-ups and EXP stop (Plan/16 §8, WP-MOVER-08; F-124, F-134, PL-083).
//!
//! * [`add_experience`] is `CMover::AddExperience`: the checks that need the mover (EXP stop, dead), the EXP
//!   factor ([`exp_factor_inputs`]), the angel buff, then the pure core `flyff_rules::progression::level_up_math`;
//!   afterwards the full refill of HP/MP/FP, the skill points of each new level and the PK hook. It changes the
//!   mover and returns an [`ExpResult`]; it sends nothing.
//! * [`grant_experience`] is `AddExperience` plus everything the callers do after it (`LevelUpSetting` /
//!   `ExpUpSetting`, `AddSetExperience`): the level-up hooks of docs 21/22, then the messages of 16 §8.4 in this
//!   order: `SetLevel` (near, without self), `SetGrowthLearningPoint` (self), three forced `SetPointParam`
//!   (PL-083, near including self) and finally `SetExperience` (self); then the directory update, the audit line
//!   `char.level_up` and an urgent save.
//! * [`set_level`], [`add_gp`], [`add_skill_points`], [`add_fxp`] and [`toggle_exp_stop`] are the small setters of
//!   §8.6, §9.4 and §8.5.
//!
//! The hooks that belong to other documents are plain functions and traits with the "feature absent" default:
//! [`ExpHooks`] (rest points: doc 22 F-135; the PK EXP hook: WP-MOVER-21), [`level_up_gift`] (doc 22) and
//! [`campus_reward`] (doc 21). Each owner fills in its body; the call sites and their order are final.

use super::env::{StatsCore, refresh_sim_params};
use super::modes::{clear_mode_bits, set_mode_bits};
use super::out::Out;
use super::pool::{fill_pools_silent, send_pools_absolute};
use crate::buff::BUFF_ITEM;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::event::EventFactors;
use crate::mover::Mover;
use flyff_common::defs::item::*;
use flyff_common::defs::itemkind::IK3_ANGEL_BUFF;
use flyff_proto::pb;
use flyff_rules::consts::EXPUP_STOP_MODE;
use flyff_rules::exp_share::flight_level;
use flyff_rules::progression::{
    AngelUpdate, ExpFactorInputs, ExpInput, LevelStep, ProgressState, Tier, exp_factor,
    level_up_math, pk_exp_step, skill_points_on_level,
};
use flyff_rules::skill_points::{grant as sp_grant, sp_cap};

/// The `Text` the `ExpStop` toggle sends (`DPSrvr.cpp:13288-13303`, literal strings).
pub const TEXT_EXP_STOP_ON: &str = "EXP Stop Activated!";
/// See [`TEXT_EXP_STOP_ON`].
pub const TEXT_EXP_STOP_OFF: &str = "EXP Stop Deactivated!";

/// The extension points of `AddExperience` that other documents fill in; every method has the "feature absent"
/// default (16 §8.2 steps 3 and 7).
pub trait ExpHooks {
    /// `GetAddExpAfterApplyRestPoint(n)` (doc 22, F-135, M7): the extra EXP of the guild-house rest points for a
    /// grant of `n` (already halved for masters and heroes). 0 until doc 22 implements it.
    fn rest_point_bonus(&mut self, _m: &mut Mover, _n: i64) -> i64 {
        0
    }

    /// The PK propensity hook of a first-call monster kill by a chaotic player (16 §16, `MoverParam.cpp:1288-1310`;
    /// WP-MOVER-21): `n` is the EXP that went into the pool, `level` the level before the level-up.
    fn pk_exp(&mut self, _m: &mut Mover, _n: i64, _level: i32) {}
}

/// The hooks of a game without docs 21/22 and the PK system.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoExpHooks;

impl ExpHooks for NoExpHooks {}

/// The PK propensity hook of `AddExperience` (`MoverParam.cpp:1288-1310`, 16 §16, WP-MOVER-21) for a chaotic player
/// that earns monster EXP: `pk_exp` collects the EXP and every full `PKSetting.LEVEL_EXP(level)` lowers the
/// propensity by one (never below 0). The change is announced with `PkRelation{PROPENSITY}` to the near set and
/// audited as `char.pk`; the messages wait in [`PkExpHooks::out`] so that `grant_experience` can place them where
/// the original sent them (inside `AddExperience`, before the level-up messages).
pub struct PkExpHooks<'a> {
    core: &'a StatsCore,
    /// The messages the hook queued.
    pub out: Out,
    /// The propensity after a change, for the audit line.
    pub changed: Option<u32>,
    /// The guild-house rest-point factor of the player (1.0 when his guild owns a house, 0.0 else; doc 22 §12.7, task
    /// guild-house); `grant_experience` sets it before the grant.
    pub rest_factor: f32,
}

impl<'a> PkExpHooks<'a> {
    /// A hook over the stats environment (the `PKSetting` table is `core.data.pk`).
    pub fn new(core: &'a StatsCore) -> PkExpHooks<'a> {
        PkExpHooks {
            core,
            out: Out::new(),
            changed: None,
            rest_factor: 0.0,
        }
    }
}

impl ExpHooks for PkExpHooks<'_> {
    // ---- guild house (doc 22 §12.7, task guild-house): the rest-point bonus
    fn rest_point_bonus(&mut self, m: &mut Mover, n: i64) -> i64 {
        crate::event::guild_house::rest::exp_bonus(m, self.core, self.rest_factor, n)
    }

    fn pk_exp(&mut self, m: &mut Mover, n: i64, level: i32) {
        if let Some(propensity) = pk_exp_hook(m, self.core, n, level, &mut self.out) {
            self.changed = Some(propensity);
        }
    }
}

/// The body of the PK EXP hook on `m` (`level` is the level before the grant): returns the new propensity when it
/// was lowered, after queueing `PkRelation{PROPENSITY}` to the near set.
pub fn pk_exp_hook(m: &mut Mover, c: &StatsCore, n: i64, level: i32, out: &mut Out) -> Option<u32> {
    let level_exp = c.data.pk.level_exp(level);
    let p = m.player_mut()?;
    if p.pk.propensity == 0 {
        return None;
    }
    let step = pk_exp_step(p.pk.pk_exp, p.pk.propensity, n, level_exp);
    p.pk.pk_exp = step.pk_exp;
    p.pk.propensity = step.propensity;
    if !step.changed {
        return None;
    }
    out.near(pb::PkRelation {
        sub: Some(pb::pk_relation::Sub::Propensity(
            pb::pk_relation::PkPropensity {
                propensity: step.propensity,
            },
        )),
    });
    Some(step.propensity)
}

/// What `add_experience` reads besides the mover.
pub struct ExpEnv<'a> {
    /// Rule tables, switches, game data.
    pub core: &'a StatsCore,
    /// The running events' factors (doc 22): `Services.event.factors()`, or a replacement in tests.
    pub events: &'a dyn EventFactors,
    /// `rates.monster_exp` (`prj.m_fMonsterExpRate`): 1.0 when unset.
    pub monster_exp_rate: f32,
    /// The hooks of other documents.
    pub hooks: &'a mut dyn ExpHooks,
}

/// What an [`add_experience`] call did (the mover itself is already changed).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExpResult {
    /// The return value of `AddExperience`: a level-up or a tier-cap reset (callers treat it as a level-up).
    pub changed: bool,
    /// The real level-ups in order.
    pub steps: Vec<LevelStep>,
    /// The level before the call.
    pub level_before: i32,
    /// The EXP that went into the pool in the first pass.
    pub added: i64,
    /// Skill points granted by the level-ups (after the cap).
    pub sp_granted: i32,
    /// The levels at which a master earned a skill-level growth (`SetMasterSkillPointUp`, WP-MOVER-19).
    pub master_skill_levels: Vec<i32>,
    /// The angel experience gain (doc 19 sends the `Angel` message).
    pub angel: Option<AngelUpdate>,
}

impl ExpResult {
    /// Nothing happened.
    fn none(level: i32) -> ExpResult {
        ExpResult {
            changed: false,
            steps: Vec::new(),
            level_before: level,
            added: 0,
            sp_granted: 0,
            master_skill_levels: Vec::new(),
            angel: None,
        }
    }
}

/// The inputs of `GetExpFactor()` for `m` (16 §8.3): the mode word, the buff list, the aggregate and the event
/// factors.
pub fn exp_factor_inputs(
    m: &Mover,
    events: &dyn EventFactors,
    monster_exp_rate: f32,
) -> ExpFactorInputs {
    let has = |id: u32| m.buffs.has(BUFF_ITEM, id);
    let normal = [
        II_SYS_SYS_SCR_AMPESA,
        II_SYS_SYS_SCR_AMPESA1,
        II_SYS_SYS_SCR_AMPESA2,
        II_SYS_SYS_SCR_AMPESB,
        II_SYS_SYS_SCR_AMPESB1,
        II_SYS_SYS_SCR_AMPESB2,
        II_SYS_SYS_SCR_AMPESC,
        II_SYS_SYS_SCR_AMPESC1,
        II_SYS_SYS_SCR_AMPESC2,
        II_SYS_SYS_SCR_AMPESD,
        II_SYS_SYS_SCR_AMPESD1,
        II_SYS_SYS_SCR_AMPESD2,
        II_SYS_SYS_SCR_AMPESE,
    ];
    let special = [
        II_SYS_SYS_SCR_AMPESS,
        II_SYS_SYS_SCR_AMPESS1,
        II_SYS_SYS_SCR_AMPESS2,
    ];
    let special_count = special.iter().filter(|id| has(**id)).count();
    let ampes_count = normal.iter().filter(|id| has(**id)).count() + special_count;
    ExpFactorInputs {
        oldboy_mode: m.mv.mode & flyff_rules::consts::EVENT_OLDBOY_MODE != 0,
        ampes_count: ampes_count as u32,
        ampes_special: special_count > 0,
        ampes_legacy: has(II_SYS_SYS_SCR_AMPES),
        ampem: has(II_SYS_SYS_SCR_AMPEM),
        glv_cheer: has(II_SYS_SYS_SCR_GLVCHEER),
        cheer_up: has(II_CHEERUP),
        lua_cheer_factor: events.lua_cheer_exp(),
        generic_event: events.generic_exp(),
        lua_event: events.lua_exp(),
        monster_exp_rate,
        lord_e_factor: events.lord_exp(),
        ls_cheering: has(II_SYS_SYS_LS_CHEERING),
        vote_thanks: has(II_SYS_SYS_VOTE_THANKS),
        weather_factor: events.weather_effect().then(|| events.lua_weather_exp()),
    }
}

/// `GetExpFactor()` of `m`.
pub fn exp_factor_of(m: &Mover, events: &dyn EventFactors, monster_exp_rate: f32) -> f32 {
    exp_factor(
        &exp_factor_inputs(m, events, monster_exp_rate),
        m.mv.level,
        &m.mv.dst.table,
    )
}

/// `HasBuffByIk3(IK3_ANGEL_BUFF)`: the angel buff's `nAdjParamVal1` (100 when the buff has no item prop), `None`
/// without the buff. Stand-in for doc 17's `BuffSet::by_ik3`.
fn angel_buff(m: &Mover, c: &StatsCore) -> Option<i32> {
    crate::buff::core::find_ik3(m, &c.data, IK3_ANGEL_BUFF)
        .and_then(|k| c.data.item(u32::from(k.id)))
        .map(|p| p.adj_param_val[0])
}

/// `/AExp <n>` (`TextCmd_AngelExp`, `FuncTextCmd.cpp:2812`, 23 row 154): with an angel buff the angel EXP grows by `n`
/// but at most to `exp1(angel level) / 100 * percent`, where `percent` is the buff item's `nAdjParamVal1` (100 when
/// the buff has no prop or the value is outside `1..=100`); reaching the cap is `complete`. Nothing happens without
/// the buff or when the angel EXP is already at the cap. `Angel{state 1, exp, level, complete}` goes to the owner.
/// Returns the new angel EXP when it changed.
pub fn add_angel_exp(m: &mut Mover, c: &StatsCore, n: i64, out: &mut Out) -> Option<i64> {
    let percent = angel_buff(m, c)?;
    let percent = if (1..=100).contains(&percent) {
        percent
    } else {
        100
    };
    let p = m.player_mut()?;
    let max = c
        .rules
        .exp_row(p.progression.angel_level)
        .map_or(0, |r| r.exp1)
        / 100
        * i64::from(percent);
    if p.progression.angel_exp >= max {
        return None;
    }
    p.progression.angel_exp = p.progression.angel_exp.wrapping_add(n);
    let complete = p.progression.angel_exp > max;
    if complete {
        p.progression.angel_exp = max;
    }
    out.to_self(pb::Angel {
        state: 1,
        angel_exp: p.progression.angel_exp,
        angel_level: p.progression.angel_level,
        complete,
    });
    Some(p.progression.angel_exp)
}

/// `CMover::AddSkillPoint(n)` (`Mover.cpp:9477`): under the job's cap (`sp_cap`) the points go to both the
/// unspent points and the ever-granted total. Returns the points granted.
pub fn add_skill_points(m: &mut Mover, c: &StatsCore, n: i32) -> i32 {
    let job = m.mv.job;
    let Some(p) = m.player_mut() else { return 0 };
    let granted = sp_grant(p.progression.skill_level_total, n, sp_cap(&c.rules, job));
    p.progression.skill_points = p.progression.skill_points.wrapping_add(granted);
    p.progression.skill_level_total = p.progression.skill_level_total.wrapping_add(granted);
    granted
}

/// `CMover::AddExperience(n, first_call, multiply, from_monster)` (`MoverParam.cpp:1057-1481`, 16 §8.2) on a
/// player: returns what happened; the mover is changed, nothing is sent.
///
/// A no-op (`changed == false`) for: `MODE_EXPUP_STOP` (with `features.exp_stop`), `n <= 0`, a dead mover and a
/// non-player. After the pure core ([`level_up_math`]) the function applies, per real level-up, the
/// consequences that need the mover: HP/MP/FP are set to the new maxima silently (the messages are the
/// sequence's, PL-083), the skill points `(level - 1) / 20 + 2` of a level that is not regained
/// (`death_level < level`), the master/hero rules (masters note the level for `SetMasterSkillPointUp`, heroes get
/// nothing), and finally the sim parameters (attack speed depends on the level).
pub fn add_experience(
    env: &mut ExpEnv<'_>,
    m: &mut Mover,
    n: i64,
    first_call: bool,
    multiply: bool,
    from_monster: bool,
) -> ExpResult {
    let c = env.core;
    let level0 = m.mv.level;
    if (c.exp.exp_stop && m.mv.mode & EXPUP_STOP_MODE != 0) || n <= 0 || m.mv.hp <= 0 {
        return ExpResult::none(level0);
    }
    let Some(p) = m.player() else {
        return ExpResult::none(level0);
    };
    let mut st = ProgressState {
        level: m.mv.level,
        exp: p.progression.exp,
        stat_points: p.progression.stat_points,
        death_level: p.progression.death_level,
        angel_exp: p.progression.angel_exp,
        angel_level: p.progression.angel_level,
    };
    let tier = Tier::of(&c.rules, m.mv.job);
    let factor = if first_call && multiply {
        exp_factor_of(m, env.events, env.monster_exp_rate)
    } else {
        1.0
    };
    let angel = if first_call { angel_buff(m, c) } else { None };
    let hooks = &mut *env.hooks;
    let o = {
        let mut rest = |n: i64| hooks.rest_point_bonus(m, n);
        level_up_math(
            &c.rules,
            tier,
            &mut st,
            ExpInput {
                n,
                first_call,
                multiply,
                factor,
                rest_bonus: &mut rest,
                angel,
            },
        )
    };
    // write the state back
    m.mv.level = st.level;
    if let Some(p) = m.player_mut() {
        p.progression.exp = st.exp;
        p.progression.stat_points = st.stat_points;
        p.progression.angel_exp = st.angel_exp;
        p.progression.max_level = p
            .progression
            .max_level
            .max(u16::try_from(st.level).unwrap_or(1));
    }
    // 7: the PK hook (first call, monster kill, chaotic player), before the level-up consequences
    if first_call && from_monster && m.player().is_some_and(|p| p.pk.propensity > 0) {
        env.hooks.pk_exp(m, o.added, o.level_before);
    }
    // the consequences of each real level-up
    let mut sp_granted = 0;
    let mut master_skill_levels = Vec::new();
    if !o.steps.is_empty() {
        fill_pools_silent(m, c);
    }
    for step in &o.steps {
        if step.regained {
            continue;
        }
        if tier.master {
            master_skill_levels.push(step.level);
        } else if !tier.hero {
            sp_granted += add_skill_points(m, c, skill_points_on_level(step.level));
        }
    }
    if o.changed || !o.steps.is_empty() {
        refresh_sim_params(m, c);
    }
    ExpResult {
        changed: o.changed,
        steps: o.steps,
        level_before: o.level_before,
        added: o.added,
        sp_granted,
        master_skill_levels,
        angel: o.angel,
    }
}

/// `AddSetExperience(exp1, level, skill point, skill level, death exp, death level)` to self: the message that
/// ends every EXP gain, level-up and death penalty (16 §8.4; the death fields are always the real values).
pub fn emit_experience(m: &Mover, out: &mut Out) {
    let Some(p) = m.player() else { return };
    out.to_self(pb::SetExperience {
        exp: p.progression.exp,
        level: u32::try_from(m.mv.level).unwrap_or(0),
        skill_level: p.progression.skill_level_total,
        skill_point: p.progression.skill_points,
        death_exp: p.progression.death_exp,
        death_level: u32::try_from(p.progression.death_level).unwrap_or(0),
    });
}

/// The messages of a level-up (`LevelUpSetting` + PL-083, 16 §8.4), in order: `SetLevel` to the near set without
/// self, `SetGrowthLearningPoint` to self, and, when HP/MP/FP were refilled, the three forced `SetPointParam`
/// to the near set including self. A tier-cap reset (`changed` without a step) sends the first two only, as the
/// source did (the client ignores a `SetLevel` that is not higher than its level).
pub fn level_up_messages(m: &mut Mover, c: &StatsCore, r: &ExpResult, out: &mut Out) {
    out.near_excl(pb::SetLevel {
        level: u32::try_from(m.mv.level).unwrap_or(0),
    });
    super::points::emit_growth_learning_point(m, out);
    if !r.steps.is_empty() {
        send_pools_absolute(m, c, out);
    }
}

/// How a grant was triggered (the three flags of `AddExperience`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExpGrant {
    /// The raw EXP.
    pub n: i64,
    /// `bFirstCall` (true for every outside caller).
    pub first_call: bool,
    /// `bMultiPly`: apply the EXP factor.
    pub multiply: bool,
    /// `bMonster`: the PK hook applies.
    pub from_monster: bool,
}

impl ExpGrant {
    /// A kill reward: factor and PK hook apply (`AddExperience(n, TRUE, TRUE, TRUE)`).
    pub fn kill(n: i64) -> ExpGrant {
        ExpGrant {
            n,
            first_call: true,
            multiply: true,
            from_monster: true,
        }
    }

    /// A script, quest or GM reward (`AddExperience(n, TRUE, bMultiPly)`): the factor applies only when the caller
    /// says so, no PK hook.
    pub fn reward(n: i64, multiply: bool) -> ExpGrant {
        ExpGrant {
            n,
            first_call: true,
            multiply,
            from_monster: false,
        }
    }
}

/// `AddExperience` and what the callers do after it (16 §8.4): the level-up hooks, the messages, the directory,
/// audit and save side effects, `SetExperience`. `pre` holds the messages that precede the grant (the flight EXP of
/// a kill). Returns `None` when `who` is not a live player.
pub fn grant_experience(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    g: ExpGrant,
    pre: Out,
) -> Option<ExpResult> {
    let core = svc.stats.handle();
    let rate = ctx.live.rates.monster_exp.unwrap_or(1.0);
    let mut hooks = PkExpHooks::new(&core);
    // ---- guild house (doc 22 §12.7, task guild-house): 1.0 when the player's guild owns a house
    hooks.rest_factor = crate::event::guild_house::rest::house_factor(svc, ctx, who);
    let r = {
        let m = ctx.worlds.player_mut(who.0)?;
        let mut env = ExpEnv {
            core: &core,
            events: svc.event.factors(),
            monster_exp_rate: rate,
            hooks: &mut hooks,
        };
        add_experience(&mut env, m, g.n, g.first_call, g.multiply, g.from_monster)
    };
    // what the original sent from inside AddExperience comes first: the flight EXP of the kill, the propensity
    // change of the PK hook, then per real level-up the master skill growth (SetMasterSkillPointUp) before the
    // hooks of the other documents
    let mut out = pre;
    let pk_changed = hooks.changed;
    for msg in hooks.out.take() {
        out.push(msg.aud, msg.body);
    }
    if let Some(propensity) = pk_changed {
        crate::audit::emit_for(
            ctx,
            who,
            flyff_db::audit::ev::CHAR_PK,
            serde_json::json!({ "kind": "propensity_exp", "propensity": propensity }),
        );
    }
    for step in r.steps.iter().filter(|s| !s.regained) {
        if r.master_skill_levels.contains(&step.level)
            && let Some(m) = ctx.worlds.player_mut(who.0)
        {
            super::skills::master_skill_up(m, &core, step.level, &mut out);
        }
        level_up_gift(svc, ctx, who, step.level);
        campus_reward(svc, ctx, who, step.level);
    }
    {
        let m = ctx.worlds.player_mut(who.0)?;
        if r.changed {
            level_up_messages(m, &core, &r, &mut out);
        }
        emit_experience(m, &mut out);
    }
    out.flush(ctx, who.0);
    if !r.steps.is_empty() {
        after_level_up(svc, ctx, who, &r);
    }
    Some(r)
}

/// The side effects of a real level-up that follow its messages: the player directory (`UpdatePlayerData`,
/// doc 21), the audit line `char.level_up` (13 §25) and an urgent save (13 §17.1).
pub fn after_level_up(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, r: &ExpResult) {
    let Some(m) = ctx.player(who) else { return };
    let new = m.mv.level;
    let (gp, sp) = m.player().map_or((0, 0), |p| {
        (p.progression.stat_points, p.progression.skill_points)
    });
    crate::social::hooks::on_level_changed(
        svc,
        ctx,
        who,
        u32::try_from(r.level_before).unwrap_or(0),
        u32::try_from(new).unwrap_or(0),
    );
    crate::audit::emit_for(
        ctx,
        who,
        flyff_db::audit::ev::CHAR_LEVEL_UP,
        serde_json::json!({ "from": r.level_before, "level": new, "gp": gp, "sp": sp }),
    );
    svc.saves.mark_urgent(who.0);
}

/// Doc 22's `prj.m_EventLua.SetLevelUpGift(user, level)` (Event.lua `GetLevelUpGift`): called once per real
/// level-up that is not a regained level, after the skill points and before the messages. The gifts are
/// delivered by [`crate::event::on_level_up`] (22 §4.6).
pub fn level_up_gift(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, level: i32) {
    if let Ok(level) = u32::try_from(level) {
        crate::event::on_level_up(svc, ctx, who, level);
    }
}

/// Doc 21's `CCampusHelper::SetLevelUpReward(user)`: called after [`level_up_gift`], once per real level-up step with
/// the level that step reached (the player's own level may already be higher). A pupil in a campus pays his master
/// and himself the reward of that level; at level 75 he graduates (WP-SOCIAL-22).
pub fn campus_reward(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, level: i32) {
    crate::social::campus::points::level_reward(svc, ctx, who, level);
}

/// `CMover::SetLevel(n)` (`Mover.cpp:1875`, GM and scripts, 16 §8.6): level `n`, EXP 0, the death record at
/// `(0, n - 1)` so that the levels below `n` are never paid again; `SetExperience` to self and `SetLevel` to the
/// near set without self. HP/MP/FP and the points are not touched.
pub fn set_level(m: &mut Mover, c: &StatsCore, n: i32, out: &mut Out) {
    m.mv.level = n;
    if let Some(p) = m.player_mut() {
        p.progression.exp = 0;
        p.progression.death_exp = 0;
        p.progression.death_level = n.wrapping_sub(1);
        p.progression.max_level = p
            .progression
            .max_level
            .max(u16::try_from(n.max(0)).unwrap_or(1));
    }
    refresh_sim_params(m, c);
    emit_experience(m, out);
    out.near_excl(pb::SetLevel {
        level: u32::try_from(n).unwrap_or(0),
    });
}

/// `CMover::AddGPPoint(n)` (`Mover.cpp:1891`): `stat_points += n`, `SetGrowthLearningPoint` to self.
pub fn add_gp(m: &mut Mover, n: i32, out: &mut Out) {
    if let Some(p) = m.player_mut() {
        p.progression.stat_points = p.progression.stat_points.wrapping_add(n);
    }
    super::points::emit_growth_learning_point(m, out);
}

/// `CMover::AddFxp(n)` (`MoverParam.cpp:1561`, 16 §9.4, KEEP): the flight EXP counter. The flight level is derived
/// (1 from character level 20, else the function does nothing) and never changes; when the counter reaches the
/// threshold of row 2 of `expFlight` it wraps to 0 and the overflow is added again. Returns `true` when it wrapped.
pub fn add_fxp(m: &mut Mover, c: &StatsCore, n: i32) -> bool {
    if n <= 0 || m.mv.hp <= 0 || flight_level(m.mv.level) == 0 {
        return false;
    }
    let Some(threshold) = c.rules.exp_flight.get(2).map(|r| r.0) else {
        return false;
    };
    let Some(p) = m.player_mut() else {
        return false;
    };
    let mut wrapped = false;
    let mut add = n;
    // The source recurses on the overflow; a threshold of 0 would never end, so the loop is bounded.
    for _ in 0..32 {
        p.progression.flight_exp = p.progression.flight_exp.wrapping_add(add);
        if (p.progression.flight_exp as u32) < threshold {
            break;
        }
        let buf = p.progression.flight_exp.wrapping_sub(threshold as i32);
        p.progression.flight_exp = 0;
        wrapped = true;
        if buf <= 0 {
            break;
        }
        add = buf;
    }
    wrapped
}

/// `SetFxp{fxp, flight_level}` to self (`AddSetFxp`).
pub fn emit_fxp(m: &Mover, out: &mut Out) {
    let Some(p) = m.player() else { return };
    out.to_self(pb::SetFxp {
        fxp: p.progression.flight_exp as u32,
        flight_level: flight_level(m.mv.level) as u32,
    });
}

/// `ExpStop{}` (`CDPSrvr::OnExpStop`, 16 §8.5, `features.exp_stop`): toggles `MODE_EXPUP_STOP`, sends the literal
/// `Text` ("EXP Stop Activated!" / "EXP Stop Deactivated!") to self and then `ModifyMode` to the near set.
/// Returns the new state of the flag, `None` when the feature is off or `m` is not a player (the message is
/// ignored).
pub fn toggle_exp_stop(m: &mut Mover, c: &StatsCore, out: &mut Out) -> Option<bool> {
    if !c.exp.exp_stop || !m.is_player() {
        return None;
    }
    let was_on = m.mv.mode & EXPUP_STOP_MODE != 0;
    out.to_self(pb::Text {
        kind: flyff_rules::chat::TEXT_GENERAL,
        text: (if was_on {
            TEXT_EXP_STOP_OFF
        } else {
            TEXT_EXP_STOP_ON
        })
        .to_owned(),
    });
    if was_on {
        clear_mode_bits(m, c, EXPUP_STOP_MODE, out);
    } else {
        set_mode_bits(m, c, EXPUP_STOP_MODE, out);
    }
    Some(!was_on)
}
