//! The campus buff (Plan/21 §12.4; `src:WORLDSERVER/User.cpp:8911-8939` `ProcessCampus`,
//! `src:_Common/Campus.cpp` `GetBuffLevel` / `IsChangeBuffLevel`).
//!
//! At every cadence step of a member: the buff level of a MASTER is the number of his pupils who are online, that of
//! a PUPIL is 1 while the master is online. A level above 0 without a running `IK3_TS_BUFF` buff applies the item of
//! that level from `Campus.lua` (`II_TS_BUFF_POWER_LOVE01..03`); the master's buff is removed first when his level
//! differs from the one stored in the campus (per campus, KEEP); level 0, or no campus, removes the buff.

use super::Campus;
use crate::buff::api::{apply_item, has_ik3, remove_by_ik3};
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::social::relation::{avatar_of, character_of};
use flyff_common::CharacterId;
use flyff_common::defs::itemkind::IK3_TS_BUFF;

/// `GetBuffLevel( idPlayer )` of a member.
fn buff_level(svc: &Services, ctx: &GameCtx<'_>, campus: &Campus, me: CharacterId) -> i32 {
    if campus.master == me {
        let online = campus
            .pupils
            .iter()
            .filter(|p| avatar_of(svc, ctx, **p).is_some())
            .count();
        i32::try_from(online).unwrap_or(i32::MAX)
    } else {
        i32::from(avatar_of(svc, ctx, campus.master).is_some())
    }
}

/// `CUser::ProcessCampus` for one cadence step of `who`.
pub fn process(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    let data = ctx.data;
    let Some(cfg) = data.lua.campus.as_ref() else {
        return;
    };
    let Some(me) = character_of(ctx, who) else {
        return;
    };
    let Some(campus) = svc.social.campus.campus_of(me) else {
        drop_buff(svc, ctx, who);
        return;
    };
    let (id, is_master) = (campus.id, campus.master == me);
    let level = buff_level(svc, ctx, campus, me);
    if level == 0 {
        drop_buff(svc, ctx, who);
        return;
    }
    if is_master && svc.social.campus.change_buff_level(id, level) {
        drop_buff(svc, ctx, who);
    }
    if !has_ik3(ctx, who.0, IK3_TS_BUFF)
        && let Some(item) = cfg
            .buffs
            .iter()
            .find(|b| b.level == level)
            .map(|b| b.item)
            .filter(|i| *i != flyff_rules::consts::NULL_ID)
        && let Err(e) = apply_item(svc, ctx, who.0, item, None)
    {
        tracing::debug!(item, error = ?e, "campus buff refused");
    }
}

fn drop_buff(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    if has_ik3(ctx, who.0, IK3_TS_BUFF) {
        remove_by_ik3(svc, ctx, who.0, IK3_TS_BUFF);
    }
}
