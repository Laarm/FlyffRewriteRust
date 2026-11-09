//! The backups page (Plan/25 §9.7; WP-ADMIN-26):
//!
//! * `GET /admin/api/backups`: the files of `data/backups/` (size, time, kind: `sched`, `premigrate`, `manual`,
//!   `shutdown`, `prerestore`), the last verification of each, the free disk space and the exact restore command;
//! * `POST /admin/api/backups`: **backup now** (`PersistHandle::backup(Manual)`, the same online backup as the
//!   scheduled one);
//! * `POST /admin/api/backups/{name}/verify`: `integrity_check` of the file, its `application_id` and schema version
//!   (`flyff_db::backup::inspect_database`, read-only);
//! * `GET /admin/api/backups/{name}/download`: the file, **P only and only from the host itself** unless
//!   `admin.allow_backup_download_remote` is on. A refused download is audited too.
//!
//! Restore stays a command-line operation with the server stopped (13 §21.4): the page shows the command.

use crate::admin::api::{ApiState, Routes};
use crate::admin::auth::{Caller, Who};
use crate::admin::error::{ApiError, ApiResult};
use crate::admin::guard::{AuditTrail, MutationJson};
use crate::admin::history::Verified;
use axum::Json;
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use flyff_common::gm::Authority;
use flyff_db::BackupKind;
use flyff_db::backup::{BackupFile, inspect_database, list_backups};
use serde_json::{Value, json};
use std::path::PathBuf;

/// Mounts the routes.
pub fn mount(r: Routes<ApiState>) -> Routes<ApiState> {
    r.get("/admin/api/backups", list)
        .post("/admin/api/backups", create)
        .post("/admin/api/backups/{name}/verify", verify)
        .get("/admin/api/backups/{name}/download", download)
}

fn dir_of(st: &ApiState) -> PathBuf {
    let cfg = st.ops.current.lock().clone();
    cfg.resolve_paths(&st.ops.reload_source.home).backups
}

fn row(st: &ApiState, f: &BackupFile) -> Value {
    let size = std::fs::metadata(&f.path).map_or(0, |m| m.len());
    let verified = st.admin.dash.verified.lock().get(&f.name).cloned();
    json!({
        "name": f.name,
        "kind": f.kind,
        "at": f.at.0,
        "size_bytes": size,
        "verified": verified,
    })
}

/// The files of the backup directory, newest first.
async fn files(dir: PathBuf) -> ApiResult<Vec<BackupFile>> {
    tokio::task::spawn_blocking(move || list_backups(&dir))
        .await
        .map_err(|e| ApiError::internal(&e.to_string()))?
        .map(|mut v| {
            v.reverse();
            v
        })
        .map_err(|e| ApiError::db(&e))
}

/// `GET /admin/api/backups`.
async fn list(State(st): State<ApiState>) -> ApiResult<Json<Value>> {
    let dir = dir_of(&st);
    let list = files(dir.clone()).await?;
    let stats = st.ops.persist.backup_stats();
    let cfg = st.ops.current.lock().clone();
    let newest = list.first().map(|f| st.ops.admin.clock.unix().0 - f.at.0);
    Ok(Json(json!({
        "dir": dir.display().to_string(),
        "files": list.iter().map(|f| row(&st, f)).collect::<Vec<_>>(),
        "free_bytes": flyff_db::backup::free_bytes(&dir),
        "newest_age_s": newest,
        "running": stats.running,
        "last_ok_unix": (stats.last_ok_unix > 0).then_some(stats.last_ok_unix),
        "failures": stats.failures,
        "policy": {
            "interval_min": cfg.backup.interval_min,
            "keep_hourly": cfg.backup.keep_hourly,
            "keep_daily": cfg.backup.keep_daily,
            "keep_weekly": cfg.backup.keep_weekly,
        },
        // restore is a command-line operation with the server stopped (13 §21.4)
        "restore_command": "flyff db restore <file>   (stop the server first)",
        "download_remote_allowed": cfg.admin.allow_backup_download_remote,
    })))
}

/// `POST /admin/api/backups` `{body: {}, reason?}`: a manual backup now.
async fn create(
    State(st): State<ApiState>,
    Who(who): Who,
    MutationJson(m): MutationJson<Value>,
) -> ApiResult<Json<Value>> {
    let reason = st.policy().reason_optional(m.reason.as_ref())?;
    let trail = AuditTrail::begin(
        &st.ops.audit,
        &who,
        flyff_db::audit::ev::ADMIN_BACKUP,
        reason,
        json!({"action": "create"}),
    )?;
    match st.ops.persist.backup(BackupKind::Manual).await {
        Ok(path) => {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let size = std::fs::metadata(&path).map_or(0, |m| m.len());
            trail.done(json!({"name": name, "size_bytes": size}));
            let file = files(dir_of(&st))
                .await?
                .into_iter()
                .find(|f| f.name == name);
            Ok(Json(json!({"file": file.as_ref().map(|f| row(&st, f))})))
        }
        Err(e) => {
            trail.failed(&e.to_string());
            Err(ApiError::unavailable(format!("the backup failed: {e}")))
        }
    }
}

/// The listed file called `name` (a name that is not in the directory is `404`: no path ever comes from the client).
async fn find(st: &ApiState, name: &str) -> ApiResult<BackupFile> {
    files(dir_of(st))
        .await?
        .into_iter()
        .find(|f| f.name == name)
        .ok_or_else(|| ApiError::not_found(format!("no backup called {name}")))
}

/// `POST /admin/api/backups/{name}/verify`.
async fn verify(
    State(st): State<ApiState>,
    Who(who): Who,
    Path(name): Path<String>,
    MutationJson(m): MutationJson<Value>,
) -> ApiResult<Json<Value>> {
    let file = find(&st, &name).await?;
    let reason = st.policy().reason_optional(m.reason.as_ref())?;
    let trail = AuditTrail::begin(
        &st.ops.audit,
        &who,
        flyff_db::audit::ev::ADMIN_BACKUP,
        reason,
        json!({"action": "verify", "name": name}),
    )?;
    let path = file.path.clone();
    let result = tokio::task::spawn_blocking(move || inspect_database(&path))
        .await
        .map_err(|e| ApiError::internal(&e.to_string()))?;
    let now = st.admin.clock.unix().0;
    let verified = match &result {
        Ok(s) => Verified {
            at: now,
            ok: true,
            detail: format!(
                "integrity_check ok; schema {}, {} accounts, {} characters, {} guilds, newest save {}",
                s.user_version,
                s.accounts,
                s.characters,
                s.guilds,
                s.newest_save_text()
            ),
        },
        Err(e) => Verified {
            at: now,
            ok: false,
            detail: e.to_string(),
        },
    };
    st.admin
        .dash
        .verified
        .lock()
        .insert(file.name.clone(), verified.clone());
    if verified.ok {
        trail.done(json!({"ok": true}));
    } else {
        trail.failed(&verified.detail);
    }
    Ok(Json(json!({"name": file.name, "verified": verified})))
}

/// `GET /admin/api/backups/{name}/download`.
async fn download(
    State(st): State<ApiState>,
    Who(who): Who,
    caller: Caller,
    Path(name): Path<String>,
) -> ApiResult<Response> {
    let file = find(&st, &name).await?;
    let trail = AuditTrail::begin(
        &st.ops.audit,
        &who,
        flyff_db::audit::ev::ADMIN_BACKUP,
        None,
        json!({"action": "download", "name": file.name, "remote": !caller.local}),
    )?;
    // P only (owners count as P), and only from the host itself unless the live key allows it
    if !who.is_owner() && who.authority < Authority::ADMINISTRATOR {
        trail.failed("denied: downloading a backup needs authority P");
        return Err(ApiError::forbidden_text(
            "downloading a backup needs authority P",
        ));
    }
    if !caller.local && !st.ops.current.lock().admin.allow_backup_download_remote {
        trail.failed("denied: remote download is off");
        return Err(ApiError::forbidden_text(
            "backups can only be downloaded from the host itself (admin.allow_backup_download_remote is off)",
        ));
    }
    let opened = tokio::fs::File::open(&file.path).await;
    let f = match opened {
        Ok(f) => f,
        Err(e) => {
            trail.failed(&e.to_string());
            return Err(ApiError::not_found(format!(
                "cannot open {}: {e}",
                file.name
            )));
        }
    };
    let len = f.metadata().await.map(|m| m.len()).unwrap_or(0);
    trail.done(json!({"size_bytes": len}));
    let body = Body::from_stream(tokio_util::io::ReaderStream::new(f));
    let mut resp = (StatusCode::OK, body).into_response();
    let h = resp.headers_mut();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    if let Ok(v) = HeaderValue::from_str(&format!("attachment; filename=\"{}\"", file.name)) {
        h.insert(header::CONTENT_DISPOSITION, v);
    }
    if let Ok(v) = HeaderValue::from_str(&len.to_string()) {
        h.insert(header::CONTENT_LENGTH, v);
    }
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(resp)
}
