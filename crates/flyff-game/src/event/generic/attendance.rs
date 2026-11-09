//! The attendance roll ("call the roll", event 531; Plan/22 §6.4, ref/15 §5; src:_Common/spevent.cpp:507-531,
//! src:WORLDSERVER/User.cpp:7557-7712).
//!
//! While the flag of event 531 is on, P3.1 [`call_the_roll`] runs once a minute: the day number `bit` is the number of
//! whole 24 h spans since the begin hour of the window ([`attendance_bit`]); every player of level 16 or more whose
//! bit `bit` is not set collects minutes online, and at 60 minutes the bit is set, the rewards of
//! [`rewards`] are mailed (one mail per item) and the player receives `CallTheRoll{attendance_bits}`. The bit, the
//! counters and the mails go into ONE `AtomicBatch` (the snapshot carries the bit and the counters). The counters
//! are character columns (`PlayerMisc.attendance_*`), saved with the normal snapshot; nothing tells the client the
//! bits at join (the original did not).

use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::econ::mail::commit::{can_snapshot, snapshot_for_batch, submit};
use crate::event::calendar::days_from_civil;
use crate::item::container::SerialSource;
use crate::item::model::Item;
use crate::item::ops::CtxSerials;
use crate::item::upgrade::random_option::{Arg, format_text};
use crate::stats::Out;
use crate::tick::LocalStamp;
use crate::tickets::TicketResult;
use flyff_common::defs::item::{
    II_CHR_FOO_COO_REMANTIS, II_GEN_TOO_COL_SILVERBATTERY, II_SYS_SYS_EVE_FUN01,
    II_SYS_SYS_SCR_AWAKECANCEL, II_SYS_SYS_SCR_AWAKESAFE, II_SYS_SYS_SCR_BAGBAG15,
    II_SYS_SYS_SCR_BX_PET_LEYENA07, II_SYS_SYS_SCR_BXVISMAGIC01, II_SYS_SYS_SCR_BXVISMELEE01,
    II_SYS_SYS_SCR_PET_MAGIC, II_SYS_TICKET_KEBARAS001,
};
use flyff_common::defs::text::{
    TID_GAME_EVENT_MAIL_CALLTHEROLL, TID_GAME_EVENT_MAIL_CALLTHEROLL_TITLE,
};
use flyff_common::{MailId, ObjId};
use flyff_db::{AtomicBatch, AtomicKind, DbOp};
use flyff_proto::pb;

/// The id of the attendance event (`__EVENT_1101`).
pub const ATTENDANCE_EVENT: u16 = 531;
/// The minimum level of a player who takes part (`GetLevel() < 16` is skipped).
pub const ATTENDANCE_MIN_LEVEL: i32 = 16;
/// The minutes online a day needs (`__MAINSERVER`: 60).
pub const ATTENDANCE_MINUTES: u32 = 60;
/// The highest usable day bit (`__int64` flags, the sign bit stays unused; the original shifted past it).
pub const ATTENDANCE_MAX_BIT: i64 = 62;

/// What every attendance day pays: `II_SYS_SYS_EVE_FUN01` x 2.
pub const ATTENDANCE_DAILY: (u32, u16) = (II_SYS_SYS_EVE_FUN01, 2);

/// What the milestone days add to [`ATTENDANCE_DAILY`], by the number of days attended (`SetEventFlagBit`'s count):
/// 5, 10, 19, 24, 29, 34 and 35 (User.cpp:7655-7699). Every other count pays the daily item only.
pub const ATTENDANCE_REWARDS: &[(u32, &[(u32, u16)])] = &[
    (5, &[(II_CHR_FOO_COO_REMANTIS, 3)]),
    (
        10,
        &[
            (II_CHR_FOO_COO_REMANTIS, 3),
            (II_SYS_SYS_SCR_BX_PET_LEYENA07, 1),
        ],
    ),
    (
        19,
        &[(II_CHR_FOO_COO_REMANTIS, 3), (II_SYS_TICKET_KEBARAS001, 1)],
    ),
    (
        24,
        &[
            (II_CHR_FOO_COO_REMANTIS, 3),
            (II_SYS_SYS_SCR_BX_PET_LEYENA07, 1),
            (II_GEN_TOO_COL_SILVERBATTERY, 1),
        ],
    ),
    (
        29,
        &[
            (II_CHR_FOO_COO_REMANTIS, 3),
            (II_SYS_SYS_SCR_AWAKECANCEL, 1),
            (II_SYS_SYS_SCR_BAGBAG15, 1),
        ],
    ),
    (
        34,
        &[
            (II_CHR_FOO_COO_REMANTIS, 3),
            (II_SYS_SYS_SCR_AWAKESAFE, 1),
            (II_SYS_SYS_SCR_BXVISMAGIC01, 2),
            (II_SYS_SYS_SCR_BXVISMELEE01, 2),
        ],
    ),
    (
        35,
        &[(II_CHR_FOO_COO_REMANTIS, 5), (II_SYS_SYS_SCR_PET_MAGIC, 1)],
    ),
];

/// The items mailed for the `count`-th attended day, in mail order: the daily item, then the milestone items.
pub fn rewards(count: u32) -> Vec<(u32, u16)> {
    let mut v = vec![ATTENDANCE_DAILY];
    if let Some((_, extra)) = ATTENDANCE_REWARDS.iter().find(|(c, _)| *c == count) {
        v.extend_from_slice(extra);
    }
    v
}

/// The number of attended days: the set bits among `0..=62` (`SetEventFlagBit` counts `i < 63`).
pub fn set_bit_count(bits: i64) -> u32 {
    (bits & i64::MAX).count_ones()
}

/// `GetEventElapsed`: the day number of `local` for a window that began at `begin` (`[year, month, day, hour]`): the
/// local minutes since the begin, divided into whole 24 h spans (`CTimeSpan::GetDays`); 0 before the begin. For a
/// window that begins at hour 0 this is the calendar-day difference of Plan §6.4.
pub fn attendance_bit(local: &LocalStamp, begin: [i32; 4]) -> i64 {
    let (Ok(y), Ok(m), Ok(d)) = (
        i16::try_from(begin[0]),
        u8::try_from(begin[1]),
        u8::try_from(begin[2]),
    ) else {
        return 0;
    };
    let start = days_from_civil(y, m, d) * 1440 + i64::from(begin[3]) * 60;
    let now = days_from_civil(local.year, local.month, local.day) * 1440
        + i64::from(local.hour) * 60
        + i64::from(local.minute);
    (now - start).max(0) / 1440
}

/// What one roll did for one player.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Roll {
    /// The day changed since the counters were written: they restart (`elapsed = bit`, `minutes = 1`).
    Restarted,
    /// Another minute was counted.
    Counted(u32),
    /// The 60th minute: the bit is set and the batch went out; the number of attended days.
    Rewarded(u32),
    /// Nothing changed (not a live player, the session cannot be snapshotted, the bit is out of range).
    Skipped,
}

/// `CUserMng::CallTheRoll`'s per-player body for the day `bit`, without the gates of the loop (event on, level,
/// bit not yet set): `elapsed < bit` restarts the counters; otherwise `minutes += 1` and at 60 the bit is set, the
/// batch of the snapshot and one mail per reward is submitted and the player is sent `CallTheRoll`. A player whose
/// session cannot be snapshotted is skipped before anything changes.
pub(crate) fn roll_player(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    bit: u32,
) -> Roll {
    if i64::from(bit) > ATTENDANCE_MAX_BIT {
        return Roll::Skipped;
    }
    let minutes = {
        let Some(p) = ctx.worlds.player_mut(who.0).and_then(|m| m.player_mut()) else {
            return Roll::Skipped;
        };
        if p.misc.attendance_elapsed < bit {
            p.misc.attendance_elapsed = bit;
            p.misc.attendance_minutes = 1;
            return Roll::Restarted;
        }
        let minutes = p.misc.attendance_minutes.saturating_add(1);
        if minutes < ATTENDANCE_MINUTES {
            p.misc.attendance_minutes = minutes;
            return Roll::Counted(minutes);
        }
        minutes
    };
    // the snapshot of the batch must be possible before anything changes
    if !can_snapshot(ctx, who) {
        return Roll::Skipped;
    }
    let Some(p) = ctx.worlds.player_mut(who.0).and_then(|m| m.player_mut()) else {
        return Roll::Skipped;
    };
    p.misc.attendance_minutes = minutes;
    p.misc.attendance_bits |= 1_i64 << bit;
    let (bits, character) = (p.misc.attendance_bits, p.ident.character_id);
    let count = set_bit_count(bits);
    commit(svc, ctx, who, bit, count, bits, character);
    Roll::Rewarded(count)
}

/// The reward batch: the snapshot (it carries the bit and the counters) and the mails.
fn commit(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    bit: u32,
    count: u32,
    bits: i64,
    character: flyff_common::CharacterId,
) {
    let data = std::sync::Arc::clone(ctx.data);
    let title = data
        .texts
        .get(TID_GAME_EVENT_MAIL_CALLTHEROLL_TITLE)
        .to_owned();
    let template = data.texts.get(TID_GAME_EVENT_MAIL_CALLTHEROLL).to_owned();
    let mut ops: Vec<DbOp> = Vec::new();
    let mut ids: Vec<MailId> = Vec::new();
    for (item_id, n) in rewards(count) {
        let mut item = Item::new(item_id, n);
        // a fresh serial (`SetSerialNumber`); flag 0
        let mut src = CtxSerials::new(&mut svc.serials, ctx.persist, ctx.tickets);
        match src.next_serial() {
            Ok(s) => item.serial = flyff_common::ItemSerial(s),
            Err(e) => {
                tracing::error!(error = %e, item = item_id, "attendance: no serial for the reward");
                continue;
            }
        }
        let name = data.item(item_id).map_or("", |p| p.name.as_str());
        let body = format_text(&template, &[Arg::D(i64::from(count)), Arg::S(name)]);
        let (id, mail_ops) =
            svc.econ
                .mail
                .send_system_with_id(character, &title, &body, Some(item), 0);
        ids.push(id);
        ops.extend(mail_ops);
    }
    let (snapshots, generation) = match snapshot_for_batch(svc, ctx, who) {
        Some((s, g)) => (vec![s], Some(g)),
        None => (Vec::new(), None),
    };
    let records: Vec<_> = ids
        .iter()
        .filter_map(|id| {
            svc.econ
                .mail
                .get(*id)
                .map(crate::econ::mail::MailEntry::to_record)
        })
        .collect();
    let batch = AtomicBatch {
        snapshots,
        ops,
        ..AtomicBatch::of_ops(AtomicKind::Other("attendance"), Vec::new())
    };
    let (obj, failed_ids) = (who.0, ids);
    submit(svc, ctx, who, generation, batch, move |svc, ctx, result| {
        on_batch_result(svc, ctx, obj, bit, failed_ids, result);
    });
    for rec in records {
        let mut ev = flyff_db::audit::econ::mail_send(&rec, 0);
        if let Some(actor) = crate::audit::actor_of(ctx, who) {
            ev = ev.by(actor);
        }
        ctx.audit.emit(ev);
    }
    send_bits(ctx, obj, bits);
}

/// The continuation of the batch: a failure is a persistence fault (Plan/19 §20.3): the database has neither the bit
/// nor the mails, so the game forgets the mails (it keeps nothing the database does not have) and takes the bit
/// back, so the next minute tries again.
fn on_batch_result(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    obj: ObjId,
    bit: u32,
    ids: Vec<MailId>,
    result: TicketResult,
) {
    if committed_or_forget(svc, &ids, result) {
        return;
    }
    let bits = ctx
        .worlds
        .player_mut(obj)
        .and_then(|m| m.player_mut())
        .map(|p| {
            p.misc.attendance_bits &= !(1_i64 << bit);
            p.misc.attendance_minutes = ATTENDANCE_MINUTES - 1;
            p.misc.attendance_bits
        });
    if let Some(bits) = bits {
        send_bits(ctx, obj, bits);
    }
}

/// `true` when the batch committed; otherwise the mails it carried are forgotten and the fault is logged.
fn committed_or_forget(svc: &mut Services, ids: &[MailId], result: TicketResult) -> bool {
    match result {
        TicketResult::Committed(_) => true,
        other => {
            for id in ids {
                svc.econ.mail.remove(*id);
            }
            tracing::error!(
                mails = ?ids.iter().map(|i| i.0).collect::<Vec<_>>(),
                result = ?other,
                "attendance batch failed: the database has neither the day bit nor the mails"
            );
            false
        }
    }
}

/// `AddCallTheRoll`: the bit set to the player itself.
fn send_bits(ctx: &mut GameCtx<'_>, obj: ObjId, bits: i64) {
    let mut out = Out::new();
    out.to_self(pb::CallTheRoll {
        attendance_bits: bits,
    });
    out.flush(ctx, obj);
}

/// P3.1 `event::generic::call_the_roll` (22 §2.3, §6.4): the attendance roll of event 531, once a minute. Nothing
/// unless event 531 exists in the data and its flag is on; a day number above 62 stops the roll (the original
/// shifted out of range). Every player in the world (ascending `ObjId`) of level 16 or more whose bit of the day is
/// not set is rolled (`roll_player`).
pub fn call_the_roll(s: &mut Services, ctx: &mut GameCtx<'_>) {
    s.event.generic.ensure(ctx.data);
    let Some(begin) = s.event.generic.window(ATTENDANCE_EVENT).map(|w| w.begin) else {
        return;
    };
    if s.event.flags.get(ATTENDANCE_EVENT) == 0 {
        return;
    }
    let bit = attendance_bit(&ctx.clock.local, begin);
    if bit > ATTENDANCE_MAX_BIT {
        return;
    }
    let Ok(bit) = u32::try_from(bit) else { return };
    let players: Vec<ObjId> = ctx.sessions.iter().map(|e| e.player).collect();
    for obj in players {
        let eligible = ctx
            .worlds
            .player(obj)
            .and_then(|m| m.player().map(|p| (m.mv.level, p.misc.attendance_bits)))
            .is_some_and(|(level, bits)| {
                level >= ATTENDANCE_MIN_LEVEL && bits & (1_i64 << bit) == 0
            });
        if eligible {
            roll_player(s, ctx, PlayerRef(obj), bit);
        }
    }
}

/// The body of `/ctr <n>` (`TextCmd_CallTheRoll`, WP-GM-25): clears the flags and both counters of `who`, then sets
/// the bits `62 - i` for `i < n` (at most 63), marks the player for an urgent save and answers the new bit word. No
/// message to the client (legacy: the window shows the new bits at the next roll). `None` when `who` is not a player.
pub fn gm_call_the_roll(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    n: i32,
) -> Option<i64> {
    let p = ctx.worlds.player_mut(who.0).and_then(|m| m.player_mut())?;
    let mut bits = 0_i64;
    for i in 0..n.clamp(0, 63) {
        bits |= 1_i64 << (ATTENDANCE_MAX_BIT - i64::from(i));
    }
    p.misc.attendance_bits = bits;
    p.misc.attendance_minutes = 0;
    p.misc.attendance_elapsed = 0;
    svc.saves.mark_urgent(who.0);
    Some(bits)
}
