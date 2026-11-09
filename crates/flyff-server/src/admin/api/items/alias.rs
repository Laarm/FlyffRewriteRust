//! `POST /admin/api/grants` (Plan/23 §8.3, WP-GM-27; 25 §0.4): the thin alias of grants v2 for one recipient, kept for
//! the rescue page, the scripts of operators and `flyff grant`. The body is
//! `{character, item_id?, count?, gold?, note?}` (`character` is a name or an id); exactly one of `item_id` and
//! `gold` is given; the answer is `{id}`, the id of the `pending_grants` row (`source = "admin_api"`), delivered at
//! the next enter-world and by the poll once WP-ECON-09 installs the delivery.

use crate::admin::auth::Who;
use crate::admin::dto::{ItemSpec, KeepUntil};
use crate::admin::dto_items::ItemIssue;
use crate::admin::error::{ApiError, ApiResult};
use crate::admin::grants::{self, NewGrant, ValidateCx};
use crate::admin::guard::{audit_admin, rank};
use crate::admin::ops::AdminOps;
use axum::Json;
use axum::body::Bytes;
use axum::extract::State;
use flyff_common::CharacterId;
use flyff_common::gm::Authority;
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

/// The body of the alias.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AliasBody {
    /// A character name, or its id.
    character: serde_json::Value,
    #[serde(default)]
    item_id: Option<u32>,
    #[serde(default)]
    count: Option<u32>,
    #[serde(default)]
    gold: Option<i64>,
    #[serde(default)]
    note: Option<String>,
}

fn issue_text(issues: &[ItemIssue]) -> String {
    issues
        .iter()
        .map(|i| format!("{}: {}", i.field, i.message))
        .collect::<Vec<_>>()
        .join("; ")
}

/// Resolves a grant target by name or id, with rank protection; returns the character.
pub(crate) async fn character_of(
    ops: &AdminOps,
    who: &crate::admin::identity::Identity,
    value: &serde_json::Value,
) -> ApiResult<(CharacterId, String)> {
    let fact = match value {
        serde_json::Value::Number(n) => {
            let id = n
                .as_u64()
                .and_then(|v| u32::try_from(v).ok())
                .ok_or_else(|| ApiError::invalid("character", "not a character id"))?;
            ops.persist
                .admin_recipient_facts(vec![CharacterId(id)])
                .await
                .map_err(|e| ApiError::db(&e))?
                .into_iter()
                .next()
        }
        serde_json::Value::String(name) => ops
            .persist
            .admin_recipient_by_name(name.trim().to_owned())
            .await
            .map_err(|e| ApiError::db(&e))?,
        _ => return Err(ApiError::invalid("character", "a character name or id")),
    };
    let Some(f) = fact else {
        return Err(ApiError::not_found("no such character"));
    };
    if f.state != flyff_db::CharacterState::Active {
        return Err(ApiError::invalid(
            "character",
            format!("{} is not a playable character", f.name),
        ));
    }
    rank::check_target(
        who,
        ops.admin.resolver.target_authority(
            Authority::from_db_byte(f.account_authority),
            Some(Authority::from_db_byte(f.character_authority)),
            &f.account_name,
            Some(&f.name),
        ),
    )?;
    Ok((f.character_id, f.name))
}

/// Builds the grant row of an alias-shaped request: exactly one of an item (`item_id`, `count`) and `gold`.
pub(crate) fn alias_grant(
    cx: &ValidateCx,
    character: CharacterId,
    item_id: Option<u32>,
    count: Option<u32>,
    gold: Option<i64>,
    source: &str,
    note: &str,
) -> ApiResult<NewGrant> {
    match (item_id, gold) {
        (Some(_), Some(_)) => Err(ApiError::invalid("gold", "give an item or penya, not both")),
        (None, None) => Err(ApiError::invalid("item_id", "give an item or penya")),
        (Some(item), None) => {
            let count = u16::try_from(count.unwrap_or(1))
                .map_err(|_| ApiError::invalid("count", "the count is 1..=9999"))?;
            let spec = ItemSpec {
                item_id: item,
                count,
                upgrade: 0,
                element: None,
                element_level: 0,
                random_options: Vec::new(),
                safe: false,
                piercing: Vec::new(),
                ultimate_gems: Vec::new(),
                vis: Vec::new(),
                pet: None,
                charged: false,
                keep_until: KeepUntil::Item,
                bound: None,
                text: None,
                guild_id: None,
            };
            let record = cx
                .record(&spec)
                .map_err(|issues| ApiError::invalid("item_id", issue_text(&issues)))?;
            Ok(NewGrant {
                character,
                item: Some(record),
                gold: 0,
                source: source.to_owned(),
                note: note.to_owned(),
            })
        }
        (None, Some(g)) => {
            if !(1..=2_147_483_647).contains(&g) {
                return Err(ApiError::invalid("gold", "the penya is 1..=2147483647"));
            }
            Ok(NewGrant {
                character,
                item: None,
                gold: g,
                source: source.to_owned(),
                note: note.to_owned(),
            })
        }
    }
}

/// `POST /admin/api/grants`.
pub async fn grants_alias(
    State(ops): State<Arc<AdminOps>>,
    Who(who): Who,
    body: Bytes,
) -> ApiResult<Json<serde_json::Value>> {
    let b: AliasBody = super::parse_json(&body)?;
    let (character, name) = character_of(&ops, &who, &b.character).await?;
    let cx = ValidateCx::of(&ops);
    let note = b.note.unwrap_or_default();
    let row = alias_grant(
        &cx,
        character,
        b.item_id,
        b.count,
        b.gold,
        "admin_api",
        &note,
    )?;
    let item_id = row.item.as_ref().map(|i| i.item_id);
    let (count, gold) = (row.item.as_ref().map(|i| i.count), row.gold);
    let id = grants::insert(&ops.persist, ops.admin.clock.unix(), row).await?;
    audit_admin(
        &ops.audit,
        "admin.grant",
        &who,
        "done",
        None,
        json!({
            "mode": "pending",
            "route": "grants",
            "recipients": 1,
            "character": {"id": character.0, "name": name},
            "item_id": item_id,
            "count": count,
            "gold": gold,
            "note": note,
            "grant_id": id,
        }),
    )?;
    Ok(Json(json!({"id": id})))
}
