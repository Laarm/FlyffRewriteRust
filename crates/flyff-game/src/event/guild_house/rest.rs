//! The rest points of the guild house (Plan/22 §12.7, F-135; WP-EVENT-15; `WORLDSERVER/User.cpp:533-549, 8796-8836`,
//! `User.h:749-753`, `DPDatabaseClient.cpp:1294-1299`).
//!
//! A player standing in the house of his own guild earns `REST_POINT_INC` (10 000) every 240 s, scaled by the
//! `DST_RESTPOINT_RATE` of the furniture (only a piece that grants it makes points accrue: the crystal grants 100); he
//! earns `REST_POINT_LOGOUT_INC` (2 500) per 240 s of his absence when he logs in inside; a first-call EXP grant
//! (multiplied, monster or reward) draws bonus EXP from the pool before the EXP factor applies. All arithmetic is the
//! C++'s `float` and `int` arithmetic, bit for bit: `add` returns the UNCLAMPED value and clamps the stored one to
//! `[0, MAX_REST_POINT]`.

use super::{
    MAX_REST_POINT, REST_POINT_INC, REST_POINT_LOGOUT_INC, REST_POINT_TICK_S, guild_of_layer,
    is_guild_house_world,
};
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::mover::Mover;
use crate::player::PlayerMisc;
use crate::social::party::now_secs;
use crate::stats::StatsCore;
use crate::stats::tick::SecondCx;
use flyff_common::GameMs;
use flyff_common::defs::attribute::DST_RESTPOINT_RATE;
use flyff_proto::pb;

/// `REST_POINT_TICK` in game milliseconds.
pub const REST_POINT_TICK_MS: u64 = 240_000;

/// `SetAddRestPoint(n)` with the house factor: `n == 0` changes nothing; else `rest = (int)(rest + n * factor)`
/// (a `float` sum), the UNCLAMPED value is returned and the stored one clamped to `[0, 1 000 000]`.
pub fn add(p: &mut PlayerMisc, n: i32, factor: f32) -> i32 {
    if n == 0 {
        return p.rest_points;
    }
    let sum = (p.rest_points as f32 + n as f32 * factor) as i32;
    p.rest_points = sum.clamp(0, MAX_REST_POINT);
    sum
}

/// `SetIncRestPoint(n)`: `SetAddRestPoint((int)(n * (rate / 100.0f)))` with `rate = GetParam(DST_RESTPOINT_RATE)`.
pub fn inc(p: &mut PlayerMisc, n: i32, rate: i32, factor: f32) -> i32 {
    add(p, (n as f32 * (rate as f32 / 100.0)) as i32, factor)
}

/// `GetParam(DST_RESTPOINT_RATE, 0)` of the avatar: the sum of its installed furniture and other sources.
pub fn rate_of(m: &Mover) -> i32 {
    m.mv.dst.table.param(DST_RESTPOINT_RATE, 0)
}

/// Legacy step 6 of the 1 s timer (`User.cpp:533-549`), called by `EventSecondHooks::on_player_second` whatever the
/// state of the Event family: inside the house of his own guild a due tick adds `REST_POINT_INC`; then, anywhere, a
/// changed value is sent as `GuildHouseRestPoint` (only on change; a re-attach resets `rest_sent`).
pub fn on_player_second(m: &mut Mover, cx: &mut SecondCx<'_>) {
    let rate = rate_of(m);
    let (world, layer) = (m.core.world, m.core.layer);
    let Some(p) = m.player_mut() else {
        return;
    };
    let own = is_guild_house_world(world)
        && p.social
            .guild
            .is_some_and(|g| guild_of_layer(layer) == Some(g));
    if own && cx.now >= p.misc.rest_tick {
        inc(&mut p.misc, REST_POINT_INC, rate, 1.0);
        p.misc.rest_tick = GameMs(cx.now.0.saturating_add(REST_POINT_TICK_MS));
    }
    if p.misc.rest_points != p.misc.rest_sent {
        cx.out.to_self(pb::GuildHouseRestPoint {
            rest_point: p.misc.rest_points,
        });
        p.misc.rest_sent = p.misc.rest_points;
    }
}

/// A re-attach (`WorldRules::reattached`, README ruling 22): the new session has not seen the gauge, so the value
/// counts as unsent and the next per-player second sends `GuildHouseRestPoint` when it is not 0.
pub fn resend(ctx: &mut GameCtx<'_>, p: PlayerRef) {
    if let Some(pl) = ctx.worlds.player_mut(p.0).and_then(|m| m.player_mut()) {
        pl.misc.rest_sent = 0;
    }
}

/// The offline bonus of a login inside the own house (`tLogOut / REST_POINT_TICK * REST_POINT_LOGOUT_INC`): the
/// seconds since the last logout (0 when the character never logged out, `DbManagerJoin.cpp:104`; a clock that went
/// back gives 0), integer division by 240 first, saturating instead of wrapping to `int`. The furniture DSTs are
/// already applied, so the crystal's rate counts.
pub fn login_bonus(ctx: &mut GameCtx<'_>, p: PlayerRef) {
    let now = now_secs(ctx).0;
    let Some(m) = ctx.worlds.player_mut(p.0) else {
        return;
    };
    let rate = rate_of(m);
    let Some(pl) = m.player_mut() else {
        return;
    };
    let logout = pl.misc.last_logout_at.0;
    let elapsed = if logout > 0 { (now - logout).max(0) } else { 0 };
    let n = (elapsed / REST_POINT_TICK_S).saturating_mul(i64::from(REST_POINT_LOGOUT_INC));
    inc(
        &mut pl.misc,
        i32::try_from(n).unwrap_or(i32::MAX),
        rate,
        1.0,
    );
}

/// `GuildHouseMng->GetRestExpFactor(level)`: the first row of `RestExpFactorTable` that holds the level (the C++ map
/// kept the first insert), 0.0 for a level without a row.
pub fn rest_exp_factor(core: &StatsCore, level: i32) -> f32 {
    core.data
        .exp
        .rest_exp_factor
        .iter()
        .find(|r| r.min_level <= level && level <= r.max_level)
        .map_or(0.0, |r| r.factor)
}

/// `GetAddExpAfterApplyRestPoint(exp)` (`User.cpp:8796-8815`), called by `level_up_math` for a first-call multiplied
/// grant, before the EXP factor (`exp` is already halved for masters and heroes). `factor` is
/// [`house_factor`]. Nothing without rest points; the bonus is `exp * rest_exp_factor(level)`; the pool pays
/// `(exp * min(level / 5, 1)) / next_exp(level) * 1 000 000 * factor` points (a level under 5 pays nothing and gets
/// nothing; FIX: an `exp1` of 0 or less gives no bonus instead of a division by zero) and a pool that cannot pay it all
/// scales the bonus by `(cost + remain) / cost`.
pub fn exp_bonus(m: &mut Mover, core: &StatsCore, factor: f32, exp: i64) -> i64 {
    let level = m.mv.level;
    let Some(p) = m.player_mut() else {
        return 0;
    };
    if p.misc.rest_points <= 0 {
        return 0;
    }
    let bonus = (exp as f32 * rest_exp_factor(core, level)) as i64;
    let exp1 = flyff_rules::progression::next_exp(&core.rules, level);
    if exp1 <= 0 {
        return 0;
    }
    let f = exp.wrapping_mul(i64::from((level / 5).clamp(0, 1))) as f64 / exp1 as f64;
    let cost = (f * f64::from(1_000_000.0_f32 * factor)) as i32;
    if cost <= 0 {
        return 0;
    }
    let remain = add(&mut p.misc, -cost, factor);
    if remain < 0 {
        bonus.wrapping_mul(i64::from(cost) + i64::from(remain)) / i64::from(cost)
    } else {
        bonus
    }
}

/// `GuildHouseMng->GetRestPointFactor(user)`: 1.0 when the player's guild owns a house (an expired one included),
/// else 0.0.
pub fn house_factor(svc: &Services, ctx: &GameCtx<'_>, who: PlayerRef) -> f32 {
    let owns = ctx
        .player(who)
        .and_then(|m| m.player())
        .and_then(|p| p.social.guild)
        .is_some_and(|g| svc.event.guild_houses.has_house(g));
    if owns { 1.0 } else { 0.0 }
}
