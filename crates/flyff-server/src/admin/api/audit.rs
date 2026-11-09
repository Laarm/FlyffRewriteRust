//! `GET /admin/api/audit` and `/audit/export` (Plan/25 §10.2; WP-ADMIN-29): the audit viewer over
//! `logs/audit/audit-YYYY-MM-DD.jsonl` and its per-day index ([`crate::admin::audit_index`]).
//!
//! **Filters** (query string): `from` / `to` (Unix milliseconds or seconds, or RFC 3339; default the last 24 h), `groups`
//! (comma list of `account character item economy guild gm admin server`), `events` (comma list of event names), `actor`
//! (`staff:joe`), `character`, `account`, `serial`, `q` (free text), `mine=1` (own actions only), `cursor`, `limit`
//! (default 100, at most 500).
//!
//! **Who sees what**
//!
//! * `view.audit` (N) sees the whole log.
//! * `view.audit.own` (L) sees only its own actions: the actor filter is forced to the identity's label and the events
//!   to `admin.*` and `gm.command` ("My actions", with Undo, Plan/25 §2.5).
//! * A *subject lookup* (`character=` or `account=`: the History and Logins tabs of the character and account drawers)
//!   needs `view.inventory` (M) or `view.audit`.
//! * IPs and e-mail addresses are removed from every row and from the export unless the identity holds `view.pii`
//!   (Plan/25 §12 rule 12).
//!
//! **Export** streams the matches as JSONL or CSV (at most [`EXPORT_MAX`] lines), one stream per identity at a time.

use crate::admin::api::{ApiState, Routes};
use crate::admin::audit_index::{
    AuditStore, Cursor, EventFilter, Filter, Hit, Page, group_of, redact_pii, summarize,
};
use crate::admin::auth::Who;
use crate::admin::caps::Cap;
use crate::admin::error::{ApiError, ApiResult};
use crate::admin::identity::Identity;
use axum::Json;
use axum::body::{Body, Bytes};
use axum::extract::{Query, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;

/// The most lines one export streams.
pub const EXPORT_MAX: usize = 100_000;
/// Rows per page at most.
pub const PAGE_MAX: usize = 500;
/// Rows per page by default.
pub const PAGE_DEFAULT: usize = 100;
/// The default window of a query.
const DEFAULT_SPAN_MS: i64 = 86_400_000;

/// Mounts the routes.
pub fn mount(r: Routes<ApiState>) -> Routes<ApiState> {
    r.get("/admin/api/audit", list)
        .get("/admin/api/audit/export", export)
}

/// The query string of both routes.
#[derive(Debug, Default, Deserialize)]
pub struct AuditQuery {
    from: Option<String>,
    to: Option<String>,
    groups: Option<String>,
    events: Option<String>,
    actor: Option<String>,
    character: Option<u32>,
    account: Option<u32>,
    serial: Option<u64>,
    q: Option<String>,
    /// `1` or `true`: only the identity's own actions.
    mine: Option<String>,
    cursor: Option<String>,
    limit: Option<usize>,
    /// `jsonl` (default) or `csv` (export only).
    fmt: Option<String>,
}

/// Unix milliseconds of a query-string time: a number (seconds below 10^11, else milliseconds) or RFC 3339.
pub fn parse_time(text: &str) -> Option<i64> {
    let t = text.trim();
    if let Ok(n) = t.parse::<i64>() {
        return Some(if n.abs() < 100_000_000_000 {
            n * 1000
        } else {
            n
        });
    }
    chrono::DateTime::parse_from_rfc3339(t)
        .ok()
        .map(|d| d.timestamp_millis())
}

fn csv_list(text: &Option<String>) -> Vec<String> {
    text.as_deref()
        .unwrap_or("")
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

/// The store of this server (created at the first use).
fn store(st: &ApiState) -> Arc<AuditStore> {
    Arc::clone(st.admin.audit_store.get_or_init(|| {
        let cfg = st.ops.current.lock().clone();
        let dir = cfg.resolve_paths(&st.ops.reload_source.home).audit;
        Arc::new(AuditStore::new(dir))
    }))
}

/// The filter of a request for `who`, with the rules of "who sees what".
fn filter_of(st: &ApiState, who: &Identity, q: &AuditQuery, now_ms: i64) -> ApiResult<Filter> {
    let caps = &st.admin.caps;
    let full = who.has(caps, Cap::ViewAudit);
    let to_ms = match q.to.as_deref() {
        Some(t) => parse_time(t).ok_or_else(|| ApiError::invalid("to", "not a time"))?,
        None => now_ms.saturating_add(1),
    };
    let from_ms = match q.from.as_deref() {
        Some(t) => parse_time(t).ok_or_else(|| ApiError::invalid("from", "not a time"))?,
        None => to_ms.saturating_sub(DEFAULT_SPAN_MS),
    };
    if from_ms >= to_ms {
        return Err(ApiError::invalid("from", "from must be before to"));
    }
    let mut events = EventFilter::of_groups(&csv_list(&q.groups));
    events.names = csv_list(&q.events);
    let mut f = Filter {
        from_ms,
        to_ms,
        events,
        actor: q.actor.clone().filter(|a| !a.trim().is_empty()),
        character: q.character,
        account: q.account,
        serial: q.serial,
        text: q.q.clone().filter(|t| !t.trim().is_empty()),
    };
    let subject_lookup = q.character.is_some() || q.account.is_some();
    let mine = q.mine.as_deref().is_some_and(|m| matches!(m, "1" | "true"));
    if full {
        // the whole log; `mine=1` only adds the actor
        if mine {
            f.actor = Some(who.label());
        }
    } else if subject_lookup && !mine {
        // History / Logins of a character or an account: inventory-level staff may look at the subject
        who.require(caps, Cap::ViewInventory)?;
    } else {
        // "My actions": the identity's own admin actions, nothing else
        who.require(caps, Cap::ViewAuditOwn)?;
        f.actor = Some(who.label());
        f.events = EventFilter::of_groups(&["admin".to_owned()]);
        f.character = None;
        f.account = None;
    }
    Ok(f)
}

/// One row of the table.
fn row_json(hit: &Hit, pii: bool) -> Value {
    let mut raw: Value =
        serde_json::from_str(&hit.line).unwrap_or_else(|_| json!({"raw": hit.line}));
    if !pii {
        redact_pii(&mut raw);
    }
    let data = raw.get("data").cloned().unwrap_or(Value::Null);
    let text = |k: &str| data.get(k).and_then(Value::as_str).map(str::to_owned);
    let ev = hit.keys.ev.as_str();
    json!({
        "id": Cursor { day: hit.day, offset: hit.offset }.encode(),
        "ts": hit.ts_ms,
        "ev": ev,
        "group": group_of(ev),
        "actor": text("actor").or_else(|| text("by")),
        "state": text("state"),
        "rid": text("rid"),
        "reason": text("reason"),
        "summary": summarize(ev, &data),
        "subject": {
            "characters": hit.keys.chrs,
            "accounts": hit.keys.accs,
            "serials": hit.keys.serials,
        },
        "raw": raw,
    })
}

/// `GET /admin/api/audit`.
async fn list(
    State(st): State<ApiState>,
    Who(who): Who,
    Query(q): Query<AuditQuery>,
) -> ApiResult<Json<Value>> {
    let now_ms = st.admin.clock.unix_ms();
    let f = filter_of(&st, &who, &q, now_ms)?;
    let limit = q.limit.unwrap_or(PAGE_DEFAULT).clamp(1, PAGE_MAX);
    let cursor = match q.cursor.as_deref() {
        Some(c) => {
            Some(Cursor::decode(c).ok_or_else(|| ApiError::invalid("cursor", "not a cursor"))?)
        }
        None => None,
    };
    let pii = who.has(&st.admin.caps, Cap::ViewPii);
    let store = store(&st);
    let started = std::time::Instant::now();
    let query = f.clone();
    let page: Page =
        tokio::task::spawn_blocking(move || store.query(&query, cursor, limit, now_ms, true))
            .await
            .map_err(|e| ApiError::internal(&e.to_string()))?
            .map_err(|e| {
                tracing::error!(error = %e, "audit query failed");
                ApiError::unavailable("the audit log could not be read")
            })?;
    Ok(Json(json!({
        "items": page.hits.iter().map(|h| row_json(h, pii)).collect::<Vec<_>>(),
        "next_cursor": page.next.map(|c| c.encode()),
        "indexed": page.indexed,
        "examined": page.examined,
        "took_ms": u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        "from": f.from_ms,
        "to": f.to_ms,
        "own_only": f.actor.as_deref() == Some(who.label().as_str()),
    })))
}

/// The CSV header of the export.
pub const CSV_HEADER: &str = "time,event,group,actor,state,reason,summary,characters,accounts,json";

fn csv_field(s: &str) -> String {
    // a formula injected into a spreadsheet starts with one of these (OWASP CSV injection)
    let guard = matches!(s.chars().next(), Some('=' | '+' | '-' | '@' | '\t' | '\r'));
    let body = s.replace('"', "\"\"");
    if guard {
        format!("\"'{body}\"")
    } else {
        format!("\"{body}\"")
    }
}

/// One CSV line of a row (no trailing newline).
pub fn csv_line(row: &Value) -> String {
    let get = |k: &str| row.get(k).and_then(Value::as_str).unwrap_or("");
    let ids = |k: &str| {
        row["subject"][k]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|v| v.to_string())
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_default()
    };
    let ts = row.get("ts").and_then(Value::as_i64).unwrap_or(0);
    let iso = chrono::DateTime::from_timestamp_millis(ts)
        .map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
        .unwrap_or_default();
    [
        iso,
        get("ev").to_owned(),
        get("group").to_owned(),
        get("actor").to_owned(),
        get("state").to_owned(),
        get("reason").to_owned(),
        get("summary").to_owned(),
        ids("characters"),
        ids("accounts"),
        row["raw"].to_string(),
    ]
    .iter()
    .map(|f| csv_field(f))
    .collect::<Vec<_>>()
    .join(",")
}

/// `GET /admin/api/audit/export?fmt=jsonl|csv&...`.
async fn export(
    State(st): State<ApiState>,
    Who(who): Who,
    Query(q): Query<AuditQuery>,
) -> ApiResult<Response> {
    let now_ms = st.admin.clock.unix_ms();
    let f = filter_of(&st, &who, &q, now_ms)?;
    let csv = match q.fmt.as_deref() {
        None | Some("jsonl") => false,
        Some("csv") => true,
        Some(other) => {
            return Err(ApiError::invalid(
                "fmt",
                format!("'{other}' is not jsonl or csv"),
            ));
        }
    };
    let permit = st.admin.streams.acquire(&who.limit_key()).ok_or_else(|| {
        ApiError::rate_limited(2, "another export or stream of yours is still running")
    })?;
    let pii = who.has(&st.admin.caps, Cap::ViewPii);
    let store = store(&st);
    let (tx, mut rx) = tokio::sync::mpsc::channel::<Result<Bytes, std::io::Error>>(8);
    let name = format!(
        "audit-{}-{}.{}",
        f.from_ms / 1000,
        f.to_ms.min(now_ms) / 1000,
        if csv { "csv" } else { "jsonl" }
    );
    tracing::info!(actor = %who.label(), name, "audit export started");
    tokio::task::spawn_blocking(move || {
        let _permit = permit; // held for the whole stream
        let mut sent = 0_usize;
        let mut cursor = None;
        if csv {
            let head = format!("{CSV_HEADER}\n");
            if tx.blocking_send(Ok(Bytes::from(head))).is_err() {
                return;
            }
        }
        while sent < EXPORT_MAX {
            let page = match store.query(&f, cursor, 1000.min(EXPORT_MAX - sent), now_ms, true) {
                Ok(p) => p,
                Err(e) => {
                    let _ = tx.blocking_send(Err(e));
                    return;
                }
            };
            let mut chunk = String::new();
            for hit in &page.hits {
                let row = row_json(hit, pii);
                if csv {
                    chunk.push_str(&csv_line(&row));
                } else {
                    chunk.push_str(&row["raw"].to_string());
                }
                chunk.push('\n');
                sent += 1;
            }
            if !chunk.is_empty() && tx.blocking_send(Ok(Bytes::from(chunk))).is_err() {
                return; // the client went away
            }
            match page.next {
                Some(c) => cursor = Some(c),
                None => return,
            }
        }
    });
    let stream = futures_util::stream::poll_fn(move |cx| rx.poll_recv(cx));
    let mut resp = (StatusCode::OK, Body::from_stream(stream)).into_response();
    let h = resp.headers_mut();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(if csv {
            "text/csv; charset=utf-8"
        } else {
            "application/x-ndjson; charset=utf-8"
        }),
    );
    if let Ok(v) = HeaderValue::from_str(&format!("attachment; filename=\"{name}\"")) {
        h.insert(header::CONTENT_DISPOSITION, v);
    }
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(resp)
}
