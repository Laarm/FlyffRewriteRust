//! JSON renderings of the admin payloads (Plan/10 §13.1 "HTTP renders `AdminOutput` as JSON"): the shapes the
//! `/admin` page, the `/admin/api/*` routes and the tests read.

use flyff_game::input::{AdminOutput, EventEntry, OnlineEntry, RateEntry, StatusSnapshot};
use serde_json::{Value, json};

/// `StatusSnapshot` as the fields of `GET /admin/api/status`.
pub fn status_json(s: &StatusSnapshot) -> Value {
    json!({
        "tick": s.tick,
        "uptime_s": s.uptime_s,
        "in_world": s.in_world,
        "lobby": s.lobby,
        "tick_p50_us": s.tick_p50_us,
        "tick_p99_us": s.tick_p99_us,
        "db_degraded": s.db_degraded,
        "db_queue": s.db_queue,
        "shutdown_in_s": s.shutdown_in_s,
    })
}

/// One online player.
pub fn online_json(e: &OnlineEntry) -> Value {
    json!({
        "session": e.session.0,
        "account_id": e.account_id.0,
        "character_id": e.character_id.0,
        "name": e.name,
        "level": e.level,
        "job": e.job,
        "world": e.world,
        "layer": e.layer,
        "pos": e.pos,
        "authority": e.authority.to_string(),
        "ip": e.ip,
        "in_world_since": e.in_world_since.0,
        "lingering": e.lingering,
    })
}

/// An `f32` as the JSON number a person typed: the shortest decimal that reads back as the same `f32`
/// (0.2, not 0.20000000298023224).
fn f32_json(v: f32) -> Value {
    v.to_string()
        .parse::<f64>()
        .ok()
        .and_then(serde_json::Number::from_f64)
        .map_or(Value::Null, Value::Number)
}

/// One rate, with the range it may be set to (`min` is excluded when `min_exclusive`; 23 §8.5).
pub fn rate_json(r: &RateEntry) -> Value {
    let range = flyff_game::overrides::RateKey::find(r.key);
    json!({
        "key": r.key,
        "config_key": r.config_key,
        "data_default": f32_json(r.data_default),
        "toml": r.toml.map(f32_json),
        "overridden": r.overridden.map(f32_json),
        "effective": f32_json(r.effective),
        "min": range.map(|k| f32_json(k.min)),
        "max": range.map(|k| f32_json(k.max)),
        "min_exclusive": range.map(|k| k.min_exclusive),
    })
}

/// One event flag.
pub fn event_json(e: &EventEntry) -> Value {
    json!({
        "id": e.id,
        "key": e.key,
        "state": e.state,
        "overridden": e.overridden,
    })
}

/// An [`AdminOutput`] as JSON, externally tagged (`{"Lines": [...]}`, `{"Done": {"affected": 3}}`, ...).
pub fn output_json(o: &AdminOutput) -> Value {
    match o {
        AdminOutput::Lines(l) => json!({"Lines": l}),
        AdminOutput::Online(rows) => {
            json!({"Online": rows.iter().map(online_json).collect::<Vec<_>>()})
        }
        AdminOutput::Status(s) => json!({"Status": status_json(s)}),
        AdminOutput::Rates(rows) => {
            json!({"Rates": rows.iter().map(rate_json).collect::<Vec<_>>()})
        }
        AdminOutput::Events(rows) => {
            json!({"Events": rows.iter().map(event_json).collect::<Vec<_>>()})
        }
        AdminOutput::Done { affected } => json!({"Done": {"affected": affected}}),
        AdminOutput::Digest { tick, digest } => {
            json!({"Digest": {"tick": tick, "digest": format!("{digest:032x}")}})
        }
        AdminOutput::Error(e) => json!({"Error": e}),
        AdminOutput::Failed(f) => json!({"Failed": {"code": f.code.name(), "message": f.message}}),
        // `AdminOutput` is non-exhaustive: a later variant is at least visible
        other => json!({"Other": format!("{other:?}")}),
    }
}

/// An [`AdminOutput`] as the text lines of the console.
pub fn output_lines(o: &AdminOutput) -> Vec<String> {
    match o {
        AdminOutput::Lines(l) => l.clone(),
        AdminOutput::Online(rows) => online_table(rows),
        AdminOutput::Status(s) => vec![status_line(s)],
        AdminOutput::Rates(rows) => rates_table(rows),
        AdminOutput::Events(rows) => events_table(rows),
        AdminOutput::Done { affected } => vec![format!("done ({affected} affected)")],
        AdminOutput::Digest { tick, digest } => vec![format!("tick {tick}: {digest:032x}")],
        AdminOutput::Error(e) => vec![format!("error: {e}")],
        AdminOutput::Failed(f) => vec![format!("error: {f}")],
        other => vec![format!("{other:?}")],
    }
}

/// The one-line status of the console word `status`.
pub fn status_line(s: &StatusSnapshot) -> String {
    format!(
        "tick {} | uptime {} s | in world {} | lobby {} | tick p50 {:.1} ms p99 {:.1} ms | db queue {}{}{}",
        s.tick,
        s.uptime_s,
        s.in_world,
        s.lobby,
        f64::from(s.tick_p50_us) / 1000.0,
        f64::from(s.tick_p99_us) / 1000.0,
        s.db_queue,
        if s.db_degraded { " | DB DEGRADED" } else { "" },
        s.shutdown_in_s
            .map_or_else(String::new, |n| format!(" | shutdown in {n} s")),
    )
}

/// Fixed-width table of the in-world characters (23 §8.1 `online`).
pub fn online_table(rows: &[OnlineEntry]) -> Vec<String> {
    let mut out = vec![format!(
        "{:<18} {:>8} {:>4} {:>4} {:>6} {:>5} {:>4} {:<15}",
        "name", "account", "lvl", "job", "world", "layer", "auth", "ip"
    )];
    for e in rows {
        out.push(format!(
            "{:<18} {:>8} {:>4} {:>4} {:>6} {:>5} {:>4} {:<15}{}",
            e.name,
            e.account_id.0,
            e.level,
            e.job,
            e.world,
            e.layer,
            e.authority,
            e.ip,
            if e.lingering { "  (lingering)" } else { "" },
        ));
    }
    out.push(format!("{} online", rows.len()));
    out
}

/// The rates as a table.
pub fn rates_table(rows: &[RateEntry]) -> Vec<String> {
    let mut out = vec![format!(
        "{:<11} {:<26} {:>8} {:>8} {:>10} {:>9}",
        "key", "config key", "default", "toml", "override", "effective"
    )];
    for r in rows {
        let opt = |v: Option<f32>| v.map_or_else(|| "-".to_owned(), |v| format!("{v}"));
        out.push(format!(
            "{:<11} {:<26} {:>8} {:>8} {:>10} {:>9}",
            r.key,
            r.config_key,
            r.data_default,
            opt(r.toml),
            opt(r.overridden),
            r.effective
        ));
    }
    out
}

/// The event flags as a table.
pub fn events_table(rows: &[EventEntry]) -> Vec<String> {
    let mut out = vec![format!("{:>4} {:<24} {:>5} {}", "id", "key", "state", "")];
    for e in rows {
        out.push(format!(
            "{:>4} {:<24} {:>5} {}",
            e.id,
            e.key.unwrap_or("-"),
            e.state,
            if e.overridden { "(override)" } else { "" }
        ));
    }
    out
}
