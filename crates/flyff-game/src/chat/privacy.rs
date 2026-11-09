//! Privacy toggles and the ignore list, stored on the server (Plan/23 §6.11, WP-GM-22; F-266, PL-095).
//!
//! The legacy client kept the twelve accept/refuse toggles and the ignore set in its own memory (`/ignore` was
//! case-sensitive and forgotten at logout). Here they are the keys of the character's `SettingsDoc`
//! (`PlayerExt.misc.settings`, 13 §6.12): `privacy.trade|whisper|party|shout|messenger|messenger_join` (booleans, an
//! absent key means `true`) and `chat.ignore` (an array of character names, at most 1000, compared ASCII
//! case-insensitively). The document rides the character snapshot like every other doc: no `DbOp`, no urgent save.
//!
//! | part | content |
//! |---|---|
//! | queries | [`privacy_flags`], [`accepts_whisper`], [`accepts_shout`], [`accepts_trade`], [`accepts_party_invite`], [`accepts_friend_request`], [`wants_friend_login_notice`], [`ignores`]: pure reads of the document of an online player (`&Worlds`, `ObjId`; [`by_character`] resolves a character id), called by the delivery code (21 §10.6) |
//! | mutators | [`set_flag`], [`ignore_add`], [`ignore_remove`]: change the document, re-check its limits (rolled back with the full error when they fail) and end with [`ChatSettingsNtf`](pb::ChatSettingsNtf) |
//! | commands | rows 13-26 of the table (`cmd_trade_agree` .. `cmd_unignore`): `DefinedText`, then the notification |
//! | join | [`ChatSettingsStep`]: the notification closes every join batch and every re-attach replay |
//!
//! **Online players only.** The settings of a player who is not in the world do not exist in memory; every query
//! answers with the defaults then (all flags `true`, nobody ignored), which is what a caller wants for a recipient
//! that cannot be asked.

use crate::chat::ingress::notice;
use crate::chat::send_to;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::gm::{CmdArgs, CmdCtx, CmdDone, CmdError, CmdResult};
use crate::outbox::Outbox;
use crate::world::Worlds;
use crate::world::join::{JoinContributor, JoinCtx, JoinStep};
use flyff_common::defs::text::{
    TID_GAME_CONNAGREE, TID_GAME_CONNREFUSE, TID_GAME_ERROR_ALREADY_BLOCKED,
    TID_GAME_ERROR_FULL_BLOCKED_USER_LIST, TID_GAME_ERROR_INVALID_USER_ID,
    TID_GAME_ERROR_MY_CHARACTER_CANT_BLOCKING, TID_GAME_ERROR_THERE_IS_NO_BLOCKED_TARGET,
    TID_GAME_MSGERAGREE, TID_GAME_MSGERREFUSE, TID_GAME_SHOUTAGREE, TID_GAME_SHOUTREFUSE,
    TID_GAME_STAGEAGREE, TID_GAME_STAGEREFUSE, TID_GAME_TRADEAGREE, TID_GAME_TRADEREFUSE,
    TID_GAME_USER_CHATTING_BLOCKING, TID_GAME_USER_CHATTING_UNBLOCKING, TID_GAME_WHISPERAGREE,
    TID_GAME_WHISPERREFUSE,
};
use flyff_common::{CharacterId, ObjId};
use flyff_db::model::docs::{
    MAX_CHAT_IGNORE, SETTING_CHAT_IGNORE, SETTING_PRIVACY_MESSENGER,
    SETTING_PRIVACY_MESSENGER_JOIN, SETTING_PRIVACY_PARTY, SETTING_PRIVACY_SHOUT,
    SETTING_PRIVACY_TRADE, SETTING_PRIVACY_WHISPER, SettingsDoc,
};
use flyff_proto::pb;
use serde_json::Value;

// ---- the view type -------------------------------------------------------------------------------------------------

/// The `privacy.*` flags of one player (21 §10.6): every flag defaults to `true` (accept).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrivacyFlags {
    /// `privacy.whisper`: whispers and messenger IMs are delivered.
    pub whisper: bool,
    /// `privacy.trade`: trade requests are accepted.
    pub trade: bool,
    /// `privacy.party`: party invitations are accepted.
    pub party: bool,
    /// `privacy.shout`: shouts are delivered.
    pub shout: bool,
    /// `privacy.messenger`: friend requests are accepted.
    pub messenger: bool,
    /// `privacy.messenger_join`: friends' login and logout notices are delivered.
    pub messenger_join: bool,
}

impl Default for PrivacyFlags {
    fn default() -> Self {
        Self {
            whisper: true,
            trade: true,
            party: true,
            shout: true,
            messenger: true,
            messenger_join: true,
        }
    }
}

impl PrivacyFlags {
    /// The flags stored in `doc` (absent key = `true`).
    pub fn of(doc: &SettingsDoc) -> PrivacyFlags {
        PrivacyFlags {
            whisper: doc.privacy(SETTING_PRIVACY_WHISPER),
            trade: doc.privacy(SETTING_PRIVACY_TRADE),
            party: doc.privacy(SETTING_PRIVACY_PARTY),
            shout: doc.privacy(SETTING_PRIVACY_SHOUT),
            messenger: doc.privacy(SETTING_PRIVACY_MESSENGER),
            messenger_join: doc.privacy(SETTING_PRIVACY_MESSENGER_JOIN),
        }
    }
}

// ---- the queries (pure reads) ----------------------------------------------------------------------------------------

/// The settings document of the online player `who`; `None` for an avatar that is not a live player.
fn settings(worlds: &Worlds, who: ObjId) -> Option<&SettingsDoc> {
    worlds.player(who)?.player().map(|p| &p.misc.settings)
}

/// The avatar of an online character (`None` when it is offline or its avatar is gone), for callers that hold a
/// character id.
pub fn by_character(svc: &Services, worlds: &Worlds, id: CharacterId) -> Option<ObjId> {
    let o = svc.presence.get(id)?.online?.obj;
    worlds
        .player(o)
        .is_some_and(|m| !m.core.deleted)
        .then_some(o)
}

/// The stored `privacy.*` flags of `who`; the defaults for an avatar that is not a live player.
pub fn privacy_flags(worlds: &Worlds, who: ObjId) -> PrivacyFlags {
    settings(worlds, who).map_or_else(PrivacyFlags::default, PrivacyFlags::of)
}

fn flag(worlds: &Worlds, who: ObjId, key: &str) -> bool {
    settings(worlds, who).is_none_or(|s| s.privacy(key))
}

/// `privacy.whisper`: whispers and IMs to `who` are delivered.
pub fn accepts_whisper(worlds: &Worlds, who: ObjId) -> bool {
    flag(worlds, who, SETTING_PRIVACY_WHISPER)
}

/// `privacy.shout`: shouts reach `who`.
pub fn accepts_shout(worlds: &Worlds, who: ObjId) -> bool {
    flag(worlds, who, SETTING_PRIVACY_SHOUT)
}

/// `privacy.trade`: trade requests to `who` are accepted (called by the trade handler of doc 19).
pub fn accepts_trade(worlds: &Worlds, who: ObjId) -> bool {
    flag(worlds, who, SETTING_PRIVACY_TRADE)
}

/// `privacy.party`: party invitations to `who` are accepted.
pub fn accepts_party_invite(worlds: &Worlds, who: ObjId) -> bool {
    flag(worlds, who, SETTING_PRIVACY_PARTY)
}

/// `privacy.messenger`: friend requests to `who` are accepted.
pub fn accepts_friend_request(worlds: &Worlds, who: ObjId) -> bool {
    flag(worlds, who, SETTING_PRIVACY_MESSENGER)
}

/// `privacy.messenger_join`: `who` hears when a friend logs in or out.
pub fn wants_friend_login_notice(worlds: &Worlds, who: ObjId) -> bool {
    flag(worlds, who, SETTING_PRIVACY_MESSENGER_JOIN)
}

/// Whether `who` ignores the character named `speaker_name` (`chat.ignore`, ASCII case-insensitive). A scan of the
/// stored array: no allocation, free on an empty list.
pub fn ignores(worlds: &Worlds, who: ObjId, speaker_name: &str) -> bool {
    settings(worlds, who)
        .and_then(|s| s.values.get(SETTING_CHAT_IGNORE))
        .and_then(Value::as_array)
        .is_some_and(|names| {
            names
                .iter()
                .filter_map(Value::as_str)
                .any(|n| n.eq_ignore_ascii_case(speaker_name))
        })
}

// ---- ChatSettingsNtf ---------------------------------------------------------------------------------------------------

/// The notification for the settings in `doc`: the six flags and the ignore list in stored order.
pub fn settings_ntf(doc: &SettingsDoc) -> pb::ChatSettingsNtf {
    let f = PrivacyFlags::of(doc);
    pb::ChatSettingsNtf {
        trade: f.trade,
        whisper: f.whisper,
        party: f.party,
        shout: f.shout,
        messenger: f.messenger,
        messenger_join: f.messenger_join,
        ignored: doc.chat_ignore().into_iter().map(str::to_owned).collect(),
    }
}

/// Sends `ChatSettingsNtf` with the current settings of `who` (nothing for an avatar that is not a live player).
pub fn send_settings(ctx: &mut GameCtx<'_>, who: PlayerRef) {
    let Some(ntf) = ctx
        .player(who)
        .and_then(|m| m.player())
        .map(|p| settings_ntf(&p.misc.settings))
    else {
        return;
    };
    send_to(ctx, who, ntf);
}

/// The join step that closes the batch (and the re-attach replay) with `ChatSettingsNtf`, so that a relog or a
/// second browser learns the stored settings (23 §6.11, 11 §2.5).
#[derive(Debug, Clone, Copy)]
pub struct ChatSettingsStep;

impl JoinContributor for ChatSettingsStep {
    fn step(&self) -> JoinStep {
        JoinStep::ChatSettings
    }

    fn write(&self, cx: &JoinCtx<'_>, out: &mut Outbox) {
        if let Some(doc) = settings(cx.worlds, cx.player) {
            out.to_session(cx.session, 0, settings_ntf(doc));
        }
    }
}

// ---- the mutators ------------------------------------------------------------------------------------------------------

/// Changes the document of `who` through `f`, then re-checks its limits (`SettingsDoc::check_limits`: 1000 names, 32
/// KiB). A document that fails is rolled back at `key` and the FULL error comes back; so does a `who` that is not a
/// live player.
fn edit<R>(
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    key: &'static str,
    f: impl FnOnce(&mut SettingsDoc) -> R,
) -> Result<R, String> {
    let Some(p) = ctx.player_mut(who).and_then(|m| m.player_mut()) else {
        return Err("not a live player".to_owned());
    };
    let doc = &mut p.misc.settings;
    let before = doc.values.get(key).cloned();
    let result = f(doc);
    if let Err(e) = doc.check_limits() {
        match before {
            Some(v) => {
                doc.values.insert(key.to_owned(), v);
            }
            None => {
                doc.values.remove(key);
            }
        }
        tracing::warn!(error = %e, key, "a settings change was rolled back");
        return Err(e);
    }
    Ok(result)
}

fn put_flag(
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    key: &'static str,
    value: bool,
) -> Result<(), String> {
    edit(ctx, who, key, |doc| {
        doc.values.insert(key.to_owned(), Value::Bool(value));
    })
}

/// Appends `name` to the ignore list (no check of the name: the command does those).
fn put_ignore(ctx: &mut GameCtx<'_>, who: PlayerRef, name: &str) -> Result<(), String> {
    edit(ctx, who, SETTING_CHAT_IGNORE, |doc| {
        let entry = doc
            .values
            .entry(SETTING_CHAT_IGNORE.to_owned())
            .or_insert_with(|| Value::Array(Vec::new()));
        match entry {
            Value::Array(names) => names.push(Value::String(name.to_owned())),
            other => *other = Value::Array(vec![Value::String(name.to_owned())]),
        }
    })
}

/// Removes every entry equal to `name` (ASCII case-insensitive); `true` when something was removed.
fn take_ignore(ctx: &mut GameCtx<'_>, who: PlayerRef, name: &str) -> Result<bool, String> {
    edit(ctx, who, SETTING_CHAT_IGNORE, |doc| {
        let Some(Value::Array(names)) = doc.values.get_mut(SETTING_CHAT_IGNORE) else {
            return false;
        };
        let before = names.len();
        names.retain(|v| v.as_str().is_none_or(|n| !n.eq_ignore_ascii_case(name)));
        names.len() != before
    })
}

/// Sets a `privacy.*` flag (`key` is one of the `SETTING_PRIVACY_*` keys) and sends `ChatSettingsNtf`.
pub fn set_flag(
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    key: &'static str,
    value: bool,
) -> Result<(), String> {
    put_flag(ctx, who, key, value)?;
    send_settings(ctx, who);
    Ok(())
}

/// Appends `name` (the directory's spelling) to the ignore list and sends `ChatSettingsNtf`. Fails with the full
/// error of `check_limits` when the list would pass 1000 names or the document 32 KiB; nothing changes then.
pub fn ignore_add(ctx: &mut GameCtx<'_>, who: PlayerRef, name: &str) -> Result<(), String> {
    put_ignore(ctx, who, name)?;
    send_settings(ctx, who);
    Ok(())
}

/// Removes `name` from the ignore list (ASCII case-insensitive) and sends `ChatSettingsNtf` when something went.
/// Returns whether the list held the name.
pub fn ignore_remove(ctx: &mut GameCtx<'_>, who: PlayerRef, name: &str) -> Result<bool, String> {
    let removed = take_ignore(ctx, who, name)?;
    if removed {
        send_settings(ctx, who);
    }
    Ok(removed)
}

// ---- the commands (rows 13-27 of 23 §4.2) --------------------------------------------------------------------------

/// A toggle: store the flag, `DefinedText(tid)`, then the notification.
fn toggle(cx: &mut CmdCtx<'_, '_>, key: &'static str, value: bool, tid: u32) -> CmdResult {
    let me = cx.self_player.ok_or(CmdError::NeedsSelf)?;
    match put_flag(cx.ctx, me, key, value) {
        Ok(()) => notice::defined(cx.ctx, me, tid, Vec::new()),
        Err(e) => tracing::warn!(error = %e, "a privacy toggle was refused"),
    }
    send_settings(cx.ctx, me);
    Ok(CmdDone::default())
}

macro_rules! toggle_row {
    ($(#[$doc:meta])* $name:ident, $key:expr, $value:expr, $tid:expr) => {
        $(#[$doc])*
        pub fn $name(cx: &mut CmdCtx<'_, '_>, _args: &mut CmdArgs<'_>) -> CmdResult {
            toggle(cx, $key, $value, $tid)
        }
    };
}

toggle_row!(
    /// Row 13 `/tradeagree (ta)`: `privacy.trade = true`.
    cmd_trade_agree, SETTING_PRIVACY_TRADE, true, TID_GAME_TRADEAGREE
);
toggle_row!(
    /// Row 14 `/traderefuse (tr)`: `privacy.trade = false`.
    cmd_trade_refuse, SETTING_PRIVACY_TRADE, false, TID_GAME_TRADEREFUSE
);
toggle_row!(
    /// Row 15 `/whisperagree (wa)`: `privacy.whisper = true`.
    cmd_whisper_agree, SETTING_PRIVACY_WHISPER, true, TID_GAME_WHISPERAGREE
);
toggle_row!(
    /// Row 16 `/whisperrefuse (wr)`: `privacy.whisper = false`.
    cmd_whisper_refuse, SETTING_PRIVACY_WHISPER, false, TID_GAME_WHISPERREFUSE
);
toggle_row!(
    /// Row 17 `/messengeragree (ma)`: `privacy.messenger = true`.
    cmd_messenger_agree, SETTING_PRIVACY_MESSENGER, true, TID_GAME_MSGERAGREE
);
toggle_row!(
    /// Row 18 `/messengerrefuse (mr)`: `privacy.messenger = false`.
    cmd_messenger_refuse, SETTING_PRIVACY_MESSENGER, false, TID_GAME_MSGERREFUSE
);
toggle_row!(
    /// Row 19 `/stageagree (ga)`: `privacy.party = true`.
    cmd_party_agree, SETTING_PRIVACY_PARTY, true, TID_GAME_STAGEAGREE
);
toggle_row!(
    /// Row 20 `/stagerefuse (gr)`: `privacy.party = false`.
    cmd_party_refuse, SETTING_PRIVACY_PARTY, false, TID_GAME_STAGEREFUSE
);
toggle_row!(
    /// Row 21 `/connectagree (ca)`: `privacy.messenger_join = true`.
    cmd_connect_agree, SETTING_PRIVACY_MESSENGER_JOIN, true, TID_GAME_CONNAGREE
);
toggle_row!(
    /// Row 22 `/connectrefuse (cr)`: `privacy.messenger_join = false`.
    cmd_connect_refuse, SETTING_PRIVACY_MESSENGER_JOIN, false, TID_GAME_CONNREFUSE
);
toggle_row!(
    /// Row 23 `/shoutagree (ha)`: `privacy.shout = true`.
    cmd_shout_agree, SETTING_PRIVACY_SHOUT, true, TID_GAME_SHOUTAGREE
);
toggle_row!(
    /// Row 24 `/shoutrefuse (hr)`: `privacy.shout = false`.
    cmd_shout_refuse, SETTING_PRIVACY_SHOUT, false, TID_GAME_SHOUTREFUSE
);

/// Row 25 `/ignore (ig) <name>` (`FuncTextCmd.cpp:3877-3935`). The refusals in the order of the original client,
/// each a `DefinedText` and no change: the list holds 1000 names (`..._FULL_BLOCKED_USER_LIST`), the name is empty
/// (`..._INVALID_USER_ID`), it is the player's own (`..._MY_CHARACTER_CANT_BLOCKING`, ASCII case-insensitive), it
/// is listed already (`..._ALREADY_BLOCKED`); then, NEW (the original client could not know), a name the player
/// directory does not know (`..._INVALID_USER_ID`). Otherwise the directory's spelling is stored,
/// `DefinedText(TID_GAME_USER_CHATTING_BLOCKING, [name])` and `ChatSettingsNtf` follow.
pub fn cmd_ignore(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let me = cx.self_player.ok_or(CmdError::NeedsSelf)?;
    let typed = args.token().to_owned();
    let refuse = |cx: &mut CmdCtx<'_, '_>, tid: u32| -> CmdResult {
        notice::defined(cx.ctx, me, tid, Vec::new());
        Ok(CmdDone::on(typed.clone()))
    };
    let (count, listed, own) = {
        let Some(p) = cx.ctx.player(me).and_then(|m| m.player()) else {
            return Err(CmdError::NeedsSelf);
        };
        let names = p.misc.settings.chat_ignore();
        (
            names.len(),
            names.iter().any(|n| n.eq_ignore_ascii_case(&typed)),
            p.ident.name.eq_ignore_ascii_case(&typed),
        )
    };
    if count >= MAX_CHAT_IGNORE {
        return refuse(cx, TID_GAME_ERROR_FULL_BLOCKED_USER_LIST);
    }
    if typed.is_empty() {
        return refuse(cx, TID_GAME_ERROR_INVALID_USER_ID);
    }
    if own {
        return refuse(cx, TID_GAME_ERROR_MY_CHARACTER_CANT_BLOCKING);
    }
    if listed {
        return refuse(cx, TID_GAME_ERROR_ALREADY_BLOCKED);
    }
    let Some(name) = cx.svc.presence.by_name(&typed).map(|e| e.name.clone()) else {
        return refuse(cx, TID_GAME_ERROR_INVALID_USER_ID);
    };
    match put_ignore(cx.ctx, me, &name) {
        Ok(()) => notice::defined(cx.ctx, me, TID_GAME_USER_CHATTING_BLOCKING, vec![name]),
        Err(e) => tracing::warn!(error = %e, "an ignore entry was refused"),
    }
    send_settings(cx.ctx, me);
    Ok(CmdDone::on(typed))
}

/// Row 26 `/unignore (uig) <name>`: no directory lookup (entries go stale when a character is renamed or purged),
/// ASCII case-insensitive removal, `DefinedText(TID_GAME_USER_CHATTING_UNBLOCKING, [name])` and the notification;
/// a name that is not listed gives `TID_GAME_ERROR_THERE_IS_NO_BLOCKED_TARGET`.
pub fn cmd_unignore(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let me = cx.self_player.ok_or(CmdError::NeedsSelf)?;
    let typed = args.token().to_owned();
    match take_ignore(cx.ctx, me, &typed) {
        Ok(true) => {
            notice::defined(
                cx.ctx,
                me,
                TID_GAME_USER_CHATTING_UNBLOCKING,
                vec![typed.clone()],
            );
            send_settings(cx.ctx, me);
        }
        Ok(false) => {
            notice::defined(
                cx.ctx,
                me,
                TID_GAME_ERROR_THERE_IS_NO_BLOCKED_TARGET,
                Vec::new(),
            );
        }
        Err(e) => {
            tracing::warn!(error = %e, "an ignore entry could not be removed");
            send_settings(cx.ctx, me);
        }
    }
    Ok(CmdDone::on(typed))
}
