//! Honor titles (Plan/21 §13, WP-SOCIAL-23; `src:_Common/Mover.cpp:11657-11898`, `src:_Common/honor.cpp`,
//! `src:WORLDSERVER/DPSrvr.cpp:10757-10790`).
//!
//! A character has 150 counters (`m_aHonorTitle`, the persisted [`HonorDoc`](flyff_db::HonorDoc) in
//! `PlayerSocial.honor_doc`, index = title id) and one selected title (`PlayerSocial.honor`, `None` = -1). A title is
//! won when its counter reaches the `need` of its row in `honorList.txt` ([`HonorTable`]); the rows are found by
//! BOTH groups (`title_by(small, large)`).
//!
//! * [`add`] (`SetHonorAdd`): the counter of a row goes up by one (or is set), the message that tells the player the
//!   title is won (`HonorListAck`) goes out when it hits `need` exactly;
//! * [`recheck`] (`CheckHonorStat`): the counters that mirror a current value (the job, the base stats, the fame, the
//!   PK value, the Lord) are recomputed and a selected title that no longer qualifies is dropped;
//! * [`check_time`] (`CheckHonorTime`): the hourly counters of collecting and vending, fed by the 90 s pass of the
//!   per-player timer through [`HonorSecondHooks`];
//! * the hooks other documents call: [`on_monster_died`] (`DeathHooks::honor_hunt`), `hooks::on_item_used`,
//!   `hooks::on_egg_hatched`, the couple counters;
//! * the two client requests [`on_list_req`] and [`on_change_req`].
//!
//! KEEP: `HS_JUMP` is never incremented (title 0 is unobtainable); `recheck(.., true)` broadcasts the selected
//! title even when nothing changed; a decouple does not clear the selected couple title.

use crate::combat::KillCtx;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::mover::Mover;
use crate::outbox::Audience;
use crate::player::PlayerExt;
use crate::social::PlayerSocial;
use crate::social::party::wire::to_one;
use crate::stats::tick::{SecondCx, SecondHooks};
use flyff_common::GameMs;
use flyff_common::defs::honor::{
    HI_COUNT_CHECK, HI_EARN_TITLE, HI_ELASPED_TIME, HI_HUNT_MONSTER, HS_COLLECT, HS_COUPLE_LV,
    HS_DEX, HS_INT, HS_LORD, HS_PK_COUNT, HS_PVP_POINT01, HS_PVP_POINT10, HS_STA, HS_STR, HS_TRADE,
};
use flyff_data::social::honor::HonorTable;
use flyff_proto::pb;
use flyff_rules::social::{HONOR_HOUR_MS, HONOR_TITLE_COUNT, honor_next_count};
use serde_json::json;

// ---- the counters ------------------------------------------------------------------------------------------------

/// The table index of a title id: ids at or above 150 are never stored (`SetHonorCount` ignores them).
fn slot_of(id: u32) -> Option<u16> {
    u16::try_from(id)
        .ok()
        .filter(|i| usize::from(*i) < HONOR_TITLE_COUNT)
}

/// The 150 counters of `HonorListAck` (index = title id).
pub fn counters_of(social: &PlayerSocial) -> Vec<i32> {
    (0..HONOR_TITLE_COUNT as u16)
        .map(|i| social.honor_doc.get(i))
        .collect()
}

/// `SetHonorCount( id, n )`: stores `n` unless it is at or above `i32::MAX` or `id` is not a stored title.
/// Returns whether the value was stored.
fn set_count(social: &mut PlayerSocial, id: u32, n: i32) -> bool {
    match (slot_of(id), honor_next_count(0, Some(n))) {
        (Some(slot), Some(n)) => {
            social.honor_doc.set(slot, n);
            true
        }
        _ => false,
    }
}

/// `GetHonorTitle( id )`: the counter of a title id (0 for an id that is not stored).
fn count_of(social: &PlayerSocial, id: u32) -> i32 {
    slot_of(id).map_or(0, |s| social.honor_doc.get(s))
}

/// The player's social state when the avatar is a live player.
fn social_mut<'a>(ctx: &'a mut GameCtx<'_>, who: PlayerRef) -> Option<&'a mut PlayerSocial> {
    ctx.worlds
        .player_mut(who.0)
        .and_then(|m| m.player_mut())
        .map(|p| &mut p.social)
}

/// `AddHonorListAck`: the 150 counters to the player itself.
pub fn list_ack(ctx: &mut GameCtx<'_>, who: PlayerRef) {
    let counters = ctx
        .worlds
        .player(who.0)
        .and_then(|m| m.player())
        .map(|p| counters_of(&p.social));
    if let Some(counters) = counters {
        to_one(ctx, who.0, pb::HonorListAck { counters });
    }
}

/// `g_UserMng.AddHonorTitleChange( pUser, id )`: the selected title to everybody near, the player included.
fn broadcast_change(svc: &Services, ctx: &mut GameCtx<'_>, who: PlayerRef, id: i32) {
    ctx.broadcast(
        Some(svc),
        who.0,
        Audience::NearInclSelf,
        pb::HonorChangeAck { id },
    );
}

/// `SetHonorAdd( small, large, value )`: finds the title by both groups (no row: nothing happens), stores
/// `value` or one more than the counter, and when the counter now equals the `need` of the row EXACTLY - which
/// happens once per climb - tells the player (`HonorListAck`), audits `honor.gain` and asks for an urgent save.
///
/// A counter never reaches `i32::MAX` (the store is ignored, as `SetHonorCount` did).
pub fn add(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    small: u32,
    large: u32,
    value: Option<i32>,
) {
    let data = ctx.data;
    let Some(title) = data.honor.title_by(small, large) else {
        return;
    };
    let (id, need) = (title.id, title.need);
    let Some(slot) = slot_of(id) else {
        return;
    };
    let Some(social) = social_mut(ctx, who) else {
        return;
    };
    if let Some(new) = honor_next_count(social.honor_doc.get(slot), value) {
        social.honor_doc.set(slot, new);
    }
    let now = social.honor_doc.get(slot);
    if now != need {
        return;
    }
    list_ack(ctx, who);
    crate::audit::emit_for(
        ctx,
        who,
        flyff_db::audit::ev::HONOR_GAIN,
        json!({ "title": id, "count": now, "source": "play" }),
    );
    svc.saves.mark_urgent(who.0);
}

/// The honor part of a decouple (`OnDecoupleResult`): `HS_COUPLE_LV` is set to 0 and, when the need of the title is not
/// below the old counter, the list follows. The SELECTED title stays (KQ-39 f, KEEP).
pub(crate) fn reset_couple_level(ctx: &mut GameCtx<'_>, who: PlayerRef) {
    let data = ctx.data;
    let Some(t) = data.honor.title_by(HS_COUPLE_LV, HI_COUNT_CHECK) else {
        return;
    };
    let (id, need) = (t.id, t.need);
    let Some(social) = social_mut(ctx, who) else {
        return;
    };
    let previous = count_of(social, id);
    set_count(social, id, 0);
    if need >= previous {
        list_ack(ctx, who);
    }
}

// ---- CheckHonorStat ----------------------------------------------------------------------------------------------

/// The values `CheckHonorStat` mirrors into the counters.
struct Facts {
    job: i32,
    base: [i32; 4],
    fame: i32,
    pk_value: i32,
    lord: bool,
}

/// The recomputation of `CheckHonorStat`, in the order of the original (see [`recheck`]).
fn recompute(social: &mut PlayerSocial, table: &HonorTable, f: &Facts) {
    // an id outside -1..150 or without a row is no selection (the original only range-checked it)
    if let Some(h) = social.honor
        && !u32::try_from(h)
            .ok()
            .is_some_and(|id| slot_of(id).is_some() && table.title(id).is_some())
    {
        social.honor = None;
    }
    // the master and hero titles: 1 while the current job is the one of the row, else 0 (the Lord is below)
    for t in table
        .titles
        .iter()
        .filter(|t| t.large_group == HI_EARN_TITLE && t.small_group != HS_LORD)
    {
        let held = i64::from(t.small_group) == i64::from(f.job);
        set_count(social, t.id, i32::from(held));
    }
    // the base stats
    for (small, v) in [
        (HS_STR, f.base[0]),
        (HS_STA, f.base[1]),
        (HS_DEX, f.base[2]),
        (HS_INT, f.base[3]),
    ] {
        if let Some(t) = table.title_by(small, HI_COUNT_CHECK) {
            set_count(social, t.id, v);
        }
    }
    // the ten fame tiers all read the fame
    for small in HS_PVP_POINT01..=HS_PVP_POINT10 {
        if let Some(t) = table.title_by(small, HI_COUNT_CHECK) {
            set_count(social, t.id, f.fame);
        }
    }
    if let Some(t) = table.title_by(HS_PK_COUNT, HI_COUNT_CHECK) {
        set_count(social, t.id, f.pk_value);
    }
    if let Some(t) = table.title_by(HS_LORD, HI_EARN_TITLE) {
        set_count(social, t.id, i32::from(f.lord));
    }
    // last: the selected title must still qualify
    if let Some(h) = social.honor {
        let id = u32::try_from(h).unwrap_or(u32::MAX);
        if table
            .need_of(id)
            .is_none_or(|need| count_of(social, id) < need)
        {
            social.honor = None;
        }
    }
}

/// The facts of a live player.
fn facts_of(m: &Mover, lord: bool) -> Option<Facts> {
    let p = m.player()?;
    Some(Facts {
        job: m.mv.job,
        base: [m.mv.base.str, m.mv.base.sta, m.mv.base.dex, m.mv.base.int],
        fame: p.progression.fame,
        pk_value: p.pk.value,
        lord,
    })
}

/// `CheckHonorStat` (`Mover.cpp:11700-11873`), in this order: a selected title outside the table is dropped; the
/// master/hero job rows (read from the table, no ids are hard-coded) hold 1 for the current job and 0 otherwise;
/// `HS_STR/STA/DEX/INT` take the BASE stats; the ten `HS_PVP_POINT` rows the fame; `HS_PK_COUNT` the PK value;
/// `HS_LORD` the lord state; finally a selected title below its need is dropped.
///
/// With `notify` the counters go to the player (`HonorListAck`) and the selected title (`HonorChangeAck`, -1 =
/// none) to everybody near, ALWAYS - also when nothing changed (KQ-39 f, KEEP).
pub fn recheck(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, notify: bool) {
    // the identity comes from the mover: at the join the session entry does not exist yet
    let Some(character) = crate::social::relation::character_of(ctx, who) else {
        return;
    };
    let lord = crate::social::hooks::is_lord(svc, character);
    let data = ctx.data;
    let Some(m) = ctx.worlds.player_mut(who.0) else {
        return;
    };
    let Some(facts) = facts_of(m, lord) else {
        return;
    };
    let Some(p) = m.player_mut() else {
        return;
    };
    recompute(&mut p.social, &data.honor, &facts);
    let selected = p.social.honor.unwrap_or(-1);
    if notify {
        list_ack(ctx, who);
        broadcast_change(svc, ctx, who, selected);
    }
}

// ---- the requests --------------------------------------------------------------------------------------------------

/// `HonorListReq` -> `HonorListAck`.
pub fn on_list_req(
    _svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    _m: pb::HonorListReq,
) {
    list_ack(ctx, who);
}

/// `HonorChangeReq{id}` (`OnHonorChangeReq`): `-1` clears; `0..150` with a row whose `need` the counter has
/// reached selects; everything else (150, -2, a title without a row, a counter that is short) is ignored
/// silently. A change goes to everybody near, the player included, and asks for an urgent save.
pub fn on_change_req(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::HonorChangeReq,
) {
    let id = m.id;
    let data = ctx.data;
    let Some(social) = social_mut(ctx, who) else {
        return;
    };
    if id == -1 {
        social.honor = None;
    } else {
        let Some(title) = u32::try_from(id).ok().filter(|i| slot_of(*i).is_some()) else {
            return;
        };
        let Some(need) = data.honor.need_of(title).filter(|n| *n >= 0) else {
            return;
        };
        if count_of(social, title) < need {
            return;
        }
        social.honor = Some(id);
    }
    broadcast_change(svc, ctx, who, id);
    svc.saves.mark_urgent(who.0);
}

// ---- the hooks -------------------------------------------------------------------------------------------------------

/// `DeathHooks::honor_hunt` (`DoDie` step 3, `Mover.cpp:5295`): a player killed a non-player; the counter of the
/// monster's `MI_` id goes up. The hook runs only for a player attacker and a non-player victim.
pub fn on_monster_died(svc: &mut Services, ctx: &mut GameCtx<'_>, k: &KillCtx) {
    let Some(index) = ctx.worlds.core(k.victim).map(|c| c.index) else {
        return;
    };
    add(
        svc,
        ctx,
        PlayerRef(k.attacker),
        index,
        HI_HUNT_MONSTER,
        None,
    );
}

/// `CheckHonorTime` (`Mover.cpp:11876-11898`), called on every 90th pass of the 1 s timer: while the player
/// collects (`OBJSTA_COLLECT`) and more than an hour (3 600 000 ms of game time) passed since the mark, the mark
/// moves to now and `HS_COLLECT` goes up; then while a personal shop is open (`VendorIsVendor || IsVendorOpen`) the
/// same test on the SAME mark counts `HS_TRADE`. Titles exist for neither in the shipped data (no row, no effect).
pub fn check_time(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    let now = ctx.clock.game_ms;
    let Some(m) = ctx.worlds.player(who.0) else {
        return;
    };
    let collecting = crate::item::collect::is_collecting(m);
    let vending = m
        .player()
        .is_some_and(|p| p.econ.vendor.is_vendor() || p.econ.vendor.is_open());
    if collecting && hour_passed(ctx, who, now) {
        add(svc, ctx, who, HS_COLLECT, HI_ELASPED_TIME, None);
    }
    if vending && hour_passed(ctx, who, now) {
        add(svc, ctx, who, HS_TRADE, HI_ELASPED_TIME, None);
    }
}

/// `( m_dwHonorCheckTime + 3600000 ) < now`: when it is, the mark moves to `now`.
fn hour_passed(ctx: &mut GameCtx<'_>, who: PlayerRef, now: GameMs) -> bool {
    let Some(social) = social_mut(ctx, who) else {
        return false;
    };
    if social.honor_time_mark.0.saturating_add(HONOR_HOUR_MS) < now.0 {
        social.honor_time_mark = now;
        true
    } else {
        false
    }
}

/// `m_dwHonorCheckTime = GetTickCount()` when collecting starts and when a shop with goods opens: the hour is
/// counted from there.
pub fn restart_clock(p: &mut PlayerExt, now: GameMs) {
    p.social.honor_time_mark = now;
}

// ---- the 90 s pass -----------------------------------------------------------------------------------------------------

/// Wraps the [`SecondHooks`] of the other documents and adds the honor step: on every 90th pass of the 1 s timer the
/// player's `honor_check_due` flag is raised (the hook only has the mover, not the services) and the inner hooks run;
/// `social::relation::process` drains the flag in the same tick and calls [`check_time`].
pub struct HonorSecondHooks {
    inner: Box<dyn SecondHooks>,
}

impl HonorSecondHooks {
    /// The wrapper around `inner` (the event hooks of the server).
    pub fn new(inner: Box<dyn SecondHooks>) -> Self {
        Self { inner }
    }
}

impl SecondHooks for HonorSecondHooks {
    fn on_player_second(&mut self, player: &mut Mover, cx: &mut SecondCx<'_>) {
        self.inner.on_player_second(player, cx);
    }

    fn sm_tick(&mut self, player: &mut Mover, cx: &mut SecondCx<'_>) {
        self.inner.sm_tick(player, cx);
    }

    fn honor_check_time(&mut self, player: &mut Mover, cx: &mut SecondCx<'_>) {
        if let Some(p) = player.player_mut() {
            p.social.honor_check_due = true;
        }
        self.inner.honor_check_time(player, cx);
    }

    fn resend_lua_desc(&mut self, player: &mut Mover, cx: &mut SecondCx<'_>) {
        self.inner.resend_lua_desc(player, cx);
    }
}
