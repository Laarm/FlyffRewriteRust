//! The dashboard feeds (Plan/25 §9.1, §9.2; WP-ADMIN-25): `GET /admin/api/history`, `/alerts` and `/activity`.

use crate::admin::api::{ApiState, Routes};
use crate::admin::error::{ApiError, ApiResult};
use crate::admin::history::Res;
use axum::Json;
use axum::extract::{Query, State};
use serde::Deserialize;
use serde_json::{Value, json};

/// Mounts the three routes.
pub fn mount(r: Routes<ApiState>) -> Routes<ApiState> {
    r.get("/admin/api/history", history)
        .get("/admin/api/alerts", alerts)
        .get("/admin/api/activity", activity)
}

#[derive(Deserialize)]
struct HistoryQuery {
    /// `10s`, `1m` or `auto` (the default: the resolution that covers `span`).
    res: Option<String>,
    /// Seconds back from now (60..=86400; default 3600).
    span: Option<i64>,
}

/// `GET /admin/api/history?res=&span=`: the samples as columns, and `current`, the cards that are not time series.
async fn history(
    State(st): State<ApiState>,
    Query(q): Query<HistoryQuery>,
) -> ApiResult<Json<Value>> {
    let dash = &st.admin.dash;
    let now = st.admin.clock.unix().0;
    let span = q.span.unwrap_or(3600).clamp(60, 86_400);
    let res = match q.res.as_deref() {
        None | Some("auto") => Res::for_span(span),
        Some(other) => Res::parse(other).ok_or_else(|| {
            ApiError::invalid(
                "res",
                format!("'{other}' is not a resolution (10s, 1m, auto)"),
            )
        })?,
    };
    let columns = dash.history.window(res, Some(now - span));
    let current = dash.current();
    let (fine, coarse) = dash.history.counts();
    Ok(Json(json!({
        "res": res,
        "step_s": res.step_s(),
        "span_s": span,
        "now": now,
        // history is lost on restart: the chart says since when it has data
        "since_start_s": (now - current.started_unix).max(0),
        "points": {"10s": fine, "1m": coarse},
        "columns": columns,
        "current": current,
    })))
}

/// `GET /admin/api/alerts`: the active alerts, the fired and cleared events and the thresholds.
async fn alerts(State(st): State<ApiState>) -> Json<Value> {
    let dash = &st.admin.dash;
    Json(json!({
        "active": dash.alerts.active(),
        "recent": dash.alerts.recent(),
        "thresholds": dash.alerts.config(),
        "evaluated_at": dash.current().sampled_at,
        "now": st.admin.clock.unix().0,
    }))
}

#[derive(Deserialize)]
struct ActivityQuery {
    limit: Option<usize>,
    /// `admin` (only `admin.*` and `gm.command`) or `all` (the default: with the notable events of the server).
    kind: Option<String>,
}

/// `GET /admin/api/activity?limit=&kind=`: the newest first.
async fn activity(State(st): State<ApiState>, Query(q): Query<ActivityQuery>) -> Json<Value> {
    let limit = q.limit.unwrap_or(20).clamp(1, 200);
    let admin_only = q.kind.as_deref() == Some("admin");
    Json(json!({
        "items": st.admin.dash.activity.recent(limit, admin_only),
        "now_ms": st.admin.clock.unix_ms(),
    }))
}
