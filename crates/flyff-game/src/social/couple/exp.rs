//! Couple experience, buffs and level rewards (Plan/21 §11.4; `src:WORLDSERVER/User.cpp:3826-3880` `ProcessCouple`,
//! `src:WORLDSERVER/couplehelper.cpp:262-300`, `src:databaseserver/couplehelper.cpp:193-260`).
//!
//! * [`step`] runs at every cadence step of a player (every 16 ticks): the per-player counter, the experience tick of
//!   the HIGHER character id, and the level buffs;
//! * [`add_experience`] is the one place that changes the experience (also `/ucp` of the GM layer): the new value goes
//!   to the database, both online members are told and lose their couple buffs, a level change feeds `HS_COUPLE_LV` and
//!   mails the level's items (`couple.inc` `Item`, empty in the shipped data) to BOTH members, offline ones too.

use super::super::honor;
use super::super::relation::{avatar_of, character_of, commit, fill_d};
use super::Couple;
use crate::buff::api::{apply_item, has_ik3, remove_by_ik3};
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::item::container::SerialSource;
use crate::item::model::{Item, ItemFlags};
use crate::item::ops::CtxSerials;
use crate::social::party::wire::to_one;
use flyff_common::defs::honor::{HI_COUNT_CHECK, HS_COUPLE_LV};
use flyff_common::defs::itemkind::IK3_COUPLE_BUFF;
use flyff_common::defs::text::{TID_GAME_COUPLE_ITEM_MAIL_TEXT, TID_GAME_COUPLE_ITEM_MAIL_TITLE};
use flyff_common::{CharacterId, ItemSerial};
use flyff_data::social::couple::CoupleItem;
use flyff_db::DbOp;
use flyff_proto::pb;
use flyff_rules::social::{COUPLE_MAX_LEVEL, COUPLE_TICK_STEPS, couple_buff_items};

/// `SEX_SEXLESS`: a level item for both members.
const SEX_SEXLESS: i32 = 2;
/// The English `TID_GAME_COUPLE_ITEM_MAIL_TITLE` / `_TEXT` for a game without the text table.
const MAIL_TITLE_FALLBACK: &str = "Couple Item";
const MAIL_TEXT_FALLBACK: &str = "Here is your reward for reaching Couple Level %d!";

/// `CUser::ProcessCouple` for one cadence step of `who`.
///
/// The counter `m_cbProcessCouple` runs for everybody (coupled or not): when it passes 60 it resets and this step is a
/// TICK - every 61st step, 65.07 s. With the partner online: on a tick, when `my id > partner id` and the couple level
/// is below 21, the couple gets one experience point; every step, when no couple buff runs, the buffs of the couple
/// level are applied. A partner who is offline, or no couple at all, removes the buffs.
pub fn step(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    let Some(me) = character_of(ctx, who) else {
        return;
    };
    let tick = match ctx.worlds.player_mut(who.0).and_then(|m| m.player_mut()) {
        Some(p) => {
            p.social.couple_counter += 1;
            if p.social.couple_counter >= COUPLE_TICK_STEPS {
                p.social.couple_counter = 0;
                true
            } else {
                false
            }
        }
        None => return,
    };
    let Some(partner) = svc.social.couple.partner_of(me) else {
        drop_buffs(svc, ctx, who);
        return;
    };
    if avatar_of(svc, ctx, partner).is_none() {
        drop_buffs(svc, ctx, who);
        return;
    }
    let level_exp = &ctx.data.couple.level_exp;
    let level = svc.social.couple.level_of(level_exp, me).unwrap_or(1);
    if tick && me > partner && level < COUPLE_MAX_LEVEL {
        add_experience(svc, ctx, me, 1);
    }
    if !has_ik3(ctx, who.0, IK3_COUPLE_BUFF) {
        let level = svc
            .social
            .couple
            .level_of(&ctx.data.couple.level_exp, me)
            .unwrap_or(level);
        apply_buffs(svc, ctx, who, level);
    }
}

fn drop_buffs(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    if has_ik3(ctx, who.0, IK3_COUPLE_BUFF) {
        remove_by_ik3(svc, ctx, who.0, IK3_COUPLE_BUFF);
    }
}

/// `CUser::ActiveCoupleBuff( level )`: every buff item of the level (`couple.inc` `SkillKind` / `SkillLevel`).
fn apply_buffs(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, level: i32) {
    let data = ctx.data;
    let items = couple_buff_items(&data.couple.skill_kinds, &data.couple.skill_levels, level);
    for item in items {
        if let Err(e) = apply_item(svc, ctx, who.0, item, None) {
            tracing::debug!(item, error = ?e, "couple buff refused");
        }
    }
}

/// `OnAddCoupleExperience`: the couple of `member` gains `n` experience. A couple at level 21 gains nothing (the
/// mirror refused it, `CCouple::AddExperience`); anything below is added WITHOUT a clamp, as the original (a GM
/// can push it past the last threshold). The result goes to the database (`UpsertCouple`, with the mails of a
/// level-up in the same batch); both members who are online lose their couple buffs (the next cadence step applies
/// the buffs of the new level), hear `AddCoupleExperience{n}`, and when the level rose get
/// `HS_COUPLE_LV = level` (higher id first, as the original).
pub fn add_experience(svc: &mut Services, ctx: &mut GameCtx<'_>, member: CharacterId, n: i32) {
    let data = ctx.data;
    let level_exp = &data.couple.level_exp;
    let Some(before) = svc.social.couple.get(member).map(|c| c.level(level_exp)) else {
        return;
    };
    if before >= COUPLE_MAX_LEVEL {
        return;
    }
    let Some(id) = svc.social.couple.get(member).map(|c| c.id) else {
        return;
    };
    let Some(after) = svc.social.couple.add_exp(id, n).cloned() else {
        return;
    };
    let level = after.level(level_exp);
    let mut ops = vec![DbOp::UpsertCouple(after.record())];
    if level > before {
        ops.extend(reward_mails(svc, ctx, &after, level));
    }
    commit(ctx, "CoupleExperience", ops);
    for m in [after.first, after.second] {
        let Some(o) = avatar_of(svc, ctx, m) else {
            continue;
        };
        if has_ik3(ctx, o.0, IK3_COUPLE_BUFF) {
            remove_by_ik3(svc, ctx, o.0, IK3_COUPLE_BUFF);
        }
        to_one(ctx, o.0, pb::AddCoupleExperience { exp: n });
        if level > before {
            honor::add(svc, ctx, o, HS_COUPLE_LV, HI_COUNT_CHECK, Some(level));
        }
    }
}

/// `PostItem( pCouple )`: the items of the new level, filtered by the sex of each member (2 = both), mailed from the
/// system. The mails exist in memory at once; the returned ops go into the batch of the experience.
fn reward_mails(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    couple: &Couple,
    level: i32,
) -> Vec<DbOp> {
    let data = ctx.data;
    let Some(items) = usize::try_from(level - 1)
        .ok()
        .and_then(|i| data.couple.items.get(i))
    else {
        return Vec::new();
    };
    let sex = |svc: &Services, c: CharacterId| {
        svc.presence
            .get(c)
            .map_or(SEX_SEXLESS, |e| i32::from(e.sex))
    };
    let (first_sex, second_sex) = (sex(svc, couple.first), sex(svc, couple.second));
    let mut ops = Vec::new();
    for ci in items {
        if ci.sex == SEX_SEXLESS || ci.sex == first_sex {
            ops.extend(mail_item(svc, ctx, couple.first, ci, level));
        }
        if ci.sex == SEX_SEXLESS || ci.sex == second_sex {
            ops.extend(mail_item(svc, ctx, couple.second, ci, level));
        }
    }
    ops
}

/// One `PostItem( idPlayer, ci, level )`: a system mail with the item (flags, `life` minutes of keep time, count).
fn mail_item(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    receiver: CharacterId,
    ci: &CoupleItem,
    level: i32,
) -> Vec<DbOp> {
    let count = u16::try_from(ci.num.clamp(1, i32::from(u16::MAX))).unwrap_or(1);
    let mut item = Item::new(ci.item, count);
    item.flags = ItemFlags(u8::try_from(ci.flags).unwrap_or(0));
    if ci.life > 0 {
        item.keep_until = ctx
            .clock
            .unix_ms
            .to_unix_time()
            .0
            .saturating_add(i64::from(ci.life) * 60);
    }
    let serial = {
        let Services { serials, .. } = &mut *svc;
        let mut src = CtxSerials::new(serials, ctx.persist, ctx.tickets);
        src.next_serial()
    };
    match serial {
        Ok(s) => item.serial = ItemSerial(s),
        Err(e) => {
            tracing::error!(receiver = receiver.0, item = ci.item, error = %e, "couple level item: no serial, not mailed");
            return Vec::new();
        }
    }
    let texts = &ctx.data.texts;
    let pick = |tid: u32, fallback: &'static str| {
        let t = texts.get(tid);
        if t.is_empty() || t == "error" {
            fallback.to_owned()
        } else {
            t.to_owned()
        }
    };
    let title = pick(TID_GAME_COUPLE_ITEM_MAIL_TITLE, MAIL_TITLE_FALLBACK);
    let body = fill_d(
        &pick(TID_GAME_COUPLE_ITEM_MAIL_TEXT, MAIL_TEXT_FALLBACK),
        &[i64::from(level)],
    );
    let (_, ops) = svc
        .econ
        .mail
        .send_system_with_id(receiver, &title, &body, Some(item), 0);
    ops.into()
}
