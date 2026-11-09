//! The maintenance and shutdown page (Plan/25 §9.7; WP-ADMIN-26): `GET /admin/api/maintenance` says whether the
//! maintenance mode is on (`auth.maintenance`, read-only until **WP-AUTH-18** lands the toggle: `PUT` answers
//! `501 needs_feature`) and whether a supervisor was detected (`NOTIFY_SOCKET` / `INVOCATION_ID`), which is what
//! makes "restart" meaningful: `POST /admin/api/shutdown {restart: true}` exits with `exit::RESTART` so that a
//! supervisor with `Restart=on-failure` starts the server again.

use crate::admin::api::{ApiState, Routes};
use axum::Json;
use axum::extract::State;
use serde_json::{Value, json};

/// Mounts the route (`PUT /admin/api/maintenance` stays the catalogue's `501 needs_feature` stub).
pub fn mount(r: Routes<ApiState>) -> Routes<ApiState> {
    r.get("/admin/api/maintenance", state)
}

/// `GET /admin/api/maintenance`.
async fn state(State(st): State<ApiState>) -> Json<Value> {
    let settings = st.ops.core.settings();
    let supervisor = st.admin.supervisor;
    Json(json!({
        "available": false,
        "needs": "WP-AUTH-18",
        "enabled": settings.auth.maintenance,
        "allow_ips": st.ops.current.lock().auth.maintenance_allow_ips.clone(),
        "supervisor": {"detected": supervisor.is_some(), "kind": supervisor},
        "restart_available": supervisor.is_some(),
        "restart_pending": st.admin.restart_requested.load(std::sync::atomic::Ordering::Relaxed),
    }))
}
