//! Moderation commands of the console (Plan/25 §4.6, §4.8, §11.5): mute, unmute, mode flags, a message to
//! characters. **WP-ADMIN-12.** The mute is `/mute`'s `modes::set_mute` (23 §2.6, N15), the mode flags are
//! `modes::change_mode` (23 §2.4: one `ModifyMode` to the viewers and the target, nothing when the word did not
//! change), the message is a `SystemNotice` or a `Text` line (23 §6.13).
//!
//! All four need the targets **online** (an offline character answers `character_online`): a mute counts down only
//! while its character is in the world (KQ-22), the mode word of an offline character is not the console's to change.

use super::common::{check_group, check_rank, fail, failed, online_player, report};
use super::{TargetId, TargetResult};
use crate::admin::types::{AdminErrorCode, AdminFailure};
use crate::chat::ingress::notice;
use crate::chat::send_to;
use crate::ctx::{GameCtx, Services};
use crate::gm::modes::{self, ModeSource};
use crate::input::{AdminActor, AdminOutput};
use flyff_common::CharacterId;
use flyff_common::gm::ModeFlags;
use flyff_proto::pb;
use serde_json::{Value, json};

/// The mode bits the console may set or clear: the ones the GM commands set (23 §2.1 "Set by": `MATCHLESS`,
/// `TRANSPARENT`, `ONEKILL`, `DONMOVE`, `SAYTALK`, `MATCHLESS2`, `NO_ATTACK`, `ITEM`, `COMMUNITY`, `TALK`, `SHOUTTALK`,
/// `EXPUP_STOP`). The other bits belong to the systems that own them (the PK options, the mailbox, sieges, ...).
pub const CONSOLE_MODE_MASK: u32 = ModeFlags::MATCHLESS.bits()
    | ModeFlags::TRANSPARENT.bits()
    | ModeFlags::ONEKILL.bits()
    | ModeFlags::DONMOVE.bits()
    | ModeFlags::SAYTALK.bits()
    | ModeFlags::MATCHLESS2.bits()
    | ModeFlags::NO_ATTACK.bits()
    | ModeFlags::ITEM.bits()
    | ModeFlags::COMMUNITY.bits()
    | ModeFlags::TALK.bits()
    | ModeFlags::SHOUTTALK.bits()
    | ModeFlags::EXPUP_STOP.bits();

/// The longest message (`SystemNotice` is cut at 511 bytes by the protocol, 23 §6.13).
pub const MAX_NOTIFY_BYTES: usize = 511;

/// Mute a character (`Mute`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mute {
    /// The character.
    pub character: CharacterId,
    /// For how long, in seconds (`u32::MAX` = effectively permanent). Zero is refused: use `Unmute`.
    pub seconds: u32,
}

/// Lift a mute (`Unmute`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unmute {
    /// The character.
    pub character: CharacterId,
}

/// Change the mode flags of a character (`SetModeFlags`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SetModeFlags {
    /// The character.
    pub character: CharacterId,
    /// The `m_dwMode` bits to set.
    pub set: u32,
    /// The bits to clear.
    pub clear: u32,
}

/// How a message to a character is shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotifyStyle {
    /// The large notice line (`/sys`).
    Notice,
    /// A system line in the chat window.
    System,
}

/// Send a message to characters (`NotifyCharacter`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotifyCharacter {
    /// The recipients.
    pub characters: Vec<CharacterId>,
    /// The text.
    pub text: String,
    /// How it is shown.
    pub style: NotifyStyle,
}

fn one(target: CharacterId, r: Result<Value, AdminFailure>) -> AdminOutput {
    report(vec![match r {
        Ok(info) => TargetResult::ok(TargetId::Character(target)).with_info(info),
        Err(f) => TargetResult::failed(TargetId::Character(target), f),
    }])
}

/// The remaining mute of a player in seconds.
fn mute_of(ctx: &GameCtx<'_>, who: crate::ctx::PlayerRef) -> u32 {
    ctx.worlds
        .player(who.0)
        .and_then(|m| m.player())
        .map_or(0, |p| p.misc.mute_remaining_s)
}

/// Sets the mute of the online character; info `{before, after}` in seconds.
fn set_mute(
    svc: &Services,
    ctx: &mut GameCtx<'_>,
    actor: &AdminActor,
    id: CharacterId,
    seconds: u32,
) -> Result<Value, AdminFailure> {
    let who = online_player(svc, ctx, id)?;
    check_rank(actor, ctx, who.0)?;
    let before = mute_of(ctx, who);
    let done = ctx
        .worlds
        .player_mut(who.0)
        .is_some_and(|m| modes::set_mute(m, seconds));
    if !done {
        return Err(fail(
            AdminErrorCode::StaleTarget,
            "the character left the world",
        ));
    }
    Ok(json!({"before": before, "after": seconds}))
}

/// `Mute`: `mute_remaining_s` of the online character becomes `seconds`; it counts down while the character is in the
/// world and is saved with it (23 §2.6). Chat of every channel is refused with `TID_GAME_STATE_MUTE` meanwhile.
pub(super) fn mute(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    actor: AdminActor,
    cmd: Mute,
) -> AdminOutput {
    if cmd.seconds == 0 {
        return failed(
            AdminErrorCode::Invalid,
            "a mute lasts at least one second: use unmute to lift it",
        );
    }
    one(
        cmd.character,
        set_mute(svc, ctx, &actor, cmd.character, cmd.seconds),
    )
}

/// `Unmute`: the mute of the online character is lifted (`/mute <name> 0`). Info `{before, after: 0}`; a character that
/// was not muted is not an error.
pub(super) fn unmute(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    actor: AdminActor,
    cmd: Unmute,
) -> AdminOutput {
    one(cmd.character, set_mute(svc, ctx, &actor, cmd.character, 0))
}

/// `SetModeFlags`: `mode = (mode & !clear) | set` through `modes::change_mode`, which sends one `ModifyMode` when the
/// word changed. Only the bits of [`CONSOLE_MODE_MASK`] can be named, a bit cannot be in both sets, and both sets
/// empty is a mistake. Info `{before, after}` (mode words).
pub(super) fn set_mode_flags(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    actor: AdminActor,
    cmd: SetModeFlags,
) -> AdminOutput {
    if cmd.set == 0 && cmd.clear == 0 {
        return failed(AdminErrorCode::Invalid, "no mode flag named");
    }
    if cmd.set & cmd.clear != 0 {
        return failed(
            AdminErrorCode::Invalid,
            "a mode flag cannot be set and cleared at once",
        );
    }
    if (cmd.set | cmd.clear) & !CONSOLE_MODE_MASK != 0 {
        return failed(
            AdminErrorCode::Invalid,
            format!(
                "only these mode bits can be changed here: {CONSOLE_MODE_MASK:#x} (asked for {:#x})",
                (cmd.set | cmd.clear) & !CONSOLE_MODE_MASK
            ),
        );
    }
    let r = (|| -> Result<Value, AdminFailure> {
        let who = online_player(svc, ctx, cmd.character)?;
        check_rank(&actor, ctx, who.0)?;
        let before = ctx.worlds.player(who.0).map_or(0, |m| m.mv.mode);
        modes::change_mode(
            svc,
            ctx,
            who,
            ModeFlags::from_bits(cmd.set),
            ModeFlags::from_bits(cmd.clear),
            ModeSource::System,
        );
        let after = ctx.worlds.player(who.0).map_or(0, |m| m.mv.mode);
        Ok(json!({"before": before, "after": after}))
    })();
    one(cmd.character, r)
}

/// `NotifyCharacter`: the text goes to every online recipient as a `SystemNotice` (the large line of `/sys`) or as a
/// `[System]` chat line; a recipient that is offline, or whose avatar lingers without a socket, is reported and skipped. No rank protection: a message changes
/// nothing about its target.
pub(super) fn notify_character(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    _actor: AdminActor,
    cmd: NotifyCharacter,
) -> AdminOutput {
    if let Err(f) = check_group(cmd.characters.len(), "characters") {
        return AdminOutput::Failed(f);
    }
    let text = cmd.text.trim();
    if text.is_empty() {
        return failed(AdminErrorCode::Invalid, "the message is empty");
    }
    if text.len() > MAX_NOTIFY_BYTES {
        return failed(
            AdminErrorCode::Invalid,
            format!("the message is longer than {MAX_NOTIFY_BYTES} bytes"),
        );
    }
    if text.chars().any(char::is_control) {
        return failed(
            AdminErrorCode::Invalid,
            "the message contains control characters",
        );
    }
    let mut per_target = Vec::with_capacity(cmd.characters.len());
    for id in cmd.characters.iter().copied() {
        let target = TargetId::Character(id);
        per_target.push(match online_player(svc, ctx, id) {
            // an avatar that lingers without a socket cannot be told anything
            Ok(who) if ctx.session_of(who).is_none() => TargetResult::failed(
                target,
                fail(
                    AdminErrorCode::CharacterOnline,
                    format!("character {} has no connection (the avatar lingers)", id.0),
                ),
            ),
            Ok(who) => {
                match cmd.style {
                    NotifyStyle::Notice => send_to(
                        ctx,
                        who,
                        pb::SystemNotice {
                            text: text.to_owned(),
                        },
                    ),
                    NotifyStyle::System => notice::text(ctx, who, &format!("[System] {text}")),
                }
                TargetResult::ok(target)
            }
            Err(f) => TargetResult::failed(target, f),
        });
    }
    report(per_target)
}
