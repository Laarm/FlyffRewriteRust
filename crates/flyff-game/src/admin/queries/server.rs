//! Server queries (Plan/25 §9.6, §11.5): `ListJobs`, the scheduler page's table (`Scheduler::jobs`).

use super::AdminQueryResult;
use crate::sched::{JobInfo, JobKind};
use crate::tick::Game;

/// The jobs the console may run on demand (`AdminCmd::RunJob`, Plan/25 §9.6): idempotent housekeeping whose body
/// is safe to call at any time. Everything else (the respawn, the roll call, the event machines, the tax payout)
/// is the scheduler's alone.
pub const RUNNABLE: &[&str] = &[
    "maintenance",
    "mail_expire",
    "monster_expire",
    "couple_expire",
    "guild_bank_removal_poll",
    "grants_poll",
    "audit_ccu",
];

/// `true` when `name` may be run on demand.
pub fn is_runnable(name: &str) -> bool {
    RUNNABLE.contains(&name)
}

/// One row of the scheduler page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JobRow {
    /// The scheduler's facts about the job.
    pub info: JobInfo,
    /// The console may run it now (`RunJob`).
    pub runnable: bool,
    /// The tick the answer was taken at.
    pub now_tick: u64,
    /// The wall clock of that tick in Unix milliseconds.
    pub now_unix_ms: i64,
}

impl JobRow {
    /// The kind as text (`periodic`, `slot`, `wall`).
    pub fn kind_name(&self) -> &'static str {
        match self.info.kind {
            JobKind::Periodic => "periodic",
            JobKind::Slot => "slot",
            JobKind::Wall => "wall",
        }
    }
}

/// `ListJobs`: every registered job with schedule, gate, statistics and next run.
pub(super) fn list_jobs(g: &Game) -> AdminQueryResult {
    let now = g.clock.tick;
    let unix = g.clock.unix_ms;
    let rows = g
        .sched
        .jobs(&g.cfg, &g.live, now)
        .into_iter()
        .map(|info| JobRow {
            runnable: is_runnable(info.name) && g.sched.job_body(info.name).is_some(),
            info,
            now_tick: now.0,
            now_unix_ms: unix.0,
        })
        .collect();
    AdminQueryResult::Jobs(rows)
}
