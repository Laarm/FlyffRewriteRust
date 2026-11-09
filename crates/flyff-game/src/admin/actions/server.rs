//! Server commands of the console (Plan/25 §9.6, §11.5): `RunJob`, the scheduler's "run now".

use crate::admin::queries::server::is_runnable;
use crate::admin::types::{AdminErrorCode, AdminFailure};
use crate::ctx::{GameCtx, Services};
use crate::input::{AdminActor, AdminOutput};
use crate::sched::JobBody;

/// Run a scheduler job now (`RunJob`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunJob {
    /// The job name (as `ListJobs` lists it).
    pub name: String,
}

/// `RunJob`: calls the body of a job that [`is_runnable`] right now, on the game thread, and counts it as a manual
/// run (`JobStats::manual_runs`). The answer says which job ran and at which tick.
pub(super) fn run_job(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    actor: AdminActor,
    cmd: RunJob,
) -> AdminOutput {
    let name = cmd.name.trim();
    if ctx.sched.job_stats(name).is_none() {
        return AdminOutput::Failed(AdminFailure::new(
            AdminErrorCode::NotFound,
            format!("there is no job named {name}"),
        ));
    }
    if !is_runnable(name) {
        return AdminOutput::Failed(AdminFailure::new(
            AdminErrorCode::Invalid,
            format!("the job {name} cannot be run on demand"),
        ));
    }
    let Some(body) = ctx.sched.job_body(name) else {
        return AdminOutput::Failed(AdminFailure::new(
            AdminErrorCode::NotFound,
            format!("the job {name} has no body to run"),
        ));
    };
    // the registered name (a `&'static str`) is what the statistics are kept under
    let registered = ctx
        .sched
        .jobs(ctx.cfg, ctx.live, ctx.clock.tick)
        .into_iter()
        .find(|j| j.name == name)
        .map(|j| j.name);
    tracing::info!(job = name, actor = %actor.label(), "admin console: job run on demand");
    match body {
        JobBody::Periodic(run) => run(svc, ctx),
        JobBody::Wall(run) => {
            let stamp = ctx.clock.local;
            run(svc, ctx, &stamp);
        }
    }
    if let Some(n) = registered {
        let (tick, unix) = (ctx.clock.tick, ctx.clock.unix_ms);
        ctx.sched.record_run(tick, unix, n, true);
    }
    AdminOutput::Lines(vec![format!(
        "job {name} ran at tick {} (by {})",
        ctx.clock.tick.0,
        actor.label()
    )])
}
