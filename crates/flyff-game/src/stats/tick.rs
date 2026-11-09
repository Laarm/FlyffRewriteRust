//! Regeneration, kill recovery and the per-player 1 s timer (Plan/16 §6, WP-MOVER-06; F-121).
//!
//! * [`tick_atk_cnt`] / [`is_attack_mode`]: the attack-mode window `0 < sim.atk_cnt < 150` (16 §2.7);
//! * [`process_recovery`]: `CMover::ProcessRecovery` (players only, alive): sitting every 2 s, standing every 3 s
//!   outside combat, both with a **strict** `>` on the tick-derived clock (`floor(n * 1000 / 15)`), so a period is
//!   31 / 46 ticks; `CHS_SETSTONE` blocks the standing regeneration (16 §6.2);
//! * [`add_kill_recovery`]: `AddKillRecovery`, called by doc 17 on every kill by a player;
//! * [`second_timer`]: the 1 s grid and the 90 s countdown of `CUser::Process` in the legacy order, with hook
//!   traits for the owners that plug in later (docs 21, 22 and WP-MOVER-16's SM countdown);
//! * [`StatsHooks`]: the `WorldHooks` leaf that runs all of it in the tick tail of every awake mover (16 §6.1).
//!   It is not installed by default: chain it into the handlers (`MovementHooks::with_inner`, or as the world hook
//!   itself) where the server builds them.

use super::env::{StatsCore, max_of, point_param, recoveries};
use super::out::Out;
use super::pool::{inc_fp, inc_hp, inc_mp, set_point_param};
use crate::mover::Mover;
use crate::world::{FrameCtx, MoverKey, WorldHooks};
use flyff_common::GameMs;
use flyff_common::defs::obj::XI_KILL_RECOVERY;
use flyff_proto::pb;
use flyff_rules::consts::{
    ATTACK_MODE_TICKS, NEXT_TICK_RECOVERY_MS, NEXT_TICK_RECOVERYSTAND_MS, SEC_SAVEPLAYER, dst::*,
};
use flyff_sim::types::OBJSTA_SIT;
use std::sync::Arc;

/// `m_nAtkCnt` counts up while positive (16 §6.1): `if sim.atk_cnt > 0 { sim.atk_cnt += 1 }`. Doc 17
/// sets it to 1 on every attack; it leaves the attack-mode window at [`ATTACK_MODE_TICKS`].
pub fn tick_atk_cnt(m: &mut Mover) {
    if m.sim.atk_cnt > 0 {
        m.sim.atk_cnt = m.sim.atk_cnt.saturating_add(1);
    }
}

/// `IsAttackMode()`: `m_nAtkCnt != 0 && m_nAtkCnt < SEC1 * 10` (150 ticks).
pub fn is_attack_mode(m: &Mover) -> bool {
    m.sim.atk_cnt != 0 && m.sim.atk_cnt < ATTACK_MODE_TICKS
}

/// Doc 21's contribution to the sitting regeneration (`PARTY_STRETCHING_MODE`): the factor for a player in an
/// advanced party with the mode active and the leader valid (1.8 for a leader of job Assist, Ringmaster or
/// Billposter, else 1.5), or `None` for the plain factor 1.0. The legacy side effect "`m_idparty = 0` when the
/// player is not a member" belongs to the implementation too.
pub trait PartyRecovery: Send {
    /// The sitting factor of `player`. The default reads the party link of the player (21 §4.8.5, WP-SOCIAL-06):
    /// the factor `party::worker` computed for it once per second.
    fn stretching_factor(&mut self, player: &Mover) -> Option<f32> {
        player
            .player()?
            .social
            .party_link
            .as_ref()?
            .stretching
            .map(crate::social::party::Stretching::factor)
    }
}

/// No party yet (until doc 21 plugs in): the factor is always 1.0.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoParty;

impl PartyRecovery for NoParty {}

/// `ProcessRecovery` (`Mover.cpp:8304-8545`; the duel countdown at its head belongs to doc 17). Players only,
/// alive. `now` is the tick's `GameMs`.
pub fn process_recovery(
    m: &mut Mover,
    c: &StatsCore,
    now: GameMs,
    party: &mut dyn PartyRecovery,
    out: &mut Out,
) {
    if !m.is_player() || m.sim.act.state.is_die() {
        return;
    }
    let sitting = m.sim.act.state.move_() == OBJSTA_SIT;
    let attack_mode = is_attack_mode(m);
    let setstone = m.mv.dst.table.adj(DST_CHRSTATE) as u32 & CHS_SETSTONE != 0;
    let (next_sit, next_stand) = match m.player() {
        Some(p) => (p.regen.next_sit_ms, p.regen.next_stand_ms),
        None => return,
    };
    if sitting {
        if now <= next_sit {
            return;
        }
        let factor = party.stretching_factor(m).unwrap_or(1.0);
        if let Some(p) = m.player_mut() {
            p.regen.next_sit_ms = GameMs(now.0 + u64::from(NEXT_TICK_RECOVERY_MS));
        }
        let (hr, mr, fr) = recoveries(m, c);
        inc_hp(m, c, (hr as f32 * factor) as i32, out);
        inc_mp(m, c, (mr as f32 * factor) as i32, out);
        inc_fp(m, c, (fr as f32 * factor) as i32, out);
        return;
    }
    // standing: the sitting timer restarts 2 s from now
    let mut regenerate = false;
    if let Some(p) = m.player_mut() {
        p.regen.next_sit_ms = GameMs(now.0 + u64::from(NEXT_TICK_RECOVERY_MS));
        let period = GameMs(now.0 + u64::from(NEXT_TICK_RECOVERYSTAND_MS));
        if !attack_mode {
            if now > next_stand {
                p.regen.next_stand_ms = period;
                regenerate = !setstone;
            }
        } else {
            // combat keeps resetting the 3 s timer
            p.regen.next_stand_ms = period;
        }
    }
    if regenerate {
        let (hr, mr, fr) = recoveries(m, c);
        inc_hp(m, c, hr, out);
        inc_mp(m, c, mr, out);
        inc_fp(m, c, fr, out);
    }
}

/// `AddKillRecovery` (`Mover.cpp:6076-6123`, players only): for HP, MP and FP with `param(DST_KILL_x, 0) != 0`
/// the pool becomes `min(getter + v, max)`; if any applied, `CreateSfxObj{XI_KILL_RECOVERY}` once to the near set.
pub fn add_kill_recovery(m: &mut Mover, c: &StatsCore, out: &mut Out) {
    if !m.is_player() {
        return;
    }
    let mut sfx = false;
    for (pool, kill) in [
        (DST_HP, DST_KILL_HP),
        (DST_MP, DST_KILL_MP),
        (DST_FP, DST_KILL_FP),
    ] {
        let v = m.mv.dst.table.param(kill, 0);
        if v == 0 {
            continue;
        }
        let cur = point_param(m, c, pool);
        let max = max_of(m, c, pool);
        let new = if cur.wrapping_add(v) >= max {
            max
        } else {
            cur.wrapping_add(v)
        };
        set_point_param(m, c, pool, new, false, out);
        sfx = true;
    }
    if sfx {
        out.near(pb::CreateSfxObj {
            sfx_id: XI_KILL_RECOVERY,
            x: 0.0,
            y: 0.0,
            z: 0.0,
            flag: false,
        });
    }
}

/// What the owners of the per-second work receive (16 §6.4).
pub struct SecondCx<'a> {
    /// The tick's `GameMs`.
    pub now: GameMs,
    /// Where a hook queues messages for the player (self and near audiences).
    pub out: &'a mut Out,
}

/// The per-second work of the other documents, called by [`second_timer`] in the legacy order; every method is
/// a no-op until its owner implements it.
pub trait SecondHooks: Send {
    /// Doc 22: `event::on_player_second` (legacy steps 2, 3 and 6: coupon, keep-connect, guild-house rest
    /// points). Runs on every pass.
    fn on_player_second(&mut self, _player: &mut Mover, _cx: &mut SecondCx<'_>) {}

    /// WP-MOVER-16: `sm::tick_second` (legacy step 4 `SubSMMode`). Runs on every pass, after
    /// [`SecondHooks::on_player_second`].
    fn sm_tick(&mut self, _player: &mut Mover, _cx: &mut SecondCx<'_>) {}

    /// Doc 21: `social::honor::check_time` (legacy step 5). Runs on every 90th pass, after the SM step.
    fn honor_check_time(&mut self, _player: &mut Mover, _cx: &mut SecondCx<'_>) {}

    /// Doc 22: `event::resend_lua_desc` (legacy step 5 `AddEventLuaDesc`). Runs on every 90th pass, after
    /// [`SecondHooks::honor_check_time`].
    fn resend_lua_desc(&mut self, _player: &mut Mover, _cx: &mut SecondCx<'_>) {}
}

/// No owner plugged in.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoSecondHooks;

impl SecondHooks for NoSecondHooks {}

/// The environment of the SM step of the 1 s timer (16 §14.2): the stat functions' environment and the wall-clock
/// sample of the tick.
#[derive(Clone, Copy)]
pub struct SmEnv<'a> {
    /// The rule tables and the game data (the expiry reverts DST sources).
    pub core: &'a StatsCore,
    /// Unix seconds of the tick.
    pub unix: i64,
}

/// The per-player 1 s timer (`CUser::Process`, `User.cpp:502-545`): while `now > next_sfs_ms` one pass runs per
/// call, `next_sfs_ms += 1000` (the legacy `m_dwTickSFS += 1000`: a late tick catches up one pass per tick, the
/// grid never drifts). A pass calls `on_player_second`, then `sm_tick`, then counts the 90 s block down; on the
/// 90th pass (and every 90th after it) it restarts the countdown and calls `honor_check_time`, then
/// `resend_lua_desc`. Saving is not here: it is the `SaveScheduler`'s (D5). Returns `true` when a pass ran.
/// This form has no SM environment: the SM timers do not run (the hooks tests); the server uses
/// [`second_timer_with`].
pub fn second_timer(
    m: &mut Mover,
    now: GameMs,
    hooks: &mut dyn SecondHooks,
    out: &mut Out,
) -> bool {
    second_timer_with(m, now, None, hooks, out)
}

/// [`second_timer`] with the SM step of WP-MOVER-16: right after `SecondHooks::sm_tick` (legacy step 4) the
/// timers of `sm` run `sm::tick_second`, so a pass that expires a timer emits its `SmMode{sm, 0}` after the
/// event hook of doc 22 and before the 90 s hooks of the same pass.
pub fn second_timer_with(
    m: &mut Mover,
    now: GameMs,
    sm: Option<SmEnv<'_>>,
    hooks: &mut dyn SecondHooks,
    out: &mut Out,
) -> bool {
    let Some(p) = m.player_mut() else {
        return false;
    };
    if now <= p.regen.next_sfs_ms {
        return false;
    }
    p.regen.next_sfs_ms = GameMs(p.regen.next_sfs_ms.0 + 1000);
    let mut cx = SecondCx { now, out };
    hooks.on_player_second(m, &mut cx);
    hooks.sm_tick(m, &mut cx);
    if let Some(env) = sm {
        super::sm::tick_second(m, env.core, env.unix, cx.out);
    }
    let block = match m.player_mut() {
        Some(p) => {
            p.regen.slow_countdown = p.regen.slow_countdown.saturating_sub(1);
            if p.regen.slow_countdown == 0 {
                p.regen.slow_countdown = SEC_SAVEPLAYER as u8;
                true
            } else {
                false
            }
        }
        None => false,
    };
    if block {
        hooks.honor_check_time(m, &mut cx);
        hooks.resend_lua_desc(m, &mut cx);
    }
    true
}

/// The order of one mover's tick tail owned here (16 §6.1): the attack counter, then for players the 1 s timer
/// and `process_recovery`, finally `m_nCount++`. Without the wall clock: the SM timers do not run (see
/// [`tick_tail_at`]).
pub fn tick_tail(
    m: &mut Mover,
    c: &StatsCore,
    now: GameMs,
    party: &mut dyn PartyRecovery,
    second: &mut dyn SecondHooks,
    out: &mut Out,
) {
    tick_tail_at(m, c, now, None, party, second, out);
}

/// [`tick_tail`] with the wall clock of the tick (unix seconds): the 1 s timer then also runs the SM timers.
pub fn tick_tail_at(
    m: &mut Mover,
    c: &StatsCore,
    now: GameMs,
    unix: Option<i64>,
    party: &mut dyn PartyRecovery,
    second: &mut dyn SecondHooks,
    out: &mut Out,
) {
    tick_atk_cnt(m);
    if m.is_player() {
        // the flying fuel report (`CUser::Process`, WP-MOVER-21): every 16 ticks while flying and accelerating
        super::fame_fuel::report_fuel(m, out);
        let env = unix.map(|unix| SmEnv { core: c, unix });
        second_timer_with(m, now, env, second, out);
        process_recovery(m, c, now, party, out);
        // the mute counts down once per `CF_SEC` pass, only while the avatar is in the world (KQ-22, 23 §2.6)
        crate::gm::modes::mute_tick(m);
    }
    m.mv.count = m.mv.count.wrapping_add(1);
}

/// The world hook that runs [`tick_tail`] for every awake mover (the `tail` of 15 §5.3). It implements only
/// `tail`, so it is a leaf: install it where the server builds the world hooks, for example as the `inner` of
/// `MovementHooks`, or call [`StatsHooks::on_tail`] from a composite hook's own `tail`.
pub struct StatsHooks {
    core: Arc<StatsCore>,
    party: Box<dyn PartyRecovery>,
    second: Box<dyn SecondHooks>,
}

impl std::fmt::Debug for StatsHooks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StatsHooks").finish_non_exhaustive()
    }
}

impl StatsHooks {
    /// Hooks over the shared environment (`Services.stats.handle()` or a separately built core).
    pub fn new(core: Arc<StatsCore>) -> StatsHooks {
        StatsHooks {
            core,
            party: Box::new(NoParty),
            second: Box::new(NoSecondHooks),
        }
    }

    /// Doc 21's stretching factor.
    pub fn with_party(mut self, party: Box<dyn PartyRecovery>) -> Self {
        self.party = party;
        self
    }

    /// The owners of the per-second work.
    pub fn with_second(mut self, second: Box<dyn SecondHooks>) -> Self {
        self.second = second;
        self
    }

    /// The tail of one awake mover: [`tick_tail`], then its messages through the world sink.
    pub fn on_tail(&mut self, ctx: &mut FrameCtx<'_>, mover: MoverKey) {
        let now = ctx.now;
        let unix = ctx.movement.unix_now;
        if let Some(m) = ctx.store.movers.get_mut(mover) {
            let mut out = Out::new();
            tick_tail_at(
                m,
                &self.core,
                now,
                Some(unix),
                &mut *self.party,
                &mut *self.second,
                &mut out,
            );
            out.flush_sink(&mut *ctx.sink, m);
        }
    }
}

impl WorldHooks for StatsHooks {
    fn tail(&mut self, ctx: &mut FrameCtx<'_>, mover: MoverKey) {
        self.on_tail(ctx, mover);
    }
}
