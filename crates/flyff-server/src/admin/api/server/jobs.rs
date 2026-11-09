//! The scheduler page (Plan/25 §9.6; WP-ADMIN-26): `GET /admin/api/jobs` is `Query::ListJobs` (`Scheduler::jobs`: name,
//! schedule, last run, run count, next run), `POST /admin/api/jobs/{name}/run` is `AdminCmd::RunJob` for the jobs flagged
//! runnable ([`flyff_game::admin::queries::server::RUNNABLE`]). A job has no result of its own: a failing job
//! panics the game thread, which ends the process, so the page shows runs and times only.

use super::game_query;
use crate::admin::api::{ApiState, Routes};
use crate::admin::auth::Who;
use crate::admin::error::{ApiError, ApiResult};
use crate::admin::guard::{AuditTrail, MutationJson};
use axum::Json;
use axum::extract::{Path, State};
use flyff_game::admin::actions::server::RunJob;
use flyff_game::admin::queries::server::JobRow;
use flyff_game::admin::queries::{AdminQuery, AdminQueryResult};
use flyff_game::input::{AdminCmd, AdminOutput};
use serde_json::{Value, json};

/// Mounts the routes.
pub fn mount(r: Routes<ApiState>) -> Routes<ApiState> {
    r.get("/admin/api/jobs", list)
        .post("/admin/api/jobs/{name}/run", run)
}

/// One job as JSON.
pub fn row_json(r: &JobRow) -> Value {
    let i = &r.info;
    // a periodic job is due at a tick: 15 ticks per second from the tick of the answer
    let next = match (i.next_tick, i.next_unix_ms) {
        (Some(t), _) => {
            let ahead = i64::try_from(t.0.saturating_sub(r.now_tick)).unwrap_or(0);
            Some(
                r.now_unix_ms
                    .saturating_add(ahead.saturating_mul(1000) / 15),
            )
        }
        (None, Some(u)) => Some(u.0),
        (None, None) => None,
    };
    json!({
        "name": i.name,
        "kind": r.kind_name(),
        "row": i.row,
        "owner": i.owner.0,
        "schedule": i.schedule,
        "gate_open": i.gate_open,
        "runnable": r.runnable,
        "runs": i.stats.runs,
        "last_tick": i.stats.last_tick.map(|t| t.0),
        "last_run_unix_ms": i.stats.last_unix_ms.map(|u| u.0),
        "manual_runs": i.stats.manual_runs,
        "last_manual_unix_ms": i.stats.last_manual_unix_ms.map(|u| u.0),
        "next_run_unix_ms": next,
    })
}

async fn jobs(st: &ApiState) -> ApiResult<Vec<JobRow>> {
    match game_query(&st.ops, AdminQuery::ListJobs).await? {
        AdminQueryResult::Jobs(rows) => Ok(rows),
        _ => Err(ApiError::internal("ListJobs answered with another result")),
    }
}

/// `GET /admin/api/jobs`.
async fn list(State(st): State<ApiState>) -> ApiResult<Json<Value>> {
    let rows = jobs(&st).await?;
    let now = rows.first().map_or(0, |r| r.now_unix_ms);
    Ok(Json(json!({
        "now_unix_ms": now,
        "jobs": rows.iter().map(row_json).collect::<Vec<_>>(),
    })))
}

/// `POST /admin/api/jobs/{name}/run` `{body: {}, reason?}`.
async fn run(
    State(st): State<ApiState>,
    Who(who): Who,
    Path(name): Path<String>,
    MutationJson(m): MutationJson<Value>,
) -> ApiResult<Json<Value>> {
    let reason = st.policy().reason_optional(m.reason.as_ref())?;
    let trail = AuditTrail::begin(
        &st.ops.audit,
        &who,
        flyff_db::audit::ev::ADMIN_JOB,
        reason,
        json!({"action": "run", "job": name}),
    )?;
    let out = match st
        .ops
        .game(AdminCmd::RunJob(RunJob { name: name.clone() }), who.actor())
        .await
    {
        Ok(o) => o,
        Err(e) => {
            trail.failed(&e.to_string());
            return Err(e.into());
        }
    };
    match out {
        AdminOutput::Failed(f) => {
            trail.failed(&f.message);
            return Err(ApiError::from(&f));
        }
        AdminOutput::Error(msg) => {
            trail.failed(&msg);
            return Err(ApiError::invalid("name", msg));
        }
        _ => {}
    }
    trail.done(json!({}));
    let job = jobs(&st)
        .await?
        .iter()
        .find(|r| r.info.name == name)
        .map(row_json);
    Ok(Json(json!({"job": job})))
}
