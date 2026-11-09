//! `/admin/api/search`, `/accounts/{name}`, `/notes`, `/moderation/*`, `/ip-blocks`, `/staff`, `/admin-sessions`, the
//! login block and the character ban (Plan/25 §2.3, §4.7, §4.8, §9.4, §11.3; WP-ADMIN-17).
//!
//! | route | cap | does |
//! |---|---|---|
//! | `GET /search?q=` | view.players | the palette: at most 8 hits per kind, online characters first, accounts (addresses and e-mails with `view.pii`), items by id or name |
//! | `GET /accounts/{name}` | view.players | the account drawer (PII fields only with `view.pii`) |
//! | `GET /accounts/{name}/alts` | view.pii | accounts that share a registration or login address, last 30 days |
//! | `GET/POST /notes`, `DELETE /notes/{id}` | view.players / act.mute | moderation notes |
//! | `GET /moderation/{bans,mutes,blocks}` | view.players | active bans, active mutes (`Query::ListMutes`), login blocks |
//! | `GET /moderation/alts` | view.pii | clusters of accounts sharing an address |
//! | `GET/POST /ip-blocks`, `DELETE /ip-blocks/{id}` | view.pii / act.ip_block | the table, merged live into `net.ip_block` |
//! | `GET /staff`, `GET /admin-sessions`, `DELETE /admin-sessions/{id}` | view.staff / act.accounts | Staff & access |
//! | `PUT /characters/{id}/login-block`, `POST /characters/{id}/ban` | act.ban | `DbOp::SetCharacterLoginBlock`; the account ban of 12 (kick included) |
//!
//! The IP of an account or an admin session is PII (25 §12 rule 12): the routes drop it without `view.pii`.

use super::{ApiState, Routes, locations};
use crate::admin::auth::Who;
use crate::admin::caps::Cap;
use crate::admin::dto::{AdminSessionRow, ErrorCode, IdentityKind, Page};
use crate::admin::dto_players::{
    AccountView, ActionDone, AltClusterView, AltClustersView, AltView, AltsView, BanBody, BanView,
    CapRow, CharacterListItem, ClusterAccount, IpBlockBody, IpBlockView, IpBlocksView,
    LoginBlockBody, MuteView, NoteBody, NoteView, SearchHit, SearchResults, StaffRow, StaffView,
};
use crate::admin::error::{ApiError, ApiResult};
use crate::admin::guard::{MutationJson, rank};
use crate::admin::ip_blocks;
use crate::admin::players::{self as pl};
use crate::admin::session::EndReason;
use crate::lobby::accounts::AdminError;
use axum::Json;
use axum::extract::{Path, Query, State};
use flyff_common::gm::Authority;
use flyff_common::{AccountId, CharacterId, UnixTime};
use flyff_db::admin::{CharacterSearch, CharacterStatus, LOGIN_BLOCK_FOREVER, normalize_cidr};
use flyff_db::{
    AdminNoteRecord, AdminReadQuery, AtomicBatch, AtomicKind, AtomicOutcome, DbError, DbErrorKind,
    DbOp, IpBlockRecord, NoteSubject, NoteSubjectKind, queries,
};
use flyff_game::admin::queries::guilds::{GuildFilter, GuildRow};
use flyff_game::admin::{AdminQuery, AdminQueryResult};
use serde::Deserialize;
use serde_json::json;
use std::collections::BTreeSet;
use std::net::IpAddr;

/// Hits per kind of the palette (25 §2.3).
const PER_KIND: usize = 8;
/// The longest query the palette reads.
const MAX_QUERY: usize = 64;
/// A note is 1 to this many characters.
const NOTE_MAX: usize = 2_000;

/// Mounts the routes of this area.
pub fn mount(r: Routes<ApiState>) -> Routes<ApiState> {
    r.get("/admin/api/search", search)
        .get("/admin/api/accounts/{name}", account)
        .get("/admin/api/accounts/{name}/alts", alts)
        .get("/admin/api/notes", notes)
        .post("/admin/api/notes", add_note)
        .delete("/admin/api/notes/{id}", delete_note)
        .get("/admin/api/moderation/bans", bans)
        .get("/admin/api/moderation/mutes", mutes)
        .get("/admin/api/moderation/blocks", blocks)
        .get("/admin/api/moderation/alts", alt_clusters)
        .get("/admin/api/ip-blocks", ip_block_list)
        .post("/admin/api/ip-blocks", add_ip_block)
        .delete("/admin/api/ip-blocks/{id}", delete_ip_block)
        .get("/admin/api/staff", staff)
        .get("/admin/api/admin-sessions", admin_sessions)
        .delete("/admin/api/admin-sessions/{id}", end_admin_session)
        .put("/admin/api/characters/{id}/login-block", login_block)
        .post("/admin/api/characters/{id}/ban", ban_character)
}

fn db(e: &DbError) -> ApiError {
    ApiError::db(e)
}

// ---- search -----------------------------------------------------------------------------------------------------------

#[derive(Deserialize)]
struct SearchQuery {
    q: Option<String>,
}

fn rank_of(name_lower: &str, q: &str) -> Option<u8> {
    if name_lower == q {
        Some(0)
    } else if name_lower.starts_with(q) {
        Some(1)
    } else if name_lower.contains(q) {
        Some(2)
    } else {
        None
    }
}

async fn search(
    State(st): State<ApiState>,
    Who(who): Who,
    Query(query): Query<SearchQuery>,
) -> ApiResult<Json<SearchResults>> {
    let text: String = query
        .q
        .unwrap_or_default()
        .trim()
        .chars()
        .take(MAX_QUERY)
        .collect();
    let mut out = SearchResults {
        q: text.clone(),
        characters: Vec::new(),
        accounts: Vec::new(),
        guilds: Vec::new(),
        items: Vec::new(),
        locations: Vec::new(),
    };
    if text.is_empty() {
        return Ok(Json(out));
    }
    let now = pl::now(&st);
    let lower = text.to_lowercase();
    let number = text.parse::<u32>().ok();
    let roster = pl::roster(&st, who.actor()).await?;

    // characters: the online ones first (exact name, prefix, contains), then the database
    let mut live: Vec<(u8, String, &flyff_game::input::OnlineEntry)> = roster
        .iter()
        .filter_map(|o| {
            let n = o.name.to_lowercase();
            rank_of(&n, &lower)
                .or_else(|| (number == Some(o.character_id.0)).then_some(0))
                .map(|r| (r, n, o))
        })
        .collect();
    live.sort_by(|a, b| (a.0, &a.1, a.2.character_id).cmp(&(b.0, &b.1, b.2.character_id)));
    let mut seen = BTreeSet::new();
    for (_, _, o) in live.into_iter().take(PER_KIND) {
        seen.insert(o.character_id.0);
        out.characters.push(SearchHit {
            kind: "character".to_owned(),
            id: o.character_id.0.to_string(),
            label: o.name.clone(),
            detail: format!("Lv {} job {} world {}", o.level, o.job, o.world),
            online: true,
            ..SearchHit::default()
        });
    }
    if out.characters.len() < PER_KIND {
        let page = st
            .ops
            .persist
            .admin_search_characters(CharacterSearch::ranked(
                text.clone(),
                UnixTime(now),
                u32::try_from(PER_KIND * 2).unwrap_or(16),
            ))
            .await
            .map_err(|e| db(&e))?;
        for h in page.items {
            if out.characters.len() >= PER_KIND {
                break;
            }
            if !seen.insert(h.id.0) {
                continue;
            }
            let status = match h.state {
                flyff_db::CharacterState::Active => "offline",
                flyff_db::CharacterState::Deleted => "deleted",
                flyff_db::CharacterState::Sealed => "sealed",
            };
            out.characters.push(SearchHit {
                kind: "character".to_owned(),
                id: h.id.0.to_string(),
                label: h.name,
                detail: format!("Lv {} job {}, {status}, {}", h.level, h.job, h.account_name),
                online: false,
                ..SearchHit::default()
            });
        }
    }

    // accounts
    let online_accounts: BTreeSet<u32> = roster.iter().map(|o| o.account_id.0).collect();
    let pii = who.has(&st.admin.caps, Cap::ViewPii);
    for a in st
        .ops
        .persist
        .admin_search_accounts(&text, pii, UnixTime(now), PER_KIND as u32)
        .await
        .map_err(|e| db(&e))?
    {
        let mut detail = format!(
            "{} character(s), authority {}",
            a.characters,
            pl::letter(a.authority)
        );
        if a.banned {
            detail.push_str(", banned");
        }
        out.accounts.push(SearchHit {
            kind: "account".to_owned(),
            id: a.name.clone(),
            label: a.name,
            detail,
            online: online_accounts.contains(&a.id.0),
            ..SearchHit::default()
        });
    }

    // guilds: by name or id, from the live model (WP-ADMIN-23)
    match pl::game_query(
        &st,
        AdminQuery::GuildList(GuildFilter {
            name: text.clone(),
            ..GuildFilter::default()
        }),
    )
    .await?
    {
        AdminQueryResult::Guilds(rows) => {
            let mut hits: Vec<(u8, String, &GuildRow)> = rows
                .iter()
                .filter_map(|r| {
                    let n = r.name.to_lowercase();
                    rank_of(&n, &lower)
                        .or_else(|| (number == Some(r.id.0)).then_some(0))
                        .map(|k| (k, n, r))
                })
                .collect();
            hits.sort_by(|a, b| (a.0, &a.1, a.2.id).cmp(&(b.0, &b.1, b.2.id)));
            for (_, _, r) in hits.into_iter().take(PER_KIND) {
                out.guilds.push(SearchHit {
                    kind: "guild".to_owned(),
                    id: r.id.0.to_string(),
                    label: if r.named {
                        r.name.clone()
                    } else {
                        format!("(unnamed #{})", r.id.0)
                    },
                    detail: format!(
                        "Lv {} \u{b7} {} members \u{b7} master {}",
                        r.level,
                        r.members,
                        r.master.as_ref().map_or("-", |m| m.1.as_str())
                    ),
                    online: r.online > 0,
                    ..SearchHit::default()
                });
            }
        }
        _ => return Err(ApiError::internal("guild list: unexpected answer")),
    }

    // items: by id or by name (the item index of the loaded game data)
    let data = &st.ops.http.assets.data;
    if let Some(id) = number
        && let Some(p) = data.item(id)
    {
        out.items.push(SearchHit {
            kind: "item".to_owned(),
            id: id.to_string(),
            label: p.name.clone(),
            detail: format!("#{id}"),
            online: false,
            ..SearchHit::default()
        });
    }
    if lower.chars().count() >= 2 {
        let mut named: Vec<(u8, String, u32)> = data
            .items
            .iter()
            .filter_map(|(id, p)| {
                let n = p.name.to_lowercase();
                rank_of(&n, &lower).map(|r| (r, n, id))
            })
            .collect();
        named.sort_by(|a, b| (a.0, a.1.len(), &a.1, a.2).cmp(&(b.0, b.1.len(), &b.1, b.2)));
        for (_, _, id) in named {
            if out.items.len() >= PER_KIND {
                break;
            }
            if out.items.iter().any(|h| h.id == id.to_string()) {
                continue;
            }
            if let Some(p) = data.item(id) {
                out.items.push(SearchHit {
                    kind: "item".to_owned(),
                    id: id.to_string(),
                    label: p.name.clone(),
                    detail: format!("#{id}"),
                    online: false,
                    ..SearchHit::default()
                });
            }
        }
    }
    // locations: the quick locations and the shared bookmarks of the map, by name or group
    if lower.chars().count() >= 2 {
        let names = locations::world_names(&st.ops.http.assets.data);
        for l in locations::search(&st, &lower, PER_KIND).await? {
            let world = names
                .get(&l.world)
                .cloned()
                .unwrap_or_else(|| format!("World {}", l.world));
            out.locations.push(SearchHit {
                kind: "location".to_owned(),
                id: l.id,
                label: l.name,
                detail: format!("{} - {world}", l.group),
                world: Some(l.world),
                x: Some(l.x),
                z: Some(l.z),
                ..SearchHit::default()
            });
        }
    }
    Ok(Json(out))
}

// ---- accounts ---------------------------------------------------------------------------------------------------------

async fn account(
    State(st): State<ApiState>,
    Who(who): Who,
    Path(name): Path<String>,
) -> ApiResult<Json<AccountView>> {
    let d = st
        .ops
        .persist
        .admin_account_detail(&name)
        .await
        .map_err(|e| db(&e))?
        .ok_or_else(|| ApiError::not_found(format!("no account called {name}")))?;
    let pii = who.has(&st.admin.caps, Cap::ViewPii);
    let now = pl::now(&st);
    let roster = pl::roster(&st, who.actor()).await?;
    let online_chars: std::collections::BTreeMap<u32, &flyff_game::input::OnlineEntry> =
        roster.iter().map(|o| (o.character_id.0, o)).collect();
    let characters: Vec<CharacterListItem> = d
        .characters
        .iter()
        .map(|h| pl::list_item(&st, h, online_chars.get(&h.id.0).copied(), now))
        .collect();
    let bans: Vec<BanView> = d
        .bans
        .iter()
        .map(|b| pl::ban_view(b, &d.name, now))
        .collect();
    let effective = st.admin.resolver.target_authority(
        Authority::from_db_byte(d.authority),
        None,
        &d.name,
        None,
    );
    let notes = pl::notes_of(&st, Some(d.id), None).await?;
    Ok(Json(AccountView {
        id: d.id.0,
        name: d.name.clone(),
        authority: pl::letter(d.authority),
        effective_authority: effective.letter().to_string(),
        created_at: d.created_at.0,
        created_via: d.created_via.clone(),
        last_login_at: d.last_login_at.map(|t| t.0),
        last_login_ip: d.last_login_ip.clone().filter(|_| pii),
        registration_ip: d.registration_ip.clone().filter(|_| pii),
        email: d.email.clone().filter(|_| pii),
        must_change_password: d.must_change_password,
        totp_enabled: d.totp_enabled_at.is_some(),
        online: roster.iter().any(|o| o.account_id == d.id),
        banned: bans.iter().find(|b| b.active).cloned(),
        characters,
        bans,
        notes,
    }))
}

#[derive(Deserialize)]
struct DaysQuery {
    days: Option<u32>,
}

async fn alts(
    State(st): State<ApiState>,
    Who(_who): Who,
    Path(name): Path<String>,
    Query(q): Query<DaysQuery>,
) -> ApiResult<Json<AltsView>> {
    let days = q.days.unwrap_or(pl::ALT_DAYS).clamp(1, 365);
    let d = st
        .ops
        .persist
        .admin_account_detail(&name)
        .await
        .map_err(|e| db(&e))?
        .ok_or_else(|| ApiError::not_found(format!("no account called {name}")))?;
    let rows = st
        .ops
        .persist
        .admin_alts(d.id, days, UnixTime(pl::now(&st)))
        .await
        .map_err(|e| db(&e))?;
    Ok(Json(AltsView {
        days,
        alts: rows
            .into_iter()
            .map(|a| AltView {
                account_id: a.account_id.0,
                name: a.name,
                authority: pl::letter(a.authority),
                created_at: a.created_at.0,
                last_login_at: a.last_login_at.map(|t| t.0),
                shared_ips: a.shared_ips,
                banned: a.banned,
                characters: a.characters,
            })
            .collect(),
    }))
}

async fn alt_clusters(
    State(st): State<ApiState>,
    Who(_who): Who,
    Query(q): Query<DaysQuery>,
) -> ApiResult<Json<AltClustersView>> {
    let days = q.days.unwrap_or(pl::ALT_DAYS).clamp(1, 365);
    let rows = st
        .ops
        .persist
        .admin_alt_clusters(days, UnixTime(pl::now(&st)))
        .await
        .map_err(|e| db(&e))?;
    Ok(Json(AltClustersView {
        days,
        clusters: rows
            .into_iter()
            .map(|c| AltClusterView {
                ip: c.ip,
                total: c.total,
                accounts: c
                    .accounts
                    .into_iter()
                    .map(|a| ClusterAccount {
                        account_id: a.account_id.0,
                        name: a.name,
                        banned: a.banned,
                    })
                    .collect(),
            })
            .collect(),
    }))
}

// ---- notes ------------------------------------------------------------------------------------------------------------

#[derive(Deserialize)]
struct NotesQuery {
    subject_kind: Option<String>,
    subject_id: Option<u32>,
    cursor: Option<String>,
    limit: Option<u32>,
}

async fn notes(
    State(st): State<ApiState>,
    Who(_who): Who,
    Query(q): Query<NotesQuery>,
) -> ApiResult<Json<Page<NoteView>>> {
    if let (Some(kind), Some(id)) = (q.subject_kind.as_deref(), q.subject_id) {
        let kind = NoteSubjectKind::parse(kind)
            .ok_or_else(|| ApiError::invalid("subject_kind", "account or character"))?;
        let rows = st
            .ops
            .persist
            .admin_read(AdminReadQuery::Notes {
                subject: NoteSubject { kind, id },
            })
            .await
            .map_err(|e| db(&e))?
            .notes()
            .map_err(|e| db(&e))?;
        return Ok(Json(Page {
            items: rows.iter().map(pl::note_view).collect(),
            next_cursor: None,
        }));
    }
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
        .admin_read(AdminReadQuery::RecentNotes {
            cursor,
            limit: q.limit.unwrap_or(50).clamp(1, 200),
        })
        .await
        .map_err(|e| db(&e))?
        .note_page()
        .map_err(|e| db(&e))?;
    Ok(Json(Page {
        items: page.items.iter().map(pl::note_view).collect(),
        next_cursor: page.next_cursor.map(|c| c.to_string()),
    }))
}

async fn add_note(
    State(st): State<ApiState>,
    Who(who): Who,
    MutationJson(m): MutationJson<NoteBody>,
) -> ApiResult<Json<NoteView>> {
    let body = m.body;
    let kind = NoteSubjectKind::parse(&body.subject_kind)
        .ok_or_else(|| ApiError::invalid("subject_kind", "account or character"))?;
    let text = body.text.trim().to_owned();
    let chars = text.chars().count();
    if chars == 0 || chars > NOTE_MAX {
        return Err(ApiError::invalid(
            "text",
            format!("a note has 1 to {NOTE_MAX} characters"),
        ));
    }
    // the subject must exist
    let exists = match kind {
        NoteSubjectKind::Character => st
            .ops
            .persist
            .admin_character(CharacterId(body.subject_id))
            .await
            .map_err(|e| db(&e))?
            .is_some(),
        NoteSubjectKind::Account => {
            let id = AccountId(body.subject_id);
            st.ops
                .persist
                .read(move |c| queries::account_by_id(c, id))
                .await
                .map_err(|e| db(&e))?
                .is_some()
        }
    };
    if !exists {
        return Err(ApiError::not_found(format!(
            "no {} with id {}",
            kind.as_str(),
            body.subject_id
        )));
    }
    let mut rec = AdminNoteRecord::new(
        NoteSubject {
            kind,
            id: body.subject_id,
        },
        text,
        who.label(),
        UnixTime(pl::now(&st)),
    );
    rec.pinned = body.pinned;
    let id = match st
        .ops
        .persist
        .call(AtomicBatch::of_ops(
            AtomicKind::Other("admin.note"),
            vec![DbOp::InsertAdminNote(rec.clone())],
        ))
        .await
        .map_err(|e| db(&e))?
    {
        AtomicOutcome::RowInserted(id) => id,
        other => return Err(ApiError::internal(&format!("admin note: {other:?}"))),
    };
    rec.id = id;
    pl::audit(
        &st,
        "admin.note",
        &who,
        "done",
        None,
        json!({"action": "add", "id": id, "subject": kind.as_str(), "subject_id": body.subject_id, "pinned": body.pinned}),
    )?;
    Ok(Json(pl::note_view(&rec)))
}

async fn delete_note(
    State(st): State<ApiState>,
    Who(who): Who,
    Path(id): Path<i64>,
) -> ApiResult<Json<ActionDone>> {
    let n = match st
        .ops
        .persist
        .call(AtomicBatch::of_ops(
            AtomicKind::Other("admin.note"),
            vec![DbOp::DeleteAdminNote { id }],
        ))
        .await
        .map_err(|e| db(&e))?
    {
        AtomicOutcome::Changed(n) => n,
        other => return Err(ApiError::internal(&format!("admin note: {other:?}"))),
    };
    if n == 0 {
        return Err(ApiError::not_found(format!("no note {id}")));
    }
    pl::audit(
        &st,
        "admin.note",
        &who,
        "done",
        None,
        json!({"action": "delete", "id": id}),
    )?;
    Ok(Json(ActionDone {
        mode: "offline".to_owned(),
        pending_id: None,
        summary: format!("note {id} deleted"),
    }))
}

// ---- moderation lists -----------------------------------------------------------------------------------------------

#[derive(Deserialize)]
struct PageQuery {
    cursor: Option<String>,
    limit: Option<u32>,
}

fn cursor_of(q: &PageQuery) -> ApiResult<Option<i64>> {
    match q.cursor.as_deref().filter(|c| !c.is_empty()) {
        None => Ok(None),
        Some(c) => c
            .parse::<i64>()
            .map(Some)
            .map_err(|_| ApiError::invalid("cursor", "not a cursor of this list")),
    }
}

async fn bans(
    State(st): State<ApiState>,
    Who(_who): Who,
    Query(q): Query<PageQuery>,
) -> ApiResult<Json<Page<BanView>>> {
    let now = pl::now(&st);
    let page = st
        .ops
        .persist
        .admin_active_bans(
            UnixTime(now),
            cursor_of(&q)?,
            q.limit.unwrap_or(100).clamp(1, 500),
        )
        .await
        .map_err(|e| db(&e))?;
    Ok(Json(Page {
        items: page
            .items
            .iter()
            .map(|b| pl::ban_view(&b.ban, &b.account_name, now))
            .collect(),
        next_cursor: page.next_cursor.map(|c| c.to_string()),
    }))
}

async fn mutes(State(st): State<ApiState>, Who(_who): Who) -> ApiResult<Json<Page<MuteView>>> {
    match pl::game_query(&st, AdminQuery::ListMutes).await? {
        AdminQueryResult::Mutes(rows) => Ok(Json(Page {
            items: rows
                .into_iter()
                .map(|m| MuteView {
                    character_id: m.character_id,
                    account_id: m.account_id,
                    name: m.name,
                    remaining_s: m.remaining_s,
                    until: m.until,
                })
                .collect(),
            next_cursor: None,
        })),
        _ => Err(ApiError::internal("list mutes: unexpected answer")),
    }
}

async fn blocks(
    State(st): State<ApiState>,
    Who(who): Who,
    Query(q): Query<PageQuery>,
) -> ApiResult<Json<Page<CharacterListItem>>> {
    let now = pl::now(&st);
    let page = st
        .ops
        .persist
        .admin_search_characters(CharacterSearch {
            status: CharacterStatus::LoginBlocked,
            cursor: cursor_of(&q)?,
            limit: q.limit.unwrap_or(100).clamp(1, 500),
            ..CharacterSearch::list("", UnixTime(now))
        })
        .await
        .map_err(|e| db(&e))?;
    let roster = pl::roster(&st, who.actor()).await?;
    Ok(Json(Page {
        items: page
            .items
            .iter()
            .map(|h| pl::list_item(&st, h, roster.iter().find(|o| o.character_id == h.id), now))
            .collect(),
        next_cursor: page.next_cursor.map(|c| c.to_string()),
    }))
}

// ---- login block and ban -------------------------------------------------------------------------------------------------

async fn login_block(
    State(st): State<ApiState>,
    Who(who): Who,
    Path(id): Path<u32>,
    MutationJson(m): MutationJson<LoginBlockBody>,
) -> ApiResult<Json<ActionDone>> {
    let policy = st.policy();
    let body = m.body;
    let reason = policy.reason_required(m.reason.as_ref())?;
    let t = pl::target(&st, &who, id).await?;
    rank::check_target(&who, t.authority)?;
    let now = pl::now(&st);
    let until = if body.lift {
        None
    } else {
        match body.until {
            Some(u) if u <= now => {
                return Err(ApiError::invalid(
                    "until",
                    "the end of a block lies in the future",
                ));
            }
            Some(u) => Some(UnixTime(u)),
            None => Some(LOGIN_BLOCK_FOREVER),
        }
    };
    let data = json!({
        "character": id,
        "account": t.hit.account_name,
        "field": "login_block",
        "until": until.map(|u| u.0),
        "lift": body.lift,
    });
    pl::audit(
        &st,
        "admin.character_edit",
        &who,
        "requested",
        Some(&reason),
        data.clone(),
    )?;
    let outcome = st
        .ops
        .persist
        .call(AtomicBatch::of_ops(
            AtomicKind::Other("admin.login_block"),
            vec![DbOp::SetCharacterLoginBlock {
                character_id: t.id(),
                until,
            }],
        ))
        .await;
    if let Err(e) = outcome {
        let _ = pl::audit(
            &st,
            "admin.character_edit",
            &who,
            "failed",
            Some(&reason),
            data,
        );
        return Err(match e.kind {
            DbErrorKind::NotFound => ApiError::not_found("the character is deleted or gone"),
            _ => db(&e),
        });
    }
    // a character that is inside is sent out: the block refuses the login, not the stay
    let mut kicked = false;
    if !body.lift && t.online.is_some() {
        kicked = st.ops.kick_account(t.account()).await? > 0;
    }
    pl::audit(
        &st,
        "admin.character_edit",
        &who,
        "done",
        Some(&reason),
        data,
    )?;
    Ok(Json(ActionDone {
        mode: "offline".to_owned(),
        pending_id: None,
        summary: if body.lift {
            "login block lifted".to_owned()
        } else if kicked {
            "login blocked, the character was kicked".to_owned()
        } else {
            "login blocked".to_owned()
        },
    }))
}

async fn ban_character(
    State(st): State<ApiState>,
    Who(who): Who,
    Path(id): Path<u32>,
    MutationJson(m): MutationJson<BanBody>,
) -> ApiResult<Json<ActionDone>> {
    let policy = st.policy();
    let reason = policy.reason_required(m.reason.as_ref())?;
    let t = pl::target(&st, &who, id).await?;
    rank::check_target(&who, t.authority)?;
    let until = m.body.until.map(UnixTime);
    let data = json!({
        "character": id,
        "account": t.hit.account_name,
        "until": until.map(|u| u.0),
    });
    pl::audit(
        &st,
        "admin.character_edit",
        &who,
        "requested",
        Some(&reason),
        data.clone(),
    )?;
    match st
        .ops
        .accounts
        .ban(&t.hit.account_name, until, reason.as_str(), &who.label())
        .await
    {
        Ok(()) => {
            pl::audit(
                &st,
                "admin.character_edit",
                &who,
                "done",
                Some(&reason),
                data,
            )?;
            Ok(Json(ActionDone {
                mode: "offline".to_owned(),
                pending_id: None,
                summary: format!("account {} banned", t.hit.account_name),
            }))
        }
        Err(e) => {
            let _ = pl::audit(
                &st,
                "admin.character_edit",
                &who,
                "failed",
                Some(&reason),
                data,
            );
            Err(match e {
                AdminError::InvalidUntil => {
                    ApiError::invalid("until", "the end of a ban lies in the future")
                }
                AdminError::NotFound => ApiError::not_found("no such account"),
                other => ApiError::internal(&other.to_string()),
            })
        }
    }
}

// ---- IP blocks -----------------------------------------------------------------------------------------------------------

fn ip_block_view(b: &IpBlockRecord, now: i64) -> IpBlockView {
    IpBlockView {
        id: b.id,
        cidr: b.cidr.clone(),
        reason: b.reason.clone(),
        until: b.until.map(|t| t.0),
        created_by: b.created_by.clone(),
        created_at: b.created_at.0,
        active: b.until.is_none_or(|u| u.0 > now),
    }
}

async fn ip_block_list(
    State(st): State<ApiState>,
    Who(_who): Who,
) -> ApiResult<Json<IpBlocksView>> {
    let now = pl::now(&st);
    let rows = st
        .ops
        .persist
        .admin_read(AdminReadQuery::IpBlocks { active_at: None })
        .await
        .map_err(|e| db(&e))?
        .ip_blocks()
        .map_err(|e| db(&e))?;
    let config = st.ops.current.lock().net.ip_block.clone();
    Ok(Json(IpBlocksView {
        config,
        table: rows.iter().map(|b| ip_block_view(b, now)).collect(),
    }))
}

/// The first and the last address of a normalised network, to see whether it covers `ip`.
fn covers(cidr: &str, ip: IpAddr) -> bool {
    let Some((addr, prefix)) = cidr.split_once('/') else {
        return false;
    };
    let (Ok(net), Ok(len)) = (addr.parse::<IpAddr>(), prefix.parse::<u32>()) else {
        return false;
    };
    match (net.to_canonical(), ip.to_canonical()) {
        (IpAddr::V4(n), IpAddr::V4(i)) => {
            let m = if len == 0 {
                0
            } else {
                u32::MAX << (32 - len.min(32))
            };
            u32::from(n) & m == u32::from(i) & m
        }
        (IpAddr::V6(n), IpAddr::V6(i)) => {
            let m = if len == 0 {
                0
            } else {
                u128::MAX << (128 - len.min(128))
            };
            u128::from(n) & m == u128::from(i) & m
        }
        _ => false,
    }
}

async fn add_ip_block(
    State(st): State<ApiState>,
    Who(who): Who,
    MutationJson(m): MutationJson<IpBlockBody>,
) -> ApiResult<Json<IpBlockView>> {
    let policy = st.policy();
    let reason = policy.reason_required(m.reason.as_ref())?;
    let body = m.body;
    let cidr = normalize_cidr(&body.cidr)
        .map_err(|_| ApiError::invalid("cidr", "an address or a network such as 203.0.113.0/24"))?;
    let now = pl::now(&st);
    if let Some(u) = body.until
        && u <= now
    {
        return Err(ApiError::invalid(
            "until",
            "the end of a block lies in the future",
        ));
    }
    // never lock the administrator out through a remote session
    if !who.local && covers(&cidr, who.ip) {
        return Err(ApiError::invalid(
            "cidr",
            "that network contains the address you are working from",
        ));
    }
    let rec = IpBlockRecord::new(
        cidr.clone(),
        reason.as_str(),
        body.until.map(UnixTime),
        who.label(),
        UnixTime(now),
    );
    let data = json!({"cidr": cidr, "action": "add", "until": body.until});
    pl::audit(
        &st,
        "admin.ip_block",
        &who,
        "requested",
        Some(&reason),
        data.clone(),
    )?;
    let inserted = st
        .ops
        .persist
        .call(AtomicBatch::of_ops(
            AtomicKind::Other("admin.ip_block"),
            vec![DbOp::InsertIpBlock(rec.clone())],
        ))
        .await;
    let id = match inserted {
        Ok(AtomicOutcome::RowInserted(id)) => id,
        Ok(other) => return Err(ApiError::internal(&format!("ip block: {other:?}"))),
        Err(e) => {
            let _ = pl::audit(&st, "admin.ip_block", &who, "failed", Some(&reason), data);
            return Err(match e.kind {
                DbErrorKind::Conflict => {
                    ApiError::new(ErrorCode::StaleTarget, "that network is blocked already")
                }
                DbErrorKind::Invalid => ApiError::invalid("cidr", "not a network"),
                _ => db(&e),
            });
        }
    };
    ip_blocks::reload(&st.ops.persist, &st.ops.http.live)
        .await
        .map_err(|e| db(&e))?;
    pl::audit(&st, "admin.ip_block", &who, "done", Some(&reason), data)?;
    let mut view = rec;
    view.id = id;
    Ok(Json(ip_block_view(&view, now)))
}

async fn delete_ip_block(
    State(st): State<ApiState>,
    Who(who): Who,
    Path(id): Path<i64>,
) -> ApiResult<Json<ActionDone>> {
    let rows = st
        .ops
        .persist
        .admin_read(AdminReadQuery::IpBlocks { active_at: None })
        .await
        .map_err(|e| db(&e))?
        .ip_blocks()
        .map_err(|e| db(&e))?;
    let row = rows
        .iter()
        .find(|b| b.id == id)
        .ok_or_else(|| ApiError::not_found(format!("no IP block {id}")))?;
    let data = json!({"cidr": row.cidr, "action": "remove", "id": id});
    pl::audit(&st, "admin.ip_block", &who, "requested", None, data.clone())?;
    match st
        .ops
        .persist
        .call(AtomicBatch::of_ops(
            AtomicKind::Other("admin.ip_block"),
            vec![DbOp::DeleteIpBlock { id }],
        ))
        .await
        .map_err(|e| db(&e))?
    {
        AtomicOutcome::Changed(0) => return Err(ApiError::not_found(format!("no IP block {id}"))),
        AtomicOutcome::Changed(_) => {}
        other => return Err(ApiError::internal(&format!("ip block: {other:?}"))),
    }
    ip_blocks::reload(&st.ops.persist, &st.ops.http.live)
        .await
        .map_err(|e| db(&e))?;
    pl::audit(&st, "admin.ip_block", &who, "done", None, data)?;
    Ok(Json(ActionDone {
        mode: "offline".to_owned(),
        pending_id: None,
        summary: format!("{} unblocked", row.cidr),
    }))
}

// ---- staff and admin sessions ---------------------------------------------------------------------------------------------

fn session_row(mut r: AdminSessionRow, pii: bool) -> AdminSessionRow {
    if !pii {
        r.ip = None;
    }
    r
}

async fn staff(State(st): State<ApiState>, Who(who): Who) -> ApiResult<Json<StaffView>> {
    let pii = who.has(&st.admin.caps, Cap::ViewPii);
    // accounts a `[gm]` grant raises are staff although their stored letter is lower (25 §9.4)
    let extra: Vec<String> = st
        .ops
        .current
        .lock()
        .gm
        .typed()
        .map(|g| g.grants.into_iter().filter_map(|x| x.account).collect())
        .unwrap_or_default();
    let rows = st
        .ops
        .persist
        .admin_read(AdminReadQuery::StaffAccounts {
            min_authority: b'L',
            extra_accounts: extra,
        })
        .await
        .map_err(|e| db(&e))?
        .staff_accounts()
        .map_err(|e| db(&e))?;
    let sessions = st.admin.sessions.rows();
    let mut staff = Vec::new();
    for r in rows {
        let resolved = st
            .admin
            .resolver
            .staff_authority(Authority::from_db_byte(r.authority), &r.name);
        if resolved < crate::admin::identity::STAFF_MIN {
            continue;
        }
        let label = format!("staff:{}", r.name);
        staff.push(StaffRow {
            account_id: r.id.0,
            account: r.name.clone(),
            authority: resolved.letter().to_string(),
            stored_authority: pl::letter(r.authority),
            caps: st
                .admin
                .caps
                .caps_for(resolved)
                .into_iter()
                .map(str::to_owned)
                .collect(),
            last_login_at: r.last_login_at.map(|t| t.0),
            last_login_ip: r.last_login_ip.clone().filter(|_| pii),
            totp_enabled: r.totp_enabled_at.is_some(),
            must_change_password: r.must_change_password,
            sessions: sessions
                .iter()
                .filter(|s| s.identity == label)
                .cloned()
                .map(|s| session_row(s, pii))
                .collect(),
        });
    }
    let owner_sessions = sessions
        .into_iter()
        .filter(|s| s.kind != IdentityKind::Staff)
        .map(|s| session_row(s, pii))
        .collect();
    let matrix = Cap::ALL
        .iter()
        .map(|c| CapRow {
            cap: c.name().to_owned(),
            grants: c.grants().to_owned(),
            min: st.admin.caps.min(*c).letter().to_string(),
            default_min: c.default_min().letter().to_string(),
        })
        .collect();
    Ok(Json(StaffView {
        staff,
        owner_sessions,
        matrix,
    }))
}

async fn admin_sessions(
    State(st): State<ApiState>,
    Who(who): Who,
) -> ApiResult<Json<Page<AdminSessionRow>>> {
    let pii = who.has(&st.admin.caps, Cap::ViewPii);
    Ok(Json(Page {
        items: st
            .admin
            .sessions
            .rows()
            .into_iter()
            .map(|s| session_row(s, pii))
            .collect(),
        next_cursor: None,
    }))
}

async fn end_admin_session(
    State(st): State<ApiState>,
    Who(who): Who,
    Path(id): Path<String>,
) -> ApiResult<Json<ActionDone>> {
    let row = st
        .admin
        .sessions
        .rows()
        .into_iter()
        .find(|s| s.id == id)
        .ok_or_else(|| ApiError::not_found("no such admin session"))?;
    let own = who.session.as_ref().is_some_and(|s| s.id == id);
    if !own {
        // an owner session is ended by an owner; a staff session by someone of a higher letter (25 §1.6)
        if row.kind != IdentityKind::Staff {
            if !who.is_owner() {
                return Err(ApiError::forbidden_text(
                    "only an owner ends the session of an owner",
                ));
            }
        } else {
            let target = Authority::from_db_byte(row.authority.bytes().next().unwrap_or(b'F'));
            rank::check_target(&who, target)?;
        }
    }
    let data = json!({"action": "revoked", "session": id, "identity": row.identity, "authority": row.authority});
    pl::audit(&st, "admin.session", &who, "requested", None, data.clone())?;
    if !st.admin.sessions.end(&id, EndReason::Revoked) {
        return Err(ApiError::not_found("the session ended meanwhile"));
    }
    pl::audit(&st, "admin.session", &who, "done", None, data)?;
    Ok(Json(ActionDone {
        mode: "offline".to_owned(),
        pending_id: None,
        summary: format!("session of {} ended", row.identity),
    }))
}
