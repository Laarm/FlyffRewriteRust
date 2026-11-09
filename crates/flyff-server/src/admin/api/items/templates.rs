//! Item templates and kits (Plan/25 §5.4, §11.6; WP-ADMIN-20): `GET/POST/PUT/DELETE /admin/api/templates`.
//!
//! * a **template** is one `ItemSpec` ("+10 Fire Angel Sword"), a **kit** a `Kit` (a named basket: item specs plus
//!   penya); both live in `admin_templates`, shared between staff, with their author and update time;
//! * every spec is validated with the rules of the builder before it is stored: a template that the validation
//!   refuses cannot be saved, so a kit never fails when it is granted later because it was malformed;
//! * names are unique per kind, case-insensitively (`400 invalid` on the field `name` otherwise);
//! * the JSON export / import of the tab is the list row itself: the import posts the spec of an exported row.

use super::{ApiState, parse_json};
use crate::admin::auth::Who;
use crate::admin::dto::{ItemSpec, Kit, Mutation};
use crate::admin::dto_items::{TemplateBody, TemplateKindDto, TemplateRowDto};
use crate::admin::error::{ApiError, ApiResult};
use crate::admin::grants::ValidateCx;
use crate::admin::guard::{ReasonText, audit_admin};
use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use flyff_db::admin::{AdminReadQuery, AdminTemplateRecord, TemplateKind};
use flyff_db::{AtomicBatch, AtomicKind, AtomicOutcome, DbErrorKind, DbOp};
use serde::Deserialize;
use serde_json::json;

/// The longest template or kit name.
pub const NAME_MAX_CHARS: usize = 64;

/// `GET /admin/api/templates?kind=`.
#[derive(Debug, Default, Deserialize)]
pub struct ListQuery {
    kind: Option<String>,
}

fn kind_of(k: TemplateKindDto) -> TemplateKind {
    match k {
        TemplateKindDto::Item => TemplateKind::Item,
        TemplateKindDto::Kit => TemplateKind::Kit,
    }
}

fn row_of(r: AdminTemplateRecord) -> Option<TemplateRowDto> {
    let (kind, item, kit) = match r.kind {
        TemplateKind::Item => (
            TemplateKindDto::Item,
            serde_json::from_value::<ItemSpec>(r.spec).ok(),
            None,
        ),
        TemplateKind::Kit => (
            TemplateKindDto::Kit,
            None,
            serde_json::from_value::<Kit>(r.spec).ok(),
        ),
    };
    if item.is_none() && kit.is_none() {
        tracing::warn!(template = r.id, "an unreadable template spec is not listed");
        return None;
    }
    Some(TemplateRowDto {
        id: r.id,
        kind,
        name: r.name,
        item,
        kit,
        created_by: r.created_by,
        created_at: r.created_at.0,
        updated_at: r.updated_at.0,
    })
}

/// `GET /admin/api/templates`.
pub async fn list(
    State(st): State<ApiState>,
    Who(_who): Who,
    Query(q): Query<ListQuery>,
) -> ApiResult<Json<Vec<TemplateRowDto>>> {
    let kind = match q.kind.as_deref().filter(|k| !k.is_empty()) {
        None => None,
        Some(k) => {
            Some(TemplateKind::parse(k).ok_or_else(|| ApiError::invalid("kind", "item or kit"))?)
        }
    };
    let rows = st
        .ops
        .persist
        .admin_read(AdminReadQuery::Templates { kind })
        .await
        .map_err(|e| ApiError::db(&e))?
        .templates()
        .map_err(|e| ApiError::db(&e))?;
    Ok(Json(rows.into_iter().filter_map(row_of).collect()))
}

/// Validates a body into the JSON stored in `spec` (and the trimmed name).
fn checked(cx: &ValidateCx, b: &TemplateBody) -> ApiResult<(String, serde_json::Value)> {
    let name = b.name.trim().to_owned();
    if name.is_empty()
        || name.chars().count() > NAME_MAX_CHARS
        || name.chars().any(char::is_control)
    {
        return Err(ApiError::invalid(
            "name",
            format!("the name is 1..={NAME_MAX_CHARS} characters"),
        ));
    }
    let spec = match (b.kind, &b.item, &b.kit) {
        (TemplateKindDto::Item, Some(item), None) => {
            if let Err(issues) = cx.record(item) {
                return Err(ApiError::invalid(
                    &format!(
                        "item.{}",
                        issues.first().map_or("item_id", |i| i.field.as_str())
                    ),
                    issues
                        .iter()
                        .map(|i| format!("{}: {}", i.field, i.message))
                        .collect::<Vec<_>>()
                        .join("; "),
                ));
            }
            serde_json::to_value(item)
        }
        (TemplateKindDto::Kit, None, Some(kit)) => {
            if kit.items.len() > crate::admin::grants::LINES_MAX {
                return Err(ApiError::invalid(
                    "kit.items",
                    format!("at most {} item lines", crate::admin::grants::LINES_MAX),
                ));
            }
            if kit.items.is_empty() && kit.gold == 0 {
                return Err(ApiError::invalid("kit.items", "a kit holds items or penya"));
            }
            if kit.gold > 2_147_483_647 {
                return Err(ApiError::invalid(
                    "kit.gold",
                    "the penya is at most 2,147,483,647",
                ));
            }
            for (i, item) in kit.items.iter().enumerate() {
                if let Err(issues) = cx.record(item) {
                    return Err(ApiError::invalid(
                        &format!(
                            "kit.items[{i}].{}",
                            issues.first().map_or("item_id", |x| x.field.as_str())
                        ),
                        issues
                            .iter()
                            .map(|x| format!("{}: {}", x.field, x.message))
                            .collect::<Vec<_>>()
                            .join("; "),
                    ));
                }
            }
            let mut kit = kit.clone();
            kit.name = name.clone();
            serde_json::to_value(&kit)
        }
        (TemplateKindDto::Item, _, _) => {
            return Err(ApiError::invalid(
                "item",
                "a template carries `item` and no `kit`",
            ));
        }
        (TemplateKindDto::Kit, _, _) => {
            return Err(ApiError::invalid(
                "kit",
                "a kit carries `kit` and no `item`",
            ));
        }
    };
    let spec = spec.map_err(|e| ApiError::internal(&format!("template spec: {e}")))?;
    Ok((name, spec))
}

fn db_error(e: &flyff_db::DbError) -> ApiError {
    match e.kind {
        DbErrorKind::Conflict => ApiError::invalid("name", "a template or kit of that name exists"),
        DbErrorKind::Invalid => ApiError::invalid("spec", e.to_string()),
        DbErrorKind::NotFound => ApiError::not_found("no such template"),
        _ => ApiError::db(e),
    }
}

fn audit(
    st: &ApiState,
    who: &crate::admin::identity::Identity,
    action: &str,
    id: i64,
    kind: &str,
    name: &str,
    reason: Option<&ReasonText>,
) -> ApiResult<()> {
    audit_admin(
        &st.ops.audit,
        "admin.template",
        who,
        "done",
        reason,
        json!({"action": action, "id": id, "kind": kind, "name": name}),
    )
}

/// `POST /admin/api/templates`.
pub async fn create(
    State(st): State<ApiState>,
    Who(who): Who,
    body: Bytes,
) -> ApiResult<Json<TemplateRowDto>> {
    let m: Mutation<TemplateBody> = parse_json(&body)?;
    let reason = st.policy().reason_optional(m.reason.as_ref())?;
    let cx = ValidateCx::of(&st.ops);
    let (name, spec) = checked(&cx, &m.body)?;
    let now = st.ops.admin.clock.unix();
    let rec = AdminTemplateRecord {
        id: 0,
        kind: kind_of(m.body.kind),
        name: name.clone(),
        spec,
        created_by: who.label(),
        created_at: now,
        updated_at: now,
    };
    let id = match st
        .ops
        .persist
        .call(AtomicBatch::of_ops(
            AtomicKind::Other("admin_template"),
            vec![DbOp::InsertAdminTemplate(rec.clone())],
        ))
        .await
        .map_err(|e| db_error(&e))?
    {
        AtomicOutcome::RowInserted(id) => id,
        other => {
            return Err(ApiError::internal(&format!(
                "a template insert answered {other:?}"
            )));
        }
    };
    audit(
        &st,
        &who,
        "create",
        id,
        rec.kind.as_str(),
        &name,
        reason.as_ref(),
    )?;
    row_of(AdminTemplateRecord { id, ..rec })
        .map(Json)
        .ok_or_else(|| ApiError::internal("the stored template does not read back"))
}

/// `PUT /admin/api/templates/{id}`: name and spec change, the kind and the author stay.
pub async fn update(
    State(st): State<ApiState>,
    Who(who): Who,
    Path(id): Path<i64>,
    body: Bytes,
) -> ApiResult<Json<TemplateRowDto>> {
    let m: Mutation<TemplateBody> = parse_json(&body)?;
    let reason = st.policy().reason_optional(m.reason.as_ref())?;
    let existing = st
        .ops
        .persist
        .admin_read(AdminReadQuery::Templates { kind: None })
        .await
        .map_err(|e| ApiError::db(&e))?
        .templates()
        .map_err(|e| ApiError::db(&e))?
        .into_iter()
        .find(|t| t.id == id)
        .ok_or_else(|| ApiError::not_found("no such template"))?;
    if kind_of(m.body.kind) != existing.kind {
        return Err(ApiError::invalid("kind", "a template keeps its kind"));
    }
    let cx = ValidateCx::of(&st.ops);
    let (name, spec) = checked(&cx, &m.body)?;
    let rec = AdminTemplateRecord {
        name: name.clone(),
        spec,
        updated_at: st.ops.admin.clock.unix(),
        ..existing
    };
    match st
        .ops
        .persist
        .call(AtomicBatch::of_ops(
            AtomicKind::Other("admin_template"),
            vec![DbOp::UpdateAdminTemplate(rec.clone())],
        ))
        .await
        .map_err(|e| db_error(&e))?
    {
        AtomicOutcome::Changed(n) if n > 0 => {}
        _ => return Err(ApiError::not_found("no such template")),
    }
    audit(
        &st,
        &who,
        "update",
        id,
        rec.kind.as_str(),
        &name,
        reason.as_ref(),
    )?;
    row_of(rec)
        .map(Json)
        .ok_or_else(|| ApiError::internal("the stored template does not read back"))
}

/// `DELETE /admin/api/templates/{id}`.
pub async fn delete(
    State(st): State<ApiState>,
    Who(who): Who,
    Path(id): Path<i64>,
    body: Bytes,
) -> ApiResult<Json<serde_json::Value>> {
    let reason = if body.iter().all(u8::is_ascii_whitespace) {
        None
    } else {
        let m: Mutation<serde_json::Value> = parse_json(&body)?;
        st.policy().reason_optional(m.reason.as_ref())?
    };
    match st
        .ops
        .persist
        .call(AtomicBatch::of_ops(
            AtomicKind::Other("admin_template"),
            vec![DbOp::DeleteAdminTemplate { id }],
        ))
        .await
        .map_err(|e| db_error(&e))?
    {
        AtomicOutcome::Changed(n) if n > 0 => {}
        _ => return Err(ApiError::not_found("no such template")),
    }
    audit(&st, &who, "delete", id, "", "", reason.as_ref())?;
    Ok(Json(json!({"id": id})))
}
