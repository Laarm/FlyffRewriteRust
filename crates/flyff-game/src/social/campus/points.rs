//! Campus points (Plan/21 §12.4; `src:WORLDSERVER/CampusHelper.cpp:344-432`, `databaseserver` `UpdateCampusPoint`):
//! the one writer of the balance, the level reward of a pupil, the recovery of a negative balance and the seam of
//! the exchange scripts.
//!
//! A balance changes through [`add_points`] / `apply` only: memory, the `SetCampusPoints` op (it writes
//! `campus_points` and NEVER the character snapshot, so an OFFLINE master is rewarded too - FIX of the race of
//! 13 §9), the `CampusUpdatePoint` message and the mirror `PlayerSocial.campus_point` of an online owner, and the
//! audit line `campus.points`.

use super::members;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::econ::exchange::CampusHooks;
use crate::player::PlayerExt;
use crate::social::Social;
use crate::social::party::wire::to_one;
use crate::social::relation::{avatar_of, character_of, commit};
use flyff_common::CharacterId;
use flyff_db::audit::social as audit;
use flyff_db::{CampusRole, DbOp};
use flyff_proto::pb;
use flyff_rules::social::CAMPUS_GRADUATION_LEVEL;

/// Why a balance changed (`SendUpdateCampusPoint( .., cause )`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointReason {
    /// `'R'`: one point back per hour online while the balance is negative.
    Recovery,
    /// `'L'`: the level reward of a pupil.
    Level,
    /// `'D'`: the penalty of a removal.
    Delete,
    /// `'G'`: a game master.
    Gm,
    /// `'E'`: paid at an exchange menu.
    Exchange,
    /// `'Q'`: a quest reward or removal (`SetEndRewardTSP`, `SetEndRemoveTSP`).
    Quest,
}

impl PointReason {
    /// The letter of the legacy log.
    pub const fn letter(self) -> char {
        match self {
            PointReason::Recovery => 'R',
            PointReason::Level => 'L',
            PointReason::Delete => 'D',
            PointReason::Gm => 'G',
            PointReason::Exchange => 'E',
            PointReason::Quest => 'Q',
        }
    }

    /// The word of the audit line.
    pub const fn word(self) -> &'static str {
        match self {
            PointReason::Recovery => "recovery",
            PointReason::Level => "level",
            PointReason::Delete => "delete",
            PointReason::Gm => "gm",
            PointReason::Exchange => "exchange",
            PointReason::Quest => "quest",
        }
    }
}

/// The memory side of a balance change and its messages; returns the op that stores the new balance. `online` is
/// the avatar of `who` when it is in the world. The balance saturates; `campus_tick` is cleared only when the new
/// balance is above zero (`OnUpdateCampusPoint`, KEEP).
pub(crate) fn apply(
    campus: &mut super::CampusService,
    ctx: &mut GameCtx<'_>,
    who: CharacterId,
    online: Option<PlayerRef>,
    delta: i32,
    reason: PointReason,
) -> DbOp {
    let new = campus.points(who).saturating_add(delta);
    campus.set_points(who, new);
    if let Some(o) = online {
        if let Some(p) = ctx.worlds.player_mut(o.0).and_then(|m| m.player_mut()) {
            p.social.campus_point = new;
            if new > 0 {
                p.social.campus_tick = None;
            }
        }
        to_one(ctx, o.0, pb::CampusUpdatePoint { point: new });
    }
    let ev = audit::campus_points(campus.campus_id_of(who), who, new, delta, reason.word());
    let ev = match online.and_then(|o| crate::audit::actor_of(ctx, o)) {
        Some(actor) => ev.by(actor),
        None => ev,
    };
    ctx.audit.emit(ev);
    DbOp::SetCampusPoints {
        character_id: who,
        points: new,
    }
}

/// `SendUpdateCampusPoint( idPlayer, n, bAdd, cause )`: changes the balance of `who` by `delta` (saturating),
/// online or not, and stores it.
pub fn add_points(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: CharacterId,
    delta: i32,
    reason: PointReason,
) {
    let online = avatar_of(svc, ctx, who);
    let op = apply(&mut svc.social.campus, ctx, who, online, delta, reason);
    commit(ctx, "CampusPoints", vec![op]);
}

/// `CCampusHelper::SetLevelUpReward( pUser )`, once per REAL level-up step of `who` (not for regained levels, GM
/// `/level` or job changes - the caller `stats::progression::campus_reward` decides): a PUPIL in a campus earns the
/// reward row of the exact new `level` (master first, then pupil; the master may be offline); at level 75 the
/// pupil also graduates - leaves without a penalty, with the texts and dissolution rules of a removal.
pub fn level_reward(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, level: i32) {
    let data = ctx.data;
    let Some(cfg) = data.lua.campus.as_ref() else {
        return;
    };
    let Some(pupil) = character_of(ctx, who) else {
        return;
    };
    let Some(campus) = svc.social.campus.campus_of(pupil) else {
        return;
    };
    if campus.role_of(pupil) != Some(CampusRole::Pupil) {
        return;
    }
    let master = campus.master;
    let mut ops = Vec::new();
    if let Some(row) = cfg.rewards.iter().find(|r| r.level == level) {
        let master_ref = avatar_of(svc, ctx, master);
        ops.push(apply(
            &mut svc.social.campus,
            ctx,
            master,
            master_ref,
            row.master,
            PointReason::Level,
        ));
        ops.push(apply(
            &mut svc.social.campus,
            ctx,
            pupil,
            Some(who),
            row.pupil,
            PointReason::Level,
        ));
    }
    if level == CAMPUS_GRADUATION_LEVEL {
        ops.extend(members::graduate(svc, ctx, pupil));
    }
    commit(ctx, "CampusLevel", ops);
}

/// `CCampusHelper::RecoveryCampusPoint( pUser, dwTick )` at a cadence step: with a negative balance the first step
/// starts the timer; once `recovery_time_ms` (an hour) passed it restarts and one point (`recovery_point`) comes back.
pub fn recover(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    let data = ctx.data;
    let Some(cfg) = data.lua.campus.as_ref() else {
        return;
    };
    let now = ctx.clock.game_ms;
    let Some(p) = ctx.worlds.player_mut(who.0).and_then(|m| m.player_mut()) else {
        return;
    };
    if p.social.campus_point >= 0 {
        return;
    }
    let character = p.ident.character_id;
    let due = match p.social.campus_tick {
        None => {
            p.social.campus_tick = Some(now);
            false
        }
        Some(t) if now.0 > t.0.saturating_add(u64::from(cfg.recovery_time_ms)) => {
            p.social.campus_tick = Some(now);
            true
        }
        Some(_) => false,
    };
    if due {
        add_points(
            svc,
            ctx,
            character,
            cfg.recovery_point,
            PointReason::Recovery,
        );
    }
}

/// The exchange scripts' view of the campus points (`POINTTYPE_CAMPUS`): the balance is the mirror of the
/// player; a payment is an [`PointReason::Exchange`] change that stores the new balance.
#[derive(Clone, Copy, Debug, Default)]
pub struct CampusExchange;

impl CampusHooks for CampusExchange {
    fn campus_points(&self, p: &PlayerExt) -> i64 {
        i64::from(p.social.campus_point)
    }

    fn remove_campus_points(&mut self, _ctx: &mut GameCtx<'_>, _who: PlayerRef, _points: i64) {
        // the service is not reachable from here: `spend_campus_points` is the entry the exchange calls
    }

    fn spend_campus_points(
        &mut self,
        social: &mut Social,
        ctx: &mut GameCtx<'_>,
        who: PlayerRef,
        points: i64,
    ) {
        let Some(c) = character_of(ctx, who) else {
            return;
        };
        let delta = i32::try_from(points.clamp(0, i64::from(i32::MAX))).unwrap_or(i32::MAX);
        let op = apply(
            &mut social.campus,
            ctx,
            c,
            Some(who),
            delta.saturating_neg(),
            PointReason::Exchange,
        );
        commit(ctx, "CampusPoints", vec![op]);
    }
}
