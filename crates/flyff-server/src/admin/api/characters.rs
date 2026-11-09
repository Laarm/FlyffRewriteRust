//! `/admin/api/characters*` (Plan/25 §4.2-§4.6, §11.3; WP-ADMIN-16): the character list, the profile, the live and
//! saved containers, the mailbox, item and penya removal, the position (unstuck), rename, undelete and pet actions.
//!
//! | route | cap | does |
//! |---|---|---|
//! | `GET /characters?q=&status=&cursor=&limit=` | view.players | search across online, offline and deleted characters (name prefix, account prefix, id), online ones marked |
//! | `GET /characters/{id}` | view.players | the drawer's data: list row, overview (live when online), notes, pending counts |
//! | `GET /characters/{id}/items?container=` | view.inventory | one container, a group or all of them; live from the game thread, banks through an online sibling, else saved |
//! | `GET /characters/{id}/mail` | view.inventory | the mailbox (`Query::Mailbox`; it is the same for an offline character) |
//! | `POST /characters/{id}/items/remove` ✱ | act.items | one atomic removal (`AdminItemRemoval`); an offline character gets a `pending_removals` row |
//! | `POST /characters/{id}/gold` ✱ | act.items | penya of the wallet or a bank; offline: a `pending_grants` / `pending_removals` row |
//! | `POST /characters/{id}/position` ✱ | act.unstuck | "return to town": `TeleportCharacter` (online) or `OfflineCharacterEdit` under an admin hold |
//! | `POST /characters/{id}/rename` ✱, `.../undelete` ✱ | act.accounts | the rename service (online) or the offline edit under a hold |
//! | `POST /characters/{id}/pet` ✱ | act.items | release or revive a pet (online) |
//!
//! Every mutation takes the `Mutation<T>` envelope, checks rank protection against the target's authority, writes
//! `admin.*` audit records (`requested` before, `done` / `failed` after) and answers an [`ActionDone`].

use super::{ApiState, Routes};
use crate::admin::auth::Who;
use crate::admin::caps::Cap;
use crate::admin::dto::{ContainerLoc, ErrorCode, Page};
use crate::admin::dto_players::{
    ActionDone, CharacterProfile, ContainerView, ContainersView, GoldBody, MailboxView, PetBody,
    PositionBody, PositionTo, RemoveItemBody, RenameBody, UndeleteBody,
};
use crate::admin::error::{ApiError, ApiResult};
use crate::admin::guard::{ConfirmFacts, MutationJson, ReasonText, rank};
use crate::admin::identity::Identity;
use crate::admin::players::{self as pl, Target};
use axum::Json;
use axum::extract::{Path, Query, State};
use flyff_common::math::Vec3;
use flyff_common::{CharacterId, LayerId, UnixTime, WorldId};
use flyff_db::admin::{AccountBankRow, CharacterDocsView, CharacterSearch, CharacterStatus};
use flyff_db::{
    AtomicBatch, AtomicKind, AtomicOutcome, CharacterState, DbOp, PendingGrantRecord,
    PendingRemovalRecord, RemovalLocation,
};
use flyff_game::admin::actions::character::{CharacterEdit, EditCharacter};
use flyff_game::admin::actions::items::{AdjustGold, PetAction, PetAdminAction, RemoveItem};
use flyff_game::admin::actions::offline::{OfflineCharacterEdit, OfflineEdit};
use flyff_game::admin::actions::world::{TeleportCharacter, TeleportMode};
use flyff_game::admin::{AdminQuery, AdminQueryResult};
use flyff_game::input::{AdminCmd, AdminOutput};
use flyff_proto::pb;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// Mounts the routes of this area.
pub fn mount(r: Routes<ApiState>) -> Routes<ApiState> {
    r.get("/admin/api/characters", list)
        .get("/admin/api/characters/{id}", profile)
        .get("/admin/api/characters/{id}/items", items)
        .get("/admin/api/characters/{id}/mail", mail)
        .post("/admin/api/characters/{id}/items/remove", remove_item)
        .post("/admin/api/characters/{id}/gold", adjust_gold)
        .post("/admin/api/characters/{id}/position", position)
        .post("/admin/api/characters/{id}/rename", rename)
        .post("/admin/api/characters/{id}/undelete", undelete)
        .post("/admin/api/characters/{id}/pet", pet)
}

// ---- reads ----------------------------------------------------------------------------------------------------------

#[derive(Deserialize)]
struct ListQuery {
    q: Option<String>,
    status: Option<String>,
    cursor: Option<String>,
    limit: Option<u32>,
}

async fn list(
    State(st): State<ApiState>,
    Who(who): Who,
    Query(q): Query<ListQuery>,
) -> ApiResult<Json<Page<crate::admin::dto_players::CharacterListItem>>> {
    let now = pl::now(&st);
    let text = q.q.unwrap_or_default();
    let limit = q.limit.unwrap_or(50).clamp(1, 200);
    let roster = pl::roster(&st, who.actor()).await?;
    let status_text = q
        .status
        .as_deref()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if status_text == "online" {
        // the roster is the truth about who is in the world; the database only labels the rows
        let t = text.trim().to_lowercase();
        let id = t.parse::<u32>().ok();
        let mut hits: Vec<&flyff_game::input::OnlineEntry> = roster
            .iter()
            .filter(|o| {
                t.is_empty()
                    || o.name.to_lowercase().starts_with(&t)
                    || id == Some(o.character_id.0)
            })
            .collect();
        hits.sort_by_key(|o| o.name.to_lowercase());
        let mut items = Vec::new();
        for o in hits.into_iter().take(limit as usize) {
            if let Some(h) = st
                .ops
                .persist
                .admin_character(o.character_id)
                .await
                .map_err(|e| ApiError::db(&e))?
            {
                items.push(pl::list_item(&st, &h, Some(o), now));
            }
        }
        return Ok(Json(Page {
            items,
            next_cursor: None,
        }));
    }
    let status = CharacterStatus::parse(&status_text).ok_or_else(|| {
        ApiError::invalid(
            "status",
            "one of any, online, active, deleted, sealed, blocked",
        )
    })?;
    let cursor = match q.cursor.as_deref().filter(|c| !c.is_empty()) {
        None => None,
        Some(c) => Some(
            c.parse::<i64>()
                .map_err(|_| ApiError::invalid("cursor", "not a cursor of this list"))?,
        ),
    };
    let page = st
        .ops
        .persist
        .admin_search_characters(CharacterSearch {
            text,
            status,
            now: UnixTime(now),
            cursor,
            limit,
            ranked: false,
        })
        .await
        .map_err(|e| ApiError::db(&e))?;
    let by_id: BTreeMap<u32, &flyff_game::input::OnlineEntry> =
        roster.iter().map(|o| (o.character_id.0, o)).collect();
    let mut items: Vec<_> = page
        .items
        .iter()
        .map(|h| pl::list_item(&st, h, by_id.get(&h.id.0).copied(), now))
        .collect();
    // online first within the page (stable)
    items.sort_by_key(|i| !i.online);
    Ok(Json(Page {
        items,
        next_cursor: page.next_cursor.map(|c| c.to_string()),
    }))
}

/// The online character of the account through which banks are live.
async fn online_sibling(st: &ApiState, t: &Target) -> ApiResult<Option<(CharacterId, String)>> {
    if let Some(o) = &t.online {
        return Ok(Some((o.character_id, o.name.clone())));
    }
    match pl::game_query(st, AdminQuery::OnlineSiblings(t.account())).await? {
        AdminQueryResult::Siblings(s) => Ok(s
            .into_iter()
            .next()
            .map(|s| (CharacterId(s.character_id), s.name))),
        _ => Ok(None),
    }
}

async fn profile(
    State(st): State<ApiState>,
    Who(who): Who,
    Path(id): Path<u32>,
) -> ApiResult<Json<CharacterProfile>> {
    let t = pl::target(&st, &who, id).await?;
    let now = pl::now(&st);
    let docs = st
        .ops
        .persist
        .admin_character_docs(t.id())
        .await
        .map_err(|e| ApiError::db(&e))?
        .ok_or_else(|| ApiError::not_found(format!("character {id} does not exist")))?;
    let detail = if t.online.is_some() {
        match pl::game_query(&st, AdminQuery::InspectCharacter(t.id())).await {
            Ok(AdminQueryResult::Character(d)) if d.online => {
                let wallet = match pl::game_query(
                    &st,
                    AdminQuery::Containers {
                        character: t.id(),
                        container: flyff_game::admin::ContainerLoc::Inventory,
                    },
                )
                .await
                {
                    Ok(AdminQueryResult::Container(c)) => Some(c.gold),
                    _ => None,
                };
                pl::live_detail(&d, Some(&docs), wallet)
            }
            // it left between the roster and the query
            _ => pl::saved_detail(&docs, false),
        }
    } else {
        pl::saved_detail(&docs, false)
    };
    let notes = pl::notes_of(&st, Some(t.account()), Some(t.id())).await?;
    let (pending_grants, pending_removals) = pl::pending_counts(&st, t.id()).await?;
    let live_via = online_sibling(&st, &t).await?.map(|(c, _)| c.0);
    Ok(Json(CharacterProfile {
        character: pl::list_item(&st, &t.hit, t.online.as_ref(), now),
        detail,
        notes,
        pending_grants,
        pending_removals,
        live_via,
    }))
}

#[derive(Deserialize)]
struct ItemsQuery {
    container: Option<String>,
}

async fn items(
    State(st): State<ApiState>,
    Who(who): Who,
    Path(id): Path<u32>,
    Query(q): Query<ItemsQuery>,
) -> ApiResult<Json<ContainersView>> {
    let wanted = pl::parse_containers(q.container.as_deref())?;
    let t = pl::target(&st, &who, id).await?;
    let sibling = online_sibling(&st, &t).await?;
    // the characters of the account label the banks (any state)
    let account_chars: BTreeMap<u32, String> = st
        .ops
        .persist
        .admin_account_detail(&t.hit.account_name)
        .await
        .map_err(|e| ApiError::db(&e))?
        .map(|d| d.characters.into_iter().map(|c| (c.id.0, c.name)).collect())
        .unwrap_or_default();
    let perm = st.ops.current.lock().features.perm_extrabags;
    let mut docs: Option<CharacterDocsView> = None;
    let mut saved_banks: Option<Vec<AccountBankRow>> = None;
    let mut out: Vec<ContainerView> = Vec::new();
    for loc in wanted {
        let live_char = match loc {
            ContainerLoc::Bank { .. } => sibling.as_ref().map(|(c, _)| *c),
            _ => t.online.as_ref().map(|o| o.character_id),
        };
        if let Some(c) = live_char {
            match pl::game_query(
                &st,
                AdminQuery::Containers {
                    character: c,
                    container: pl::game_loc(loc),
                },
            )
            .await
            {
                Ok(AdminQueryResult::Container(snap)) => {
                    let owner = if snap.container.owner_character_id == t.id().0 {
                        t.hit.name.clone()
                    } else {
                        account_chars
                            .get(&snap.container.owner_character_id)
                            .cloned()
                            .unwrap_or_default()
                    };
                    let via = sibling.as_ref().map(|(_, n)| n.as_str());
                    out.push(pl::container_view(&snap, &owner, via));
                    continue;
                }
                // a bank slot nobody owns, or the avatar left meanwhile: fall back to the saved copy
                Ok(_) => continue,
                Err(e) if e.code == ErrorCode::NotFound => {}
                Err(e) => return Err(e),
            }
        }
        // saved
        if docs.is_none() {
            docs = st
                .ops
                .persist
                .admin_character_docs(t.id())
                .await
                .map_err(|e| ApiError::db(&e))?;
        }
        let Some(d) = docs.as_ref() else {
            return Err(ApiError::not_found(format!(
                "character {id} does not exist"
            )));
        };
        if matches!(loc, ContainerLoc::Bank { .. }) && saved_banks.is_none() {
            saved_banks = Some(
                st.ops
                    .persist
                    .admin_account_banks(t.account())
                    .await
                    .map_err(|e| ApiError::db(&e))?,
            );
        }
        let banks = saved_banks.as_deref().unwrap_or(&[]);
        for snap in pl::saved_containers(d, banks, &[loc], perm) {
            let owner = account_chars
                .get(&snap.container.owner_character_id)
                .cloned()
                .unwrap_or_default();
            out.push(pl::container_view(&snap, &owner, None));
        }
    }
    Ok(Json(ContainersView { containers: out }))
}

async fn mail(
    State(st): State<ApiState>,
    Who(_who): Who,
    Path(id): Path<u32>,
) -> ApiResult<Json<MailboxView>> {
    match pl::game_query(&st, AdminQuery::Mailbox(CharacterId(id))).await {
        Ok(AdminQueryResult::Mailbox(rows)) => Ok(Json(MailboxView {
            mails: rows.iter().map(pl::mail_row).collect(),
            limit: pl::MAIL_LIMIT,
        })),
        Ok(_) => Err(ApiError::internal("mailbox: unexpected answer")),
        Err(e) => Err(e),
    }
}

// ---- helpers of the mutations ---------------------------------------------------------------------------------------

fn actor_label(who: &Identity) -> String {
    who.label()
}

/// Wraps the work of a mutation in its audit records: `requested` before, `done` or `failed` after.
async fn audited<F, Fut>(
    st: &ApiState,
    ev: &'static str,
    who: &Identity,
    reason: Option<&ReasonText>,
    data: Value,
    work: F,
) -> ApiResult<ActionDone>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = ApiResult<ActionDone>>,
{
    pl::audit(st, ev, who, "requested", reason, data.clone())?;
    match work().await {
        Ok(done) => {
            let mut d = data;
            if let Some(o) = d.as_object_mut() {
                o.insert("mode".to_owned(), json!(done.mode));
                o.insert("summary".to_owned(), json!(done.summary));
                if let Some(p) = done.pending_id {
                    o.insert("pending_id".to_owned(), json!(p));
                }
            }
            pl::audit(st, ev, who, "done", reason, d)?;
            Ok(done)
        }
        Err(e) => {
            let mut d = data;
            if let Some(o) = d.as_object_mut() {
                o.insert("error".to_owned(), json!(e.code.name()));
            }
            // the failure record is best effort: the original error is what the caller needs
            let _ = pl::audit(st, ev, who, "failed", reason, d);
            Err(e)
        }
    }
}

fn done(mode: &str, summary: impl Into<String>) -> ActionDone {
    ActionDone {
        mode: mode.to_owned(),
        pending_id: None,
        summary: summary.into(),
    }
}

/// The cell of a snapshot, by display slot.
fn cell(c: &flyff_game::admin::watch::ContainerSnapshot, slot: u32) -> Option<&pb::ItemElem> {
    c.items.iter().find(|i| i.obj_id == slot)
}

/// Does the item need a typed confirmation to be removed ("+7" or better, ultimate gems; 25 §4.5)?
pub(super) fn precious(i: &pb::ItemElem) -> bool {
    i.ability_option >= 7
        || i.piercing
            .as_ref()
            .is_some_and(|p| !p.ultimate_gem_ids.is_empty())
}

async fn pending_insert(st: &ApiState, kind: &'static str, op: DbOp) -> ApiResult<i64> {
    match st
        .ops
        .persist
        .call(AtomicBatch::of_ops(AtomicKind::Other(kind), vec![op]))
        .await
        .map_err(|e| ApiError::db(&e))?
    {
        AtomicOutcome::RowInserted(id) => Ok(id),
        other => Err(ApiError::internal(&format!(
            "{kind}: unexpected outcome {other:?}"
        ))),
    }
}

// ---- RemoveItem -----------------------------------------------------------------------------------------------------

async fn remove_item(
    State(st): State<ApiState>,
    Who(who): Who,
    Path(id): Path<u32>,
    MutationJson(m): MutationJson<RemoveItemBody>,
) -> ApiResult<Json<ActionDone>> {
    let policy = st.policy();
    let reason = policy.reason_required(m.reason.as_ref())?;
    let body = m.body;
    if body.count == 0 {
        return Err(ApiError::invalid("count", "at least one piece"));
    }
    if matches!(body.container, ContainerLoc::GuildBank(_)) {
        return Err(ApiError::invalid(
            "container",
            "the guild bank is edited on the guild: PATCH /admin/api/guilds/{id}",
        ));
    }
    if matches!(body.container, ContainerLoc::Mailbox) {
        return Err(ApiError::new(
            ErrorCode::NeedsFeature,
            "mail attachments are not removed here (WP-ADMIN-27)",
        ));
    }
    let t = pl::target(&st, &who, id).await?;
    rank::check_target(&who, t.authority)?;
    // the cell as it is now (live or saved): the serial check and the facts of the confirmation
    let is_bank = matches!(body.container, ContainerLoc::Bank { .. });
    let sibling = if is_bank {
        online_sibling(&st, &t).await?
    } else {
        t.online.as_ref().map(|o| (o.character_id, o.name.clone()))
    };
    let snapshot = match &sibling {
        Some((via, _)) => match pl::game_query(
            &st,
            AdminQuery::Containers {
                character: *via,
                container: pl::game_loc(body.container),
            },
        )
        .await?
        {
            AdminQueryResult::Container(c) => *c,
            _ => return Err(ApiError::internal("containers: unexpected answer")),
        },
        None => {
            let docs = st
                .ops
                .persist
                .admin_character_docs(t.id())
                .await
                .map_err(|e| ApiError::db(&e))?
                .ok_or_else(|| ApiError::not_found(format!("character {id} does not exist")))?;
            let banks = if is_bank {
                st.ops
                    .persist
                    .admin_account_banks(t.account())
                    .await
                    .map_err(|e| ApiError::db(&e))?
            } else {
                Vec::new()
            };
            let perm = st.ops.current.lock().features.perm_extrabags;
            pl::saved_containers(&docs, &banks, &[body.container], perm)
                .into_iter()
                .next()
                .ok_or_else(|| ApiError::not_found("no such container"))?
        }
    };
    let item = cell(&snapshot, body.slot)
        .filter(|i| i.serial == body.serial)
        .cloned()
        .ok_or_else(|| {
            ApiError::new(
                ErrorCode::StaleTarget,
                format!(
                    "slot {} of {} no longer holds serial {}",
                    body.slot,
                    pl::loc_name(body.container),
                    body.serial
                ),
            )
        })?;
    if i64::from(item.num) < i64::from(body.count) {
        return Err(ApiError::new(
            ErrorCode::StaleTarget,
            format!("the stack holds {}, not {}", item.num, body.count),
        ));
    }
    policy.confirm(
        &ConfirmFacts {
            targets: 1,
            target_name: Some(&t.hit.name),
            irreversible: precious(&item),
            items: u64::from(body.count),
            ..ConfirmFacts::default()
        },
        m.confirm.as_deref(),
    )?;
    let data = json!({
        "character": id,
        "account": t.hit.account_name,
        "container": pl::loc_name(body.container),
        "slot": body.slot,
        "item_id": item.item_id,
        "serial": body.serial,
        "count": body.count,
        "upgrade": item.ability_option,
    });
    let owner = CharacterId(snapshot.container.owner_character_id);
    let result = audited(&st, "admin.removal", &who, Some(&reason), data, || async {
        match sibling {
            Some((via, _)) => {
                let out = pl::game_command(
                    &st,
                    AdminCmd::RemoveItem(RemoveItem {
                        character: via,
                        container: pl::game_loc(body.container),
                        slot: body.slot,
                        serial: body.serial,
                        count: body.count,
                    }),
                    who.actor(),
                )
                .await?;
                let left = report_info(&out)
                    .and_then(|i| i["left"].as_u64())
                    .unwrap_or(0);
                Ok(done(
                    "live",
                    format!("removed {} of {}, {left} left", body.count, item.num),
                ))
            }
            None => {
                // nobody of the account is online: the row is applied at the next login (19 §20.4)
                let location = match body.container {
                    ContainerLoc::Bank { .. } => RemovalLocation::Bank,
                    ContainerLoc::Pocket(_) => RemovalLocation::Pocket,
                    _ => RemovalLocation::Inventory,
                };
                let mut rec = PendingRemovalRecord::from_character(
                    owner,
                    location,
                    item.item_id,
                    body.count,
                    actor_label(&who),
                    UnixTime(pl::now(&st)),
                );
                rec.serial = Some(body.serial);
                rec.reason = reason.as_str().to_owned();
                let row =
                    pending_insert(&st, "admin.removal", DbOp::InsertPendingRemoval(rec)).await?;
                Ok(ActionDone {
                    mode: "pending".to_owned(),
                    pending_id: Some(row),
                    summary: format!("removal #{row} waits for the next login"),
                })
            }
        }
    })
    .await?;
    Ok(Json(result))
}

/// The `info` of the only target of a report.
fn report_info(out: &AdminOutput) -> Option<Value> {
    match out {
        AdminOutput::Report { per_target } => per_target.first().and_then(|t| t.info.clone()),
        _ => None,
    }
}

// ---- AdjustGold -----------------------------------------------------------------------------------------------------

async fn adjust_gold(
    State(st): State<ApiState>,
    Who(who): Who,
    Path(id): Path<u32>,
    MutationJson(m): MutationJson<GoldBody>,
) -> ApiResult<Json<ActionDone>> {
    let policy = st.policy();
    let reason = policy.reason_required(m.reason.as_ref())?;
    let body = m.body;
    if body.delta == 0 {
        return Err(ApiError::invalid("delta", "the change is 0"));
    }
    if !matches!(
        body.container,
        ContainerLoc::Inventory | ContainerLoc::Bank { .. }
    ) {
        return Err(ApiError::invalid(
            "container",
            "penya lives in the inventory and in the banks",
        ));
    }
    let t = pl::target(&st, &who, id).await?;
    rank::check_target(&who, t.authority)?;
    policy.confirm(
        &ConfirmFacts {
            targets: 1,
            target_name: Some(&t.hit.name),
            gold: body.delta.unsigned_abs(),
            ..ConfirmFacts::default()
        },
        m.confirm.as_deref(),
    )?;
    let is_bank = matches!(body.container, ContainerLoc::Bank { .. });
    let sibling = if is_bank {
        online_sibling(&st, &t).await?
    } else {
        t.online.as_ref().map(|o| (o.character_id, o.name.clone()))
    };
    let data = json!({
        "character": id,
        "account": t.hit.account_name,
        "container": pl::loc_name(body.container),
        "delta": body.delta,
    });
    let result = audited(&st, "admin.gold", &who, Some(&reason), data, || async {
        match sibling {
            Some((via, _)) => {
                let out = pl::game_command(
                    &st,
                    AdminCmd::AdjustGold(AdjustGold {
                        character: via,
                        container: pl::game_loc(body.container),
                        delta: body.delta,
                    }),
                    who.actor(),
                )
                .await?;
                let info = report_info(&out).unwrap_or_default();
                Ok(done(
                    "live",
                    format!(
                        "{} -> {} ({:+})",
                        info["before"].as_i64().unwrap_or(0),
                        info["after"].as_i64().unwrap_or(0),
                        info["applied"].as_i64().unwrap_or(0)
                    ),
                ))
            }
            None => {
                let now = UnixTime(pl::now(&st));
                if body.delta > 0 {
                    if is_bank {
                        return Err(ApiError::new(
                            ErrorCode::CharacterOnline,
                            "bank penya can be added only while a character of the account is online",
                        ));
                    }
                    let mut rec = PendingGrantRecord::gold(
                        t.id(),
                        body.delta,
                        format!("admin:{}", actor_label(&who)),
                        reason.as_str(),
                        now,
                    );
                    rec.note = reason.as_str().to_owned();
                    let row = pending_insert(&st, "admin.grant", DbOp::InsertPendingGrant(rec)).await?;
                    Ok(ActionDone {
                        mode: "pending".to_owned(),
                        pending_id: Some(row),
                        summary: format!("grant #{row} of {} penya waits for the next login", body.delta),
                    })
                } else {
                    let (owner, location) = match body.container {
                        ContainerLoc::Bank { slot } => {
                            let banks = st
                                .ops
                                .persist
                                .admin_account_banks(t.account())
                                .await
                                .map_err(|e| ApiError::db(&e))?;
                            let b = banks.iter().find(|b| b.slot == slot).ok_or_else(|| {
                                ApiError::not_found(format!("the account has no bank {slot}"))
                            })?;
                            (b.character_id, RemovalLocation::Bank)
                        }
                        _ => (t.id(), RemovalLocation::Inventory),
                    };
                    let mut rec = PendingRemovalRecord::from_character(
                        owner,
                        location,
                        0,
                        u32::try_from(body.delta.unsigned_abs()).unwrap_or(u32::MAX),
                        actor_label(&who),
                        now,
                    );
                    rec.reason = reason.as_str().to_owned();
                    let row =
                        pending_insert(&st, "admin.removal", DbOp::InsertPendingRemoval(rec)).await?;
                    Ok(ActionDone {
                        mode: "pending".to_owned(),
                        pending_id: Some(row),
                        summary: format!("removal #{row} of {} penya waits for the next login", -body.delta),
                    })
                }
            }
        }
    })
    .await?;
    Ok(Json(result))
}

// ---- position -------------------------------------------------------------------------------------------------------

async fn position(
    State(st): State<ApiState>,
    Who(who): Who,
    Path(id): Path<u32>,
    MutationJson(m): MutationJson<PositionBody>,
) -> ApiResult<Json<ActionDone>> {
    let policy = st.policy();
    let reason = policy.reason_optional(m.reason.as_ref())?;
    let body = m.body;
    let t = pl::target(&st, &who, id).await?;
    rank::check_target(&who, t.authority)?;
    if matches!(body.to, PositionTo::Point { .. }) {
        who.require(&st.admin.caps, Cap::ActTeleportOthers)?;
    }
    let data = json!({
        "character": id,
        "account": t.hit.account_name,
        "to": serde_json::to_value(&body.to).unwrap_or(Value::Null),
        "online": t.online.is_some(),
    });
    let result = audited(
        &st,
        "admin.character_edit",
        &who,
        reason.as_ref(),
        data,
        || async {
            if t.online.is_some() {
                let cmd = match body.to {
                    PositionTo::Revival => TeleportCharacter {
                        characters: vec![t.id()],
                        world: WorldId(0),
                        layer: LayerId(0),
                        pos: Vec3::ZERO,
                        mode: TeleportMode::Unstuck,
                    },
                    PositionTo::Point { world, layer, pos } => TeleportCharacter {
                        characters: vec![t.id()],
                        world: WorldId(world),
                        layer: LayerId(layer),
                        pos: Vec3::new(pos[0], pos[1], pos[2]),
                        mode: TeleportMode::Point,
                    },
                };
                let out =
                    pl::game_command(&st, AdminCmd::TeleportCharacter(cmd), who.actor()).await?;
                let to = report_info(&out)
                    .map(|i| i["to"].to_string())
                    .unwrap_or_default();
                return Ok(done("live", format!("moved to {to}")));
            }
            // offline: only "return to town" (25 §4.6), under a hold, through the game thread
            if !matches!(body.to, PositionTo::Revival) {
                return Err(ApiError::invalid(
                    "to",
                    "an offline character can only be returned to town",
                ));
            }
            if t.hit.state != CharacterState::Active {
                return Err(ApiError::invalid(
                    "character",
                    "only a playable character can be moved",
                ));
            }
            let docs = st
                .ops
                .persist
                .admin_character_docs(t.id())
                .await
                .map_err(|e| ApiError::db(&e))?
                .ok_or_else(|| ApiError::not_found(format!("character {id} does not exist")))?;
            let hold = pl::hold(&st, t.account())?;
            let out = pl::game_command(
                &st,
                AdminCmd::OfflineCharacterEdit(OfflineCharacterEdit {
                    character: t.id(),
                    edit: OfflineEdit::ToRevival {
                        world: WorldId(docs.row.world_id),
                        pos: Vec3::new(docs.row.pos[0], docs.row.pos[1], docs.row.pos[2]),
                    },
                }),
                who.actor(),
            )
            .await;
            drop(hold);
            let out = out?;
            let to = report_info(&out).map(|i| i.to_string()).unwrap_or_default();
            Ok(done("offline", format!("moved to {to}")))
        },
    )
    .await?;
    Ok(Json(result))
}

// ---- rename / undelete ----------------------------------------------------------------------------------------------

async fn rename(
    State(st): State<ApiState>,
    Who(who): Who,
    Path(id): Path<u32>,
    MutationJson(m): MutationJson<RenameBody>,
) -> ApiResult<Json<ActionDone>> {
    let policy = st.policy();
    let reason = policy.reason_optional(m.reason.as_ref())?;
    let body = m.body;
    let name = body.name.trim().to_owned();
    if name.is_empty() {
        return Err(ApiError::invalid("name", "the name is empty"));
    }
    let t = pl::target(&st, &who, id).await?;
    rank::check_target(&who, t.authority)?;
    // a rename cannot be undone by the console: the typed confirmation is the old name
    policy.confirm(
        &ConfirmFacts {
            targets: 1,
            target_name: Some(&t.hit.name),
            irreversible: true,
            ..ConfirmFacts::default()
        },
        m.confirm.as_deref(),
    )?;
    let data = json!({
        "character": id,
        "account": t.hit.account_name,
        "field": "name",
        "before": t.hit.name,
        "after": name,
    });
    let result = audited(
        &st,
        "admin.character_edit",
        &who,
        reason.as_ref(),
        data,
        || async {
            if t.online.is_some() {
                pl::game_command(
                    &st,
                    AdminCmd::EditCharacter(EditCharacter {
                        character: t.id(),
                        edit: CharacterEdit::Rename(name.clone()),
                    }),
                    who.actor(),
                )
                .await?;
                return Ok(done("live", format!("renamed to {name}")));
            }
            let hold = pl::hold(&st, t.account())?;
            let out = pl::game_command(
                &st,
                AdminCmd::OfflineCharacterEdit(OfflineCharacterEdit {
                    character: t.id(),
                    edit: OfflineEdit::Rename(name.clone()),
                }),
                who.actor(),
            )
            .await;
            drop(hold);
            out?;
            Ok(done("offline", format!("renamed to {name}")))
        },
    )
    .await?;
    Ok(Json(result))
}

async fn undelete(
    State(st): State<ApiState>,
    Who(who): Who,
    Path(id): Path<u32>,
    MutationJson(m): MutationJson<UndeleteBody>,
) -> ApiResult<Json<ActionDone>> {
    let policy = st.policy();
    let reason = policy.reason_optional(m.reason.as_ref())?;
    let body = m.body;
    if body.slot > 2 {
        return Err(ApiError::invalid("slot", "the slots are 0 to 2"));
    }
    let t = pl::target(&st, &who, id).await?;
    rank::check_target(&who, t.authority)?;
    if t.hit.state != CharacterState::Deleted {
        return Err(ApiError::new(
            ErrorCode::StaleTarget,
            "the character is not deleted",
        ));
    }
    let data = json!({
        "character": id,
        "account": t.hit.account_name,
        "field": "undelete",
        "slot": body.slot,
    });
    let result = audited(
        &st,
        "admin.character_edit",
        &who,
        reason.as_ref(),
        data,
        || async {
            let hold = pl::hold(&st, t.account())?;
            let out = pl::game_command(
                &st,
                AdminCmd::OfflineCharacterEdit(OfflineCharacterEdit {
                    character: t.id(),
                    edit: OfflineEdit::Undelete { slot: body.slot },
                }),
                who.actor(),
            )
            .await;
            drop(hold);
            out?;
            Ok(done("offline", format!("restored into slot {}", body.slot)))
        },
    )
    .await?;
    Ok(Json(result))
}

// ---- pets -------------------------------------------------------------------------------------------------------------

async fn pet(
    State(st): State<ApiState>,
    Who(who): Who,
    Path(id): Path<u32>,
    MutationJson(m): MutationJson<PetBody>,
) -> ApiResult<Json<ActionDone>> {
    let policy = st.policy();
    let reason = policy.reason_optional(m.reason.as_ref())?;
    let body = m.body;
    let t = pl::target(&st, &who, id).await?;
    rank::check_target(&who, t.authority)?;
    if t.online.is_none() {
        return Err(ApiError::new(
            ErrorCode::CharacterOnline,
            "pet actions need the character in the world",
        ));
    }
    let data = json!({
        "character": id,
        "field": "pet",
        "action": serde_json::to_value(body).unwrap_or(Value::Null),
    });
    let result = audited(
        &st,
        "admin.character_edit",
        &who,
        reason.as_ref(),
        data,
        || async {
            let action = match body {
                PetBody::Release => PetAdminAction::Release,
                PetBody::Revive { slot, serial } => PetAdminAction::Revive { slot, serial },
            };
            pl::game_command(
                &st,
                AdminCmd::PetAction(PetAction {
                    character: t.id(),
                    action,
                }),
                who.actor(),
            )
            .await?;
            Ok(done(
                "live",
                match body {
                    PetBody::Release => "pet released",
                    PetBody::Revive { .. } => "pet revived",
                },
            ))
        },
    )
    .await?;
    Ok(Json(result))
}
