//! `POST /admin/api/grants/v2` ✱ (Plan/25 §5.3, §12 rule 9; WP-ADMIN-20): the body is `Mutation<GrantRequest>` and
//! the header `Idempotency-Key` is mandatory (the gate stores the answer under it: a retry after a timeout returns
//! the stored report and never grants twice; the key also seeds the `request_id` the game thread remembers for ten
//! minutes).
//!
//! Order of the guards: the request checks (limits, every item line validated), the recipients, the **reason**
//! (mandatory), the **typed confirmation** (more than 10 recipients, everybody online, penya above
//! `admin.confirm_gold_threshold`, more than `admin.confirm_item_threshold` pieces), then the delivery.

use super::ApiState;
use crate::admin::api::IdemKey;
use crate::admin::auth::Who;
use crate::admin::dto::{GrantReport, GrantRequest};
use crate::admin::error::ApiResult;
use crate::admin::grants;
use crate::admin::guard::MutationJson;
use axum::Json;
use axum::extract::{Extension, State};

/// `POST /admin/api/grants/v2`.
pub async fn grant(
    State(st): State<ApiState>,
    Who(who): Who,
    key: Option<Extension<IdemKey>>,
    MutationJson(m): MutationJson<GrantRequest>,
) -> ApiResult<Json<GrantReport>> {
    let policy = st.policy();
    let req = m.body;
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
