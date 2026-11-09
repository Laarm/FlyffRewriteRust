//! Communication commands of the console (Plan/25 §8, §11.5; **WP-ADMIN-27**): the in-memory schedule of notices
//! (`SetNoticeSchedule`), the wall job `admin.notices` that fires it, and the notice that goes out at once
//! (`SendNotice`).
//!
//! **The schedule.** The rows live in `scheduled_notices` (flyff-db). The server reads them at start and sends them
//! with [`SetNoticeSchedule`], and again after every create, update or delete (the command replaces the whole
//! schedule). The game thread works only from [`NoticeSchedule`] (`Services.notices`): it never reads the database.
//! The static wall job [`JOB`] (`WallSpec::EveryMinute`) evaluates the schedule once per wall minute, so a notice
//! fires **at its minute**:
//!
//! * a **single** notice (no repeat) fires at its start minute once. A start that was missed (the server was down,
//!   the schedule arrived late) still fires within [`LATE_GRACE_S`] of it, never later; editing a fired notice to a
//!   later start arms it again (it fires when it was never fired since its start);
//! * **every N minutes** fires at `start + k * N` minutes;
//! * **daily at hh:mm** fires when the server's local clock shows hh:mm (a time that does not exist on a
//!   spring-forward day skips that day; the zone is `server.timezone`);
//! * a paused notice never fires; nothing fires after the end time (inclusive); nothing fires before the start.
//!
//! A fired notice is stamped in memory and in the database (`DbOp::NoticeFired{id, at}`, one-minute granularity), so
//! a restart does not fire it twice. Missed repeats are not caught up. A schedule that arrives with an older
//! `last_fired` than the one in memory keeps the newer one (a fire between the database read and the command).
//!
//! Delivery ([`deliver`]) is shared by the schedule and by [`SendNotice`]: the large `SystemNotice` line of `/sys`
//! (at most 511 bytes on the wire) or the centred `Caption` (at most 260 bytes), to everybody, to one world, or to
//! a list of characters (offline ones are skipped).

use crate::admin::actions::common::{check_group, failed, online_player};
use crate::admin::types::AdminErrorCode;
use crate::chat::send_to;
use crate::ctx::{GameCtx, Services};
use crate::input::{AdminActor, AdminOutput};
use crate::outbox::Audience;
use crate::sched::DocId;
use crate::sched::Scheduler;
use crate::sched::wall::{CatchUp, WallJob, WallSpec, WallZone};
use crate::tick::LocalStamp;
use flyff_common::{CharacterId, ObjId, UnixMs, UnixTime, WorldId};
use flyff_db::DbOp;
use flyff_db::ScheduledNoticeRecord;
use flyff_db::audit::{AuditEvent, ev};
use flyff_proto::pb;
use serde_json::json;
use smallvec::smallvec;

/// The wall job that fires the schedule (`schedule.last_fired.admin.notices` is never written: the job is a minute
/// job and keeps its own stamps per notice).
pub const JOB: &str = "admin.notices";
/// The most notices a schedule holds.
pub const MAX_SCHEDULED: usize = 500;
/// A single notice whose minute passed fires up to this long afterwards (a restart, a late schedule), seconds.
pub const LATE_GRACE_S: i64 = 10 * 60;
/// The longest `SystemNotice` text on the wire (`str<=511`).
pub const MAX_SYSTEM_BYTES: usize = 511;
/// The longest `Caption` text on the wire (`str<=260`).
pub const MAX_CAPTION_BYTES: usize = 260;
/// The most characters one notice names.
pub const MAX_NOTICE_CHARACTERS: usize = 100;

/// How a notice looks to the player.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoticeStyle {
    /// The large `/sys` line (`SystemNotice`).
    System,
    /// The centred caption (`Caption`, large).
    Caption,
}

impl NoticeStyle {
    /// `system` or `caption` (the column `scheduled_notices.style`).
    pub fn name(self) -> &'static str {
        match self {
            NoticeStyle::System => "system",
            NoticeStyle::Caption => "caption",
        }
    }

    /// The style named `text`.
    pub fn parse(text: &str) -> Option<NoticeStyle> {
        match text {
            "system" => Some(NoticeStyle::System),
            "caption" => Some(NoticeStyle::Caption),
            _ => None,
        }
    }

    /// The longest text of this style, bytes.
    pub fn max_bytes(self) -> usize {
        match self {
            NoticeStyle::System => MAX_SYSTEM_BYTES,
            NoticeStyle::Caption => MAX_CAPTION_BYTES,
        }
    }
}

/// Who receives a notice.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NoticeTarget {
    /// Everybody in the world.
    All,
    /// The players of one world.
    World(WorldId),
    /// These characters (the ones that are not online are skipped).
    Characters(Vec<CharacterId>),
}

impl NoticeTarget {
    /// The text of the column `scheduled_notices.target`: `all`, `world:<id>` or `chars:<id>,<id>`.
    pub fn text(&self) -> String {
        match self {
            NoticeTarget::All => "all".to_owned(),
            NoticeTarget::World(w) => format!("world:{}", w.0),
            NoticeTarget::Characters(ids) => {
                let list: Vec<String> = ids.iter().map(|c| c.0.to_string()).collect();
                format!("chars:{}", list.join(","))
            }
        }
    }

    /// Reads the column text back.
    pub fn parse(text: &str) -> Result<NoticeTarget, String> {
        let t = text.trim();
        if t == "all" {
            return Ok(NoticeTarget::All);
        }
        if let Some(w) = t.strip_prefix("world:") {
            return w
                .parse::<u32>()
                .map(|w| NoticeTarget::World(WorldId(w)))
                .map_err(|_| format!("'{t}': not a world id"));
        }
        if let Some(list) = t.strip_prefix("chars:") {
            let ids: Result<Vec<CharacterId>, _> = list
                .split(',')
                .filter(|s| !s.is_empty())
                .map(|s| s.parse::<u32>().map(CharacterId))
                .collect();
            return match ids {
                Ok(ids) if !ids.is_empty() && ids.len() <= MAX_NOTICE_CHARACTERS => {
                    Ok(NoticeTarget::Characters(ids))
                }
                Ok(_) => Err(format!("'{t}': 1..={MAX_NOTICE_CHARACTERS} character ids")),
                Err(_) => Err(format!("'{t}': not a list of character ids")),
            };
        }
        Err(format!("'{t}': not a notice target"))
    }
}

/// When a notice repeats.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoticeRepeat {
    /// Once, at the start minute.
    Once,
    /// Every this many minutes after the start.
    EveryMinutes(u32),
    /// Every day at this local time.
    Daily {
        /// 0..=23.
        hour: u8,
        /// 0..=59.
        minute: u8,
    },
}

/// Where a notice is in its life (the list of the console).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoticeState {
    /// It will fire (see `next_fire`).
    Scheduled,
    /// Paused by an operator.
    Paused,
    /// A single notice that has fired.
    Done,
    /// A single notice whose minute passed unnoticed (beyond [`LATE_GRACE_S`]).
    Missed,
    /// A repeating notice past its end time.
    Ended,
}

impl NoticeState {
    /// `scheduled`, `paused`, `done`, `missed` or `ended`.
    pub fn name(self) -> &'static str {
        match self {
            NoticeState::Scheduled => "scheduled",
            NoticeState::Paused => "paused",
            NoticeState::Done => "done",
            NoticeState::Missed => "missed",
            NoticeState::Ended => "ended",
        }
    }
}

/// One scheduled or recurring notice (a row of `scheduled_notices`, parsed).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScheduledNotice {
    /// The row id.
    pub id: i64,
    /// The text.
    pub text: String,
    /// How it looks.
    pub style: NoticeStyle,
    /// Who gets it.
    pub target: NoticeTarget,
    /// The first minute, Unix seconds.
    pub start_unix: i64,
    /// When it repeats.
    pub repeat: NoticeRepeat,
    /// The last moment it may fire (inclusive), Unix seconds.
    pub end_unix: Option<i64>,
    /// An operator paused it.
    pub paused: bool,
    /// The minute it fired last (`scheduled_notices.last_fired_at`), Unix seconds.
    pub last_fired_unix: Option<i64>,
}

/// The start of the minute of `unix`.
fn floor_min(unix: i64) -> i64 {
    unix - unix.rem_euclid(60)
}

/// Why a text cannot be a notice of `style`.
pub fn check_text(text: &str, style: NoticeStyle) -> Result<(), String> {
    let t = text.trim();
    if t.is_empty() {
        return Err("the notice is empty".to_owned());
    }
    if t.len() > style.max_bytes() {
        return Err(format!(
            "a {} notice is at most {} bytes, this one has {}",
            style.name(),
            style.max_bytes(),
            t.len()
        ));
    }
    if t.chars().any(char::is_control) {
        return Err("the notice contains control characters".to_owned());
    }
    Ok(())
}

impl ScheduledNotice {
    /// Reads a database row. A row that does not parse (a style or target the job does not know) is an error with
    /// the reason: the caller leaves it out of the schedule and logs it.
    pub fn from_record(r: &ScheduledNoticeRecord) -> Result<ScheduledNotice, String> {
        let style = NoticeStyle::parse(&r.style)
            .ok_or_else(|| format!("notice {}: unknown style '{}'", r.id, r.style))?;
        let target = NoticeTarget::parse(&r.target).map_err(|e| format!("notice {}: {e}", r.id))?;
        let repeat = match (r.repeat_min, r.daily_at.as_deref()) {
            (Some(n), None) if n >= 1 => NoticeRepeat::EveryMinutes(n),
            (None, Some(hm)) => {
                let (h, m) = hm
                    .split_once(':')
                    .and_then(|(h, m)| Some((h.parse::<u8>().ok()?, m.parse::<u8>().ok()?)))
                    .filter(|(h, m)| *h < 24 && *m < 60)
                    .ok_or_else(|| format!("notice {}: daily_at '{hm}' is not HH:MM", r.id))?;
                NoticeRepeat::Daily { hour: h, minute: m }
            }
            (None, None) => NoticeRepeat::Once,
            _ => return Err(format!("notice {}: an impossible repeat", r.id)),
        };
        let n = ScheduledNotice {
            id: r.id,
            text: r.text.clone(),
            style,
            target,
            start_unix: r.start_at.0,
            repeat,
            end_unix: r.end_at.map(|t| t.0),
            paused: r.paused,
            last_fired_unix: r.last_fired_at.map(|t| t.0),
        };
        n.validate()?;
        Ok(n)
    }

    /// The limits of a notice: its text and its character list.
    pub fn validate(&self) -> Result<(), String> {
        check_text(&self.text, self.style).map_err(|e| format!("notice {}: {e}", self.id))?;
        if let NoticeTarget::Characters(ids) = &self.target
            && (ids.is_empty() || ids.len() > MAX_NOTICE_CHARACTERS)
        {
            return Err(format!(
                "notice {}: 1..={MAX_NOTICE_CHARACTERS} characters",
                self.id
            ));
        }
        if let NoticeRepeat::EveryMinutes(0) = self.repeat {
            return Err(format!("notice {}: repeat every 0 minutes", self.id));
        }
        Ok(())
    }

    /// `true` when the notice fires at the wall minute `minute` (Unix seconds, minute-aligned); `local` is that
    /// minute in the server's zone. See the module documentation for the rules.
    pub fn due(&self, minute: i64, local: &LocalStamp) -> bool {
        if self.paused {
            return false;
        }
        let start = floor_min(self.start_unix);
        if minute < start || self.end_unix.is_some_and(|end| minute > end) {
            return false;
        }
        if self.last_fired_unix.is_some_and(|l| floor_min(l) >= minute) {
            return false;
        }
        match self.repeat {
            NoticeRepeat::Once => {
                minute - start <= LATE_GRACE_S && self.last_fired_unix.is_none_or(|l| l < start)
            }
            NoticeRepeat::EveryMinutes(n) => {
                let step = i64::from(n.max(1)) * 60;
                (minute - start) % step == 0
            }
            NoticeRepeat::Daily { hour, minute: m } => local.hour == hour && local.minute == m,
        }
    }

    /// Where the notice is in its life at `now_unix` and the first minute after `now_unix` at which it fires
    /// (`None` unless `Scheduled`). The first-fire rules are those of [`ScheduledNotice::due`].
    pub fn status(&self, zone: &WallZone, now_unix: i64) -> (NoticeState, Option<i64>) {
        if self.paused {
            return (NoticeState::Paused, None);
        }
        let start = floor_min(self.start_unix);
        let this_minute = floor_min(now_unix);
        let next_boundary = this_minute + 60;
        let past_end = |m: i64| self.end_unix.is_some_and(|end| m > end);
        match self.repeat {
            NoticeRepeat::Once => {
                if self.last_fired_unix.is_some_and(|l| l >= start) {
                    return (NoticeState::Done, None);
                }
                if this_minute - start > LATE_GRACE_S {
                    return (NoticeState::Missed, None);
                }
                let m = start.max(next_boundary);
                if past_end(m) {
                    (NoticeState::Missed, None)
                } else {
                    (NoticeState::Scheduled, Some(m))
                }
            }
            NoticeRepeat::EveryMinutes(n) => {
                let step = i64::from(n.max(1)) * 60;
                let from = start.max(next_boundary);
                let k = (from - start + step - 1).div_euclid(step);
                let m = start + k * step;
                if past_end(m) {
                    (NoticeState::Ended, None)
                } else {
                    (NoticeState::Scheduled, Some(m))
                }
            }
            NoticeRepeat::Daily { hour, minute } => {
                let after = start.max(next_boundary) - 1;
                let next = zone
                    .next_occurrence(
                        &WallSpec::Daily { hour, minute },
                        UnixMs(after.saturating_mul(1000)),
                    )
                    .map(|t| t.0.div_euclid(1000));
                match next {
                    Some(m) if !past_end(m) => (NoticeState::Scheduled, Some(m)),
                    _ => (NoticeState::Ended, None),
                }
            }
        }
    }
}

/// The notices the game thread fires from (`Services.notices`). Plain data: it is replaced as a whole by
/// [`SetNoticeSchedule`] and read by the wall job.
#[derive(Debug, Default)]
pub struct NoticeSchedule {
    notices: Vec<ScheduledNotice>,
    /// How many schedules were installed since the start.
    pub installs: u64,
    /// How many notices fired since the start.
    pub fired: u64,
}

impl NoticeSchedule {
    /// The notices in id order.
    pub fn notices(&self) -> &[ScheduledNotice] {
        &self.notices
    }

    /// Installs `incoming` as the schedule. A notice that is known keeps the newer of the two `last_fired` stamps.
    pub fn replace(&mut self, mut incoming: Vec<ScheduledNotice>) {
        for n in &mut incoming {
            if let Some(old) = self.notices.iter().find(|o| o.id == n.id) {
                n.last_fired_unix = n.last_fired_unix.max(old.last_fired_unix);
            }
        }
        incoming.sort_by_key(|n| n.id);
        self.notices = incoming;
        self.installs += 1;
    }

    /// The notices that fire at `minute`.
    pub fn due(&self, minute: i64, local: &LocalStamp) -> Vec<ScheduledNotice> {
        self.notices
            .iter()
            .filter(|n| n.due(minute, local))
            .cloned()
            .collect()
    }

    /// Stamps `id` as fired at `minute`.
    pub fn mark_fired(&mut self, id: i64, minute: i64) {
        if let Some(n) = self.notices.iter_mut().find(|n| n.id == id) {
            n.last_fired_unix = Some(minute);
        }
        self.fired += 1;
    }
}

/// Replace the schedule of notices the game thread works through (`SetNoticeSchedule`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SetNoticeSchedule {
    /// The whole schedule.
    pub notices: Vec<ScheduledNotice>,
}

/// Send a notice now (`SendNotice`, 25 §8.1 "Now").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SendNotice {
    /// The text.
    pub text: String,
    /// How it looks.
    pub style: NoticeStyle,
    /// Who gets it.
    pub target: NoticeTarget,
}

/// Registers the wall job `admin.notices` (called by `sched::registry::register_all`).
pub fn register(sched: &mut Scheduler) {
    sched.register_wall(WallJob {
        name: JOB,
        owner: DocId(25),
        specs: smallvec![WallSpec::EveryMinute],
        catch_up: CatchUp::Skip,
        run: fire_due,
    });
}

/// The body of the wall job: fires what is due at the crossed minute.
fn fire_due(svc: &mut Services, ctx: &mut GameCtx<'_>, stamp: &LocalStamp) {
    let minute = floor_min(stamp.unix_s);
    let due = svc.notices.due(minute, stamp);
    for n in due {
        let sent = deliver(svc, ctx, &n.text, n.style, &n.target);
        svc.notices.mark_fired(n.id, minute);
        ctx.persist.op(DbOp::NoticeFired {
            id: n.id,
            at: UnixTime(minute),
        });
        ctx.audit.emit(AuditEvent::new(
            ev::ADMIN_NOTICE,
            json!({
                "action": "fired",
                "id": n.id,
                "style": n.style.name(),
                "target": n.target.text(),
                "recipients": sent,
                "bytes": n.text.len(),
                "actor": "scheduler",
                "state": "done",
            }),
        ));
        tracing::info!(id = n.id, recipients = sent, "scheduled notice fired");
    }
}

/// The message of a notice.
fn message(style: NoticeStyle, text: &str, world: WorldId) -> pb::server_msg::Msg {
    match style {
        NoticeStyle::System => pb::SystemNotice {
            text: text.to_owned(),
        }
        .into(),
        NoticeStyle::Caption => pb::Caption {
            small: false,
            world_id: world.0,
            text: text.to_owned(),
        }
        .into(),
    }
}

/// Sends a notice and returns how many players it reached (see the module documentation).
pub fn deliver(
    svc: &Services,
    ctx: &mut GameCtx<'_>,
    text: &str,
    style: NoticeStyle,
    target: &NoticeTarget,
) -> u32 {
    let text = text.trim();
    match target {
        NoticeTarget::All => {
            let reached = count_in_world(ctx, None);
            ctx.broadcast(
                Some(svc),
                ObjId::NULL,
                Audience::AllOnline,
                message(style, text, WorldId(0)),
            );
            reached
        }
        NoticeTarget::World(w) => {
            let reached = count_in_world(ctx, Some(*w));
            ctx.broadcast(
                Some(svc),
                ObjId::NULL,
                Audience::World(*w),
                message(style, text, *w),
            );
            reached
        }
        NoticeTarget::Characters(ids) => {
            let mut reached = 0;
            for id in ids {
                if let Ok(who) = online_player(svc, ctx, *id)
                    && ctx.session_of(who).is_some()
                {
                    send_to(ctx, who, message(style, text, WorldId(0)));
                    reached += 1;
                }
            }
            reached
        }
    }
}

/// The connected players in `world` (any world for `None`).
fn count_in_world(ctx: &GameCtx<'_>, world: Option<WorldId>) -> u32 {
    let n = ctx
        .sessions
        .bound()
        .filter(|(_, obj)| {
            ctx.worlds
                .player(*obj)
                .is_some_and(|m| !m.core.deleted && world.is_none_or(|w| m.core.world == w))
        })
        .count();
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// `SetNoticeSchedule`: validates the list, then replaces the schedule.
pub(super) fn set_notice_schedule(
    svc: &mut Services,
    _ctx: &mut GameCtx<'_>,
    _actor: AdminActor,
    cmd: SetNoticeSchedule,
) -> AdminOutput {
    if cmd.notices.len() > MAX_SCHEDULED {
        return failed(
            AdminErrorCode::TooManyTargets,
            format!("at most {MAX_SCHEDULED} scheduled notices"),
        );
    }
    for (i, n) in cmd.notices.iter().enumerate() {
        if let Err(e) = n.validate() {
            return failed(AdminErrorCode::Invalid, e);
        }
        if cmd.notices[..i].iter().any(|o| o.id == n.id) {
            return failed(
                AdminErrorCode::Invalid,
                format!("notice {} is listed twice", n.id),
            );
        }
    }
    let count = u32::try_from(cmd.notices.len()).unwrap_or(u32::MAX);
    svc.notices.replace(cmd.notices);
    AdminOutput::Done { affected: count }
}

/// `SendNotice`: validates the text and the target, then delivers. Answers `Done{affected}` with the players
/// reached.
pub(super) fn send_notice(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    _actor: AdminActor,
    cmd: SendNotice,
) -> AdminOutput {
    if let Err(e) = check_text(&cmd.text, cmd.style) {
        return failed(AdminErrorCode::Invalid, e);
    }
    match &cmd.target {
        NoticeTarget::All => {}
        NoticeTarget::World(w) => {
            if w.0 == 0 || w.0 == u32::MAX || ctx.worlds.world(*w).is_none() {
                return failed(
                    AdminErrorCode::Invalid,
                    format!("world {} is not loaded", w.0),
                );
            }
        }
        NoticeTarget::Characters(ids) => {
            if let Err(f) = check_group(ids.len(), "characters") {
                return AdminOutput::Failed(f);
            }
            if ids.len() > MAX_NOTICE_CHARACTERS {
                return failed(
                    AdminErrorCode::TooManyTargets,
                    format!("at most {MAX_NOTICE_CHARACTERS} characters per notice"),
                );
            }
        }
    }
    let reached = deliver(svc, ctx, &cmd.text, cmd.style, &cmd.target);
    AdminOutput::Done { affected: reached }
}
