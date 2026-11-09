//! `GET /metrics`: the Prometheus text exposition (Plan/10 §12.2; WP-OPS-10 completes the catalogue).
//!
//! The text is built from [`Telemetry`] (tick, gateway, health, start-up steps), `PersistHandle::stats()`,
//! the HTTP counters, the asset framework's telemetry snapshot, the gateway statistics, the log writer and
//! `/proc/self`. The route sits behind the admin authorization of §13.2 ([`crate::admin::AdminAuth`]):
//! loopback without forwarding headers, or `admin.allow_remote` with the bearer token.
//!
//! Every metric carries `# HELP` and `# TYPE` lines; labelled families that have no sample yet still print
//! their header. WP-OPS-10 added the gateway sessions per state (`flyff_sessions{state}`), the upgrade
//! refusals (`flyff_ws_upgrades_rejected_total{reason}`), the rolling tick quantiles, the movement counters of
//! 18 §17 and the lobby statistics of 12 §19.2. WP-OPS-10 (runtime-obs-panic) completed the M2 set: the per-phase
//! timings of the tick (`flyff_tick_phase_*`), the world frame histograms (`flyff_world_phase_seconds{phase}`), the inputs
//! by kind, the object and layer gauges, the snapshots by reason, the correction counters and the database commit and
//! flush-wait histograms. Metrics of §12.2 whose source does not exist yet (the Lua host, per-message-type
//! counters) are added by WP-OPS-18 and the owners of those sources.

use crate::admin::AdminAuth;
use crate::assets::AssetsState;
use crate::gateway::pipeline::DropReason;
use crate::gateway::{GatewayShared, GatewayState};
use crate::http::HttpTelemetry;
use crate::lobby::LobbyCore;
use crate::logging::LogHandles;
use axum::Router;
use axum::extract::State;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use flyff_assets::JobResult;
use flyff_assets::kinds::AssetKind;
use flyff_common::rt::KickReason;
use flyff_common::rt::telemetry::{
    FINE_BUCKET_BOUNDS_NS, FINE_BUCKETS, FineSnapshot, HIST_BUCKETS, HistogramSnapshot,
    OBJECT_KINDS, PHASE_BUCKET_BOUNDS_NS, PHASE_BUCKETS, PhaseSnapshot, SAVE_REASONS,
    TICK_BUCKET_BOUNDS_NS,
};
use flyff_common::rt::{HealthState, InputKind, Telemetry, TickPhase, WorldPhase};
use flyff_db::PersistHandle;
use std::fmt::Write as _;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Instant;

/// Build identity labels of `flyff_build_info`.
#[derive(Clone, Debug)]
pub struct BuildLabels {
    /// The executable's `BUILD_ID`.
    pub version: String,
    /// `PROTO_HASH`.
    pub proto_hash: String,
    /// The embedded web build id.
    pub web_build: String,
}

/// Everything `/metrics` reads.
pub struct MetricsSources {
    /// Shared counters.
    pub telemetry: Arc<Telemetry>,
    /// Persistence statistics.
    pub persist: PersistHandle,
    /// HTTP counters.
    pub http: Arc<HttpTelemetry>,
    /// The asset framework.
    pub assets: Arc<AssetsState>,
    /// The gateway (statistics); `None` in HTTP-only tests.
    pub gateway: Option<Arc<GatewayShared>>,
    /// The lobby (login, registration, takeover, re-attach, character counters, the online registry); `None` in
    /// HTTP-only tests.
    pub lobby: Option<Arc<LobbyCore>>,
    /// The admin console (sessions, counters of Plan/25 §11.9); `None` in HTTP-only tests.
    pub admin: Option<Arc<crate::admin::shared::AdminShared>>,
    /// The log writer (dropped lines).
    pub log: Arc<LogHandles>,
    /// Build identity.
    pub build: BuildLabels,
    /// Process start, Unix seconds.
    pub started_unix_s: u64,
    /// Process start, monotonic.
    pub started: Instant,
}

/// The `/metrics` router behind the admin authorization.
pub fn router(sources: Arc<MetricsSources>, auth: &Arc<AdminAuth>) -> Router {
    let r = Router::new()
        .route("/metrics", get(handler))
        .with_state(sources);
    auth.layer(r)
}

async fn handler(State(src): State<Arc<MetricsSources>>) -> Response {
    // Rendering reads `/proc` and a few directory entries: keep it off the I/O workers' hot path.
    let text = tokio::task::spawn_blocking(move || render(&src)).await;
    match text {
        Ok(body) => (
            [
                (
                    header::CONTENT_TYPE,
                    HeaderValue::from_static("text/plain; version=0.0.4; charset=utf-8"),
                ),
                (header::CACHE_CONTROL, HeaderValue::from_static("no-store")),
            ],
            body,
        )
            .into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// Builds the exposition text.
pub fn render(src: &MetricsSources) -> String {
    let mut w = Writer::default();
    render_process(&mut w, src);
    render_tick(&mut w, &src.telemetry);
    render_runtime(&mut w, &src.telemetry);
    render_movement(&mut w, &src.telemetry);
    render_net(&mut w, src);
    render_lobby(&mut w, src);
    render_db(&mut w, src);
    render_admin(&mut w, src);
    render_assets(&mut w, &src.assets);
    w.counter(
        "flyff_log_dropped_lines_total",
        "Log lines dropped by the non-blocking file writer.",
        src.log.dropped_lines(),
    );
    let mut out = w.out;
    src.http.render_prometheus(&mut out);
    out
}

fn render_process(w: &mut Writer, src: &MetricsSources) {
    let b = &src.build;
    w.header("flyff_build_info", "Build identity (always 1).", "gauge");
    w.sample(
        "flyff_build_info",
        &[
            ("version", &b.version),
            ("proto_hash", &b.proto_hash),
            ("web_build", &b.web_build),
        ],
        1,
    );
    w.gauge(
        "flyff_start_time_seconds",
        "Process start time, seconds since the Unix epoch.",
        src.started_unix_s,
    );
    w.gauge(
        "flyff_uptime_seconds",
        "Seconds since the process started.",
        src.started.elapsed().as_secs(),
    );
    w.header(
        "flyff_startup_step_seconds",
        "Duration of the start-up steps S1..S12 (Plan/10 3.1).",
        "gauge",
    );
    for (i, ns) in src.telemetry.startup_ns.iter().enumerate() {
        let v = ns.load(Ordering::Relaxed);
        w.sample_f(
            "flyff_startup_step_seconds",
            &[("step", &format!("s{}", i + 1))],
            v as f64 / 1e9,
        );
    }
    let health = src.telemetry.health.state();
    w.gauge(
        "flyff_health_running",
        "1 while the health state is running or shutting_down.",
        u64::from(matches!(
            health,
            HealthState::Running | HealthState::ShuttingDown
        )),
    );
    let p = ProcStats::read();
    w.gauge(
        "process_resident_memory_bytes",
        "Resident set size.",
        p.rss_bytes,
    );
    w.header(
        "process_cpu_seconds_total",
        "User plus system CPU time.",
        "counter",
    );
    w.sample_f("process_cpu_seconds_total", &[], p.cpu_seconds);
    w.gauge("process_open_fds", "Open file descriptors.", p.open_fds);
    w.gauge("process_threads", "OS threads of the process.", p.threads);
}

fn render_tick(w: &mut Writer, t: &Telemetry) {
    w.histogram(
        "flyff_tick_seconds",
        "Game tick duration (phases 1-5).",
        &[],
        &t.tick_hist.snapshot(),
    );
    w.header(
        "flyff_tick_overruns_total",
        "Ticks slower than 66.7 ms (warn) or 100 ms (error).",
        "counter",
    );
    w.sample(
        "flyff_tick_overruns_total",
        &[("severity", "warn")],
        t.overruns_warn.load(Ordering::Relaxed),
    );
    w.sample(
        "flyff_tick_overruns_total",
        &[("severity", "error")],
        t.overruns_error.load(Ordering::Relaxed),
    );
    w.counter(
        "flyff_tick_resyncs_total",
        "Re-anchored (late) ticks.",
        t.resyncs.load(Ordering::Relaxed),
    );
    w.gauge_f(
        "flyff_tick_p50_seconds",
        "Median tick duration of the last 900 ticks.",
        t.tick_p50_ns.load(Ordering::Relaxed) as f64 / 1e9,
    );
    w.gauge_f(
        "flyff_tick_p99_seconds",
        "99th percentile tick duration of the last 900 ticks.",
        t.tick_p99_ns.load(Ordering::Relaxed) as f64 / 1e9,
    );
    w.gauge_f(
        "flyff_tick_lag_seconds",
        "Lateness of the last tick start.",
        t.lag_ns.load(Ordering::Relaxed) as f64 / 1e9,
    );
    w.gauge(
        "flyff_tick",
        "Index of the current tick.",
        t.tick.load(Ordering::Relaxed),
    );
    w.gauge(
        "flyff_game_inbox_depth",
        "Length of the game inbox at the start of the last tick.",
        t.inbox_depth.load(Ordering::Relaxed),
    );
    w.gauge(
        "flyff_players_in_world",
        "Players in the world.",
        t.players_in_world.load(Ordering::Relaxed),
    );
    w.gauge(
        "flyff_players_lingering",
        "Players lingering without a socket.",
        t.players_lingering.load(Ordering::Relaxed),
    );
    w.gauge(
        "flyff_sched_timers",
        "Pending scheduler one-shots, rows and wall jobs.",
        t.sched_timers.load(Ordering::Relaxed),
    );
    w.counter(
        "flyff_outbound_batches_total",
        "Server batches handed to the sessions.",
        t.out_batches.load(Ordering::Relaxed),
    );
    w.counter(
        "flyff_outbound_bytes_total",
        "Bytes of those batches.",
        t.out_bytes.load(Ordering::Relaxed),
    );
    w.counter(
        "flyff_slow_consumer_total",
        "Sessions closed because their outbound queue was full.",
        t.slow_consumers.load(Ordering::Relaxed),
    );
    w.header("flyff_panics_total", "Caught panics by scope.", "counter");
    for (scope, c) in ["player", "multiparty", "unscoped", "task"]
        .iter()
        .zip(&t.panics)
    {
        w.sample(
            "flyff_panics_total",
            &[("scope", scope)],
            c.load(Ordering::Relaxed),
        );
    }
}

fn render_net(w: &mut Writer, src: &MetricsSources) {
    let t = &src.telemetry;
    w.gauge(
        "flyff_ws_sessions",
        "Open WebSocket sessions.",
        t.sessions.load(Ordering::Relaxed),
    );
    w.counter(
        "flyff_inbound_msgs_total",
        "Accepted client messages.",
        t.inbound_msgs.load(Ordering::Relaxed),
    );
    w.counter(
        "flyff_inbound_bytes_total",
        "Bytes of accepted client messages.",
        t.inbound_bytes.load(Ordering::Relaxed),
    );
    let Some(gw) = &src.gateway else {
        return;
    };
    w.header("flyff_sessions", "Gateway sessions by state.", "gauge");
    let mut per_state = [0u64; 5];
    for info in gw.directory.list() {
        let st = info.handle.meta().state();
        if let Some(i) = GatewayState::ALL.iter().position(|s| *s == st)
            && let Some(c) = per_state.get_mut(i)
        {
            *c += 1;
        }
    }
    for (st, n) in GatewayState::ALL.iter().zip(per_state) {
        w.sample("flyff_sessions", &[("state", st.name())], n);
    }
    w.header(
        "flyff_ws_upgrades_rejected_total",
        "WebSocket upgrades refused before the handshake, by reason.",
        "counter",
    );
    let lim = &gw.ip_limiter.counters;
    for (reason, n) in [
        ("blocked", src.http.ws_blocked_total()),
        (
            "origin",
            gw.stats.upgrades_rejected_origin.load(Ordering::Relaxed),
        ),
        ("rate", lim.rate.load(Ordering::Relaxed)),
        ("conns", lim.conns.load(Ordering::Relaxed)),
        (
            "state",
            gw.stats.upgrades_rejected_state.load(Ordering::Relaxed),
        ),
    ] {
        w.sample("flyff_ws_upgrades_rejected_total", &[("reason", reason)], n);
    }
    w.counter(
        "flyff_ws_upgrades_total",
        "WebSocket upgrades that passed the per-IP limits.",
        lim.granted.load(Ordering::Relaxed),
    );
    w.counter(
        "flyff_admin_token_failures_total",
        "Failed admin-token attempts charged to the per-IP upgrade bucket.",
        lim.charged.load(Ordering::Relaxed),
    );
    let snap = gw.stats.snapshot();
    w.header(
        "flyff_inbound_dropped_total",
        "Client messages dropped before routing, by reason.",
        "counter",
    );
    for r in DropReason::ALL {
        w.sample(
            "flyff_inbound_dropped_total",
            &[("reason", r.label())],
            snap.dropped.get(r as usize).copied().unwrap_or(0),
        );
    }
    w.header(
        "flyff_kicks_total",
        "Sessions kicked, by reason.",
        "counter",
    );
    for r in KickReason::ALL {
        let n = snap.kicks.get(usize::from(r.as_u8())).copied().unwrap_or(0);
        w.sample("flyff_kicks_total", &[("reason", &format!("{r:?}"))], n);
    }
}

/// The per-phase timings and the game-side gauges of Plan/10 §12.2 (WP-OPS-10). Every labelled family prints its
/// header and one sample per label value, also before the first tick.
fn render_runtime(w: &mut Writer, t: &Telemetry) {
    w.header(
        "flyff_tick_phase_seconds_total",
        "Time spent in each top-level phase of the tick: input, worlds, timers, flush, post.",
        "counter",
    );
    for p in TickPhase::ALL {
        let ns = t
            .phase_ns
            .get(p.index())
            .map_or(0, |c| c.sum.load(Ordering::Relaxed));
        w.sample_f(
            "flyff_tick_phase_seconds_total",
            &[("phase", p.name())],
            ns as f64 / 1e9,
        );
    }
    w.header(
        "flyff_tick_phase_count",
        "Ticks in which each top-level phase ran (the divisor of flyff_tick_phase_seconds_total).",
        "counter",
    );
    for p in TickPhase::ALL {
        let n = t
            .phase_ns
            .get(p.index())
            .map_or(0, |c| c.count.load(Ordering::Relaxed));
        w.sample("flyff_tick_phase_count", &[("phase", p.name())], n);
    }
    w.header(
        "flyff_world_phase_seconds",
        "Duration of each world sub-phase per tick, summed over the layers; observed on every tick, 0 for a silent phase (finer buckets than the tick: 3 ms is an edge).",
        "histogram",
    );
    for p in WorldPhase::ALL {
        if let Some(h) = t.world_phase_hist.get(p.index()) {
            w.phase_body(
                "flyff_world_phase_seconds",
                &[("phase", p.name())],
                &h.snapshot(),
            );
        }
    }
    w.header(
        "flyff_game_inputs_total",
        "Inputs dispatched by the game thread, by kind (the GameInput variants and the persistence replies).",
        "counter",
    );
    for k in InputKind::ALL {
        let n = t
            .inputs
            .get(k.index())
            .map_or(0, |c| c.load(Ordering::Relaxed));
        w.sample("flyff_game_inputs_total", &[("kind", k.name())], n);
    }
    w.header(
        "flyff_objects",
        "Live objects by kind, recounted once per second.",
        "gauge",
    );
    for (kind, c) in OBJECT_KINDS.iter().zip(&t.objects) {
        w.sample(
            "flyff_objects",
            &[("kind", kind)],
            c.load(Ordering::Relaxed),
        );
    }
    w.gauge(
        "flyff_layers",
        "Live layers over all worlds.",
        t.layers.load(Ordering::Relaxed),
    );
    w.counter(
        "flyff_outbound_msgs_total",
        "Server messages appended to the session queues.",
        t.out_msgs.load(Ordering::Relaxed),
    );
    w.header(
        "flyff_saves_total",
        "Character snapshots handed to persistence, by reason (logout includes the shutdown snapshots, shutdown is that subset).",
        "counter",
    );
    for (reason, c) in SAVE_REASONS.iter().zip(&t.saves) {
        w.sample(
            "flyff_saves_total",
            &[("reason", reason)],
            c.load(Ordering::Relaxed),
        );
    }
    // derived from the movement counters (the game thread owns those): no field of their own in Telemetry
    let m = &t.movement;
    let guard = flyff_game::movement::IgnoreReason::Guard as usize;
    let set_pos = flyff_game::movement::PendingIntent::SetPos.kind();
    w.header(
        "flyff_mover_corrections_total",
        "Server-side corrections of movers: NPC MoverCorr, SetPos warps and the 1 km move guard (D9).",
        "counter",
    );
    for (kind, n) in [
        ("movercorr", m.corrections.load(Ordering::Relaxed)),
        (
            "setpos",
            m.intents
                .get(set_pos)
                .map_or(0, |c| c.load(Ordering::Relaxed)),
        ),
        (
            "move_guard",
            m.ignored
                .get(guard)
                .map_or(0, |c| c.load(Ordering::Relaxed)),
        ),
    ] {
        w.sample("flyff_mover_corrections_total", &[("kind", kind)], n);
    }
    w.counter(
        "flyff_npc_keyframes_total",
        "MoverKeyframe messages produced (periodic, settle and dev).",
        m.keyframes.iter().map(|c| c.load(Ordering::Relaxed)).sum(),
    );
}

/// The movement counters of 18 §17.
fn render_movement(w: &mut Writer, t: &Telemetry) {
    const INTENTS: [&str; 12] = [
        "dest_pos",
        "dest_obj",
        "dest_angle",
        "move_pattern",
        "speed_factor",
        "set_pos",
        "corr",
        "push",
        "act_msg",
        "motion",
        "force_pose",
        "motion_arrive",
    ];
    const KEYFRAMES: [&str; 3] = ["periodic", "settle", "dev"];
    let m = &t.movement;
    w.header(
        "flyff_move_states_total",
        "Accepted PlayerMoveState messages by reason.",
        "counter",
    );
    for (i, c) in m.states.iter().enumerate() {
        let reason = i32::try_from(i)
            .ok()
            .and_then(|n| flyff_proto::pb::MoveReason::try_from(n).ok())
            .map_or("unknown", |r| r.as_str_name());
        w.sample(
            "flyff_move_states_total",
            &[("reason", reason)],
            c.load(Ordering::Relaxed),
        );
    }
    w.header(
        "flyff_move_ignored_total",
        "Movement messages ignored, by reason.",
        "counter",
    );
    for (r, c) in flyff_game::movement::IgnoreReason::ALL
        .iter()
        .zip(&m.ignored)
    {
        w.sample(
            "flyff_move_ignored_total",
            &[("reason", r.label())],
            c.load(Ordering::Relaxed),
        );
    }
    w.header(
        "flyff_move_requests_total",
        "Accepted click-to-move style requests.",
        "counter",
    );
    for (kind, v) in [
        ("dest_pos", m.dest_pos_reqs.load(Ordering::Relaxed)),
        ("dest_obj", m.dest_obj_reqs.load(Ordering::Relaxed)),
        ("motion_accepted", m.motion_reqs[0].load(Ordering::Relaxed)),
        ("motion_rejected", m.motion_reqs[1].load(Ordering::Relaxed)),
    ] {
        w.sample("flyff_move_requests_total", &[("kind", kind)], v);
    }
    w.header(
        "flyff_npc_intents_total",
        "NPC movement intents captured, by kind.",
        "counter",
    );
    for (name, c) in INTENTS.iter().zip(&m.intents) {
        w.sample(
            "flyff_npc_intents_total",
            &[("kind", name)],
            c.load(Ordering::Relaxed),
        );
    }
    w.header(
        "flyff_keyframes_total",
        "NPC keyframes sent, by kind.",
        "counter",
    );
    for (name, c) in KEYFRAMES.iter().zip(&m.keyframes) {
        w.sample(
            "flyff_keyframes_total",
            &[("kind", name)],
            c.load(Ordering::Relaxed),
        );
    }
    w.counter(
        "flyff_npc_corrections_total",
        "NPC collision and drop corrections.",
        m.corrections.load(Ordering::Relaxed),
    );
    w.counter(
        "flyff_sim_frames_total",
        "step_frame calls of the world frame loop.",
        m.sim_frames.load(Ordering::Relaxed),
    );
    w.counter(
        "flyff_sim_events_total",
        "Sim events dispatched at the frame barriers.",
        m.events.load(Ordering::Relaxed),
    );
    w.gauge(
        "flyff_movers_awake",
        "Awake movers of the last processed layer pass.",
        m.awake.load(Ordering::Relaxed),
    );
    w.counter(
        "flyff_move_sanitised_total",
        "Frames that reset a non-finite position or velocity.",
        m.sanitised.load(Ordering::Relaxed),
    );
}

/// The lobby statistics of 12 §19.2 and the audit counters.
fn render_lobby(w: &mut Writer, src: &MetricsSources) {
    let Some(core) = &src.lobby else {
        return;
    };
    let st = &core.stats;
    let counters = |w: &mut Writer, name: &str, help: &str, rows: Vec<(&'static str, u64)>| {
        w.header(name, help, "counter");
        for (label, n) in rows {
            w.sample(name, &[("result", label)], n);
        }
    };
    counters(
        w,
        "flyff_lobby_login_total",
        "Logins by result (ok or the error code of 12 §16.1).",
        st.logins(),
    );
    counters(
        w,
        "flyff_lobby_register_total",
        "Registrations by result.",
        st.registers(),
    );
    counters(
        w,
        "flyff_lobby_takeover_total",
        "Forced-login takeovers by result.",
        st.takeovers(),
    );
    counters(
        w,
        "flyff_lobby_reattach_total",
        "Re-attach attempts of a Hello for an avatar in the world.",
        st.reattaches(),
    );
    counters(
        w,
        "flyff_lobby_char_create_total",
        "Character creations by result (ok or the LobbyError code).",
        st.char_creates(),
    );
    counters(
        w,
        "flyff_lobby_char_delete_total",
        "Character deletions by result (ok or the LobbyError code).",
        st.char_deletes(),
    );
    w.gauge(
        "flyff_online_accounts",
        "Accounts with an entry in the online registry.",
        core.registry.len() as u64,
    );
    w.gauge(
        "flyff_sessions_in_world",
        "Registry entries entering or in the world.",
        u64::from(core.registry.in_world()),
    );
    w.gauge(
        "flyff_session_linger_active",
        "Avatars in the world without a socket (the linger of 12 §12.2).",
        src.telemetry.players_lingering.load(Ordering::Relaxed),
    );
    w.histogram(
        "flyff_lobby_hash_wait_seconds",
        "Time a password hash waited for one of the two hashing permits.",
        &[],
        &st.hash_wait.snapshot(),
    );
    w.histogram(
        "flyff_lobby_hash_seconds",
        "Duration of one password hash computation.",
        &[],
        &st.hash.snapshot(),
    );
    w.histogram(
        "flyff_lobby_enter_world_seconds",
        "EnterWorld accepted until Join was sent.",
        &[],
        &st.enter_world.snapshot(),
    );
    w.histogram(
        "flyff_lobby_flush_wait_seconds",
        "Waits for the previous logout save to commit (13 §19 step 2).",
        &[],
        &st.flush_wait.snapshot(),
    );
    let audit = core.audit.stats();
    w.counter(
        "flyff_audit_dropped_total",
        "Audit events lost: queue full, line too long or write failure.",
        audit.dropped,
    );
    w.counter(
        "flyff_audit_lines_total",
        "Audit lines written.",
        audit.written,
    );
    w.gauge(
        "flyff_audit_queue_depth",
        "Audit events waiting for the writer thread.",
        audit.queued as u64,
    );
}

/// The metrics of the admin console (Plan/25 §11.9, WP-ADMIN-02) and the admin counters of the persistence layer.
fn render_admin(w: &mut Writer, src: &MetricsSources) {
    use crate::admin::metrics::{FRAME_BYTES_BOUNDS, TIERS};
    let s = src.persist.stats();
    w.gauge(
        "flyff_admin_heavy_reads_running",
        "1 while an admin whole-database scan (item search, duplicates, economy) holds the server-wide permit.",
        u64::from(src.persist.admin_heavy_scan_running()),
    );
    w.counter(
        "flyff_db_admin_chunks_total",
        "Chunks of admin scans run by the reader threads.",
        s.admin_chunks,
    );
    w.counter(
        "flyff_db_admin_rows_total",
        "Rows examined by admin scans.",
        s.admin_rows,
    );
    w.header(
        "flyff_db_admin_scans_total",
        "Admin scans by outcome.",
        "counter",
    );
    for (result, n) in [
        ("done", s.admin_scans),
        ("failed", s.admin_scans_failed),
        ("cancelled", s.admin_scans_cancelled),
        ("rejected", s.admin_scans_rejected),
    ] {
        w.sample("flyff_db_admin_scans_total", &[("result", result)], n);
    }
    w.counter(
        "flyff_admin_map_visits_total",
        "Entities the admin map collector visited (cells count 1/16).",
        src.telemetry.admin_map_visits.load(Ordering::Relaxed),
    );
    w.fine_histogram(
        "flyff_admin_views_seconds",
        "Game-thread time of the admin views' post phase per tick that had an admin connection (Plan/25 3.17).",
        &src.telemetry.admin_views_hist.snapshot(),
    );
    let Some(admin) = &src.admin else {
        return;
    };
    let counts = admin.sessions.counts();
    w.header(
        "flyff_admin_sessions",
        "Live admin console sessions by kind.",
        "gauge",
    );
    for (kind, n) in ["staff", "owner_token", "owner_local"].iter().zip(counts) {
        w.sample("flyff_admin_sessions", &[("kind", kind)], n as u64);
    }
    let m = admin.metrics.snapshot();
    w.gauge(
        "flyff_admin_ws_connections",
        "Open admin console WebSockets.",
        m.ws_connections,
    );
    w.header(
        "flyff_admin_map_subscriptions",
        "Live map subscriptions by detail tier.",
        "gauge",
    );
    for (tier, n) in TIERS.iter().zip(m.map_subscriptions) {
        w.sample("flyff_admin_map_subscriptions", &[("tier", tier)], n);
    }
    w.header(
        "flyff_admin_map_frames_total",
        "Map frames sent by detail tier.",
        "counter",
    );
    for (tier, n) in TIERS.iter().zip(m.map_frames) {
        w.sample("flyff_admin_map_frames_total", &[("tier", tier)], n);
    }
    w.header(
        "flyff_admin_map_frame_bytes",
        "Size of the map frames in bytes.",
        "histogram",
    );
    let mut cumulative = 0u64;
    for (i, n) in m.frame_buckets.iter().enumerate() {
        cumulative += n;
        let le = FRAME_BYTES_BOUNDS
            .get(i)
            .map_or_else(|| "+Inf".to_owned(), ToString::to_string);
        w.sample(
            "flyff_admin_map_frame_bytes_bucket",
            &[("le", &le)],
            cumulative,
        );
    }
    w.sample("flyff_admin_map_frame_bytes_sum", &[], m.frame_bytes_sum);
    w.sample("flyff_admin_map_frame_bytes_count", &[], m.frame_count);
    w.header(
        "flyff_admin_actions_total",
        "Admin console actions by action and result.",
        "counter",
    );
    for ((action, result), n) in &m.actions {
        w.sample(
            "flyff_admin_actions_total",
            &[("action", action), ("result", result)],
            *n,
        );
    }
    w.header(
        "flyff_admin_login_failures_total",
        "Refused admin console logins (wrong password, wrong token, banned account, authority below L); remote = not from the host itself.",
        "counter",
    );
    for (remote, n) in ["false", "true"].iter().zip(m.login_failures) {
        w.sample("flyff_admin_login_failures_total", &[("remote", remote)], n);
    }
}

fn render_db(w: &mut Writer, src: &MetricsSources) {
    let persist = &src.persist;
    let s = persist.stats();
    w.gauge(
        "flyff_db_queue_depth",
        "Commands waiting in the persistence writer queue.",
        s.queue_depth as u64,
    );
    w.counter(
        "flyff_db_batches_total",
        "Transactions committed by the writer.",
        s.batches,
    );
    w.counter(
        "flyff_db_snapshots_written_total",
        "Character snapshots written.",
        s.snapshots_written,
    );
    w.counter(
        "flyff_db_snapshots_stale_total",
        "Snapshots skipped by the generation guard.",
        s.snapshots_stale,
    );
    w.counter(
        "flyff_db_anomalies_total",
        "Grants or removals closed twice (logged as CRITICAL; the batch is not rolled back).",
        s.anomalies,
    );
    w.counter(
        "flyff_db_maintenance_runs_total",
        "Daily retention runs that finished.",
        s.maintenance_runs,
    );
    w.counter(
        "flyff_db_maintenance_failures_total",
        "Daily retention runs that failed (the job is idempotent and runs again).",
        s.maintenance_failures,
    );
    w.gauge(
        "flyff_db_degraded",
        "1 while the writer is in transient-retry mode.",
        u64::from(s.degraded),
    );
    w.gauge_f(
        "flyff_db_last_batch_commit_seconds",
        "Duration of the last commit batch.",
        s.last_commit_us as f64 / 1e6,
    );
    w.histogram(
        "flyff_db_batch_commit_seconds",
        "Duration of every committed batch of the writer (BEGIN to COMMIT; the 50 ms p99 budget of 60 12.4).",
        &[],
        &persist.commit_histogram(),
    );
    // the same observations as `flyff_lobby_flush_wait_seconds` under the name of 13 §22
    let flush_wait = src.lobby.as_ref().map_or_else(
        || flyff_common::rt::Histogram::default().snapshot(),
        |core| core.stats.flush_wait.snapshot(),
    );
    w.histogram(
        "flyff_db_flush_wait_seconds",
        "Waits for the previous logout save to commit (13 19 step 2); alias of flyff_lobby_flush_wait_seconds.",
        &[],
        &flush_wait,
    );
    w.counter(
        "flyff_db_checkpoint_busy_total",
        "WAL checkpoints that reported busy.",
        s.checkpoint_busy,
    );
    w.gauge(
        "flyff_db_wal_bytes",
        "Size of the WAL file after the last checkpoint.",
        s.wal_bytes,
    );
    w.gauge_f(
        "flyff_db_checkpoint_seconds",
        "Duration of the last WAL checkpoint.",
        s.last_checkpoint_us as f64 / 1e6,
    );
    let b = persist.backup_stats();
    w.gauge(
        "flyff_db_last_backup_timestamp_seconds",
        "Unix time of the last successful backup (0 = none yet).",
        u64::try_from(b.last_ok_unix).unwrap_or(0),
    );
    w.counter(
        "flyff_db_backups_total",
        "Successful database backups.",
        b.total,
    );
    w.counter(
        "flyff_db_backup_failures_total",
        "Failed database backups.",
        b.failures,
    );
    w.counter(
        "flyff_db_backup_skipped_total",
        "Scheduled backups skipped because the previous one still ran.",
        b.skipped,
    );
    w.counter(
        "flyff_db_backup_hook_failures_total",
        "Post-backup hooks that failed to start, exited non-zero or timed out.",
        b.hook_failures,
    );
    w.gauge_f(
        "flyff_db_last_backup_seconds",
        "Duration of the last backup.",
        b.last_duration_ms as f64 / 1e3,
    );
}

fn render_assets(w: &mut Writer, assets: &AssetsState) {
    let snap = assets.prep.telemetry_snapshot();
    w.header(
        "flyff_asset_jobs_total",
        "Asset jobs finished, by kind and result.",
        "counter",
    );
    for k in AssetKind::ALL {
        for (ri, r) in JobResult::ALL.into_iter().enumerate() {
            let n = snap
                .jobs
                .get(k.index())
                .and_then(|row| row.get(ri))
                .copied()
                .unwrap_or(0);
            if n > 0 {
                w.sample(
                    "flyff_asset_jobs_total",
                    &[("kind", k.slug()), ("result", r.label())],
                    n,
                );
            }
        }
    }
    w.header(
        "flyff_asset_queue_depth",
        "Queued asset jobs by priority.",
        "gauge",
    );
    for (name, n) in ["startup", "interactive", "prewarm"]
        .iter()
        .zip(snap.queue_depth)
    {
        w.sample("flyff_asset_queue_depth", &[("priority", name)], n);
    }
    w.header(
        "flyff_asset_build_seconds",
        "Asset build durations by kind.",
        "histogram",
    );
    for k in AssetKind::ALL {
        if let Some(h) = snap.build.get(k.index()).filter(|h| h.count > 0) {
            w.histogram_body("flyff_asset_build_seconds", &[("kind", k.slug())], h);
        }
    }
    let p = assets.prep.progress();
    w.header(
        "flyff_asset_prepare_done",
        "Jobs finished in the current or last asset wave.",
        "gauge",
    );
    w.sample("flyff_asset_prepare_done", &[("kind", "all")], p.done);
    w.header(
        "flyff_asset_prepare_total",
        "Jobs enqueued in the current or last asset wave.",
        "gauge",
    );
    w.sample("flyff_asset_prepare_total", &[("kind", "all")], p.total);
    w.gauge(
        "flyff_cache_bytes",
        "Size of cache/.",
        snap.cache_bytes.iter().sum(),
    );
    w.counter(
        "flyff_asset_write_errors_total",
        "Failed cache writes.",
        snap.write_errors,
    );
}

// ---- exposition helpers -----------------------------------------------------------------------------------

#[derive(Default)]
struct Writer {
    out: String,
}

impl Writer {
    fn header(&mut self, name: &str, help: &str, ty: &str) {
        let _ = writeln!(self.out, "# HELP {name} {help}");
        let _ = writeln!(self.out, "# TYPE {name} {ty}");
    }

    fn sample(&mut self, name: &str, labels: &[(&str, &str)], v: u64) {
        let _ = writeln!(self.out, "{name}{} {v}", fmt_labels(labels));
    }

    fn sample_f(&mut self, name: &str, labels: &[(&str, &str)], v: f64) {
        let _ = writeln!(self.out, "{name}{} {}", fmt_labels(labels), fmt_f(v));
    }

    fn gauge(&mut self, name: &str, help: &str, v: u64) {
        self.header(name, help, "gauge");
        self.sample(name, &[], v);
    }

    fn gauge_f(&mut self, name: &str, help: &str, v: f64) {
        self.header(name, help, "gauge");
        self.sample_f(name, &[], v);
    }

    fn counter(&mut self, name: &str, help: &str, v: u64) {
        self.header(name, help, "counter");
        self.sample(name, &[], v);
    }

    fn histogram(
        &mut self,
        name: &str,
        help: &str,
        labels: &[(&str, &str)],
        h: &HistogramSnapshot,
    ) {
        self.header(name, help, "histogram");
        self.histogram_body(name, labels, h);
    }

    fn fine_histogram(&mut self, name: &str, help: &str, h: &FineSnapshot) {
        self.header(name, help, "histogram");
        let mut cumulative = 0u64;
        for i in 0..FINE_BUCKETS {
            cumulative += h.buckets.get(i).copied().unwrap_or(0);
            let le = FINE_BUCKET_BOUNDS_NS
                .get(i)
                .map_or_else(|| "+Inf".to_owned(), |ns| fmt_f(*ns as f64 / 1e9));
            let _ = writeln!(
                self.out,
                "{name}_bucket{} {cumulative}",
                fmt_labels(&[("le", &le)])
            );
        }
        let _ = writeln!(self.out, "{name}_sum {}", fmt_f(h.sum_ns as f64 / 1e9));
        let _ = writeln!(self.out, "{name}_count {}", h.count);
    }

    fn histogram_body(&mut self, name: &str, labels: &[(&str, &str)], h: &HistogramSnapshot) {
        self.bucketed(
            name,
            labels,
            &TICK_BUCKET_BOUNDS_NS,
            HIST_BUCKETS,
            &h.buckets,
            (h.sum_ns, h.count),
        );
    }

    /// The series of a [`PhaseSnapshot`] histogram (the world-phase bounds); the `# HELP`/`# TYPE` header is the
    /// caller's, once per family.
    fn phase_body(&mut self, name: &str, labels: &[(&str, &str)], h: &PhaseSnapshot) {
        self.bucketed(
            name,
            labels,
            &PHASE_BUCKET_BOUNDS_NS,
            PHASE_BUCKETS,
            &h.buckets,
            (h.sum_ns, h.count),
        );
    }

    /// One histogram series: the cumulative `_bucket` lines (`le` last, after the series labels), `_sum` in seconds
    /// and `_count`. `bounds` has one entry fewer than `buckets`: the last bucket is `+Inf`.
    fn bucketed(
        &mut self,
        name: &str,
        labels: &[(&str, &str)],
        bounds: &[u64],
        n_buckets: usize,
        buckets: &[u64],
        (sum_ns, count): (u64, u64),
    ) {
        let mut cumulative = 0u64;
        for i in 0..n_buckets {
            cumulative += buckets.get(i).copied().unwrap_or(0);
            let le = bounds
                .get(i)
                .map_or_else(|| "+Inf".to_owned(), |ns| fmt_f(*ns as f64 / 1e9));
            let mut l: Vec<(&str, &str)> = labels.to_vec();
            l.push(("le", &le));
            let _ = writeln!(self.out, "{name}_bucket{} {cumulative}", fmt_labels(&l));
        }
        let _ = writeln!(
            self.out,
            "{name}_sum{} {}",
            fmt_labels(labels),
            fmt_f(sum_ns as f64 / 1e9)
        );
        let _ = writeln!(self.out, "{name}_count{} {}", fmt_labels(labels), count);
    }
}

fn fmt_f(v: f64) -> String {
    if v.is_finite() {
        format!("{v}")
    } else {
        "0".to_owned()
    }
}

fn fmt_labels(labels: &[(&str, &str)]) -> String {
    if labels.is_empty() {
        return String::new();
    }
    let mut s = String::from("{");
    for (i, (k, v)) in labels.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        let _ = write!(s, "{k}=\"{}\"", escape(v));
    }
    s.push('}');
    s
}

fn escape(v: &str) -> String {
    v.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

// ---- /proc/self -------------------------------------------------------------------------------------------

/// The process figures of the standard `process_*` metrics.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ProcStats {
    /// `VmRSS`.
    pub rss_bytes: u64,
    /// utime + stime.
    pub cpu_seconds: f64,
    /// Entries of `/proc/self/fd`.
    pub open_fds: u64,
    /// `Threads:`.
    pub threads: u64,
}

impl ProcStats {
    /// Reads `/proc/self` (zeros where a file is unreadable, e.g. on a non-Linux host).
    pub fn read() -> ProcStats {
        let mut s = ProcStats::default();
        if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
            for line in status.lines() {
                if let Some(v) = line.strip_prefix("VmRSS:") {
                    s.rss_bytes = parse_kb(v) * 1024;
                } else if let Some(v) = line.strip_prefix("Threads:") {
                    s.threads = v.trim().parse().unwrap_or(0);
                }
            }
        }
        if let Ok(stat) = std::fs::read_to_string("/proc/self/stat") {
            // The command name is in parentheses and may contain spaces: split after the last ')'.
            if let Some((_, rest)) = stat.rsplit_once(')') {
                let f: Vec<&str> = rest.split_whitespace().collect();
                // After the name the fields start at `state` (field 3): utime = field 14, stime = 15.
                let ticks = |i: usize| f.get(i).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
                s.cpu_seconds = (ticks(11) + ticks(12)) / 100.0;
            }
        }
        s.open_fds = std::fs::read_dir("/proc/self/fd").map_or(0, |d| d.count() as u64);
        s
    }
}

fn parse_kb(v: &str) -> u64 {
    v.split_whitespace()
        .next()
        .and_then(|n| n.parse().ok())
        .unwrap_or(0)
}
