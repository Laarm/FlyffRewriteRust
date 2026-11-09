//! The configuration page (Plan/25 §9.5; WP-ADMIN-26): `GET /admin/api/config` is 10's projection of the effective
//! configuration (key, value, source, class), `PUT` / `DELETE /admin/api/config/{key}` change a *live* key through
//! `AdminCmd::SetRuntime` / `UnsetRuntime` and write the audit record `admin.config` (`key`, `before`, `after`).
//!
//! | class | keys | how it changes |
//! |---|---|---|
//! | `live` | the runtime keys: `[rates]`, `[events]`, `events.flag.<id>` | here (persisted as a `runtime_override` row) |
//! | `reload` | `log.level`, `net.ip_block`, the live `[admin]` and `[auth]` keys, ... | edit `flyff.toml`, then `POST /admin/api/reload` |
//! | `restart` | everything else | edit `flyff.toml` and restart: the row carries the TOML snippet to copy |
//!
//! `source` is `default`, `file` (`flyff.toml`), `env`, `cli`, `options` or `override` (a runtime change: the
//! `runtime_override` row, with who set it). The "restart required" diff against the file on disk is
//! `GET /admin/api/reload` (`AdminOps::reload_preview`, WP-OPS-16).

use crate::admin::api::{ApiState, Routes};
use crate::admin::auth::Who;
use crate::admin::dto::ErrorCode;
use crate::admin::dto::Mutation;
use crate::admin::error::{ApiError, ApiResult};
use crate::admin::guard::{AuditTrail, MutationJson};
use crate::config::keys::{self, KeyInfo, Kind};
use crate::config::{self, ValueSource};
use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, State};
use flyff_game::input::{AdminCmd, AdminOutput, EventEntry, RateEntry};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// Mounts the routes.
pub fn mount(r: Routes<ApiState>) -> Routes<ApiState> {
    r.get("/admin/api/config", list)
        .put("/admin/api/config/{key}", put)
        .delete("/admin/api/config/{key}", unset)
}

/// One row of the page.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Row {
    /// `rates.monster_exp`.
    pub key: String,
    /// `rates`.
    pub section: String,
    /// `live`, `reload` or `restart`.
    pub class: &'static str,
    /// `class == "live"`.
    pub editable: bool,
    /// `bool`, `int`, `float`, `str`, `str_array`, `table_array`, `table`.
    pub kind: &'static str,
    /// The effective value.
    pub value: Value,
    /// The documented default (`null` for a key that is unset by default).
    pub default: Value,
    /// Where the effective value comes from.
    pub source: &'static str,
    /// The detail of the source (`flyff.toml:12`, the variable, who set the override).
    pub source_detail: Option<String>,
    /// The accepted values in words.
    pub range: String,
    /// The description.
    pub doc: String,
    /// The owning plan document.
    pub owner: String,
    /// The TOML to put into `flyff.toml` for this key at its current value.
    pub snippet: String,
}

fn kind_name(k: Kind) -> &'static str {
    match k {
        Kind::Str => "str",
        Kind::Int => "int",
        Kind::Float => "float",
        Kind::Bool => "bool",
        Kind::StrArray => "str_array",
        Kind::TableArray => "table_array",
        Kind::Table => "table",
    }
}

fn class_of(info: &KeyInfo) -> &'static str {
    if info.runtime {
        "live"
    } else if info.reload == config::schema::Reload::Yes {
        "reload"
    } else {
        "restart"
    }
}

/// An `f32` as the JSON number a person typed (0.2, not 0.20000000298023224).
fn f32_json(v: f32) -> Value {
    v.to_string()
        .parse::<f64>()
        .ok()
        .and_then(serde_json::Number::from_f64)
        .map_or(Value::Null, Value::Number)
}

fn toml_json(v: &toml::Value) -> Value {
    serde_json::to_value(v).unwrap_or(Value::Null)
}

/// The TOML text of a key at a value: `[net.tls]\ncert = "x"`.
pub fn snippet(key: &str, value: &Value) -> String {
    let section = keys::section_of(key);
    let name = key
        .strip_prefix(section)
        .map_or(key, |n| n.trim_start_matches('.'));
    let text = match toml::Value::try_from(value) {
        Ok(v) => {
            // `toml` prints a bare value through a one-key table
            let mut t = toml::Table::new();
            t.insert("v".to_owned(), v);
            toml::to_string(&t)
                .ok()
                .and_then(|s| s.trim().strip_prefix("v = ").map(str::to_owned))
                .unwrap_or_else(|| value.to_string())
        }
        Err(_) => value.to_string(),
    };
    format!("[{section}]\n{name} = {text}")
}

fn parse_default(info: &KeyInfo) -> Value {
    info.default_text
        .and_then(|t| toml::from_str::<toml::Table>(&format!("v = {t}")).ok())
        .and_then(|t| t.get("v").map(toml_json))
        .unwrap_or(Value::Null)
}

fn source_of(sources: &BTreeMap<String, ValueSource>, key: &str) -> (&'static str, Option<String>) {
    match sources.get(key) {
        None | Some(ValueSource::Default) => ("default", None),
        Some(ValueSource::File { line }) => (
            "file",
            Some(line.map_or_else(|| "flyff.toml".to_owned(), |l| format!("flyff.toml:{l}"))),
        ),
        Some(ValueSource::Env { var }) => ("env", Some(var.clone())),
        Some(ValueSource::Cli { flag }) => ("cli", Some((*flag).to_owned())),
        Some(ValueSource::Override { set_by }) => ("options", Some(set_by.clone())),
    }
}

/// What the game thread says about the runtime keys.
struct Live {
    rates: Vec<RateEntry>,
    events: Vec<EventEntry>,
}

async fn live(st: &ApiState, who: &crate::admin::identity::Identity) -> ApiResult<Live> {
    let rates = match st.ops.game(AdminCmd::GetRates, who.actor()).await? {
        AdminOutput::Rates(r) => r,
        _ => Vec::new(),
    };
    let events = match st.ops.game(AdminCmd::GetEvents, who.actor()).await? {
        AdminOutput::Events(e) => e,
        _ => Vec::new(),
    };
    Ok(Live { rates, events })
}

/// The projection: every key of the table, then the override-only `events.flag.<id>` rows.
async fn rows(st: &ApiState, who: &crate::admin::identity::Identity) -> ApiResult<Vec<Row>> {
    let cfg = st.ops.current.lock().clone();
    let src = st.ops.reload_source.clone();
    let sources = tokio::task::spawn_blocking(move || {
        config::load(&src.home, &src.cli, &src.env, &src.overrides)
            .map(|l| l.sources)
            .unwrap_or_default()
    })
    .await
    .unwrap_or_default();
    let live = live(st, who).await?;
    let mut out = Vec::new();
    for info in keys::all_keys() {
        let class = class_of(info);
        let mut value = cfg
            .value_of(info.key)
            .map_or(Value::Null, |v| toml_json(&v));
        let (mut source, mut detail) = source_of(&sources, info.key);
        if let Some(r) = live.rates.iter().find(|r| r.config_key == info.key) {
            value = f32_json(r.effective);
            if r.overridden.is_some() {
                (source, detail) = ("override", Some("runtime_override".to_owned()));
            }
        } else if let Some(e) = live.events.iter().find(|e| e.key == Some(info.key)) {
            value = json!(e.state);
            if e.overridden {
                (source, detail) = ("override", Some("runtime_override".to_owned()));
            }
        }
        out.push(Row {
            key: info.key.to_owned(),
            section: keys::section_of(info.key).to_owned(),
            class,
            editable: class == "live",
            kind: kind_name(info.kind),
            snippet: snippet(info.key, &value),
            value,
            default: parse_default(info),
            source,
            source_detail: detail,
            range: info.range.describe(),
            doc: info.doc.to_owned(),
            owner: info.owner.to_owned(),
        });
    }
    for e in live
        .events
        .iter()
        .filter(|e| e.key.is_none() && e.overridden)
    {
        let key = format!("events.flag.{}", e.id);
        out.push(Row {
            section: "events.flag".to_owned(),
            class: "live",
            editable: true,
            kind: "int",
            snippet: String::new(),
            value: json!(e.state),
            default: Value::Null,
            source: "override",
            source_detail: Some("runtime_override".to_owned()),
            range: "0..=255".to_owned(),
            doc: format!(
                "Event flag {} (override only: it has no flyff.toml key).",
                e.id
            ),
            owner: "22".to_owned(),
            key,
        });
    }
    Ok(out)
}

/// `GET /admin/api/config`.
async fn list(State(st): State<ApiState>, Who(who): Who) -> ApiResult<Json<Value>> {
    let rows = rows(&st, &who).await?;
    let count = |c: &str| rows.iter().filter(|r| r.class == c).count();
    let overrides = rows.iter().filter(|r| r.source == "override").count();
    Ok(Json(json!({
        "counts": {"live": count("live"), "reload": count("reload"), "restart": count("restart")},
        "overrides": overrides,
        "persist_overrides": st.ops.current.lock().fidelity.persist_gm_overrides,
        "keys": rows,
    })))
}

/// The class of `key` for the mutation routes: `live` keys only.
fn require_live(key: &str) -> ApiResult<()> {
    // the override-only form
    if let Some(id) = key.strip_prefix("events.flag.") {
        return match id.parse::<u16>() {
            Ok(n) if n <= flyff_game::overrides::MAX_FLAG_ID => Ok(()),
            _ => Err(ApiError::new(
                ErrorCode::NotFound,
                format!("no such key: {key}"),
            )),
        };
    }
    let Some(info) = keys::key_info(key) else {
        return Err(ApiError::not_found(format!("no such key: {key}")));
    };
    match class_of(info) {
        "live" => Ok(()),
        "reload" => Err(ApiError::invalid(
            "key",
            format!("{key} is changed in flyff.toml: edit the file, then press Reload"),
        )),
        _ => Err(ApiError::invalid(
            "key",
            format!("{key} needs a restart: edit flyff.toml and restart the server"),
        )),
    }
}

#[derive(Deserialize)]
struct ValueBody {
    value: Value,
}

fn find(rows: &[Row], key: &str) -> Option<Row> {
    rows.iter().find(|r| r.key == key).cloned()
}

/// Runs one change: the audit trail around `SetRuntime` / `UnsetRuntime`, the updated row as the answer.
async fn change(
    st: &ApiState,
    who: &crate::admin::identity::Identity,
    key: &str,
    reason: Option<crate::admin::guard::ReasonText>,
    cmd: AdminCmd,
    after_value: Option<&Value>,
) -> ApiResult<Json<Value>> {
    let before = rows(st, who).await?;
    let old = find(&before, key);
    let trail = AuditTrail::begin(
        &st.ops.audit,
        who,
        flyff_db::audit::ev::ADMIN_CONFIG,
        reason,
        json!({
            "key": key,
            "before": old.as_ref().map(|r| r.value.clone()),
            "before_source": old.as_ref().map(|r| r.source),
            "requested": after_value,
        }),
    )?;
    let out = match st.ops.game(cmd, who.actor()).await {
        Ok(o) => o,
        Err(e) => {
            trail.failed(&e.to_string());
            return Err(e.into());
        }
    };
    match out {
        AdminOutput::Lines(_) | AdminOutput::Done { .. } => {}
        AdminOutput::Failed(f) => {
            trail.failed(&f.message);
            return Err(ApiError::from(&f));
        }
        AdminOutput::Error(msg) => {
            trail.failed(&msg);
            return Err(ApiError::invalid("value", msg));
        }
        _ => {}
    }
    let after = rows(st, who).await?;
    let new = find(&after, key);
    trail.done(json!({
        "after": new.as_ref().map(|r| r.value.clone()),
        "after_source": new.as_ref().map(|r| r.source),
    }));
    Ok(Json(json!({"row": new})))
}

/// `PUT /admin/api/config/{key}` `{body: {value}, reason?}`.
async fn put(
    State(st): State<ApiState>,
    Who(who): Who,
    Path(key): Path<String>,
    MutationJson(m): MutationJson<ValueBody>,
) -> ApiResult<Json<Value>> {
    require_live(&key)?;
    let reason = st.policy().reason_optional(m.reason.as_ref())?;
    let value = m.body.value;
    change(
        &st,
        &who,
        &key,
        reason,
        AdminCmd::SetRuntime {
            key: key.clone(),
            value: value.clone(),
        },
        Some(&value),
    )
    .await
}

/// `DELETE /admin/api/config/{key}`: removes the runtime override (the file value is effective again).
async fn unset(
    State(st): State<ApiState>,
    Who(who): Who,
    Path(key): Path<String>,
    body: Bytes,
) -> ApiResult<Json<Value>> {
    require_live(&key)?;
    // the envelope is optional on a DELETE: `{body: {}, reason?}`
    let reason = if body.iter().all(u8::is_ascii_whitespace) {
        None
    } else {
        let m: Mutation<Value> = serde_json::from_slice(&body)
            .map_err(|e| ApiError::bad_request(format!("malformed body: {e}")))?;
        st.policy().reason_optional(m.reason.as_ref())?
    };
    change(
        &st,
        &who,
        &key,
        reason,
        AdminCmd::UnsetRuntime { key: key.clone() },
        None,
    )
    .await
}
