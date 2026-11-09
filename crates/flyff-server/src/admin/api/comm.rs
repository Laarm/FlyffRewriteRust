//! The Communication area of the console (Plan/25 §8; **WP-ADMIN-27**): notices that go out now, scheduled and
//! recurring notices, and system mail.
//!
//! | route | cap | what |
//! |---|---|---|
//! | `POST /admin/api/broadcast` | `act.broadcast` | a notice now: everybody, one world, or up to 100 characters (`SendNotice`) |
//! | `GET /admin/api/notices` | `act.broadcast` | the scheduled notices with state, last and next fire time, and the server's zone |
//! | `POST /admin/api/notices`, `PUT /admin/api/notices/{id}`, `DELETE /admin/api/notices/{id}` | `act.broadcast` | create, replace (pause is `paused: true`), delete |
//! | `POST /admin/api/mail` ✱ | `act.items` | a system mail with attachments: the grant pipeline in Mail mode |
//! | `GET /admin/api/mail?limit=&character=` | `act.items` | the history of system mails (`SystemMails`) |
//!
//! **The schedule** lives in the table `scheduled_notices`; the game thread works from memory. Every change writes the
//! row, reads all rows again and replaces the schedule of the game thread (`SetNoticeSchedule`) under one lock, and
//! at start [`schedule::spawn_loader`] installs the stored rows. The wall job `admin.notices` fires a notice at its
//! minute and stamps `last_fired_at`.
//!
//! **Audit** (`admin.notice`): `{action: broadcast | create | update | delete, ...}` with `requested` before the
//! change and `done` or `failed` after; a fired notice is audited by the game (`action: fired`). A system mail is
//! audited as the grant it is (`admin.grant`, mode `mail`).

pub mod dto;
pub mod schedule;

use super::{ApiState, Routes};
use crate::admin::api::IdemKey;
use crate::admin::auth::Who;
use crate::admin::dto::{DeliveryMode, GrantReport, GrantRequest, Mutation};
use crate::admin::error::{ApiError, ApiResult};
use crate::admin::grants;
use crate::admin::guard::{AuditTrail, ConfirmFacts, MutationJson};
use axum::Json;
use axum::body::Bytes;
use axum::extract::{Extension, Path, Query, State};
use dto::{
    BroadcastBody, BroadcastResult, MailBody, NoticeBody, NoticeList, NoticeRow, SystemMailDto,
    SystemMailList,
};
use flyff_common::CharacterId;
use flyff_db::{AtomicBatch, AtomicKind, AtomicOutcome, DbOp};
use flyff_game::admin::actions::comm::{
    MAX_CAPTION_BYTES, MAX_SYSTEM_BYTES, NoticeTarget, SendNotice,
};
use flyff_game::admin::queries::comm::SYSTEM_MAILS_MAX;
use flyff_game::admin::{AdminQuery, AdminQueryResult};
use flyff_game::econ::mail::{MAX_TEXT_BYTES, MAX_TITLE_BYTES};
use flyff_game::input::{AdminCmd, AdminOutput};
use schedule::{
    MAX_NOTICES, SCHEDULE_LOCK, check_notice_text, now_s, offset_s, push, record_of, row_of, rows,
    style_of, target_of, zone_of,
};
use serde::Deserialize;
use serde_json::json;

/// Mounts the routes of this area.
pub fn mount(r: Routes<ApiState>) -> Routes<ApiState> {
    r.post("/admin/api/broadcast", broadcast)
        .get("/admin/api/notices", list)
        .post("/admin/api/notices", create)
        .put("/admin/api/notices/{id}", update)
        .delete("/admin/api/notices/{id}", delete)
        .post("/admin/api/mail", mail)
        .get("/admin/api/mail", mail_history)
}

// ---- a notice now -------------------------------------------------------------------------------------------------------

/// `POST /admin/api/broadcast`.
async fn broadcast(
    State(st): State<ApiState>,
    Who(who): Who,
    MutationJson(m): MutationJson<BroadcastBody>,
) -> ApiResult<Json<BroadcastResult>> {
    let policy = st.policy();
    let b = m.body;
    check_notice_text(&b.text, b.style)?;
    let target = target_of(&b.target, "target")?;
    let requested = match &target {
        NoticeTarget::Characters(ids) => Some(u32::try_from(ids.len()).unwrap_or(u32::MAX)),
        _ => None,
    };
    let reason = policy.reason_optional(m.reason.as_ref())?;
    policy.confirm(
        &ConfirmFacts {
            targets: match &target {
                NoticeTarget::Characters(ids) => ids.len(),
                _ => 0,
            },
            ..ConfirmFacts::default()
        },
        m.confirm.as_deref(),
    )?;
    let trail = AuditTrail::begin(
        &st.ops.audit,
        &who,
        "admin.notice",
        reason,
        json!({
            "action": "broadcast",
            "style": style_of(b.style).name(),
            "target": target.text(),
            "text": b.text.trim(),
            "bytes": b.text.trim().len(),
        }),
    )?;
    let out = st
        .ops
        .game(
            AdminCmd::SendNotice(SendNotice {
                text: b.text.trim().to_owned(),
                style: style_of(b.style),
                target,
            }),
            who.actor(),
        )
        .await;
    match out {
        Ok(AdminOutput::Done { affected }) => {
            trail.done(json!({"delivered": affected}));
            Ok(Json(BroadcastResult {
                delivered: affected,
                requested,
            }))
        }
        Ok(AdminOutput::Failed(f)) => {
            trail.failed(&f.to_string());
            Err(ApiError::from(&f))
        }
        Ok(other) => {
            trail.failed("unexpected answer");
            Err(ApiError::internal(&format!(
                "the game answered a notice with {other:?}"
            )))
        }
        Err(e) => {
            trail.failed(&e.to_string());
            Err(ApiError::from(e))
        }
    }
}

// ---- the scheduled notices ------------------------------------------------------------------------------------------------

/// `GET /admin/api/notices`.
async fn list(State(st): State<ApiState>, Who(_who): Who) -> ApiResult<Json<NoticeList>> {
    let zone = zone_of(&st.ops);
    let now = now_s(&st.ops);
    let items: Vec<NoticeRow> = rows(&st.ops)
        .await?
        .iter()
        .filter_map(|r| row_of(r, &zone, now))
        .collect();
    Ok(Json(NoticeList {
        now,
        zone: zone.name().to_owned(),
        utc_offset_s: offset_s(&zone, now),
        max_system_bytes: u32::try_from(MAX_SYSTEM_BYTES).unwrap_or(0),
        max_caption_bytes: u32::try_from(MAX_CAPTION_BYTES).unwrap_or(0),
        max_notices: u32::try_from(MAX_NOTICES).unwrap_or(0),
        items,
    }))
}

fn single_row(st: &ApiState, rec: &flyff_db::ScheduledNoticeRecord) -> ApiResult<Json<NoticeRow>> {
    let zone = zone_of(&st.ops);
    row_of(rec, &zone, now_s(&st.ops))
        .map(Json)
        .ok_or_else(|| ApiError::internal("the stored notice does not read back"))
}

/// Runs one change of the schedule: the audit `requested`, the row, the new schedule on the game thread, the audit
/// `done` or `failed`. `write` returns the row's id and what the audit records.
async fn change<F, Fut>(
    st: &ApiState,
    who: &crate::admin::identity::Identity,
    reason: Option<crate::admin::guard::ReasonText>,
    action: &str,
    data: serde_json::Value,
    write: F,
) -> ApiResult<i64>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = ApiResult<i64>>,
{
    let mut audit = json!({"action": action});
    if let (Some(a), serde_json::Value::Object(d)) = (audit.as_object_mut(), data) {
        a.extend(d);
    }
    let trail = AuditTrail::begin(&st.ops.audit, who, "admin.notice", reason, audit)?;
    let _guard = SCHEDULE_LOCK.lock().await;
    let id = match write().await {
        Ok(id) => id,
        Err(e) => {
            trail.failed(&e.to_string());
            return Err(e);
        }
    };
    // the row is stored; the game thread learns about it now (and at the next change, should this fail)
    if let Err(e) = push(&st.ops, who.actor()).await {
        tracing::warn!(error = %e, "the notice is stored but the game thread did not take the schedule");
        trail.done(json!({"id": id, "installed": false}));
    } else {
        trail.done(json!({"id": id, "installed": true}));
    }
    Ok(id)
}

async fn write_op(st: &ApiState, op: DbOp) -> ApiResult<AtomicOutcome> {
    st.ops
        .persist
        .call(AtomicBatch::of_ops(
            AtomicKind::Other("admin_notice"),
            vec![op],
        ))
        .await
        .map_err(|e| match e.kind {
            flyff_db::DbErrorKind::Invalid => ApiError::invalid("notice", e.to_string()),
            _ => ApiError::db(&e),
        })
}

/// `POST /admin/api/notices`.
async fn create(
    State(st): State<ApiState>,
    Who(who): Who,
    MutationJson(m): MutationJson<NoticeBody>,
) -> ApiResult<Json<NoticeRow>> {
    let reason = st.policy().reason_optional(m.reason.as_ref())?;
    let now = flyff_common::UnixTime(now_s(&st.ops));
    let rec = record_of(&m.body, 0, &who.label(), now, false)?;
    if rows(&st.ops).await?.len() >= MAX_NOTICES {
        return Err(ApiError::new(
            crate::admin::dto::ErrorCode::Limit,
            format!("at most {MAX_NOTICES} scheduled notices: delete one first"),
        ));
    }
    let id = change(
        &st,
        &who,
        reason,
        "create",
        json!({"style": rec.style, "target": rec.target, "start_at": rec.start_at.0, "bytes": rec.text.len()}),
        || async {
            match write_op(&st, DbOp::InsertScheduledNotice(rec.clone())).await? {
                AtomicOutcome::RowInserted(id) => Ok(id),
                other => Err(ApiError::internal(&format!(
                    "a notice insert answered {other:?}"
                ))),
            }
        },
    )
    .await?;
    single_row(&st, &flyff_db::ScheduledNoticeRecord { id, ..rec })
}

/// `PUT /admin/api/notices/{id}`: every editable field is replaced; the author and the creation time stay; editing
/// the schedule of a fired single notice arms it again when the new start is later than its last fire.
async fn update(
    State(st): State<ApiState>,
    Who(who): Who,
    Path(id): Path<i64>,
    MutationJson(m): MutationJson<NoticeBody>,
) -> ApiResult<Json<NoticeRow>> {
    let reason = st.policy().reason_optional(m.reason.as_ref())?;
    let existing = rows(&st.ops)
        .await?
        .into_iter()
        .find(|r| r.id == id)
        .ok_or_else(|| ApiError::not_found("no such notice"))?;
    let keeps_start = existing.start_at.0 == m.body.start_at;
    let rec = record_of(
        &m.body,
        id,
        &existing.created_by,
        existing.created_at,
        keeps_start,
    )?;
    change(
        &st,
        &who,
        reason,
        "update",
        json!({"id": id, "style": rec.style, "target": rec.target, "paused": rec.paused}),
        || async {
            match write_op(&st, DbOp::UpdateScheduledNotice(rec.clone())).await? {
                AtomicOutcome::Changed(n) if n > 0 => Ok(id),
                _ => Err(ApiError::not_found("no such notice")),
            }
        },
    )
    .await?;
    let stored = rows(&st.ops)
        .await?
        .into_iter()
        .find(|r| r.id == id)
        .ok_or_else(|| ApiError::not_found("no such notice"))?;
    single_row(&st, &stored)
}

/// `DELETE /admin/api/notices/{id}` (the `Mutation` envelope is optional).
async fn delete(
    State(st): State<ApiState>,
    Who(who): Who,
    Path(id): Path<i64>,
    body: Bytes,
) -> ApiResult<Json<serde_json::Value>> {
    let reason = if body.iter().all(u8::is_ascii_whitespace) {
        None
    } else {
        let m: Mutation<serde_json::Value> = serde_json::from_slice(&body)
            .map_err(|e| ApiError::bad_request(format!("malformed body: {e}")))?;
        st.policy().reason_optional(m.reason.as_ref())?
    };
    change(&st, &who, reason, "delete", json!({"id": id}), || async {
        match write_op(&st, DbOp::DeleteScheduledNotice { id }).await? {
            AtomicOutcome::Changed(n) if n > 0 => Ok(id),
            _ => Err(ApiError::not_found("no such notice")),
        }
    })
    .await?;
    Ok(Json(json!({"id": id})))
}

// ---- system mail ----------------------------------------------------------------------------------------------------------

/// `POST /admin/api/mail` ✱: the grant pipeline in Mail mode (`POST /admin/api/grants/v2` with `mode: mail`), with
/// the subject and text limits of the mail service checked up front. A mail needs an attachment (items or penya).
async fn mail(
    State(st): State<ApiState>,
    Who(who): Who,
    key: Option<Extension<IdemKey>>,
    MutationJson(m): MutationJson<MailBody>,
) -> ApiResult<Json<GrantReport>> {
    let policy = st.policy();
    let b = m.body;
    let subject = b.subject.trim().to_owned();
    if subject.is_empty() || subject.len() > MAX_TITLE_BYTES {
        return Err(ApiError::invalid(
            "subject",
            format!("the subject is 1..={MAX_TITLE_BYTES} bytes"),
        ));
    }
    let text = b.text.trim().to_owned();
    if text.len() > MAX_TEXT_BYTES {
        return Err(ApiError::invalid(
            "text",
            format!("the text is at most {MAX_TEXT_BYTES} bytes"),
        ));
    }
    if subject
        .chars()
        .chain(text.chars())
        .any(|c| c.is_control() && c != '\n')
    {
        return Err(ApiError::invalid("text", "no control characters"));
    }
    let req = GrantRequest {
        recipients: b.recipients,
        items: b.items,
        gold: b.gold,
        mode: DeliveryMode::Mail { subject, text },
    };
    let plan = grants::plan(&st, &who, &req).await?;
    let reason = policy.reason_required(m.reason.as_ref())?;
    policy.confirm(
        &grants::confirm_facts(&req, plan.recipient_count(), plan.only_name()),
        m.confirm.as_deref(),
    )?;
    let report = grants::run(
        &st,
        &who,
        plan,
        &req,
        &reason,
        key.as_ref().map(|k| k.0.0.as_str()),
    )
    .await?;
    Ok(Json(report))
}

/// `GET /admin/api/mail?limit=&character=`.
#[derive(Debug, Default, Deserialize)]
pub struct HistoryQuery {
    limit: Option<usize>,
    character: Option<u32>,
}

/// `GET /admin/api/mail`: the history of system mails, newest first.
async fn mail_history(
    State(st): State<ApiState>,
    Who(_who): Who,
    Query(q): Query<HistoryQuery>,
) -> ApiResult<Json<SystemMailList>> {
    let limit = q.limit.unwrap_or(100).clamp(1, SYSTEM_MAILS_MAX);
    let answer = crate::admin::api::items::game_query(
        &st.ops,
        AdminQuery::SystemMails {
            limit,
            receiver: q.character.map(CharacterId),
        },
    )
    .await?;
    match answer {
        AdminQueryResult::SystemMails(rows) => Ok(Json(SystemMailList {
            max_subject_bytes: u32::try_from(MAX_TITLE_BYTES).unwrap_or(0),
            max_text_bytes: u32::try_from(MAX_TEXT_BYTES).unwrap_or(0),
            items: rows
                .into_iter()
                .map(|r| SystemMailDto {
                    id: r.id,
                    receiver: r.receiver,
                    receiver_name: r.receiver_name,
                    title: r.title,
                    text: r.text,
                    gold: r.gold,
                    item_id: r.item_id,
                    item_count: r.item_count,
                    created_at: r.created_at,
                    read: r.read,
                    online: r.online,
                })
                .collect(),
        })),
        AdminQueryResult::Failed(f) => Err(ApiError::from(&f)),
        other => Err(ApiError::internal(&format!(
            "SystemMails answered {other:?}"
        ))),
    }
}
