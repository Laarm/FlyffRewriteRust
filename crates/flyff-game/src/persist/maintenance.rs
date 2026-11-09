//! The daily maintenance trigger (Plan/13 §23, WP-DB-16): the wall job `maintenance`, due every day at
//! `[retention] maintenance_hour` local time (`WallSpec::Daily`), with `CatchUp::RunOnceAtStart` so that a
//! maintenance hour missed while the server was down runs once at the next start. The boundary-crossed wall
//! engine records `schedule.last_fired.maintenance` in `server_kv` (13 §15.2, `flyff-server`'s `KvWallStore`).
//!
//! The job body ([`run_now`], also the console command `db maintenance now`) selects the purge candidates from
//! the [`PlayerDirectory`](crate::presence::PlayerDirectory) - soft-deleted characters older than `[retention]
//! deleted_character_days` -, removes them from the directory and queues `MaintenanceJob::Daily` on the
//! persistence actor, which purges them with their cascades and trims the retention tables in one transaction
//! (`flyff_db::maintenance::run_daily`). The actor re-checks the age of every id and the whole run is idempotent.

use crate::config::GameConfig;
use crate::ctx::{GameCtx, Services};
use crate::sched::wall::{CatchUp, WallJob, WallSpec};
use crate::sched::{DocId, Scheduler};
use crate::tick::LocalStamp;
use flyff_common::UnixTime;

/// The wall job's name: the suffix of `server_kv` key `schedule.last_fired.maintenance` (13 §15.2).
pub const JOB: &str = "maintenance";

/// Registers the daily wall job (called by `sched::registry::register_all`).
pub fn register(sched: &mut Scheduler, cfg: &GameConfig) {
    sched.register_wall(WallJob {
        name: JOB,
        owner: DocId(13),
        specs: [WallSpec::Daily {
            hour: cfg.retention.maintenance_hour.min(23),
            minute: 0,
        }]
        .into_iter()
        .collect(),
        catch_up: CatchUp::RunOnceAtStart,
        run: daily,
    });
}

fn daily(svc: &mut Services, ctx: &mut GameCtx<'_>, _stamp: &LocalStamp) {
    let selected = run_now(svc, ctx);
    tracing::info!(selected, "daily maintenance queued");
}

/// Selects the characters whose retention ran out (`deleted_at < now - deleted_character_days`), forgets them in
/// the directory and queues the daily job. Returns how many characters were selected. Never blocks: the report
/// is logged by the writer and kept for `PersistHandle::last_maintenance`.
pub fn run_now(svc: &mut Services, ctx: &mut GameCtx<'_>) -> usize {
    let now = ctx.clock.unix_ms.to_unix_time();
    let window = i64::from(ctx.cfg.retention.deleted_character_days) * 86_400;
    let purge = svc
        .presence
        .purge_candidates(UnixTime(now.0.saturating_sub(window)));
    svc.presence.purge(&purge);
    // 21 §3.6: couples and campuses of the purged characters vanish in memory like in the database
    crate::social::relation::purge(svc, ctx, &purge);
    let selected = purge.len();
    ctx.persist.maintenance_daily(purge, now);
    selected
}
