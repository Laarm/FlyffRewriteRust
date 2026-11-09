//! `GET /healthz` (Plan/10 §12.3, Plan/24 R15): 200 while the game thread ticks, 503 otherwise. Moved onto the
//! router from the walking skeleton (WP-CORE-93); the body and the checks are unchanged.

use super::state::HttpState;
use axum::body::Body;
use axum::extract::State;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use flyff_common::rt::HealthState;
use std::sync::atomic::Ordering;

/// The handler.
pub async fn healthz(State(st): State<HttpState>) -> Response {
    let t = &st.core.telemetry;
    let healthy = t.is_healthy();
    let state: HealthState = t.health.state();
    let age = t
        .last_tick_age_ms()
        .map_or_else(|| "null".to_owned(), |a| a.to_string());
    let body = format!(
        "{{\"state\":\"{}\",\"tick\":{},\"last_tick_age_ms\":{},\"players\":{},\"db_degraded\":{},\"accepting_logins\":{}}}",
        state.name(),
        t.tick.load(Ordering::Relaxed),
        age,
        t.players_in_world.load(Ordering::Relaxed),
        st.core.persist.stats().degraded,
        t.health.accepting_logins.load(Ordering::Relaxed),
    );
    let status = if healthy {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::from(body))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}
