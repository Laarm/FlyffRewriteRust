//! The logic behind the players routes (Plan/25 §4, §9.4; WP-ADMIN-16, WP-ADMIN-17): talking to the game thread with
//! the right time-outs, resolving a character with its rank-protection authority, the views of characters, containers,
//! mail and accounts, and the admin audit records. The handlers in `api/characters.rs` and `api/players.rs` are thin
//! on top of it.
//!
//! **Live or saved.** A character that is in the world is read from the game thread (`AdminQuery`), an offline one from
//! the database (`flyff_db::admin::players`, chunked behind lobby work). Banks are account-scoped (25 ADM-D8): they
//! are live through *any* online character of the account (`OnlineSiblings`), and read from the `banks` table only
//! when the whole account is offline.

use super::api::ApiState;
use super::dto::{ContainerLoc, ErrorCode};
use super::dto_players::BuffEntry as BuffDto;
use super::dto_players::{
    BanView, CharacterDetailView, CharacterLife, CharacterListItem, ContainerView, ItemCell,
    MailRow, NoteView, QuestEntry, SkillEntry,
};
use super::error::{ApiError, ApiResult};
use super::guard::{ReasonText, audit_admin};
use super::identity::Identity;
use base64::Engine;
use flyff_common::gm::Authority;
use flyff_common::rt::reply_pair;
use flyff_common::{AccountId, CharacterId, UnixTime};
use flyff_db::admin::{AccountBankRow, CharacterDocsView, CharacterHit};
use flyff_db::{
    AccountBanRecord, AdminNoteRecord, AdminReadQuery, CharacterState, NoteSubject,
    NoteSubjectKind, PendingFilter, queries,
};
use flyff_game::GameInput;
use flyff_game::admin::watch::{self, ContainerKind, ContainerSnapshot};
use flyff_game::admin::{AdminConnId, AdminQuery, AdminQueryResult, AdminViewOp};
use flyff_game::input::{AdminActor, AdminCmd, AdminOutput, OnlineEntry};
use flyff_proto::admin_pb as apb;
use flyff_proto::pb;
use prost::Message;
use serde_json::{Value, json};
use std::time::Duration;

/// How long a read-only query waits for the game thread (it answers in the tick).
pub const QUERY_TIMEOUT: Duration = Duration::from_secs(5);
/// How long a command waits: an offline edit answers when the database committed.
pub const COMMAND_TIMEOUT: Duration = Duration::from_secs(15);
/// How long a command waits for room in the game inbox.
const SEND_TIMEOUT: Duration = Duration::from_secs(2);
/// The window of the "Alts" reads (25 §4.7).
pub const ALT_DAYS: u32 = 30;
/// The mailbox limit (`MAX_MAIL`).
pub const MAIL_LIMIT: u32 = 50;

// ---- the game thread ------------------------------------------------------------------------------------------------

/// A read-only query to the game thread; a failure of the game (`NotFound`, ...) is the matching API error.
pub async fn game_query(st: &ApiState, q: AdminQuery) -> ApiResult<AdminQueryResult> {
    let (reply, rx) = reply_pair();
    let input = GameInput::AdminView {
        conn: AdminConnId(0),
        op: AdminViewOp::Query { q, reply },
    };
    match tokio::time::timeout(SEND_TIMEOUT, st.ops.game_tx.send_async(input)).await {
        Ok(Ok(())) => {}
        Ok(Err(_)) => return Err(ApiError::unavailable("the game thread is not running")),
        Err(_) => {
            return Err(ApiError::unavailable(
                "the game thread did not take the query",
            ));
        }
    }
    match tokio::time::timeout(QUERY_TIMEOUT, rx.recv_async()).await {
        Ok(Ok(AdminQueryResult::Failed(f))) => Err(ApiError::from(&f)),
        Ok(Ok(other)) => Ok(other),
        Ok(Err(_)) => Err(ApiError::unavailable("the game thread is not running")),
        Err(_) => Err(ApiError::unavailable(
            "the game thread did not answer in time",
        )),
    }
}

/// A command to the game thread. A refusal (`AdminOutput::Failed`) is the matching API error; a timeout may have been
/// applied late, so it is marked `maybe_applied` (an idempotency key stays in flight).
pub async fn game_command(
    st: &ApiState,
    cmd: AdminCmd,
    actor: AdminActor,
) -> ApiResult<AdminOutput> {
    let (reply, rx) = reply_pair();
    let input = GameInput::Admin { cmd, actor, reply };
    match tokio::time::timeout(SEND_TIMEOUT, st.ops.game_tx.send_async(input)).await {
        Ok(Ok(())) => {}
        Ok(Err(_)) => return Err(ApiError::unavailable("the game thread is not running")),
        Err(_) => {
            return Err(ApiError::unavailable(
                "the game thread did not take the command",
            ));
        }
    }
    match tokio::time::timeout(COMMAND_TIMEOUT, rx.recv_async()).await {
        Ok(Ok(AdminOutput::Failed(f))) => Err(ApiError::from(&f)),
        Ok(Ok(other)) => Ok(other),
        Ok(Err(_)) => Err(ApiError::unavailable("the game thread is not running")),
        Err(_) => {
            Err(ApiError::unavailable("the game thread did not answer in time").maybe_applied())
        }
    }
}

/// Every avatar in the world (the roster).
pub async fn roster(st: &ApiState, actor: AdminActor) -> ApiResult<Vec<OnlineEntry>> {
    Ok(st.ops.online(None, actor).await?)
}

/// The unix second now.
pub fn now(st: &ApiState) -> i64 {
    st.admin.clock.unix().0
}

// ---- audit ----------------------------------------------------------------------------------------------------------

/// Writes an `admin.*` record (25 §10.3): `state` is `requested`, `done` or `failed`.
pub fn audit(
    st: &ApiState,
    ev: &'static str,
    who: &Identity,
    state: &str,
    reason: Option<&ReasonText>,
    data: Value,
) -> ApiResult<()> {
    audit_admin(&st.ops.audit, ev, who, state, reason, data)
}

// ---- characters ----------------------------------------------------------------------------------------------------

/// A character with what the routes need to act on it.
pub struct Target {
    /// The database row.
    pub hit: CharacterHit,
    /// The avatar, when the character is in the world.
    pub online: Option<OnlineEntry>,
    /// The authority rank protection compares (the session's when online, else the resolved one).
    pub authority: Authority,
}

impl Target {
    /// The character id.
    pub fn id(&self) -> CharacterId {
        self.hit.id
    }

    /// The account id.
    pub fn account(&self) -> AccountId {
        self.hit.account_id
    }
}

/// Resolves character `id`: `404` when it does not exist.
pub async fn target(st: &ApiState, who: &Identity, id: u32) -> ApiResult<Target> {
    let hit = st
        .ops
        .persist
        .admin_character(CharacterId(id))
        .await
        .map_err(|e| ApiError::db(&e))?
        .ok_or_else(|| ApiError::not_found(format!("character {id} does not exist")))?;
    let online = roster(st, who.actor())
        .await?
        .into_iter()
        .find(|o| o.character_id.0 == id);
    let authority = match &online {
        Some(o) => Authority::from_db_byte(u8::try_from(u32::from(o.authority)).unwrap_or(0)),
        None => account_authority(st, &hit).await?,
    };
    Ok(Target {
        hit,
        online,
        authority,
    })
}

/// The authority rank protection compares for an offline character: the account's letter, the character's own and the
/// `[gm]` grants (25 §1.4).
async fn account_authority(st: &ApiState, hit: &CharacterHit) -> ApiResult<Authority> {
    let account = hit.account_id;
    let rec = st
        .ops
        .persist
        .read(move |c| queries::account_by_id(c, account))
        .await
        .map_err(|e| ApiError::db(&e))?;
    let Some(rec) = rec else {
        return Ok(Authority::GENERAL);
    };
    Ok(st.admin.resolver.target_authority(
        Authority::from_db_byte(rec.authority),
        Some(Authority::from_db_byte(hit.authority)),
        &rec.name,
        Some(&hit.name),
    ))
}

/// The authority letter as text (`F` for an unset byte).
pub fn letter(byte: u8) -> String {
    let c = if byte == 0 { 'F' } else { char::from(byte) };
    c.to_string()
}

/// The list row of a character.
pub fn list_item(
    st: &ApiState,
    h: &CharacterHit,
    online: Option<&OnlineEntry>,
    now: i64,
) -> CharacterListItem {
    let purge_days = i64::from(st.ops.current.lock().retention.deleted_character_days);
    CharacterListItem {
        id: h.id.0,
        name: h.name.clone(),
        account_id: h.account_id.0,
        account: h.account_name.clone(),
        slot: h.slot,
        status: match h.state {
            CharacterState::Active => CharacterLife::Active,
            CharacterState::Deleted => CharacterLife::Deleted,
            CharacterState::Sealed => CharacterLife::Sealed,
        },
        online: online.is_some(),
        level: online.map_or(u32::from(h.level), |o| u32::from(o.level)),
        job: online.map_or(u32::from(h.job), |o| u32::from(o.job)),
        world: online.map_or(h.world_id, |o| o.world),
        authority: letter(h.authority),
        created_at: h.created_at.0,
        last_seen: h.last_seen.0,
        deleted_at: h.deleted_at.map(|t| t.0),
        purge_in_days: h
            .deleted_at
            .filter(|_| h.state == CharacterState::Deleted)
            .map(|d| ((d.0 + purge_days * 86_400 - now) / 86_400).max(0)),
        login_blocked_until: h.login_blocked_until.filter(|t| t.0 > now).map(|t| t.0),
    }
}

/// The saved state of a character as the overview shows it.
pub fn saved_detail(d: &CharacterDocsView, online: bool) -> CharacterDetailView {
    let row = &d.row;
    let mut quests: Vec<QuestEntry> = d
        .docs
        .quests
        .active
        .iter()
        .map(|q| QuestEntry {
            quest: u32::from(q.quest_id),
            state: u32::from(q.state),
        })
        .collect();
    quests.extend(d.docs.quests.completed.iter().map(|q| QuestEntry {
        quest: u32::from(*q),
        state: 14,
    }));
    CharacterDetailView {
        character_id: d.identity.id.0,
        name: d.identity.name.clone(),
        online,
        level: u32::from(row.level),
        job: u32::from(row.job),
        sex: row.sex,
        exp: row.exp,
        exp_permille: None,
        str: u32::try_from(row.stat_str.max(0)).unwrap_or(0),
        sta: u32::try_from(row.stat_sta.max(0)).unwrap_or(0),
        dex: u32::try_from(row.stat_dex.max(0)).unwrap_or(0),
        int: u32::try_from(row.stat_int.max(0)).unwrap_or(0),
        stat_points: u32::try_from(row.stat_points.max(0)).unwrap_or(0),
        skill_points: u32::try_from(row.skill_points.max(0)).unwrap_or(0),
        hp: u32::try_from(row.hp.max(0)).unwrap_or(0),
        hp_max: None,
        mp: u32::try_from(row.mp.max(0)).unwrap_or(0),
        mp_max: None,
        fp: u32::try_from(row.fp.max(0)).unwrap_or(0),
        fp_max: None,
        gold: row.gold,
        skills: d
            .docs
            .skills
            .slots
            .iter()
            .flatten()
            .map(|s| SkillEntry {
                skill: s.skill_id,
                level: s.level,
            })
            .collect(),
        buffs: d
            .docs
            .buffs
            .buffs
            .iter()
            .map(|b| BuffDto {
                buff: (u32::from(b.kind) << 16) | u32::from(b.id),
                level: b.level,
                remaining_ms: b.remaining_ms,
            })
            .collect(),
        quests,
        mode_flags: row.mode_flags,
        mute_until: 0,
        mute_remaining_s: row.mute_remaining_s,
        world: row.world_id,
        layer: row.layer,
        x: row.pos[0],
        y: row.pos[1],
        z: row.pos[2],
        guild_id: d.guild_id.map_or(0, |g| g.0),
        party_id: 0,
        play_time_s: row.play_time_s,
        last_logout_at: row.last_logout_at.0,
        saved_at: d.saved_at.map(|t| t.0),
    }
}

/// The live state: the game's detail with the saved values the game does not send (raw EXP, play time).
pub fn live_detail(
    live: &watch::CharacterDetail,
    saved: Option<&CharacterDocsView>,
    wallet: Option<u64>,
) -> CharacterDetailView {
    let mut v = saved.map_or_else(
        || CharacterDetailView {
            character_id: live.character_id,
            name: live.name.clone(),
            online: true,
            level: 0,
            job: 0,
            sex: 0,
            exp: 0,
            exp_permille: None,
            str: 0,
            sta: 0,
            dex: 0,
            int: 0,
            stat_points: 0,
            skill_points: 0,
            hp: 0,
            hp_max: None,
            mp: 0,
            mp_max: None,
            fp: 0,
            fp_max: None,
            gold: 0,
            skills: Vec::new(),
            buffs: Vec::new(),
            quests: Vec::new(),
            mode_flags: 0,
            mute_until: 0,
            mute_remaining_s: 0,
            world: 0,
            layer: 0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
            guild_id: 0,
            party_id: 0,
            play_time_s: 0,
            last_logout_at: 0,
            saved_at: None,
        },
        |d| saved_detail(d, true),
    );
    v.online = true;
    v.name.clone_from(&live.name);
    v.level = live.level;
    v.job = live.job;
    v.exp_permille = Some(live.exp_permille);
    v.str = live.str;
    v.sta = live.sta;
    v.dex = live.dex;
    v.int = live.int;
    v.stat_points = live.stat_points;
    v.skill_points = live.skill_points;
    v.hp = live.hp;
    v.hp_max = Some(live.hp_max);
    v.mp = live.mp;
    v.mp_max = Some(live.mp_max);
    v.fp = live.fp;
    v.fp_max = Some(live.fp_max);
    if let Some(g) = wallet {
        v.gold = i64::try_from(g).unwrap_or(i64::MAX);
    }
    v.skills = live
        .skills
        .iter()
        .map(|s| SkillEntry {
            skill: s.skill,
            level: s.level,
        })
        .collect();
    v.buffs = live
        .buffs
        .iter()
        .map(|b| BuffDto {
            buff: b.buff,
            level: b.level,
            remaining_ms: b.remaining_ms,
        })
        .collect();
    v.quests = live
        .quests
        .iter()
        .map(|q| QuestEntry {
            quest: q.quest,
            state: q.state,
        })
        .collect();
    v.mode_flags = live.mode_flags;
    v.mute_until = i64::try_from(live.mute_until).unwrap_or(i64::MAX);
    v.mute_remaining_s = 0;
    v.world = live.world;
    v.layer = live.layer;
    v.x = live.x;
    v.z = live.z;
    v.guild_id = live.guild_id;
    v.party_id = live.party_id;
    v
}

// ---- notes ----------------------------------------------------------------------------------------------------------

/// A note as the API shows it.
pub fn note_view(n: &AdminNoteRecord) -> NoteView {
    NoteView {
        id: n.id,
        subject_kind: n.subject.kind.as_str().to_owned(),
        subject_id: n.subject.id,
        text: n.text.clone(),
        pinned: n.pinned,
        author: n.author.clone(),
        created_at: n.created_at.0,
    }
}

/// The notes of an account and of one of its characters, pinned first.
pub async fn notes_of(
    st: &ApiState,
    account: Option<AccountId>,
    character: Option<CharacterId>,
) -> ApiResult<Vec<NoteView>> {
    let mut out = Vec::new();
    if let Some(a) = account {
        let rows = st
            .ops
            .persist
            .admin_read(AdminReadQuery::Notes {
                subject: NoteSubject::account(a),
            })
            .await
            .map_err(|e| ApiError::db(&e))?
            .notes()
            .map_err(|e| ApiError::db(&e))?;
        out.extend(rows.iter().map(note_view));
    }
    if let Some(c) = character {
        let rows = st
            .ops
            .persist
            .admin_read(AdminReadQuery::Notes {
                subject: NoteSubject {
                    kind: NoteSubjectKind::Character,
                    id: c.0,
                },
            })
            .await
            .map_err(|e| ApiError::db(&e))?
            .notes()
            .map_err(|e| ApiError::db(&e))?;
        out.extend(rows.iter().map(note_view));
    }
    out.sort_by(|a, b| b.pinned.cmp(&a.pinned).then(b.id.cmp(&a.id)));
    Ok(out)
}

/// The number of open pending grants and removals of a character.
pub async fn pending_counts(st: &ApiState, c: CharacterId) -> ApiResult<(u32, u32)> {
    let filter = PendingFilter {
        character: Some(c),
        limit: 500,
        ..PendingFilter::open()
    };
    let grants = st
        .ops
        .persist
        .admin_read(AdminReadQuery::PendingGrants(filter.clone()))
        .await
        .map_err(|e| ApiError::db(&e))?
        .grants()
        .map_err(|e| ApiError::db(&e))?;
    let removals = st
        .ops
        .persist
        .admin_read(AdminReadQuery::PendingRemovals(filter))
        .await
        .map_err(|e| ApiError::db(&e))?
        .removals()
        .map_err(|e| ApiError::db(&e))?;
    Ok((
        u32::try_from(grants.items.len()).unwrap_or(u32::MAX),
        u32::try_from(removals.items.len()).unwrap_or(u32::MAX),
    ))
}

// ---- containers -----------------------------------------------------------------------------------------------------

/// The game's container location of the DTO's.
pub fn game_loc(loc: ContainerLoc) -> flyff_game::admin::ContainerLoc {
    use flyff_game::admin::ContainerLoc as G;
    match loc {
        ContainerLoc::Equipment => G::Equipment,
        ContainerLoc::Inventory => G::Inventory,
        ContainerLoc::Pocket(i) => G::Pocket(i),
        ContainerLoc::Bank { slot } => G::Bank { slot },
        ContainerLoc::GuildBank(g) => G::GuildBank(flyff_common::GuildId(g)),
        ContainerLoc::Mailbox => G::Mailbox,
    }
}

/// `equipment`, `inventory`, `pocket0`.., `bank0`.. (the audit text of a container).
pub fn loc_name(loc: ContainerLoc) -> String {
    match loc {
        ContainerLoc::Equipment => "equipment".to_owned(),
        ContainerLoc::Inventory => "inventory".to_owned(),
        ContainerLoc::Pocket(i) => format!("pocket{i}"),
        ContainerLoc::Bank { slot } => format!("bank{slot}"),
        ContainerLoc::GuildBank(g) => format!("guild_bank{g}"),
        ContainerLoc::Mailbox => "mailbox".to_owned(),
    }
}

/// Parses `?container=`: one container, a group (`pockets`, `banks`) or `all` (the default).
pub fn parse_containers(text: Option<&str>) -> ApiResult<Vec<ContainerLoc>> {
    let all = || {
        let mut v = vec![ContainerLoc::Equipment, ContainerLoc::Inventory];
        v.extend((0..3).map(ContainerLoc::Pocket));
        v.extend((0..3).map(|slot| ContainerLoc::Bank { slot }));
        v
    };
    let t = text.map(str::trim).unwrap_or("");
    let one = |digits: &str, make: fn(u8) -> ContainerLoc| -> ApiResult<Vec<ContainerLoc>> {
        digits
            .parse::<u8>()
            .ok()
            .filter(|n| *n < 3)
            .map(|n| vec![make(n)])
            .ok_or_else(|| ApiError::invalid("container", "pockets and banks are numbered 0 to 2"))
    };
    match t {
        "" | "all" => Ok(all()),
        "equipment" => Ok(vec![ContainerLoc::Equipment]),
        "inventory" => Ok(vec![ContainerLoc::Inventory]),
        "pockets" => Ok((0..3).map(ContainerLoc::Pocket).collect()),
        "banks" => Ok((0..3).map(|slot| ContainerLoc::Bank { slot }).collect()),
        other => {
            if let Some(d) = other.strip_prefix("pocket") {
                one(d.trim_start_matches(':'), ContainerLoc::Pocket)
            } else if let Some(d) = other.strip_prefix("bank") {
                one(d.trim_start_matches(':'), |slot| ContainerLoc::Bank {
                    slot,
                })
            } else {
                Err(ApiError::invalid(
                    "container",
                    "one of all, equipment, inventory, pockets, pocket0..2, banks, bank0..2",
                ))
            }
        }
    }
}

fn cell_of(i: &pb::ItemElem) -> ItemCell {
    ItemCell {
        slot: i.obj_id,
        item_id: i.item_id,
        serial: i.serial,
        count: i.num,
        upgrade: i.ability_option,
        element: i.item_resist,
        element_level: i.resist_ability_option,
        flag: i.flag,
        keep_time_at: i.keep_time_at,
    }
}

/// The REST view of a container snapshot. `owner_name` and `via_name` label the owner and the serving sibling.
pub fn container_view(
    c: &ContainerSnapshot,
    owner_name: &str,
    via_name: Option<&str>,
) -> ContainerView {
    let msg = apb::AdminServerMsg {
        msg: Some(apb::admin_server_msg::Msg::ContainerSnapshot(
            super::ws::container_msg(0, c),
        )),
    };
    let kind = match c.container.kind {
        ContainerKind::Equipment => "equipment",
        ContainerKind::Inventory => "inventory",
        ContainerKind::Pocket => "pocket",
        ContainerKind::Bank => "bank",
        ContainerKind::GuildBank => "guild_bank",
        ContainerKind::Mailbox => "mailbox",
    };
    ContainerView {
        kind: kind.to_owned(),
        index: c.container.index,
        owner_id: c.container.owner_character_id,
        owner_name: owner_name.to_owned(),
        rev: c.rev,
        capacity: c.capacity,
        gold: c.gold,
        live: c.live,
        saved_at: i64::try_from(c.saved_at).unwrap_or(0),
        live_via: (c.live_via_character_id != 0).then_some(c.live_via_character_id),
        live_via_name: via_name.map(str::to_owned),
        items: c.items.iter().map(cell_of).collect(),
        pb: base64::engine::general_purpose::STANDARD
            .encode(flyff_proto::admin::encode_server(&msg)),
    }
}

/// The saved containers of a character in the order of [`parse_containers`] (`all`).
pub fn saved_containers(
    d: &CharacterDocsView,
    banks: &[AccountBankRow],
    wanted: &[ContainerLoc],
    perm_extrabags: bool,
) -> Vec<ContainerSnapshot> {
    let owner = d.identity.id;
    let saved_at = d.saved_at.map_or(0, |t| u64::try_from(t.0).unwrap_or(0));
    let [equipment, inventory] =
        watch::saved_inventory(owner, &d.docs.inventory.container, d.row.gold, saved_at);
    let mut out = Vec::new();
    for loc in wanted {
        match *loc {
            ContainerLoc::Equipment => out.push(equipment.clone()),
            ContainerLoc::Inventory => out.push(inventory.clone()),
            ContainerLoc::Pocket(i) => {
                if let Some(p) = d.docs.pockets.pockets.get(usize::from(i)) {
                    let usable = watch::pocket_usable(
                        perm_extrabags,
                        usize::from(i),
                        p.available,
                        p.expired,
                    );
                    out.push(watch::saved_pocket(
                        owner,
                        i,
                        &p.container,
                        usable,
                        saved_at,
                    ));
                }
            }
            ContainerLoc::Bank { slot } => {
                if let Some(b) = banks.iter().find(|b| b.slot == slot) {
                    out.push(watch::saved_bank(
                        b.character_id,
                        b.slot,
                        &b.doc.container,
                        b.gold,
                        b.saved_at.map_or(0, |t| u64::try_from(t.0).unwrap_or(0)),
                    ));
                }
            }
            ContainerLoc::GuildBank(_) | ContainerLoc::Mailbox => {}
        }
    }
    out
}

/// A mail row of the mailbox view.
pub fn mail_row(m: &flyff_game::admin::queries::mail::MailView) -> MailRow {
    MailRow {
        id: m.id,
        sender_id: m.sender_id,
        sender_name: m.sender_name.clone(),
        title: m.title.clone(),
        text: m.body.clone(),
        gold: m.gold,
        item: m.item.as_ref().map(cell_of),
        item_pb: m
            .item
            .as_ref()
            .map(|i| base64::engine::general_purpose::STANDARD.encode(i.encode_to_vec())),
        created_at: m.created_at,
        expires_at: m.expires_at,
        read: m.read,
    }
}

// ---- accounts and bans -----------------------------------------------------------------------------------------------

/// An account ban as the API shows it.
pub fn ban_view(b: &AccountBanRecord, account: &str, now: i64) -> BanView {
    BanView {
        id: b.id,
        account_id: b.account_id.0,
        account: account.to_owned(),
        created_at: b.created_at.0,
        until: b.until.map(|t| t.0),
        reason: b.reason.clone(),
        by: b.created_by.clone(),
        lifted_at: b.lifted_at.map(|t| t.0),
        lifted_by: b.lifted_by.clone(),
        active: b.is_active(UnixTime(now)),
    }
}

/// `409 character_online`: the account is logged in (or its logout is draining) where the action needs it offline.
pub fn account_online_error(account: AccountId) -> ApiError {
    ApiError::new(
        ErrorCode::CharacterOnline,
        format!("account {} is in the world; use the live edit", account.0),
    )
}

/// Takes the admin hold of an account (25 §4.6): `409 character_online` while it is logged in or draining,
/// `409 in_flight` while another administrator holds it.
pub fn hold(st: &ApiState, account: AccountId) -> ApiResult<crate::lobby::online::AdminHold<'_>> {
    use crate::lobby::online::HoldRefusal;
    st.ops
        .core
        .registry
        .admin_hold(account)
        .map_err(|e| match e {
            HoldRefusal::Online(_) => account_online_error(account),
            HoldRefusal::Held => ApiError::new(
                ErrorCode::InFlight,
                "another edit of this account is running",
            ),
        })
}

/// `{"op": ..}` style summary object for the audit of an action's result.
pub fn done_json(mode: &str, extra: Value) -> Value {
    let mut m = serde_json::Map::new();
    m.insert("mode".to_owned(), json!(mode));
    if let Value::Object(o) = extra {
        m.extend(o);
    }
    Value::Object(m)
}
