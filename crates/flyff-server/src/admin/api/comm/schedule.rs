//! The scheduled notices between the database rows, the wire and the game thread (Plan/25 §8.1): the checks of a
//! notice, the rows of the list with their next fire time, and `push`, which installs the rows on the game thread
//! (`AdminCmd::SetNoticeSchedule`) after every change and at start ([`spawn_loader`]).

use super::dto::{NoticeBody, NoticeRepeatDto, NoticeRow, NoticeStyleDto, NoticeTargetDto};
use crate::admin::error::{ApiError, ApiResult};
use crate::admin::ops::AdminOps;
use flyff_common::{CharacterId, UnixMs, UnixTime, WorldId};
use flyff_db::ScheduledNoticeRecord;
use flyff_db::admin::AdminReadQuery;
use flyff_game::admin::actions::comm::{
    LATE_GRACE_S, MAX_NOTICE_CHARACTERS, MAX_SCHEDULED, NoticeRepeat, NoticeState, NoticeStyle,
    NoticeTarget, ScheduledNotice, SetNoticeSchedule, check_text,
};
use flyff_game::input::{AdminActor, AdminCmd, AdminOutput};
use flyff_game::sched::wall::WallZone;
use std::sync::Arc;
use std::time::Duration;

/// The longest repeat interval, minutes (30 days).
pub const REPEAT_MAX_MIN: u32 = 43_200;
/// The earliest start the schedule accepts (2000-01-01).
const START_MIN: i64 = 946_684_800;
/// The latest start: five years ahead.
const START_AHEAD_S: i64 = 5 * 366 * 86_400;

/// One schedule change at a time: every change reads the rows again and replaces the whole schedule on the game
/// thread, so two changes must not interleave their reads and their pushes.
pub(super) static SCHEDULE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub(super) fn style_of(s: NoticeStyleDto) -> NoticeStyle {
    match s {
        NoticeStyleDto::System => NoticeStyle::System,
        NoticeStyleDto::Caption => NoticeStyle::Caption,
    }
}

pub(super) fn style_dto(s: NoticeStyle) -> NoticeStyleDto {
    match s {
        NoticeStyle::System => NoticeStyleDto::System,
        NoticeStyle::Caption => NoticeStyleDto::Caption,
    }
}

/// The audience of a body, checked.
pub(super) fn target_of(t: &NoticeTargetDto, field: &str) -> ApiResult<NoticeTarget> {
    match t {
        NoticeTargetDto::All => Ok(NoticeTarget::All),
        NoticeTargetDto::World { world } => {
            if *world == 0 || *world == u32::MAX {
                Err(ApiError::invalid(
                    &format!("{field}.world"),
                    "name a world id",
                ))
            } else {
                Ok(NoticeTarget::World(WorldId(*world)))
            }
        }
        NoticeTargetDto::Characters { characters } => {
            let mut ids: Vec<CharacterId> = Vec::with_capacity(characters.len());
            for c in characters {
                let id = CharacterId(*c);
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
            if ids.is_empty() || ids.len() > MAX_NOTICE_CHARACTERS {
                Err(ApiError::invalid(
                    &format!("{field}.characters"),
                    format!("name 1..={MAX_NOTICE_CHARACTERS} characters"),
                ))
            } else {
                Ok(NoticeTarget::Characters(ids))
            }
        }
    }
}

pub(super) fn target_dto(t: &NoticeTarget) -> NoticeTargetDto {
    match t {
        NoticeTarget::All => NoticeTargetDto::All,
        NoticeTarget::World(w) => NoticeTargetDto::World { world: w.0 },
        NoticeTarget::Characters(ids) => NoticeTargetDto::Characters {
            characters: ids.iter().map(|c| c.0).collect(),
        },
    }
}

/// The text of a notice: `400 invalid{text}` with the reason.
pub(super) fn check_notice_text(text: &str, style: NoticeStyleDto) -> ApiResult<()> {
    check_text(text, style_of(style)).map_err(|e| ApiError::invalid("text", e))
}

/// The row that a body makes (`id` 0 for an insert). `allow_past_start` lets an update keep the start of a single
/// notice that has been in the past for a while.
pub(super) fn record_of(
    body: &NoticeBody,
    id: i64,
    created_by: &str,
    now: UnixTime,
    allow_past_start: bool,
) -> ApiResult<ScheduledNoticeRecord> {
    check_notice_text(&body.text, body.style)?;
    let target = target_of(&body.target, "target")?;
    if body.start_at < START_MIN || body.start_at > now.0.saturating_add(START_AHEAD_S) {
        return Err(ApiError::invalid(
            "start_at",
            "the start is a time between the year 2000 and five years ahead",
        ));
    }
    if let Some(end) = body.end_at
        && end <= body.start_at
    {
        return Err(ApiError::invalid(
            "end_at",
            "the end must be after the start",
        ));
    }
    let (repeat_min, daily_at) = match &body.repeat {
        NoticeRepeatDto::Once => {
            let floor = now.0 - now.0.rem_euclid(60) - LATE_GRACE_S;
            if body.start_at < floor && !allow_past_start {
                return Err(ApiError::invalid(
                    "start_at",
                    "a single notice cannot start in the past: it would never fire",
                ));
            }
            (None, None)
        }
        NoticeRepeatDto::Every { minutes } => {
            if *minutes == 0 || *minutes > REPEAT_MAX_MIN {
                return Err(ApiError::invalid(
                    "repeat.minutes",
                    format!("repeat every 1..={REPEAT_MAX_MIN} minutes"),
                ));
            }
            (Some(*minutes), None)
        }
        NoticeRepeatDto::Daily { at } => {
            let ok = at.len() == 5
                && at.as_bytes().get(2) == Some(&b':')
                && at
                    .split_once(':')
                    .and_then(|(h, m)| Some((h.parse::<u8>().ok()?, m.parse::<u8>().ok()?)))
                    .is_some_and(|(h, m)| h < 24 && m < 60);
            if !ok {
                return Err(ApiError::invalid("repeat.at", "the time is HH:MM"));
            }
            (None, Some(at.clone()))
        }
    };
    Ok(ScheduledNoticeRecord {
        id,
        text: body.text.trim().to_owned(),
        style: style_of(body.style).name().to_owned(),
        target: target.text(),
        start_at: UnixTime(body.start_at),
        repeat_min,
        daily_at,
        end_at: body.end_at.map(UnixTime),
        paused: body.paused,
        created_by: created_by.to_owned(),
        created_at: now,
        last_fired_at: None,
    })
}

/// The clock of the notices, Unix seconds: the game thread's wall clock (what `ServerTime` and the wall jobs read; the
/// system clock unless a test moves it), so that what the console shows is what the job fires by.
pub(super) fn now_s(ops: &AdminOps) -> i64 {
    ops.gateway.wall.now_ms().0.div_euclid(1000)
}

/// The server's time zone (`UTC` when the name does not resolve).
pub(super) fn zone_of(ops: &AdminOps) -> WallZone {
    WallZone::from_iana(&ops.reload_source.zone_name).unwrap_or_else(|_| WallZone::utc())
}

/// A row of the list: `None` for a stored row the job cannot read (it is logged, not shown).
pub(super) fn row_of(rec: &ScheduledNoticeRecord, zone: &WallZone, now: i64) -> Option<NoticeRow> {
    let n = match ScheduledNotice::from_record(rec) {
        Ok(n) => n,
        Err(e) => {
            tracing::warn!(id = rec.id, error = %e, "a stored notice cannot be read");
            return None;
        }
    };
    let (state, next) = n.status(zone, now);
    Some(NoticeRow {
        id: n.id,
        text: n.text.clone(),
        style: style_dto(n.style),
        target: target_dto(&n.target),
        start_at: n.start_unix,
        repeat: match n.repeat {
            NoticeRepeat::Once => NoticeRepeatDto::Once,
            NoticeRepeat::EveryMinutes(minutes) => NoticeRepeatDto::Every { minutes },
            NoticeRepeat::Daily { hour, minute } => NoticeRepeatDto::Daily {
                at: format!("{hour:02}:{minute:02}"),
            },
        },
        end_at: n.end_unix,
        paused: n.paused,
        created_by: rec.created_by.clone(),
        created_at: rec.created_at.0,
        last_fired_at: n.last_fired_unix,
        state: state_name(state).to_owned(),
        next_fire: next,
    })
}

fn state_name(s: NoticeState) -> &'static str {
    s.name()
}

/// The stored rows in id order.
pub(super) async fn rows(ops: &AdminOps) -> ApiResult<Vec<ScheduledNoticeRecord>> {
    ops.persist
        .admin_read(AdminReadQuery::ScheduledNotices)
        .await
        .map_err(|e| ApiError::db(&e))?
        .scheduled_notices()
        .map_err(|e| ApiError::db(&e))
}

/// The most notices the schedule holds.
pub(super) const MAX_NOTICES: usize = MAX_SCHEDULED;

/// Replaces the schedule of the game thread by the stored rows (the rows the job cannot read are left out and logged).
/// Returns how many notices the game thread holds now. Hold [`SCHEDULE_LOCK`] across the change and this call.
pub(super) async fn push(ops: &AdminOps, actor: AdminActor) -> ApiResult<u32> {
    let notices: Vec<ScheduledNotice> = rows(ops)
        .await?
        .iter()
        .filter_map(|r| match ScheduledNotice::from_record(r) {
            Ok(n) => Some(n),
            Err(e) => {
                tracing::warn!(id = r.id, error = %e, "a stored notice is left out of the schedule");
                None
            }
        })
        .collect();
    match ops
        .game(
            AdminCmd::SetNoticeSchedule(SetNoticeSchedule { notices }),
            actor,
        )
        .await?
    {
        AdminOutput::Done { affected } => Ok(affected),
        AdminOutput::Failed(f) => Err(ApiError::from(&f)),
        other => Err(ApiError::internal(&format!(
            "the game answered the schedule with {other:?}"
        ))),
    }
}

/// Installs the stored schedule on the game thread at start (25 §8.1 "loaded at start through a read query"). The
/// game thread is ready when this runs; a refusal is retried a few times and then logged: the next change of the
/// schedule installs it anyway.
pub fn spawn_loader(rt: &tokio::runtime::Handle, ops: Arc<AdminOps>) {
    rt.spawn(async move {
        for attempt in 1..=5_u32 {
            let result = {
                let _guard = SCHEDULE_LOCK.lock().await;
                push(&ops, AdminActor::Console).await
            };
            match result {
                Ok(0) => return,
                Ok(n) => {
                    tracing::info!(notices = n, "the scheduled notices are installed");
                    return;
                }
                Err(e) => {
                    tracing::warn!(attempt, error = %e, "cannot install the scheduled notices");
                    tokio::time::sleep(Duration::from_secs(2 * u64::from(attempt))).await;
                }
            }
        }
        tracing::error!("the scheduled notices were not installed: they fire after the next change of the schedule");
    });
}

/// Seconds since the epoch of `ms` (the clock of the zone helpers).
pub(super) fn offset_s(zone: &WallZone, now: i64) -> i32 {
    zone.local(UnixMs(now.saturating_mul(1000))).utc_offset_s
}
