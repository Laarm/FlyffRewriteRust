//! The mutating admin commands of the console (Plan/25 §11.5; WP-ADMIN-06 lands every variant, the area files
//! implement them).
//!
//! Each variant of [`AdminCmd`] that the console adds is a newtype over a payload struct defined in the area file that
//! owns it, so a later work package changes its payload and its body **without touching `input.rs` or this file**.
//! [`run`] routes the variant to the function of its area file; until the owning work package lands, that function
//! answers [`AdminOutput::Failed`] with `AdminErrorCode::NotImplemented`. The commands are recorded inputs
//! (`GameInput::Admin`, replay-visible, unlike `GameInput::AdminView`); each mutating body emits its atomic persistence
//! batch in the same commit block (13 §18) and the `admin.*` audit is completed after the batch is queued.
//!
//! | area file | variants | work package |
//! |---|---|---|
//! | [`world`] | `TeleportCharacter`, `SpawnMonsters`, `RespawnRegion`, `DropItem`; `SetWeather`, `SetSpawnScale` | 12; 30, 31 |
//! | [`common`] | what the bodies share: the group cap, rank protection, online-character lookup | 12 |
//! | [`combat`] | `Heal`, `Resurrect`, `KillObject`, `DespawnObject`, `DespawnArea` | 12 |
//! | [`moderation`] | `Mute`, `Unmute`, `SetModeFlags`, `NotifyCharacter` | 12 |
//! | [`character`] | `EditCharacter` | 12 (`Rename`: 16) |
//! | [`items`] | `RemoveItem`, `AdjustGold`, `PetAction` | 16 |
//! | [`offline`] | `OfflineCharacterEdit` | 16 |
//! | [`grants`] | `GrantBatch`, `CancelPendingGrant`, `CancelPendingRemoval` | 20 |
//! | [`guilds`] | `GuildCreate`, `GuildEdit`, `GuildDisband` (deferred: they answer when the batch committed) | 23 |
//! | [`comm`] | `SetNoticeSchedule`, `SendNotice` | 27 |
//! | [`server`] | `RunJob` | 26 |

pub mod character;
pub mod combat;
pub mod comm;
pub mod common;
pub mod grants;
pub mod guilds;
pub mod items;
pub mod moderation;
pub mod offline;
pub mod server;
pub mod world;

use super::types::AdminFailure;
use crate::ctx::{GameCtx, Services};
use crate::input::{AdminActor, AdminCmd, AdminOutput};
use flyff_common::{CharacterId, GuildId, ObjId};

/// What one target of a group command came to (`AdminOutput::Report`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetId {
    /// A world object.
    Object(ObjId),
    /// A character (online or offline).
    Character(CharacterId),
    /// A guild (WP-ADMIN-23).
    Guild(GuildId),
}

/// The outcome for one target of a group command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TargetResult {
    /// The target.
    pub target: TargetId,
    /// `Ok` or why it was skipped (a dead, offline, gone or outranking target).
    pub outcome: Result<(), AdminFailure>,
    /// What the HTTP layer audits and shows for this target: the place a teleport came from, the value an edit
    /// replaced (WP-ADMIN-12). A JSON object; `None` when the command has nothing to add.
    pub info: Option<serde_json::Value>,
}

impl TargetResult {
    /// The target was handled.
    pub fn ok(target: TargetId) -> TargetResult {
        TargetResult {
            target,
            outcome: Ok(()),
            info: None,
        }
    }

    /// The target was skipped for `failure`.
    pub fn failed(target: TargetId, failure: AdminFailure) -> TargetResult {
        TargetResult {
            target,
            outcome: Err(failure),
            info: None,
        }
    }

    /// With the `info` object.
    #[must_use]
    pub fn with_info(mut self, info: serde_json::Value) -> TargetResult {
        self.info = Some(info);
        self
    }

    /// `true` when the target was handled.
    pub fn is_ok(&self) -> bool {
        self.outcome.is_ok()
    }
}

/// `true` for the commands this module executes (everything the console added; the older commands go to the
/// runtime's built-ins or to the GM handler).
pub fn handles(cmd: &AdminCmd) -> bool {
    matches!(
        cmd,
        AdminCmd::TeleportCharacter(_)
            | AdminCmd::SpawnMonsters(_)
            | AdminCmd::RespawnRegion(_)
            | AdminCmd::DropItem(_)
            | AdminCmd::SetWeather(_)
            | AdminCmd::SetSpawnScale(_)
            | AdminCmd::Heal(_)
            | AdminCmd::Resurrect(_)
            | AdminCmd::KillObject(_)
            | AdminCmd::DespawnObject(_)
            | AdminCmd::DespawnArea(_)
            | AdminCmd::Mute(_)
            | AdminCmd::Unmute(_)
            | AdminCmd::SetModeFlags(_)
            | AdminCmd::NotifyCharacter(_)
            | AdminCmd::EditCharacter(_)
            | AdminCmd::OfflineCharacterEdit(_)
            | AdminCmd::RemoveItem(_)
            | AdminCmd::AdjustGold(_)
            | AdminCmd::PetAction(_)
            | AdminCmd::GrantBatch(_)
            | AdminCmd::CancelPendingGrant(_)
            | AdminCmd::CancelPendingRemoval(_)
            | AdminCmd::GuildCreate(_)
            | AdminCmd::GuildEdit(_)
            | AdminCmd::GuildDisband(_)
            | AdminCmd::SetNoticeSchedule(_)
            | AdminCmd::SendNotice(_)
            | AdminCmd::RunJob(_)
    )
}

/// `true` for the commands whose answer waits for the persistence layer: the offline edits, the rename of an
/// online character (25 §4.6) and the three guild commands (WP-ADMIN-23). The dispatcher gives them the reply
/// ([`run_deferred`]) instead of calling [`run`].
pub fn defers(cmd: &AdminCmd) -> bool {
    match cmd {
        AdminCmd::OfflineCharacterEdit(_) => true,
        AdminCmd::EditCharacter(c) => matches!(c.edit, character::CharacterEdit::Rename(_)),
        // the conditional cancels answer when the database committed (WP-ADMIN-20)
        AdminCmd::CancelPendingGrant(_) | AdminCmd::CancelPendingRemoval(_) => true,
        // a guild change answers when its `AdminGuildCreate` / `AdminGuildEdit` batch committed
        AdminCmd::GuildCreate(_) | AdminCmd::GuildEdit(_) | AdminCmd::GuildDisband(_) => true,
        _ => false,
    }
}

/// Executes a command for which [`defers`] is `true`; the answer is sent through `reply` when the batch committed
/// (or at once when the command is refused). Any other command is run and answered immediately.
pub fn run_deferred(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    cmd: AdminCmd,
    actor: AdminActor,
    reply: flyff_common::rt::Reply<AdminOutput>,
) {
    match cmd {
        AdminCmd::OfflineCharacterEdit(c) => {
            offline::offline_character_edit(svc, ctx, actor, c, reply)
        }
        AdminCmd::EditCharacter(c) if matches!(c.edit, character::CharacterEdit::Rename(_)) => {
            offline::rename_online(svc, ctx, actor, c, reply);
        }
        AdminCmd::CancelPendingGrant(c) => grants::cancel_pending_grant(svc, ctx, actor, c, reply),
        AdminCmd::CancelPendingRemoval(c) => {
            grants::cancel_pending_removal(svc, ctx, actor, c, reply);
        }
        AdminCmd::GuildCreate(c) => guilds::guild_create(svc, ctx, actor, c, reply),
        AdminCmd::GuildEdit(c) => guilds::guild_edit(svc, ctx, actor, c, reply),
        AdminCmd::GuildDisband(c) => guilds::guild_disband(svc, ctx, actor, c, reply),
        other => {
            let out = run(svc, ctx, other, actor);
            let _ = reply.send(out);
        }
    }
}

/// Executes one console command on the game thread (see the module documentation). `actor` is the identity that
/// issued it (rank protection and the reason were checked by the HTTP layer; bodies still refuse targets that
/// vanished or changed: `StaleTarget`).
pub fn run(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    cmd: AdminCmd,
    actor: AdminActor,
) -> AdminOutput {
    match cmd {
        AdminCmd::TeleportCharacter(c) => world::teleport_character(svc, ctx, actor, c),
        AdminCmd::SpawnMonsters(c) => world::spawn_monsters(svc, ctx, actor, c),
        AdminCmd::RespawnRegion(c) => world::respawn_region(svc, ctx, actor, c),
        AdminCmd::DropItem(c) => world::drop_item(svc, ctx, actor, c),
        AdminCmd::SetWeather(c) => world::set_weather(svc, ctx, actor, c),
        AdminCmd::SetSpawnScale(c) => world::set_spawn_scale(svc, ctx, actor, c),
        AdminCmd::Heal(c) => combat::heal(svc, ctx, actor, c),
        AdminCmd::Resurrect(c) => combat::resurrect(svc, ctx, actor, c),
        AdminCmd::KillObject(c) => combat::kill_object(svc, ctx, actor, c),
        AdminCmd::DespawnObject(c) => combat::despawn_object(svc, ctx, actor, c),
        AdminCmd::DespawnArea(c) => combat::despawn_area(svc, ctx, actor, c),
        AdminCmd::Mute(c) => moderation::mute(svc, ctx, actor, c),
        AdminCmd::Unmute(c) => moderation::unmute(svc, ctx, actor, c),
        AdminCmd::SetModeFlags(c) => moderation::set_mode_flags(svc, ctx, actor, c),
        AdminCmd::NotifyCharacter(c) => moderation::notify_character(svc, ctx, actor, c),
        AdminCmd::EditCharacter(c) => character::edit_character(svc, ctx, actor, c),
        // answered when the database committed: the dispatcher sends these to `run_deferred` (see `defers`)
        AdminCmd::OfflineCharacterEdit(_) => AdminOutput::Failed(AdminFailure::new(
            super::types::AdminErrorCode::Internal,
            "an offline edit answers when the database committed (run_deferred)",
        )),
        AdminCmd::RemoveItem(c) => items::remove_item(svc, ctx, actor, c),
        AdminCmd::AdjustGold(c) => items::adjust_gold(svc, ctx, actor, c),
        AdminCmd::PetAction(c) => items::pet_action(svc, ctx, actor, c),
        AdminCmd::GrantBatch(c) => grants::grant_batch(svc, ctx, actor, c),
        // answered when the database committed: the dispatcher sends these to `run_deferred` (see `defers`)
        AdminCmd::CancelPendingGrant(_) | AdminCmd::CancelPendingRemoval(_) => {
            AdminOutput::Failed(AdminFailure::new(
                super::types::AdminErrorCode::Internal,
                "a cancel answers when the database committed (run_deferred)",
            ))
        }
        // answered when the database committed: the dispatcher sends these to `run_deferred` (see `defers`)
        AdminCmd::GuildCreate(_) | AdminCmd::GuildEdit(_) | AdminCmd::GuildDisband(_) => {
            AdminOutput::Failed(AdminFailure::new(
                super::types::AdminErrorCode::Internal,
                "a guild command answers when the database committed (run_deferred)",
            ))
        }
        AdminCmd::SetNoticeSchedule(c) => comm::set_notice_schedule(svc, ctx, actor, c),
        AdminCmd::SendNotice(c) => comm::send_notice(svc, ctx, actor, c),
        AdminCmd::RunJob(c) => server::run_job(svc, ctx, actor, c),
        // the older commands never come here (`handles` is false for them)
        other => AdminOutput::Error(format!(
            "{} is not a console command",
            crate::dispatch::admin_name(&other)
        )),
    }
}

/// The scaffolding answer of a command whose work package has not landed.
pub(crate) fn not_implemented(what: &str, wp: &str) -> AdminOutput {
    AdminOutput::Failed(AdminFailure::not_implemented(what, wp))
}
