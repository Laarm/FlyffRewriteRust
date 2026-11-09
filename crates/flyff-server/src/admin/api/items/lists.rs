//! The grant and removal lists with their cancels (Plan/25 §5.6; WP-ADMIN-20).
//!
//! * `GET /admin/api/grants?state=open|done|cancelled|all&character=&days=&cursor=&limit=`: newest first, `done` and
//!   `cancelled` limited to the last `days` (default 30);
//! * `POST /admin/api/grants/{id}/cancel`, `POST /admin/api/removals/{id}/cancel`: `AdminCmd::CancelPending...` on the
//!   game thread (`409 delivery_in_progress` while the game delivers the grant, `409 already_delivered` when the row
//!   was delivered or applied). Cancelling twice answers `already_cancelled`.

use super::{ApiState, parse_json};
use crate::admin::auth::Who;
use crate::admin::dto::Page;
use crate::admin::dto_items::{CancelResult, GrantRowDto, RemovalRowDto};
use crate::admin::error::{ApiError, ApiResult};
use crate::admin::grants::stored_dto;
use crate::admin::guard::audit_admin;
use axum::Json;
use axum::extract::{Path, Query, State};
use flyff_common::UnixTime;
use flyff_db::admin::{
    AdminGrantRow, AdminReadQuery, AdminRemovalRow, PendingFilter, PendingState,
};
use flyff_game::admin::AdminErrorCode;
use flyff_game::admin::actions::grants::{CancelPendingGrant, CancelPendingRemoval};
use flyff_game::input::{AdminCmd, AdminOutput};
use serde::Deserialize;
use serde_json::json;

/// The query of both lists.
#[derive(Debug, Default, Deserialize)]
pub struct ListQuery {
    state: Option<String>,
    character: Option<u32>,
    days: Option<u32>,
    cursor: Option<String>,
    limit: Option<u32>,
}

fn filter_of(q: &ListQuery, now: UnixTime) -> ApiResult<PendingFilter> {
    let state = match q.state.as_deref().unwrap_or("open") {
        "open" => PendingState::Open,
        "done" | "delivered" | "applied" => PendingState::Done,
        "cancelled" | "canceled" => PendingState::Cancelled,
        "all" => PendingState::All,
        other => {
            return Err(ApiError::invalid(
                "state",
                format!("'{other}' is not open, done, cancelled or all"),
            ));
        }
    };
    let cursor = match q.cursor.as_deref().filter(|c| !c.is_empty()) {
        None => None,
        Some(c) => Some(
            c.parse::<i64>()
                .map_err(|_| ApiError::invalid("cursor", "not a cursor of this list"))?,
        ),
    };
    let since = matches!(state, PendingState::Done | PendingState::Cancelled).then(|| {
        UnixTime(
            now.0
                .saturating_sub(i64::from(q.days.unwrap_or(30).clamp(1, 3650)) * 86_400),
        )
    });
    Ok(PendingFilter {
        state,
        id: None,
        character: q.character.map(flyff_common::CharacterId),
        since,
        cursor,
        limit: q.limit.unwrap_or(100).clamp(1, 500),
    })
}

fn state_name(delivered: bool, cancelled: bool) -> &'static str {
    if cancelled {
        "cancelled"
    } else if delivered {
        "done"
    } else {
        "open"
    }
}

fn grant_row(r: AdminGrantRow) -> GrantRowDto {
    let g = r.grant;
    GrantRowDto {
        id: g.id,
        character_id: g.character_id.0,
        character_name: r.character_name,
        kind: match g.kind {
            flyff_db::model::GrantKind::Item => "item",
            flyff_db::model::GrantKind::Gold => "gold",
        }
        .to_owned(),
        item: g.item.as_ref().map(|s| stored_dto(&s.item)),
        gold: g.gold,
        source: g.source,
        note: g.note,
        created_at: g.created_at.0,
        delivered_at: g.delivered_at.map(|t| t.0),
        cancelled_at: g.cancelled_at.map(|t| t.0),
        cancelled_by: g.cancelled_by,
        state: state_name(g.delivered_at.is_some(), g.cancelled_at.is_some()).to_owned(),
    }
}

fn removal_row(r: AdminRemovalRow) -> RemovalRowDto {
    let x = r.removal;
    RemovalRowDto {
        id: x.id,
        character_id: x.character_id.map(|c| c.0),
        character_name: r.character_name,
        guild_id: x.guild_id.map(|g| g.0),
        guild_name: r.guild_name,
        location: x.location.as_str().to_owned(),
        item_id: x.item_id,
        serial: x.serial,
        count: i64::from(x.count),
        requested_by: x.requested_by,
        reason: x.reason,
        created_at: x.created_at.0,
        done_at: x.done_at.map(|t| t.0),
        removed_count: x.removed_count.map(i64::from),
        cancelled_at: x.cancelled_at.map(|t| t.0),
        cancelled_by: x.cancelled_by,
        state: state_name(x.done_at.is_some(), x.cancelled_at.is_some()).to_owned(),
    }
}

/// `GET /admin/api/grants`.
pub async fn grants(
    State(st): State<ApiState>,
    Who(_who): Who,
    Query(q): Query<ListQuery>,
) -> ApiResult<Json<Page<GrantRowDto>>> {
    let filter = filter_of(&q, st.ops.admin.clock.unix())?;
    let page = st
        .ops
        .persist
        .admin_read(AdminReadQuery::PendingGrants(filter))
        .await
        .map_err(|e| ApiError::db(&e))?
        .grants()
        .map_err(|e| ApiError::db(&e))?;
    Ok(Json(Page {
        items: page.items.into_iter().map(grant_row).collect(),
        next_cursor: page.next_cursor.map(|c| c.to_string()),
    }))
}

/// `GET /admin/api/removals`.
pub async fn removals(
    State(st): State<ApiState>,
    Who(_who): Who,
    Query(q): Query<ListQuery>,
) -> ApiResult<Json<Page<RemovalRowDto>>> {
    let filter = filter_of(&q, st.ops.admin.clock.unix())?;
    let page = st
        .ops
        .persist
        .admin_read(AdminReadQuery::PendingRemovals(filter))
        .await
        .map_err(|e| ApiError::db(&e))?
        .removals()
        .map_err(|e| ApiError::db(&e))?;
    Ok(Json(Page {
        items: page.items.into_iter().map(removal_row).collect(),
        next_cursor: page.next_cursor.map(|c| c.to_string()),
    }))
}

/// What happened to a row after a cancel changed nothing: `Cancelled` before, delivered / applied, or unknown.
enum Row {
    Cancelled,
    Done,
    Missing,
    Open,
}

async fn row_state(st: &ApiState, id: i64, removal: bool) -> ApiResult<Row> {
    let filter = PendingFilter {
        id: Some(id),
        ..PendingFilter::of(PendingState::All)
    };
    if removal {
        let page = st
            .ops
            .persist
            .admin_read(AdminReadQuery::PendingRemovals(filter))
            .await
            .map_err(|e| ApiError::db(&e))?
            .removals()
            .map_err(|e| ApiError::db(&e))?;
        Ok(match page.items.first() {
            None => Row::Missing,
            Some(r) if r.removal.cancelled_at.is_some() => Row::Cancelled,
            Some(r) if r.removal.done_at.is_some() => Row::Done,
            Some(_) => Row::Open,
        })
    } else {
        let page = st
            .ops
            .persist
            .admin_read(AdminReadQuery::PendingGrants(filter))
            .await
            .map_err(|e| ApiError::db(&e))?
            .grants()
            .map_err(|e| ApiError::db(&e))?;
        Ok(match page.items.first() {
            None => Row::Missing,
            Some(r) if r.grant.cancelled_at.is_some() => Row::Cancelled,
            Some(r) if r.grant.delivered_at.is_some() => Row::Done,
            Some(_) => Row::Open,
        })
    }
}

async fn cancel(
    st: &ApiState,
    who: &crate::admin::identity::Identity,
    id: i64,
    removal: bool,
    reason: Option<&crate::admin::guard::ReasonText>,
) -> ApiResult<Json<CancelResult>> {
    let cmd = if removal {
        AdminCmd::CancelPendingRemoval(CancelPendingRemoval { id })
    } else {
        AdminCmd::CancelPendingGrant(CancelPendingGrant { id })
    };
    let what = if removal { "removal" } else { "grant" };
    let state = match st.ops.game(cmd, who.actor()).await? {
        AdminOutput::Done { .. } => "cancelled",
        AdminOutput::Failed(f) if f.code == AdminErrorCode::AlreadyDelivered => {
            match row_state(st, id, removal).await? {
                Row::Cancelled => "already_cancelled",
                Row::Missing => return Err(ApiError::not_found(format!("no pending {what} {id}"))),
                Row::Done | Row::Open => {
                    return Err(ApiError::new(
                        crate::admin::dto::ErrorCode::AlreadyDelivered,
                        if removal {
                            format!("removal {id} was applied already")
                        } else {
                            format!("grant {id} was delivered already")
                        },
                    ));
                }
            }
        }
        AdminOutput::Failed(f) => return Err(ApiError::from(&f)),
        other => {
            return Err(ApiError::internal(&format!(
                "the game answered a cancel with {other:?}"
            )));
        }
    };
    audit_admin(
        &st.ops.audit,
        if removal {
            "admin.removal_cancel"
        } else {
            "admin.grant_cancel"
        },
        who,
        "done",
        reason,
        json!({"id": id, "state": state}),
    )?;
    Ok(Json(CancelResult {
        id,
        state: state.to_owned(),
    }))
}

/// `POST /admin/api/grants/{id}/cancel`.
pub async fn cancel_grant(
    State(st): State<ApiState>,
    Who(who): Who,
    Path(id): Path<i64>,
    body: axum::body::Bytes,
) -> ApiResult<Json<CancelResult>> {
    let reason = reason_of(&st, &body)?;
    cancel(&st, &who, id, false, reason.as_ref()).await
}

/// `POST /admin/api/removals/{id}/cancel`.
pub async fn cancel_removal(
    State(st): State<ApiState>,
    Who(who): Who,
    Path(id): Path<i64>,
    body: axum::body::Bytes,
) -> ApiResult<Json<CancelResult>> {
    let reason = reason_of(&st, &body)?;
    cancel(&st, &who, id, true, reason.as_ref()).await
}

/// The optional reason of a cancel (`{reason?}` or the `Mutation` envelope; an empty body is fine).
fn reason_of(st: &ApiState, body: &[u8]) -> ApiResult<Option<crate::admin::guard::ReasonText>> {
    if body.iter().all(u8::is_ascii_whitespace) {
        return Ok(None);
    }
    let m: crate::admin::dto::Mutation<serde_json::Value> = parse_json(body)?;
    st.policy().reason_optional(m.reason.as_ref())
}
