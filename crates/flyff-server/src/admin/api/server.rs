//! The server pages of the console (Plan/25 §9; WP-ADMIN-25, WP-ADMIN-26): the dashboard feeds, the configuration, the
//! scheduler, the backups and the maintenance page.
//!
//! | route | cap | does | file |
//! |---|---|---|---|
//! | `GET /admin/api/history?res=10s\|1m\|auto&span=` | `view.dashboard` | the metrics history as columns, plus the cards that are not time series (`current`) | [`dash`] |
//! | `GET /admin/api/alerts` | `view.dashboard` | the active alerts, the fired/cleared history, the thresholds | [`dash`] |
//! | `GET /admin/api/activity?limit=&kind=` | `view.dashboard` | the last admin actions and notable audit events | [`dash`] |
//! | `GET /admin/api/config` | `act.server` | every key with its effective value, source and class (`live` / `reload` / `restart`) | [`config`] |
//! | `PUT /admin/api/config/{key}`, `DELETE ...` | `act.server` | `SetRuntime` / `UnsetRuntime` of a live key, audited as `admin.config` | [`config`] |
//! | `GET /admin/api/jobs`, `POST /admin/api/jobs/{name}/run` | `act.server` | the scheduler's jobs (`Query::ListJobs`), "run now" (`AdminCmd::RunJob`) | [`jobs`] |
//! | `GET/POST /admin/api/backups`, `POST .../{name}/verify`, `GET .../{name}/download` | `act.server` | list, backup now, verify, download (P; loopback unless `admin.allow_backup_download_remote`) | [`backups`] |
//! | `GET/PUT /admin/api/maintenance` | `act.server` | maintenance state, supervisor detection; the toggle needs WP-AUTH-18 | [`maintenance`] |
//!
//! Every mutation writes `requested` before it dispatches and `done` / `failed` after (`AuditTrail`); an
//! unacknowledged `requested` record answers `503 audit_unavailable` and nothing is dispatched.

pub mod backups;
pub mod config;
pub mod dash;
pub mod jobs;
pub mod maintenance;

use super::{ApiState, Routes};
use crate::admin::error::{ApiError, ApiResult};
use crate::admin::ops::AdminOps;
use flyff_common::rt::reply_pair;
use flyff_game::GameInput;
use flyff_game::admin::queries::{AdminQuery, AdminQueryResult};
use flyff_game::admin::types::AdminConnId;
use flyff_game::admin::views::AdminViewOp;
use std::time::Duration;

/// Mounts the routes of this area.
pub fn mount(r: Routes<ApiState>) -> Routes<ApiState> {
    let r = dash::mount(r);
    let r = config::mount(r);
    let r = jobs::mount(r);
    let r = backups::mount(r);
    maintenance::mount(r)
}

/// How long a query waits for room in the game inbox.
const SEND_TIMEOUT: Duration = Duration::from_secs(2);
/// How long a query waits for the answer (it is answered within the tick).
const ANSWER_TIMEOUT: Duration = Duration::from_secs(5);

/// Asks the game thread a read-only question (`GameInput::AdminView`, not recorded) and waits for the answer. A
/// failed query is the API error of its failure.
pub(super) async fn game_query(ops: &AdminOps, q: AdminQuery) -> ApiResult<AdminQueryResult> {
    let (reply, rx) = reply_pair();
    let input = GameInput::AdminView {
        conn: AdminConnId(0),
        op: AdminViewOp::Query { q, reply },
    };
    match tokio::time::timeout(SEND_TIMEOUT, ops.game_tx.send_async(input)).await {
        Ok(Ok(())) => {}
        Ok(Err(_)) => return Err(ApiError::unavailable("the game thread is not running")),
        Err(_) => {
            return Err(ApiError::unavailable(
                "the game thread did not answer in time",
            ));
        }
    }
    match tokio::time::timeout(ANSWER_TIMEOUT, rx.recv_async()).await {
        Ok(Ok(AdminQueryResult::Failed(f))) => Err(ApiError::from(&f)),
        Ok(Ok(out)) => Ok(out),
        Ok(Err(_)) => Err(ApiError::unavailable("the game thread is not running")),
        Err(_) => Err(ApiError::unavailable(
            "the game thread did not answer in time",
        )),
    }
}

/// `true` when a process supervisor started the server (`NOTIFY_SOCKET` / `INVOCATION_ID` of systemd): what makes
/// "restart" meaningful (Plan/25 §9.7). `get` reads the environment (tests inject theirs).
pub fn detect_supervisor(get: impl Fn(&str) -> Option<String>) -> Option<&'static str> {
    let set = |k: &str| get(k).is_some_and(|v| !v.trim().is_empty());
    if set("NOTIFY_SOCKET") || set("INVOCATION_ID") {
        Some("systemd")
    } else {
        None
    }
}

/// The supervisor of the running process, from the real environment.
pub fn supervisor_of_process() -> Option<&'static str> {
    detect_supervisor(|k| std::env::var(k).ok())
}
