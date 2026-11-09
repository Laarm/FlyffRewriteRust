//! Edits of offline characters and the rename (Plan/25 §4.6, §11.5, ADM-D8; **WP-ADMIN-16**): they go **through
//! the game thread** under an online-registry hold (`OnlineRegistry::admin_hold`, taken by the HTTP route), never
//! straight to the writer.
//!
//! * `offline_character_edit` (`AdminCmd::OfflineCharacterEdit`): `SetPosition` / `ToRevival` (an offline unstuck),
//!   `Rename` and `Undelete`. The game thread checks that the character exists, that nobody of its account is in the
//!   world, reserves the new name in the [`PlayerDirectory`](crate::presence::PlayerDirectory) (rename) or re-inserts
//!   the character (undelete, with the CLI's name and slot checks), and sends **one `AdminOfflineEdit` batch**
//!   (`SetCharacterPosition` / `RenameCharacter` / `UndeleteCharacter`). The answer waits for `DbReply::Committed`;
//!   a failed batch rolls the directory change back and answers the failure.
//! * `rename_online` (`EditCharacter{Rename}`, 21 §3.3): the rename of an online character; the same steps as the
//!   rename scroll (`CharacterRename` batch, directory, avatar, `QuerySetPlayerNameNtf` to everybody) without the
//!   scroll.
//!
//! Both **defer their answer** (they take the `Reply`, [`super::run_deferred`]); the dispatcher asks
//! [`super::defers`] first.

use super::common::{check_rank, fail};
use super::{TargetId, TargetResult};
use crate::admin::actions::character::{CharacterEdit, EditCharacter};
use crate::admin::types::{AdminErrorCode, AdminFailure};
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::input::{AdminActor, AdminOutput};
use crate::mover::Mover;
use crate::presence::DirectoryError;
use crate::tickets::{Pending, ServiceTicket, TicketResult};
use flyff_common::math::Vec3;
use flyff_common::rt::Reply;
use flyff_common::{CharacterId, ObjId, WorldId};
use flyff_db::{
    AdminOfflineEdit, AtomicBatch, AtomicKind, BatchReply, CharacterState, DbError, DbErrorKind,
    DbOp,
};
use flyff_rules::names::{NameError, validate_character_name};
use serde_json::{Value, json};

/// One edit of an offline character (25 §11.10 `OfflineEdit`, plus [`OfflineEdit::ToRevival`]).
#[derive(Clone, Debug, PartialEq)]
pub enum OfflineEdit {
    /// Put the character at a position of a loaded world (layer 0).
    SetPosition {
        /// World.
        world: WorldId,
        /// Position (metres).
        pos: Vec3,
    },
    /// "Return to town" for an offline character: the revival point of the continent (or world) of its **last**
    /// position, resolved on the game thread by the rule of the online unstuck
    /// ([`unstuck_destination`](super::world::unstuck_destination)).
    ToRevival {
        /// The world the character was saved in.
        world: WorldId,
        /// The position it was saved at.
        pos: Vec3,
    },
    /// Rename.
    Rename(String),
    /// Undelete the character into a slot.
    Undelete {
        /// The slot `0..3`.
        slot: u8,
    },
}

/// Edit an offline character (`OfflineCharacterEdit`).
#[derive(Clone, Debug, PartialEq)]
pub struct OfflineCharacterEdit {
    /// The character.
    pub character: CharacterId,
    /// The edit.
    pub edit: OfflineEdit,
}

/// A persistence failure as the console sees it.
pub fn db_failure(e: &DbError) -> AdminFailure {
    let code = match e.kind {
        DbErrorKind::NotFound => AdminErrorCode::NotFound,
        DbErrorKind::NameTaken | DbErrorKind::SlotTaken | DbErrorKind::Invalid => {
            AdminErrorCode::Invalid
        }
        DbErrorKind::Conflict => AdminErrorCode::StaleTarget,
        _ => AdminErrorCode::Internal,
    };
    AdminFailure::new(code, e.to_string())
}

/// The text of a refused name (the rules of `validate_character_name`).
fn name_error_text(e: NameError) -> &'static str {
    match e {
        NameError::Empty => "the name is empty",
        NameError::Length => "the name has the wrong length",
        NameError::LeadingDigit => "the name must not start with a digit",
        NameError::InvalidChar => "the name may contain letters and digits only",
        NameError::Forbidden => "the name is not allowed",
        #[allow(unreachable_patterns)]
        _ => "the name is not valid",
    }
}

/// The validated name (trimmed) of a rename.
fn checked_name(ctx: &GameCtx<'_>, raw: &str) -> Result<String, AdminFailure> {
    let g = &ctx.cfg.rules.gameplay;
    validate_character_name(
        raw,
        &ctx.data.name_rules,
        g.min_name_letters,
        g.max_name_letters,
    )
    .map(str::to_owned)
    .map_err(|e| fail(AdminErrorCode::Invalid, name_error_text(e)))
}

fn directory_failure(e: DirectoryError) -> AdminFailure {
    match e {
        DirectoryError::NameTaken => fail(AdminErrorCode::Invalid, "the name is taken"),
        DirectoryError::SlotTaken => fail(AdminErrorCode::Invalid, "the slot is taken"),
        DirectoryError::NotFound => fail(AdminErrorCode::NotFound, "no such character"),
    }
}

fn answer(reply: Reply<AdminOutput>, out: AdminOutput) {
    let _ = reply.send(out);
}

fn refuse(reply: Reply<AdminOutput>, f: AdminFailure) {
    answer(reply, AdminOutput::Failed(f));
}

fn one(character: CharacterId, info: Value) -> AdminOutput {
    AdminOutput::Report {
        per_target: vec![TargetResult::ok(TargetId::Character(character)).with_info(info)],
    }
}

/// Sends the batch of an offline edit; `on_result` runs on the game thread with the outcome.
fn send_edit(
    ctx: &mut GameCtx<'_>,
    edit: AdminOfflineEdit,
    on_result: impl FnOnce(&mut Services, &mut GameCtx<'_>, TicketResult) + Send + 'static,
) {
    let ticket = ctx
        .tickets
        .issue(Pending::Service(ServiceTicket::new(on_result)));
    ctx.persist.atomic(AtomicBatch {
        reply: BatchReply::Game(ticket),
        ..AtomicBatch::admin_offline_edit(edit)
    });
}

/// `OfflineCharacterEdit`: see the module documentation.
pub(crate) fn offline_character_edit(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    _actor: AdminActor,
    cmd: OfflineCharacterEdit,
    reply: Reply<AdminOutput>,
) {
    let id = cmd.character;
    let Some(entry) = svc.presence.get(id).cloned() else {
        return refuse(
            reply,
            fail(
                AdminErrorCode::NotFound,
                format!("character {} does not exist", id.0),
            ),
        );
    };
    // nobody of the account may be in the world (the route holds the registry entry; this is the second line)
    if ctx.sessions.iter().any(|e| e.account == entry.account) {
        return refuse(
            reply,
            fail(
                AdminErrorCode::CharacterOnline,
                format!("account {} is in the world", entry.account.0),
            ),
        );
    }
    match cmd.edit {
        OfflineEdit::SetPosition { world, pos } => {
            if entry.state != CharacterState::Active {
                return refuse(
                    reply,
                    fail(
                        AdminErrorCode::Invalid,
                        "only a playable character can be moved",
                    ),
                );
            }
            set_position(ctx, id, world, pos, reply);
        }
        OfflineEdit::ToRevival { world, pos } => {
            if entry.state != CharacterState::Active {
                return refuse(
                    reply,
                    fail(
                        AdminErrorCode::Invalid,
                        "only a playable character can be moved",
                    ),
                );
            }
            let Some((w, _, p)) = super::world::unstuck_destination(ctx.worlds, world, pos) else {
                return refuse(
                    reply,
                    fail(
                        AdminErrorCode::Invalid,
                        "no town to send the character to is loaded",
                    ),
                );
            };
            set_position(ctx, id, w, p, reply);
        }
        OfflineEdit::Rename(raw) => {
            let name = match checked_name(ctx, &raw) {
                Ok(n) => n,
                Err(f) => return refuse(reply, f),
            };
            let old = entry.name.clone();
            if old == name {
                return refuse(
                    reply,
                    fail(AdminErrorCode::Invalid, "that is the name already"),
                );
            }
            if let Err(e) = svc.presence.rename(id, &name) {
                return refuse(reply, directory_failure(e));
            }
            let new = name.clone();
            let (account, slot) = (entry.account, entry.slot);
            send_edit(
                ctx,
                AdminOfflineEdit::Rename {
                    character_id: id,
                    name: name.clone(),
                },
                move |svc, ctx, result| match result {
                    TicketResult::Committed(_) => {
                        crate::social::player_data::announce_rename(svc, ctx, id, &new);
                        // 13 §25.2: every rename audits `char.rename` (WP-GM-23)
                        crate::social::player_data::audit_rename(
                            ctx,
                            None,
                            (id, account, slot),
                            &old,
                            &new,
                            "admin",
                        );
                        tracing::info!(character = id.0, from = %old, to = %new, "offline character renamed");
                        answer(reply, one(id, json!({"from": old, "to": new})));
                    }
                    other => {
                        // the database refused: the directory goes back
                        let _ = svc.presence.rename(id, &old);
                        refuse(reply, failure_of(&other));
                    }
                },
            );
        }
        OfflineEdit::Undelete { slot } => {
            if entry.state != CharacterState::Deleted {
                return refuse(
                    reply,
                    fail(AdminErrorCode::Invalid, "the character is not deleted"),
                );
            }
            let deleted_at = entry.deleted_at.unwrap_or(flyff_common::UnixTime(0));
            if let Err(e) = svc.presence.undelete(id, slot) {
                return refuse(reply, directory_failure(e));
            }
            let name = entry.name.clone();
            send_edit(
                ctx,
                AdminOfflineEdit::Undelete {
                    character_id: id,
                    slot,
                },
                move |svc, _ctx, result| match result {
                    TicketResult::Committed(_) => {
                        tracing::info!(character = id.0, slot, "character undeleted");
                        answer(reply, one(id, json!({"slot": slot, "name": name})));
                    }
                    other => {
                        svc.presence.mark_deleted(id, deleted_at);
                        refuse(reply, failure_of(&other));
                    }
                },
            );
        }
    }
}

fn failure_of(result: &TicketResult) -> AdminFailure {
    match result {
        TicketResult::Failed(e) => db_failure(e),
        other => fail(
            AdminErrorCode::Internal,
            format!("the database answered unexpectedly: {other:?}"),
        ),
    }
}

fn set_position(
    ctx: &mut GameCtx<'_>,
    id: CharacterId,
    world: WorldId,
    pos: Vec3,
    reply: Reply<AdminOutput>,
) {
    if ctx.worlds.world(world).is_none() {
        return refuse(
            reply,
            fail(
                AdminErrorCode::Invalid,
                format!("world {} is not loaded", world.0),
            ),
        );
    }
    if !(pos.x.is_finite() && pos.y.is_finite() && pos.z.is_finite()) {
        return refuse(
            reply,
            fail(AdminErrorCode::Invalid, "the position is not finite"),
        );
    }
    send_edit(
        ctx,
        AdminOfflineEdit::SetPosition {
            character_id: id,
            world_id: world.0,
            pos: [pos.x, pos.y, pos.z],
        },
        move |_svc, _ctx, result| match result {
            TicketResult::Committed(_) => answer(
                reply,
                one(
                    id,
                    json!({"world": world.0, "layer": 0, "pos": [pos.x, pos.y, pos.z]}),
                ),
            ),
            other => refuse(reply, failure_of(&other)),
        },
    );
}

// ---- the rename of an online character -------------------------------------------------------------------------------

/// `EditCharacter{Rename}` (21 §3.3): see the module documentation. Any other edit is not this function's.
pub(crate) fn rename_online(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    actor: AdminActor,
    cmd: EditCharacter,
    reply: Reply<AdminOutput>,
) {
    let CharacterEdit::Rename(raw) = cmd.edit else {
        return refuse(reply, fail(AdminErrorCode::Internal, "not a rename"));
    };
    let id = cmd.character;
    let Some(who) = ctx
        .sessions
        .iter()
        .find(|e| e.character == id && ctx.worlds.player(e.player).is_some())
        .map(|e| PlayerRef(e.player))
    else {
        return refuse(
            reply,
            fail(
                AdminErrorCode::CharacterOnline,
                format!("character {} is not in the world", id.0),
            ),
        );
    };
    if let Err(f) = check_rank(&actor, ctx, who.0) {
        return refuse(reply, f);
    }
    let name = match checked_name(ctx, &raw) {
        Ok(n) => n,
        Err(f) => return refuse(reply, f),
    };
    let Some(p) = ctx.worlds.player(who.0).and_then(Mover::player) else {
        return refuse(reply, fail(AdminErrorCode::NotFound, "the avatar is gone"));
    };
    let old = p.ident.name.clone();
    if old == name {
        return refuse(
            reply,
            fail(AdminErrorCode::Invalid, "that is the name already"),
        );
    }
    if p.social.rename_pending {
        return refuse(
            reply,
            fail(
                AdminErrorCode::StaleTarget,
                "a rename of this character is already running",
            ),
        );
    }
    let taken = match svc.presence.by_name(&name) {
        Some(e) => e.id != id,
        None => svc.presence.name_taken(&name),
    };
    if taken {
        return refuse(reply, fail(AdminErrorCode::Invalid, "the name is taken"));
    }
    if let Some(p) = ctx.worlds.player_mut(who.0).and_then(Mover::player_mut) {
        p.social.rename_pending = true;
    }
    let obj: ObjId = who.0;
    let new = name.clone();
    send_edit_rename(ctx, id, name, move |svc, ctx, result| {
        let avatar = ctx
            .worlds
            .player(obj)
            .and_then(Mover::player)
            .is_some_and(|p| p.ident.character_id == id);
        if avatar && let Some(p) = ctx.worlds.player_mut(obj).and_then(Mover::player_mut) {
            p.social.rename_pending = false;
        }
        match result {
            TicketResult::Committed(_) => {
                // the shared tail of the scroll, `/spn` and the console: directory, avatar, broadcast, `char.rename`
                let who = avatar.then_some(PlayerRef(obj));
                if !crate::social::player_data::complete_rename(
                    svc,
                    ctx,
                    id,
                    who,
                    &new,
                    crate::social::player_data::RenameVia::ADMIN,
                ) {
                    return refuse(
                        reply,
                        fail(
                            AdminErrorCode::Internal,
                            "the directory refused a committed rename",
                        ),
                    );
                }
                answer(reply, one(id, json!({"from": old, "to": new})));
            }
            other => refuse(reply, failure_of(&other)),
        }
    });
}

/// The `CharacterRename` batch of the rename (the kind of the rename scroll's batch).
fn send_edit_rename(
    ctx: &mut GameCtx<'_>,
    id: CharacterId,
    name: String,
    on_result: impl FnOnce(&mut Services, &mut GameCtx<'_>, TicketResult) + Send + 'static,
) {
    let ticket = ctx
        .tickets
        .issue(Pending::Service(ServiceTicket::new(on_result)));
    ctx.persist.atomic(AtomicBatch {
        reply: BatchReply::Game(ticket),
        ..AtomicBatch::of_ops(
            AtomicKind::CharacterRename,
            vec![DbOp::RenameCharacter {
                character_id: id,
                name,
            }],
        )
    });
}
