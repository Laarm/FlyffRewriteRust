//! `/admin/api/guilds*` (Plan/25 §6, §11.3, §12; **WP-ADMIN-23**): the guild list, the drawer's detail, the wizard's
//! metadata and name check, the bank log, the war history, and the mutations (create with the cloak, edit, members,
//! disband).
//!
//! | route | cap | does |
//! |---|---|---|
//! | `GET /guilds?q=&level_min=&level_max=&at_war=&member=&sort=&dir=&cursor=&limit=` | view.players | the guild list from the game thread, sorted and paged here (`Page<GuildRow>`, the cursor is the offset) |
//! | `GET /guilds/meta` | view.players | levels, caps, logo and name limits, the cooldown switch, the penya threshold (no game round trip) |
//! | `GET /guilds/validate-name?name=` | view.players | `GuildNameCheck` (rules + case-insensitive uniqueness) |
//! | `GET /guilds/{id}` | view.players | the detail; the bank cells only with view.inventory |
//! | `GET /guilds/{id}/log?kind=&before=&limit=` | view.inventory | the bank log of the database, newest first |
//! | `GET /guilds/{id}/wars?before=&limit=` | view.players | the war history of the database |
//! | `POST /guilds` ✱ | act.guild | `GuildCreate` (+ the cloak delivered through the grant pipeline) |
//! | `PATCH /guilds/{id}` ✱ | act.guild (+ act.items for `bank_remove` and `gold`) | one `GuildEdit` |
//! | `POST /guilds/{id}/members`, `DELETE /guilds/{id}/members/{character}` | act.guild | `AddMember` / `RemoveMember` |
//! | `DELETE /guilds/{id}` | act.guild | `GuildDisband` |
//!
//! **Order of a mutation:** reason and typed confirmation (what the request alone decides), the characters the
//! command names (`404` for an unknown id) with **rank protection** (guild-level edits name no character and are not
//! rank-checked), the **account holds** (every distinct account, sorted, `OnlineRegistry::admin_hold`: a held account
//! cannot log in until the answer; an account with a registry entry gets no hold and the game thread decides: an
//! avatar in the world is accepted, an offline character of such an account is `409 character_online`; a hold
//! another administrator has is `409 in_flight`), the `admin.guild` `requested` record (`503 audit_unavailable`
//! before anything is dispatched), the command on the game thread (it answers when the batch committed), then ONE
//! `done` record per returned change with `change.audit_data()` merged in, or `failed` with the error code.

use super::{ApiState, Routes};
use crate::admin::api::IdemKey;
use crate::admin::auth::Who;
use crate::admin::caps::Cap;
use crate::admin::dto::{
    DeliveryMode, ErrorCode, ErrorDetails, GrantOutcome, GrantRequest, GuildEdit as DtoEdit,
    GuildRank as DtoRank, GuildSpec, ItemSpec, KeepUntil, Page, Reason, Recipients,
};
use crate::admin::dto_guilds::{
    AddMemberBody, GuildChanged, GuildCreated, GuildDetailView, GuildLevelRow, GuildListQuery,
    GuildLogRow, GuildLogTail, GuildMemberView, GuildMeta, GuildNameCheck, GuildNameProblem,
    GuildPerson, GuildRow, GuildWarRecord, GuildWarRow, GuildWarSide, GuildWarState,
};
use crate::admin::error::{ApiError, ApiResult};
use crate::admin::grants;
use crate::admin::guard::{AuditTrail, ConfirmFacts, MutationJson, ReasonText, rank};
use crate::admin::identity::Identity;
use crate::admin::players as pl;
use crate::lobby::online::{AdminHold, HoldRefusal};
use axum::Json;
use axum::extract::{Extension, Path, Query, State};
use axum::http::StatusCode;
use flyff_common::gm::Authority;
use flyff_common::{AccountId, CharacterId, GuildId, GuildWarId};
use flyff_db::admin::items::RecipientFact;
use flyff_db::{GuildBankLogKind, repo};
use flyff_game::admin::actions::guilds::{
    EditGuild, GuildCreate, GuildDisband, GuildEdit, GuildRank, GuildSpec as GameSpec,
};
use flyff_game::admin::queries::guilds::{
    GuildDetail, GuildFilter, GuildMemberRow, GuildRow as GameRow,
};
use flyff_game::admin::{AdminQuery, AdminQueryResult};
use flyff_game::input::{AdminCmd, AdminOutput};
use flyff_game::social::guild::bank::cloak::cloak_item_id;
use flyff_rules::social::{
    GUILD_LOGO_GM_FROM, GUILD_LOGO_MAX, GUILD_NAME_MAX, GUILD_NOTICE_MAX, GUILD_RANK_CAPS,
    MAX_GUILD_LEVEL, guild_max_members,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

/// Rows of a list page when `limit` is absent.
const PAGE_DEFAULT: u32 = 100;
/// The most rows of a page (the largest the database reads allow).
const PAGE_MAX: u32 = 500;
/// The shortest guild name (UD-21).
const NAME_MIN: u32 = 3;
/// The longest name the name check looks at.
const NAME_CHECK_MAX: usize = 64;

/// Mounts the routes of this area.
pub fn mount(r: Routes<ApiState>) -> Routes<ApiState> {
    r.get("/admin/api/guilds", list)
        .get("/admin/api/guilds/meta", meta)
        .get("/admin/api/guilds/validate-name", validate_name)
        .get("/admin/api/guilds/{id}", detail)
        .get("/admin/api/guilds/{id}/log", bank_log)
        .get("/admin/api/guilds/{id}/wars", wars)
        .post("/admin/api/guilds", create)
        .patch("/admin/api/guilds/{id}", edit)
        .post("/admin/api/guilds/{id}/members", add_member)
        .delete("/admin/api/guilds/{id}/members/{character}", remove_member)
        .delete("/admin/api/guilds/{id}", disband)
}

// ---- conversions ----------------------------------------------------------------------------------------------------

fn rank_in(r: DtoRank) -> GuildRank {
    match r {
        DtoRank::Master => GuildRank::Master,
        DtoRank::Kingpin => GuildRank::Kingpin,
        DtoRank::Captain => GuildRank::Captain,
        DtoRank::Supporter => GuildRank::Supporter,
        DtoRank::Member => GuildRank::Rookie,
    }
}

fn rank_out(r: GuildRank) -> DtoRank {
    match r {
        GuildRank::Master => DtoRank::Master,
        GuildRank::Kingpin => DtoRank::Kingpin,
        GuildRank::Captain => DtoRank::Captain,
        GuildRank::Supporter => DtoRank::Supporter,
        GuildRank::Rookie => DtoRank::Member,
    }
}

fn game_spec(s: &GuildSpec) -> GameSpec {
    GameSpec {
        name: s.name.clone(),
        master: CharacterId(s.master),
        members: s
            .members
            .iter()
            .map(|(c, r)| (CharacterId(*c), rank_in(*r)))
            .collect(),
        level: s.level,
        gold: s.gold,
        pxp: s.pxp,
        logo: s.logo,
        notice: s.notice.clone(),
        give_cloak: s.give_cloak,
        override_cooldown: s.override_cooldown,
    }
}

fn game_edit(e: &DtoEdit) -> GuildEdit {
    match e {
        DtoEdit::Rename(n) => GuildEdit::Rename(n.clone()),
        DtoEdit::Logo(l) => GuildEdit::Logo(*l),
        DtoEdit::Master(c) => GuildEdit::Master(CharacterId(*c)),
        DtoEdit::Level(l) => GuildEdit::Level(*l),
        DtoEdit::Pxp(p) => GuildEdit::Pxp(*p),
        DtoEdit::Gold(d) => GuildEdit::Gold(*d),
        DtoEdit::Notice(n) => GuildEdit::Notice(n.clone()),
        DtoEdit::AddMember(c, r) => GuildEdit::AddMember(CharacterId(*c), rank_in(*r)),
        DtoEdit::RemoveMember(c) => GuildEdit::RemoveMember(CharacterId(*c)),
        DtoEdit::Rank(c, r) => GuildEdit::Rank(CharacterId(*c), rank_in(*r)),
        DtoEdit::BankRemove {
            slot,
            serial,
            count,
        } => GuildEdit::BankRemove {
            slot: *slot,
            serial: *serial,
            count: *count,
        },
    }
}

/// The `action` of the `admin.guild` record of an edit (Plan/25 §10.3).
fn action_name(e: &DtoEdit) -> &'static str {
    match e {
        DtoEdit::Rename(_) => "rename",
        DtoEdit::Logo(_) => "logo",
        DtoEdit::Master(_) => "master",
        DtoEdit::Level(_) | DtoEdit::Pxp(_) | DtoEdit::Gold(_) => "level",
        DtoEdit::Notice(_) => "notice",
        DtoEdit::AddMember(..) | DtoEdit::RemoveMember(_) | DtoEdit::Rank(..) => "member",
        DtoEdit::BankRemove { .. } => "bank",
    }
}

/// The character an edit names (rank protection applies to it).
fn edit_character(e: &DtoEdit) -> Option<u32> {
    match e {
        DtoEdit::Master(c)
        | DtoEdit::AddMember(c, _)
        | DtoEdit::RemoveMember(c)
        | DtoEdit::Rank(c, _) => Some(*c),
        _ => None,
    }
}

fn to_war(r: &GameRow, names: &BTreeMap<GuildId, String>) -> Option<GuildWarState> {
    r.war.map(|(war, enemy)| GuildWarState {
        war_id: war.0,
        enemy_id: enemy.0,
        enemy_name: names.get(&enemy).cloned().unwrap_or_default(),
    })
}

fn to_row(r: &GameRow, names: &BTreeMap<GuildId, String>) -> GuildRow {
    GuildRow {
        id: r.id.0,
        name: r.name.clone(),
        named: r.named,
        level: u32::from(r.level),
        logo: r.logo,
        master: r.master.as_ref().map(|(c, n)| GuildPerson {
            id: c.0,
            name: n.clone(),
        }),
        members: r.members,
        online: r.online,
        max_members: r.max_members,
        gold: u64::from(r.gold),
        pxp: r.pxp,
        war: to_war(r, names),
        created_at: r.created_at,
    }
}

/// An invalid-field error from the game thread carries its field in the first words of the message
/// (`name: ...`, `level is not valid`): the details name it for the wizard (Plan/25 §6.2 "field errors land on
/// their step").
fn with_field(mut e: ApiError) -> ApiError {
    if matches!(e.code, ErrorCode::Invalid | ErrorCode::StaleTarget) && e.details.is_none() {
        let m = e.message.as_str();
        let field = [
            "name",
            "master",
            "members",
            "logo",
            "level",
            "gold",
            "notice",
            "character",
        ]
        .into_iter()
        .find(|f| {
            m.strip_prefix(f)
                .is_some_and(|rest| rest.starts_with(':') || rest.starts_with(" is not valid"))
        });
        if let Some(f) = field {
            e.details = Some(ErrorDetails {
                field: Some(f.to_owned()),
                ..ErrorDetails::default()
            });
        }
    }
    e
}

// ---- reads of the game thread ---------------------------------------------------------------------------------------

async fn rows(st: &ApiState, filter: GuildFilter) -> ApiResult<Vec<GameRow>> {
    match pl::game_query(st, AdminQuery::GuildList(filter)).await? {
        AdminQueryResult::Guilds(v) => Ok(v),
        _ => Err(ApiError::internal("guild list: unexpected answer")),
    }
}

async fn detail_of(st: &ApiState, id: u32) -> ApiResult<Box<GuildDetail>> {
    match pl::game_query(st, AdminQuery::GuildDetail(GuildId(id))).await? {
        AdminQueryResult::Guild(d) => Ok(d),
        _ => Err(ApiError::internal("guild detail: unexpected answer")),
    }
}

/// The names of every guild (for the enemy of a war, the sides of a war row).
async fn guild_names(st: &ApiState) -> ApiResult<BTreeMap<GuildId, String>> {
    Ok(rows(st, GuildFilter::default())
        .await?
        .into_iter()
        .map(|r| (r.id, r.name))
        .collect())
}

// ---- GET /guilds ----------------------------------------------------------------------------------------------------

/// The key a list is sorted by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SortKey {
    Name,
    Level,
    Members,
    Gold,
    Pxp,
    Created,
}

fn parse_sort(sort: Option<&str>) -> ApiResult<SortKey> {
    Ok(match sort.map(str::trim).filter(|s| !s.is_empty()) {
        None | Some("name") => SortKey::Name,
        Some("level") => SortKey::Level,
        Some("members") => SortKey::Members,
        Some("gold") => SortKey::Gold,
        Some("pxp") => SortKey::Pxp,
        Some("created") => SortKey::Created,
        Some(_) => {
            return Err(ApiError::invalid(
                "sort",
                "one of name, level, members, gold, pxp, created",
            ));
        }
    })
}

fn parse_desc(dir: Option<&str>) -> ApiResult<bool> {
    match dir.map(str::trim).filter(|s| !s.is_empty()) {
        None | Some("asc") => Ok(false),
        Some("desc") => Ok(true),
        Some(_) => Err(ApiError::invalid("dir", "asc or desc")),
    }
}

/// Sorts the rows by `key` (ties by id, so a page boundary is stable).
fn sort_rows(rows: &mut [GameRow], key: SortKey, desc: bool) {
    rows.sort_by(|a, b| {
        let ord = match key {
            // an unnamed guild sorts after the named ones by its id
            SortKey::Name => (a.name.is_empty(), a.name.to_lowercase())
                .cmp(&(b.name.is_empty(), b.name.to_lowercase())),
            SortKey::Level => a.level.cmp(&b.level),
            SortKey::Members => a.members.cmp(&b.members),
            SortKey::Gold => a.gold.cmp(&b.gold),
            SortKey::Pxp => a.pxp.cmp(&b.pxp),
            SortKey::Created => a.created_at.cmp(&b.created_at),
        };
        let ord = if desc { ord.reverse() } else { ord };
        ord.then(a.id.cmp(&b.id))
    });
}

fn parse_offset(cursor: Option<&str>) -> ApiResult<usize> {
    match cursor.filter(|c| !c.is_empty()) {
        None => Ok(0),
        Some(c) => c
            .parse::<usize>()
            .map_err(|_| ApiError::invalid("cursor", "not a cursor of this list")),
    }
}

async fn list(
    State(st): State<ApiState>,
    Query(q): Query<GuildListQuery>,
) -> ApiResult<Json<Page<GuildRow>>> {
    let key = parse_sort(q.sort.as_deref())?;
    let desc = parse_desc(q.dir.as_deref())?;
    let offset = parse_offset(q.cursor.as_deref())?;
    let limit = q.limit.unwrap_or(PAGE_DEFAULT).clamp(1, PAGE_MAX) as usize;
    let mut all = rows(
        &st,
        GuildFilter {
            name: q.q.unwrap_or_default(),
            level_min: q.level_min,
            level_max: q.level_max,
            at_war: q.at_war,
            member: q.member.map(CharacterId),
        },
    )
    .await?;
    sort_rows(&mut all, key, desc);
    let total = all.len();
    let page: Vec<GameRow> = all.into_iter().skip(offset).take(limit).collect();
    // the enemy names: from the page itself when it holds the enemy, else from the whole list
    let mut names: BTreeMap<GuildId, String> =
        page.iter().map(|r| (r.id, r.name.clone())).collect();
    if page
        .iter()
        .any(|r| r.war.is_some_and(|(_, e)| !names.contains_key(&e)))
    {
        names = guild_names(&st).await?;
    }
    let next = offset + page.len();
    Ok(Json(Page {
        items: page.iter().map(|r| to_row(r, &names)).collect(),
        next_cursor: (next < total).then(|| next.to_string()),
    }))
}

// ---- GET /guilds/meta -----------------------------------------------------------------------------------------------

/// The metadata, built without a game round trip.
fn build_meta(
    levels: &[flyff_data::social::guild::GuildLevelRow],
    cooldowns: bool,
    threshold: u64,
) -> GuildMeta {
    GuildMeta {
        levels: (1..=MAX_GUILD_LEVEL)
            .filter_map(|l| {
                guild_max_members(l, levels).map(|m| GuildLevelRow {
                    level: u32::try_from(l).unwrap_or(0),
                    max_members: m,
                })
            })
            .collect(),
        rank_caps: GUILD_RANK_CAPS
            .iter()
            .map(|c| u32::try_from(*c).unwrap_or(0))
            .collect(),
        logo_max: GUILD_LOGO_MAX,
        logo_gm_from: GUILD_LOGO_GM_FROM,
        name_min: NAME_MIN,
        name_max: u32::try_from(GUILD_NAME_MAX).unwrap_or(16),
        notice_max_bytes: u32::try_from(GUILD_NOTICE_MAX).unwrap_or(127),
        gold_max: u64::from(u32::MAX),
        cooldowns,
        confirm_gold_threshold: threshold,
    }
}

async fn meta(State(st): State<ApiState>) -> ApiResult<Json<GuildMeta>> {
    let cooldowns = !st.ops.current.lock().features.remove_guild_cooldown;
    Ok(Json(build_meta(
        &st.ops.http.assets.data.guild.levels,
        cooldowns,
        st.policy().confirm_gold_threshold,
    )))
}

// ---- GET /guilds/validate-name ---------------------------------------------------------------------------------------

#[derive(Deserialize)]
struct NameQuery {
    name: Option<String>,
}

async fn validate_name(
    State(st): State<ApiState>,
    Query(q): Query<NameQuery>,
) -> ApiResult<Json<GuildNameCheck>> {
    let name: String = q
        .name
        .unwrap_or_default()
        .chars()
        .take(NAME_CHECK_MAX)
        .collect();
    match pl::game_query(&st, AdminQuery::GuildNameCheck(name)).await? {
        AdminQueryResult::GuildName(c) => Ok(Json(GuildNameCheck {
            ok: c.ok,
            name: c.name,
            error: c.error.map(|e| GuildNameProblem {
                code: e.code.to_owned(),
                message: e.message,
            }),
        })),
        _ => Err(ApiError::internal("guild name check: unexpected answer")),
    }
}

// ---- GET /guilds/{id} -----------------------------------------------------------------------------------------------

fn member_view(m: &GuildMemberRow, last_seen: Option<i64>) -> GuildMemberView {
    GuildMemberView {
        character: m.character.0,
        name: m.name.clone(),
        account_id: m.account.0,
        rank: rank_out(m.rank),
        class: u32::from(m.class),
        alias: m.alias.clone(),
        level: u32::from(m.level),
        job: u32::from(m.job),
        sex: u32::from(m.sex),
        online: m.online,
        give_gold: i64::from(m.give_gold),
        give_pxp: m.give_pxp,
        surrenders: m.surrenders,
        joined_at: m.joined_at,
        last_seen,
    }
}

/// The detail view; the bank cells stay out unless `with_items`.
async fn detail_view(
    st: &ApiState,
    d: &GuildDetail,
    with_items: bool,
) -> ApiResult<GuildDetailView> {
    let mut names = BTreeMap::new();
    if let Some(enemy) = d.row.war.map(|(_, e)| e) {
        names.insert(enemy, d.enemy_name.clone().unwrap_or_default());
    }
    // the last time of an offline member: one read each (a guild has at most 80 members)
    let mut members = Vec::with_capacity(d.members.len());
    for m in &d.members {
        let seen = if m.online {
            None
        } else {
            st.ops
                .persist
                .admin_character(m.character)
                .await
                .map_err(|e| ApiError::db(&e))?
                .map(|h| h.last_seen.0)
                .filter(|t| *t > 0)
        };
        members.push(member_view(m, seen));
    }
    let mut bank = d.bank.clone();
    if !with_items {
        bank.items.clear();
    }
    let label = if d.row.named {
        d.row.name.clone()
    } else {
        format!("(unnamed #{})", d.row.id.0)
    };
    Ok(GuildDetailView {
        guild: to_row(&d.row, &names),
        notice: d.notice.clone(),
        powers: d.powers.to_vec(),
        salaries: d.salaries.to_vec(),
        war_record: GuildWarRecord {
            wins: d.war_record.wins,
            losses: d.war_record.losses,
            surrenders: d.war_record.surrenders,
            points: d.war_record.points,
        },
        war: to_war(&d.row, &names),
        rank_caps: d.rank_caps.to_vec(),
        members,
        bank: pl::container_view(&bank, &label, None),
        bank_hidden: !with_items,
        log_tail: GuildLogTail {
            put: d.log_tail.put,
            taken: d.log_tail.taken,
            penya_in: d.log_tail.penya_in,
            penya_out: d.log_tail.penya_out,
        },
    })
}

async fn detail(
    State(st): State<ApiState>,
    Who(who): Who,
    Path(id): Path<u32>,
) -> ApiResult<Json<GuildDetailView>> {
    let d = detail_of(&st, id).await?;
    let with_items = who.has(&st.admin.caps, Cap::ViewInventory);
    Ok(Json(detail_view(&st, &d, with_items).await?))
}

// ---- GET /guilds/{id}/log and /wars ----------------------------------------------------------------------------------

#[derive(Deserialize)]
struct LogQuery {
    kind: Option<String>,
    before: Option<i64>,
    cursor: Option<String>,
    limit: Option<u32>,
}

/// The kind letter of `?kind=`.
fn parse_kind(kind: Option<&str>) -> ApiResult<Option<GuildBankLogKind>> {
    let Some(k) = kind.map(str::trim).filter(|k| !k.is_empty()) else {
        return Ok(None);
    };
    GuildBankLogKind::ALL
        .into_iter()
        .find(|c| k.len() == 1 && k.starts_with(c.code()))
        .map(Some)
        .ok_or_else(|| ApiError::invalid("kind", "one of A, D, I, O, C, P, R, L"))
}

/// `limit`: 0 or absent = 100, at most 500 (a larger value is the read's `invalid`).
fn parse_limit(limit: Option<u32>) -> ApiResult<u32> {
    match limit {
        None | Some(0) => Ok(PAGE_DEFAULT),
        Some(n) if n <= PAGE_MAX => Ok(n),
        Some(_) => Err(ApiError::invalid("limit", "at most 500 rows per page")),
    }
}

async fn bank_log(
    State(st): State<ApiState>,
    Path(id): Path<u32>,
    Query(q): Query<LogQuery>,
) -> ApiResult<Json<Page<GuildLogRow>>> {
    let kind = parse_kind(q.kind.as_deref())?;
    let limit = parse_limit(q.limit)?;
    let before = match (q.before, q.cursor.as_deref().filter(|c| !c.is_empty())) {
        (Some(b), _) => Some(b),
        (None, Some(c)) => Some(
            c.parse::<i64>()
                .map_err(|_| ApiError::invalid("cursor", "not a cursor of this list"))?,
        ),
        (None, None) => None,
    };
    // the log of a guild that is gone is not served: the drawer opens live guilds only
    detail_of(&st, id).await?;
    let guild = GuildId(id);
    let log = st
        .ops
        .persist
        .read(move |c| repo::guilds::bank_log_page(c, guild, kind, before, limit))
        .await
        .map_err(|e| ApiError::db(&e))?;
    let mut who: BTreeMap<u32, String> = BTreeMap::new();
    for row in &log {
        if row.character_id.0 != 0 && !who.contains_key(&row.character_id.0) {
            let name = st
                .ops
                .persist
                .admin_character(row.character_id)
                .await
                .map_err(|e| ApiError::db(&e))?
                .map_or_else(|| format!("#{}", row.character_id.0), |h| h.name);
            who.insert(row.character_id.0, name);
        }
    }
    let next = (log.len() >= limit as usize)
        .then(|| log.last().map(|r| r.id.to_string()))
        .flatten();
    Ok(Json(Page {
        items: log
            .iter()
            .map(|r| GuildLogRow {
                id: r.id,
                character_id: r.character_id.0,
                character_name: who.get(&r.character_id.0).cloned().unwrap_or_default(),
                kind: r.kind.code().to_string(),
                item_id: r.item_id,
                upgrade_level: r.upgrade_level,
                count: r.count,
                at: r.at.0,
            })
            .collect(),
        next_cursor: next,
    }))
}

#[derive(Deserialize)]
struct WarsQuery {
    before: Option<u32>,
    cursor: Option<String>,
    limit: Option<u32>,
}

/// `declarer`, `acceptor`, `truce` or `draw` for a `WR_*` code (the even codes of `0..=7` are the declarer's wins).
fn outcome_of(code: u8) -> &'static str {
    match code {
        0 | 2 | 4 | 6 => "declarer",
        1 | 3 | 5 | 7 => "acceptor",
        8 => "truce",
        _ => "draw",
    }
}

async fn wars(
    State(st): State<ApiState>,
    Path(id): Path<u32>,
    Query(q): Query<WarsQuery>,
) -> ApiResult<Json<Page<GuildWarRow>>> {
    let limit = parse_limit(q.limit)?;
    let before = match (q.before, q.cursor.as_deref().filter(|c| !c.is_empty())) {
        (Some(b), _) => Some(b),
        (None, Some(c)) => Some(
            c.parse::<u32>()
                .map_err(|_| ApiError::invalid("cursor", "not a cursor of this list"))?,
        ),
        (None, None) => None,
    };
    detail_of(&st, id).await?;
    let guild = GuildId(id);
    let wars = st
        .ops
        .persist
        .read(move |c| repo::guilds::wars_of(c, guild, before.map(GuildWarId), limit))
        .await
        .map_err(|e| ApiError::db(&e))?;
    let names = guild_names(&st).await?;
    let name = |g: GuildId| {
        names
            .get(&g)
            .filter(|n| !n.is_empty())
            .cloned()
            .unwrap_or_else(|| format!("#{}", g.0))
    };
    let side = |s: &flyff_db::WarSideRecord| GuildWarSide {
        members: s.members,
        deaths: s.deaths,
        surrenders: s.surrenders,
        absent_s: s.absent_s,
    };
    let next = (wars.len() >= limit as usize)
        .then(|| wars.last().map(|w| w.id.0.to_string()))
        .flatten();
    Ok(Json(Page {
        items: wars
            .iter()
            .map(|w| GuildWarRow {
                id: w.id.0,
                declarer: w.declarer.0,
                declarer_name: name(w.declarer),
                acceptor: w.acceptor.0,
                acceptor_name: name(w.acceptor),
                started_at: w.started_at.0,
                ended_at: w.ended_at.map(|t| t.0),
                result: w.result.map(u32::from),
                outcome: w.result.map(|r| outcome_of(r).to_owned()),
                decl: side(&w.decl),
                acpt: side(&w.acpt),
                decl_points_after: w.decl_points_after,
                acpt_points_after: w.acpt_points_after,
            })
            .collect(),
        next_cursor: next,
    }))
}

// ---- the common steps of a mutation -----------------------------------------------------------------------------------

/// The accounts held for the duration of a command (dropped after the answer).
struct Holds<'a> {
    _guards: Vec<AdminHold<'a>>,
    held: BTreeSet<AccountId>,
}

/// Takes the admin hold of every distinct account, sorted (25 §4.6). An account with a registry entry gets no hold:
/// the game thread decides; a hold another administrator has is `409 in_flight`.
fn take_holds<'a>(st: &'a ApiState, accounts: &BTreeSet<AccountId>) -> ApiResult<Holds<'a>> {
    let mut guards = Vec::new();
    let mut held = BTreeSet::new();
    for account in accounts {
        match st.ops.core.registry.admin_hold(*account) {
            Ok(g) => {
                held.insert(*account);
                guards.push(g);
            }
            Err(HoldRefusal::Online(_)) => {}
            Err(HoldRefusal::Held) => {
                return Err(ApiError::new(
                    ErrorCode::InFlight,
                    "another edit of this account is running",
                ));
            }
        }
    }
    Ok(Holds {
        _guards: guards,
        held,
    })
}

/// The characters the command names, resolved in one read (`404` for an unknown id), each rank-protected.
async fn resolve_protected(
    st: &ApiState,
    who: &Identity,
    ids: &[u32],
) -> ApiResult<Vec<RecipientFact>> {
    let mut seen = BTreeSet::new();
    let ids: Vec<CharacterId> = ids
        .iter()
        .filter(|i| seen.insert(**i))
        .map(|i| CharacterId(*i))
        .collect();
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let facts = st
        .ops
        .persist
        .admin_recipient_facts(ids.clone())
        .await
        .map_err(|e| ApiError::db(&e))?;
    for id in &ids {
        let Some(f) = facts.iter().find(|f| f.character_id == *id) else {
            return Err(ApiError::not_found(format!(
                "character {} does not exist",
                id.0
            )));
        };
        let authority = st.admin.resolver.target_authority(
            Authority::from_db_byte(f.account_authority),
            Some(Authority::from_db_byte(f.character_authority)),
            &f.account_name,
            Some(&f.name),
        );
        rank::check_target(who, authority)?;
    }
    Ok(facts)
}

fn accounts_of(facts: &[RecipientFact]) -> BTreeSet<AccountId> {
    facts.iter().map(|f| f.account_id).collect()
}

/// `Some(name)` for the typed confirmation of a named guild, `None` (= `CONFIRM`) for an unnamed one.
fn confirm_name(name: &str) -> Option<&str> {
    let n = name.trim();
    (!n.is_empty()).then_some(n)
}

/// What a command answered: the audit data of its changes.
fn changes_of(out: AdminOutput) -> ApiResult<Vec<Value>> {
    match out {
        AdminOutput::Report { per_target } => {
            let mut changes = Vec::new();
            for t in per_target {
                if let Err(f) = &t.outcome {
                    return Err(with_field(ApiError::from(f)));
                }
                if let Some(info) = t.info {
                    changes.push(info);
                }
            }
            Ok(changes)
        }
        other => Err(ApiError::internal(&format!(
            "the game answered a guild edit with {other:?}"
        ))),
    }
}

/// Runs a command that changes an existing guild under its audit trail and holds; answers the changes.
#[allow(clippy::too_many_arguments)] // the pieces of one audited mutation, each from a different step
async fn run_audited(
    st: &ApiState,
    who: &Identity,
    reason: Option<ReasonText>,
    data: Value,
    facts: &[RecipientFact],
    cmd: impl FnOnce(BTreeSet<AccountId>) -> AdminCmd,
) -> ApiResult<GuildChanged> {
    let holds = take_holds(st, &accounts_of(facts))?;
    let trail = AuditTrail::begin(&st.ops.audit, who, "admin.guild", reason, data)?;
    let result = pl::game_command(st, cmd(holds.held.clone()), who.actor()).await;
    drop(holds);
    match result
        .map_err(with_field)
        .and_then(|out| changes_of(out).map_err(with_field))
    {
        Ok(changes) => {
            if changes.is_empty() {
                trail.done(json!({"changed": false}));
            }
            for c in &changes {
                trail.done(c.clone());
            }
            Ok(GuildChanged {
                changed: !changes.is_empty(),
                changes,
            })
        }
        Err(e) => {
            trail.failed(e.code.name());
            Err(e)
        }
    }
}

// ---- POST /guilds -----------------------------------------------------------------------------------------------------

/// The overrides of the admin flow that a creation used (Plan/25 §6.2 "listed in the audit record").
fn overrides_of(spec: &GuildSpec, master_offline: bool) -> Vec<&'static str> {
    let mut v = vec!["no_npc", "no_cost", "no_party"];
    if master_offline {
        v.push("offline_master");
    }
    v.push("logo_any_level");
    if u32::from(spec.logo) >= GUILD_LOGO_GM_FROM && u32::from(spec.logo) <= GUILD_LOGO_MAX {
        v.push("gm_logo");
    }
    if spec.override_cooldown {
        v.push("cooldown_override");
    }
    v
}

/// The guild cloak for the master through the WP-ADMIN-20 pipeline in Auto mode (online with room: the bag; online
/// without room: a mail; offline: a pending grant). A failure never undoes the guild.
async fn give_cloak(
    st: &ApiState,
    who: &Identity,
    guild: u32,
    spec: &GuildSpec,
    reason: Option<&ReasonText>,
    key: Option<&str>,
) -> GrantOutcome {
    let failed = |code: ErrorCode, message: String| GrantOutcome::Failed { code, message };
    let Some(item_id) = cloak_item_id(u32::from(spec.logo), true) else {
        return failed(ErrorCode::Invalid, "this logo has no cloak".to_owned());
    };
    let text = format!("guild cloak of {}", spec.name.trim());
    let reason = match reason {
        Some(r) => r.clone(),
        None => match st.policy().reason_optional(Some(&Reason {
            preset: None,
            text: Some(text),
        })) {
            Ok(Some(r)) => r,
            _ => return failed(ErrorCode::Internal, "no reason for the cloak".to_owned()),
        },
    };
    let req = GrantRequest {
        recipients: Recipients::Characters(vec![spec.master]),
        items: vec![ItemSpec {
            item_id,
            count: 1,
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
            guild_id: Some(guild),
        }],
        gold: 0,
        mode: DeliveryMode::Auto,
    };
    let plan = match grants::plan(st, who, &req).await {
        Ok(p) => p,
        Err(e) => return failed(e.code, e.message),
    };
    let key = key.map(|k| format!("{k}:cloak"));
    match grants::run(st, who, plan, &req, &reason, key.as_deref()).await {
        Ok(report) => report.results.into_iter().next().map_or_else(
            || failed(ErrorCode::Internal, "no result".to_owned()),
            |(_, o)| o,
        ),
        Err(e) => failed(e.code, e.message),
    }
}

async fn create(
    State(st): State<ApiState>,
    Who(who): Who,
    key: Option<Extension<IdemKey>>,
    MutationJson(m): MutationJson<GuildSpec>,
) -> ApiResult<(StatusCode, Json<GuildCreated>)> {
    let policy = st.policy();
    let reason = policy.reason_optional(m.reason.as_ref())?;
    let spec = m.body;
    // the pool is a 32-bit number: refused before the confirmation asks for a name
    if spec.gold > u64::from(u32::MAX) {
        return Err(ApiError::invalid(
            "gold",
            "gold is not valid (at most 4294967295)",
        ));
    }
    policy.confirm(
        &ConfirmFacts {
            targets: 1,
            target_name: confirm_name(&spec.name),
            gold: spec.gold,
            ..ConfirmFacts::default()
        },
        m.confirm.as_deref(),
    )?;
    let mut ids = vec![spec.master];
    ids.extend(spec.members.iter().map(|(c, _)| *c));
    let facts = resolve_protected(&st, &who, &ids).await?;
    let holds = take_holds(&st, &accounts_of(&facts))?;
    let master_offline = facts
        .iter()
        .find(|f| f.character_id.0 == spec.master)
        .is_some_and(|f| holds.held.contains(&f.account_id));
    let data = json!({
        "action": "create",
        "guild": null,
        "request": serde_json::to_value(&spec).unwrap_or(Value::Null),
    });
    let trail = AuditTrail::begin(&st.ops.audit, &who, "admin.guild", reason.clone(), data)?;
    let answer = pl::game_command(
        &st,
        AdminCmd::GuildCreate(GuildCreate {
            spec: game_spec(&spec),
            held: holds.held.clone(),
        }),
        who.actor(),
    )
    .await
    .map_err(with_field);
    drop(holds);
    let id = match answer {
        Ok(AdminOutput::Guild { id }) => id.0,
        Ok(other) => {
            let e = ApiError::internal(&format!("the game answered a creation with {other:?}"));
            trail.failed(e.code.name());
            return Err(e);
        }
        Err(e) => {
            trail.failed(e.code.name());
            return Err(e);
        }
    };
    trail.done(json!({
        "action": "create",
        "guild": id,
        "character": spec.master,
        "before": null,
        "after": {
            "name": spec.name.trim(),
            "master": spec.master,
            "level": spec.level,
            "gold": spec.gold,
            "pxp": spec.pxp,
            "logo": spec.logo,
            "notice": spec.notice,
            "members": spec.members.iter().map(|(c, r)| json!([c, serde_json::to_value(r).unwrap_or(Value::Null)])).collect::<Vec<_>>(),
        },
        "overrides": overrides_of(&spec, master_offline),
    }));
    // the cloak: AFTER the guild exists, its own audited grant; a failure never undoes the guild
    let cloak = if spec.give_cloak && spec.logo != 0 {
        Some(
            give_cloak(
                &st,
                &who,
                id,
                &spec,
                reason.as_ref(),
                key.as_ref().map(|k| k.0.0.as_str()),
            )
            .await,
        )
    } else {
        None
    };
    Ok((StatusCode::CREATED, Json(GuildCreated { id, cloak })))
}

// ---- PATCH /guilds/{id} ------------------------------------------------------------------------------------------------

async fn edit(
    State(st): State<ApiState>,
    Who(who): Who,
    Path(id): Path<u32>,
    MutationJson(m): MutationJson<DtoEdit>,
) -> ApiResult<Json<GuildChanged>> {
    let policy = st.policy();
    let body = m.body;
    // the bank and its penya are under act.items as well (Plan/25 §6.3)
    if matches!(body, DtoEdit::BankRemove { .. } | DtoEdit::Gold(_)) {
        who.require(&st.admin.caps, Cap::ActItems)?;
    }
    let reason = if matches!(body, DtoEdit::BankRemove { .. } | DtoEdit::Gold(_)) {
        Some(policy.reason_required(m.reason.as_ref())?)
    } else {
        policy.reason_optional(m.reason.as_ref())?
    };
    let d = detail_of(&st, id).await?;
    let name = confirm_name(&d.row.name);
    match &body {
        DtoEdit::Gold(delta) => {
            policy.confirm(
                &ConfirmFacts {
                    targets: 1,
                    target_name: name,
                    gold: delta.unsigned_abs(),
                    ..ConfirmFacts::default()
                },
                m.confirm.as_deref(),
            )?;
        }
        DtoEdit::BankRemove {
            slot,
            serial,
            count,
        } => {
            if *count == 0 {
                return Err(ApiError::invalid("count", "at least one piece"));
            }
            // the cell as the console sees it now: a changed serial is stale (Plan/25 §14.1)
            let cell = d
                .bank
                .items
                .iter()
                .find(|i| i.obj_id == u32::from(*slot))
                .filter(|i| i.serial == *serial)
                .ok_or_else(|| {
                    ApiError::new(
                        ErrorCode::StaleTarget,
                        format!("bank slot {slot} no longer holds serial {serial}"),
                    )
                })?;
            policy.confirm(
                &ConfirmFacts {
                    targets: 1,
                    target_name: name,
                    irreversible: super::characters::precious(cell),
                    items: u64::from(*count),
                    ..ConfirmFacts::default()
                },
                m.confirm.as_deref(),
            )?;
        }
        _ => {}
    }
    let facts = resolve_protected(
        &st,
        &who,
        &edit_character(&body).into_iter().collect::<Vec<_>>(),
    )
    .await?;
    let data = json!({
        "action": action_name(&body),
        "guild": id,
        "request": serde_json::to_value(&body).unwrap_or(Value::Null),
    });
    let game = game_edit(&body);
    let out = run_audited(&st, &who, reason, data, &facts, move |held| {
        AdminCmd::GuildEdit(EditGuild {
            guild: GuildId(id),
            edit: game,
            held,
        })
    })
    .await?;
    Ok(Json(out))
}

// ---- members ---------------------------------------------------------------------------------------------------------

async fn add_member(
    State(st): State<ApiState>,
    Who(who): Who,
    Path(id): Path<u32>,
    MutationJson(m): MutationJson<AddMemberBody>,
) -> ApiResult<Json<GuildChanged>> {
    let reason = st.policy().reason_optional(m.reason.as_ref())?;
    let body = m.body;
    detail_of(&st, id).await?;
    let facts = resolve_protected(&st, &who, &[body.character]).await?;
    let data = json!({
        "action": "member",
        "guild": id,
        "request": {"add_member": [body.character, serde_json::to_value(body.rank).unwrap_or(Value::Null)]},
    });
    let out = run_audited(&st, &who, reason, data, &facts, move |held| {
        AdminCmd::GuildEdit(EditGuild {
            guild: GuildId(id),
            edit: GuildEdit::AddMember(CharacterId(body.character), rank_in(body.rank)),
            held,
        })
    })
    .await?;
    Ok(Json(out))
}

async fn remove_member(
    State(st): State<ApiState>,
    Who(who): Who,
    Path((id, character)): Path<(u32, u32)>,
    MutationJson(m): MutationJson<Value>,
) -> ApiResult<Json<GuildChanged>> {
    let reason = st.policy().reason_optional(m.reason.as_ref())?;
    detail_of(&st, id).await?;
    let facts = resolve_protected(&st, &who, &[character]).await?;
    let data = json!({
        "action": "member",
        "guild": id,
        "request": {"remove_member": character},
    });
    let out = run_audited(&st, &who, reason, data, &facts, move |held| {
        AdminCmd::GuildEdit(EditGuild {
            guild: GuildId(id),
            edit: GuildEdit::RemoveMember(CharacterId(character)),
            held,
        })
    })
    .await?;
    Ok(Json(out))
}

// ---- DELETE /guilds/{id} ----------------------------------------------------------------------------------------------

async fn disband(
    State(st): State<ApiState>,
    Who(who): Who,
    Path(id): Path<u32>,
    MutationJson(m): MutationJson<Value>,
) -> ApiResult<Json<GuildChanged>> {
    let policy = st.policy();
    let reason = policy.reason_required(m.reason.as_ref())?;
    let d = detail_of(&st, id).await?;
    policy.confirm(
        &ConfirmFacts {
            targets: 1,
            target_name: confirm_name(&d.row.name),
            irreversible: true,
            ..ConfirmFacts::default()
        },
        m.confirm.as_deref(),
    )?;
    // the master is the character of the dissolution (rank protection); the other members are not named
    let master: Vec<u32> = d.row.master.iter().map(|(c, _)| c.0).collect();
    let facts = resolve_protected(&st, &who, &master).await?;
    let data = json!({
        "action": "disband",
        "guild": id,
        "request": {"name": d.row.name, "members": d.row.members},
    });
    let text = reason.as_str().to_owned();
    let out = run_audited(&st, &who, Some(reason), data, &facts, move |held| {
        AdminCmd::GuildDisband(GuildDisband {
            guild: GuildId(id),
            reason: text,
            held,
        })
    })
    .await?;
    Ok(Json(out))
}
