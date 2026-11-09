//! Player data: the queries, the update fan-out and the rename by scroll (Plan/21 §3.2-§3.4; WP-SOCIAL-08).
//!
//! * [`on_query_player_data`] / [`on_query_player_data2`]: a client asks for characters it has cached at some
//!   version; every entry whose directory version differs is answered with one `QueryPlayerDataNtf`. More than 1024
//!   entries in one request are ignored as a whole.
//! * The fan-out ([`announce_login`], [`announce_logout`], [`announce_change`], [`announce_rename`]): the directory
//!   bumps the version and `UpdatePlayerData` goes to the online characters that list the subject as party member,
//!   friend or guild member (deduplicated, [`related`]); login, level and job changes also tell every online player
//!   with `QueryPlayerDataNtf`; a logout adds `PresenceLogout`. The join batch carries the data of the friends and
//!   guild mates (`messenger::join`, step 7).
//! * [`on_query_set_player_name`]: the rename scroll. The name is validated (an invalid one **stops**, the original
//!   went on), the scroll is checked and locked, and one `CharacterRename` batch commits; on success the scroll is
//!   consumed, the directory and the avatar take the new name and `QuerySetPlayerNameNtf` goes to everybody.
//! * `submit_rename` / `complete_rename`: the batch and the success tail of a rename, shared by the scroll, the
//!   GM command `/spn` (row 120) and the admin console's rename of an online character; the tail audits
//!   `char.rename` (Plan/13 §25.2) for all of them.

use super::party::wire::to_one;
use crate::chat::ingress::notice;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::item::ops::{RemoveReason, remove_item_by_id};
use crate::outbox::Audience;
use crate::tickets::{Pending, ServiceTicket, TicketResult};
use flyff_common::defs::item::II_SYS_SYS_SCR_CHANAM;
use flyff_common::defs::text::{TID_DIAG_0020, TID_DIAG_0034};
use flyff_common::defs::world::WI_WORLD_QUIZ;
use flyff_common::{AccountId, CharacterId, ObjId};
use flyff_db::audit::ev;
use flyff_db::{AtomicBatch, AtomicKind, AuditActor, AuditEvent, BatchReply, CharacterState, DbOp};
use flyff_proto::pb;
use flyff_rules::consts::QUERYSETPLAYERNAME_MODE;
use flyff_rules::names::validate_character_name;
use std::collections::BTreeSet;

/// A `QueryPlayerData2` with more entries than this is ignored (`ref/07a §16.8`).
pub const MAX_QUERY_ENTRIES: usize = 1024;

// ---- the directory facts ------------------------------------------------------------------------------------------

/// The `QueryPlayerDataNtf` of a character the directory knows and that still exists.
pub(crate) fn ntf_of(svc: &Services, id: CharacterId) -> Option<pb::QueryPlayerDataNtf> {
    let e = svc.presence.get(id)?;
    if e.state == CharacterState::Deleted {
        return None;
    }
    Some(pb::QueryPlayerDataNtf {
        id_player: id.0,
        name: e.name.clone(),
        data: svc.presence.player_data(id),
    })
}

/// The characters that list `c` as a party member, friend or guild member (21 §3.2), in `CharacterId` order and
/// without `c`: the party mates, the couple partner and the campus members, the other members of its guild
/// (WP-SOCIAL-09) and its friends (the list of `c` while it is in the world, WP-SOCIAL-19), deduplicated. For a
/// character that is not in the world (a deletion) [`MessengerService::lists`](crate::social::messenger::MessengerService::lists)
/// answers who lists it.
pub fn related(svc: &Services, c: CharacterId) -> BTreeSet<CharacterId> {
    let mut set = BTreeSet::new();
    if let Some(p) = svc.social.party.party_record_of(c) {
        set.extend(p.members.iter().map(|m| m.id));
    }
    // the couple partner and the campus members (`OnUpdatePlayerData` of the two helpers)
    set.extend(
        svc.social.couple.partner_of(c).into_iter().chain(
            svc.social
                .campus
                .campus_of(c)
                .map(|k| k.member_ids())
                .unwrap_or_default(),
        ),
    );
    set.extend(svc.social.guild.members_of(c));
    // the friends (WP-SOCIAL-19): the list of `c` while it is in the world
    set.extend(svc.social.messenger.friends_of(c));
    set.remove(&c);
    set
}

/// The avatars of the characters of `chars` (a contact set from [`related`]) that are in the world, in the order
/// given.
fn online_objs(svc: &Services, chars: &BTreeSet<CharacterId>) -> Vec<ObjId> {
    chars
        .iter()
        .filter_map(|c| svc.presence.get(*c).and_then(|e| e.online).map(|o| o.obj))
        .collect()
}

// ---- the queries ----------------------------------------------------------------------------------------------------

/// Answers one cached entry: nothing when the client's version is current or the character is unknown.
fn answer_if_stale(
    svc: &Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    id: CharacterId,
    version: i32,
) {
    let stale = svc
        .presence
        .get(id)
        .is_some_and(|e| e.state != CharacterState::Deleted && e.data_version != version);
    if stale && let Some(n) = ntf_of(svc, id) {
        to_one(ctx, who.0, n);
    }
}

/// `QueryPlayerDataReq{id_player, version}` (`OnQueryPlayerData`).
pub fn on_query_player_data(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::QueryPlayerDataReq,
) {
    answer_if_stale(svc, ctx, who, CharacterId(m.id_player), m.version);
}

/// `QueryPlayerData2{entries}` (`OnQueryPlayerData2`): more than [`MAX_QUERY_ENTRIES`] ignores the whole request.
pub fn on_query_player_data2(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::QueryPlayerData2,
) {
    if m.entries.len() > MAX_QUERY_ENTRIES {
        return;
    }
    for e in &m.entries {
        answer_if_stale(svc, ctx, who, CharacterId(e.id_player), e.version);
    }
}

// ---- the fan-out ----------------------------------------------------------------------------------------------------

/// `UpdatePlayerData{id, data}` of `c` to the online characters that list it.
fn update_related(svc: &Services, ctx: &mut GameCtx<'_>, c: CharacterId) {
    let Some(data) = svc.presence.player_data(c) else {
        return;
    };
    for o in online_objs(svc, &related(svc, c)) {
        to_one(
            ctx,
            o,
            pb::UpdatePlayerData {
                id_player: c.0,
                data: Some(data),
            },
        );
    }
}

/// `QueryPlayerDataNtf` of `c` to every online player (`CUserMng::AddQueryPlayerData`; the original sends it to
/// all on login, level and job changes - KEEP). Encoded once.
fn query_all(svc: &Services, ctx: &mut GameCtx<'_>, c: CharacterId) {
    if let Some(n) = ntf_of(svc, c) {
        ctx.broadcast(Some(svc), ObjId::NULL, Audience::AllOnline, n);
    }
}

/// 21 §3.4 join step 1, after the directory marked `c` online (and bumped the version): the listeners learn it and
/// every player gets the new data.
pub fn announce_login(svc: &Services, ctx: &mut GameCtx<'_>, c: CharacterId) {
    update_related(svc, ctx, c);
    query_all(svc, ctx, c);
}

/// 21 §3.4 leave steps 5 and 6: `PresenceLogout{c}` to the online characters that list `c`, then the directory
/// marks it offline (version bump) and they get `UpdatePlayerData` with `login = 0`.
pub fn announce_logout(svc: &mut Services, ctx: &mut GameCtx<'_>, c: CharacterId) {
    let listeners = online_objs(svc, &related(svc, c));
    for &o in &listeners {
        to_one(ctx, o, pb::PresenceLogout { id_player: c.0 });
    }
    svc.presence.set_online(c, None);
    update_related(svc, ctx, c);
}

/// A level or job change (the directory already follows): the listeners get `UpdatePlayerData`, everybody
/// `QueryPlayerDataNtf`.
pub fn announce_change(svc: &Services, ctx: &mut GameCtx<'_>, c: CharacterId) {
    update_related(svc, ctx, c);
    query_all(svc, ctx, c);
}

/// A rename: `QuerySetPlayerNameNtf{id, name}` to everybody and `UpdatePlayerData` (the version moved) to the
/// listeners.
pub fn announce_rename(svc: &Services, ctx: &mut GameCtx<'_>, c: CharacterId, name: &str) {
    ctx.broadcast(
        Some(svc),
        ObjId::NULL,
        Audience::AllOnline,
        pb::QuerySetPlayerNameNtf {
            id_player: c.0,
            name: name.to_owned(),
        },
    );
    update_related(svc, ctx, c);
}

// ---- the rename by scroll ---------------------------------------------------------------------------------------------

/// The character and avatar name of a player, when it is in the world.
fn who_am_i(ctx: &GameCtx<'_>, who: PlayerRef) -> Option<CharacterId> {
    ctx.worlds
        .player(who.0)
        .and_then(|m| m.player())
        .map(|p| p.ident.character_id)
}

/// `QuerySetPlayerNameReq{data, name}` (`OnQuerySetPlayerName`, 21 §3.3). `LOWORD(data)` is the slot of the scroll.
pub fn on_query_set_player_name(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::QuerySetPlayerNameReq,
) {
    let Some(character) = who_am_i(ctx, who) else {
        return;
    };
    // 1. no renames in the quiz world (silent)
    if ctx
        .worlds
        .core(who.0)
        .is_some_and(|c| c.world.0 == WI_WORLD_QUIZ)
    {
        return;
    }
    // 2. the name rules: an invalid name stops here (FIX VR-046: the original went on)
    let gameplay = &ctx.cfg.rules.gameplay;
    let Ok(name) = validate_character_name(
        &m.name,
        &ctx.data.name_rules,
        gameplay.min_name_letters,
        gameplay.max_name_letters,
    ) else {
        notice::defined(ctx, who, TID_DIAG_0020, Vec::new());
        return;
    };
    let name = name.to_owned();
    // 3. the scroll: a usable `II_SYS_SYS_SCR_CHANAM` in the named slot; a rename that is in flight locks it
    let slot = (m.data & 0xFFFF) as u16;
    if (slot as i16) < 0 {
        return; // 0xFFFF: the forced rename of the server merge, never a client's
    }
    let Some(p) = ctx.worlds.player(who.0).and_then(|mv| mv.player()) else {
        return;
    };
    let scroll_ok = p
        .items
        .inventory
        .get_at_id(slot)
        .is_some_and(|it| it.is_usable() && it.item_id == II_SYS_SYS_SCR_CHANAM);
    if !scroll_ok || p.social.rename_pending {
        return;
    }
    // 4. uniqueness (the commit re-checks): the original DB error reply
    let taken = match svc.presence.by_name(&name) {
        Some(e) => e.id != character,
        None => svc.presence.name_taken(&name), // a pending creation holds it
    };
    if taken {
        notice::defined(ctx, who, TID_DIAG_0034, Vec::new());
        return;
    }
    // 5. one atomic batch; the continuation finishes the job
    let via = RenameVia {
        clear_mode: (m.data >> 16) != 0,
        ..RenameVia::SCROLL
    };
    submit_rename(ctx, who, character, name, via);
}

/// How a rename came about (the `by` of the `char.rename` audit line and the two extras of the scroll).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RenameVia {
    /// `scroll`, `gm` (the command `/spn`) or `admin` (the console).
    pub by: &'static str,
    /// The rename scroll (`II_SYS_SYS_SCR_CHANAM`, one piece) is consumed after the commit.
    pub consume_scroll: bool,
    /// The NPC's `QUERYSETPLAYERNAME_MODE` flag is cleared after the commit.
    pub clear_mode: bool,
}

impl RenameVia {
    /// The rename scroll.
    pub(crate) const SCROLL: RenameVia = RenameVia {
        by: "scroll",
        consume_scroll: true,
        clear_mode: false,
    };
    /// The GM command `/spn` (no scroll, no mode flag: the legacy path with slot `0xFFFF`).
    pub(crate) const GM: RenameVia = RenameVia {
        by: "gm",
        consume_scroll: false,
        clear_mode: false,
    };
    /// The admin console's rename of an online character.
    pub(crate) const ADMIN: RenameVia = RenameVia {
        by: "admin",
        consume_scroll: false,
        clear_mode: false,
    };
}

/// Locks the rename of `who` (`rename_pending`) and submits the ONE `CharacterRename` batch (no snapshot); the
/// continuation is [`finish_rename`]. The caller has validated the name and its uniqueness.
pub(crate) fn submit_rename(
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    character: CharacterId,
    name: String,
    via: RenameVia,
) {
    if let Some(p) = ctx.worlds.player_mut(who.0).and_then(|mv| mv.player_mut()) {
        p.social.rename_pending = true;
    }
    let obj = who.0;
    let new_name = name.clone();
    let ticket = ctx.tickets.issue(Pending::Service(ServiceTicket::new(
        move |svc, ctx, result| {
            finish_rename(svc, ctx, character, obj, new_name, via, result);
        },
    )));
    ctx.persist.atomic(AtomicBatch {
        reply: BatchReply::Game(ticket),
        ..AtomicBatch::of_ops(
            AtomicKind::CharacterRename,
            vec![DbOp::RenameCharacter {
                character_id: character,
                name,
            }],
        )
    });
}

/// `char.rename` (Plan/13 §25.2): `slot`, `old_name`, `new_name`, `target_account` plus the character and how it came
/// about. The actor is the avatar when there is one, else the character itself.
pub(crate) fn audit_rename(
    ctx: &GameCtx<'_>,
    avatar: Option<PlayerRef>,
    character: (CharacterId, AccountId, u8),
    old: &str,
    new: &str,
    by: &str,
) {
    let (id, account, slot) = character;
    let event = AuditEvent::new(
        ev::CHAR_RENAME,
        serde_json::json!({
            "character": id.0,
            "slot": slot,
            "old_name": old,
            "new_name": new,
            "target_account": account.0,
            "by": by,
        }),
    );
    let actor = avatar
        .and_then(|w| crate::audit::actor_of(ctx, w))
        .unwrap_or_else(|| AuditActor {
            account_id: Some(account),
            character_id: Some(id),
            name: Some(new.to_owned()),
            ..AuditActor::default()
        });
    ctx.audit.emit(event.by(actor));
}

/// The success tail of a rename, after the batch committed (`CDPCoreClient::OnSetPlayerName`): the directory and the
/// avatar take `name`, the scroll goes when `via.consume_scroll`, the NPC's mode flag is cleared when
/// `via.clear_mode`, `QuerySetPlayerNameNtf` goes to everybody (with the `UpdatePlayerData` of the listeners) and
/// `char.rename` is audited. `false` when the directory refuses a name the database accepted (logged; nothing else
/// happens then).
pub(crate) fn complete_rename(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    character: CharacterId,
    avatar: Option<PlayerRef>,
    name: &str,
    via: RenameVia,
) -> bool {
    let Some((old, account, slot)) = svc
        .presence
        .get(character)
        .map(|e| (e.name.clone(), e.account, e.slot))
    else {
        tracing::error!(character = character.0, %name, "a committed rename names a character the directory does not know");
        return false;
    };
    if svc.presence.rename(character, name).is_err() {
        // the database has the name; the directory is out of step only if it was reserved meanwhile
        tracing::error!(character = character.0, %name, "a committed rename could not enter the directory");
        return false;
    }
    if let Some(who) = avatar {
        if let Some(p) = ctx.worlds.player_mut(who.0).and_then(|mv| mv.player_mut()) {
            p.ident.name = name.to_owned();
        }
        if via.consume_scroll {
            let _ = remove_item_by_id(
                svc,
                ctx,
                who,
                II_SYS_SYS_SCR_CHANAM,
                Some(1),
                RemoveReason::Consume,
            );
        }
        if via.clear_mode {
            clear_rename_mode(svc, ctx, who);
        }
    }
    tracing::info!(character = character.0, from = %old, to = %name, by = via.by, "character renamed");
    announce_rename(svc, ctx, character, name);
    audit_rename(ctx, avatar, (character, account, slot), &old, name, via.by);
    true
}

/// The `DbReply` of a rename (`CDPCoreClient::OnSetPlayerName`): on success [`complete_rename`]; on failure the lock
/// opens and the player hears `TID_DIAG_0034`.
fn finish_rename(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    character: CharacterId,
    obj: ObjId,
    name: String,
    via: RenameVia,
    result: TicketResult,
) {
    // the avatar may be gone or may have been replaced in the meantime
    let avatar = ctx
        .worlds
        .player(obj)
        .and_then(|m| m.player())
        .is_some_and(|p| p.ident.character_id == character)
        .then_some(PlayerRef(obj));
    if let Some(who) = avatar
        && let Some(p) = ctx.worlds.player_mut(who.0).and_then(|mv| mv.player_mut())
    {
        p.social.rename_pending = false;
    }
    match result {
        TicketResult::Committed(_) => {
            complete_rename(svc, ctx, character, avatar, &name, via);
        }
        TicketResult::Failed(e) => {
            tracing::debug!(character = character.0, error = %e, "rename refused");
            if let Some(who) = avatar {
                notice::defined(ctx, who, TID_DIAG_0034, Vec::new());
            }
        }
        other => {
            tracing::error!(
                ?other,
                "CharacterRename completed with an unexpected result"
            );
            if let Some(who) = avatar {
                notice::defined(ctx, who, TID_DIAG_0034, Vec::new());
            }
        }
    }
}

/// `SetNotMode( QUERYSETPLAYERNAME_MODE )` + `AddModifyMode` (the rename was allowed by an NPC's mode flag).
fn clear_rename_mode(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    let core = svc.stats.handle();
    let mut out = crate::stats::out::Out::new();
    if let Some(m) = ctx.worlds.mover_mut(who.0) {
        crate::stats::modes::clear_mode_bits(m, &core, QUERYSETPLAYERNAME_MODE, &mut out);
    }
    out.flush(ctx, who.0);
}
