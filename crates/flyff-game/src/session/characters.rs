//! Character creation and deletion on the game thread (Plan/12 §9.5, §10.2; WP-AUTH-07, WP-AUTH-08; rule R1).
//!
//! The lobby validated the request and sent `GameInput::CreateCharacter` / `DeleteCharacter`; **names and
//! serials are owned by this thread**, so the authoritative checks and the commit happen here:
//!
//! * [`on_create_character`]: directory check (name free, slot free, fewer than three characters), a
//!   `Pending(ticket)` reservation of name and slot, [`build`] (hair colour, start position, starter kit with
//!   fresh serials), one `AtomicBatch{kind: CharacterCreate}` with `reply: Game(ticket)`. On `Committed` the
//!   reservation is released and the new character enters the directory; on any failure the reservation is
//!   released and the error goes to the lobby ([`char_op_error`]).
//! * [`on_delete_character`]: the character must be an active, offline character of the account;
//!   [`CharacterDeleteHooks::check`] may refuse (guild war: `Wartime`, PL-040); then the directory marks it
//!   deleted (name and slot are free at once, PL-057) and one `AtomicBatch{kind: CharacterDelete}` carries
//!   `SoftDeleteCharacter` plus the hooks' ops.
//!
//! [`CharacterRules`] plugs both into the `SessionRules` seam of the dispatcher and forwards everything else to
//! the rules it wraps (the M1 stub today, doc 15's join driver later).

use super::rules::{JoinDone, JoinInfo, LeaveMode, LeaveTrigger, SessionRules};
use crate::ctx::{GameCtx, Services};
use crate::input::{CharOpError, CreateCharacterRequest, JoinFailReason};
use crate::persist::new_character::{
    BuildContext, BuildError, BuildOptions, NewCharacterTemplate, build,
};
use crate::persist::serial::GamePersist;
use crate::presence::{DirEntry, DirectoryError, SLOTS};
use crate::tickets::{Pending, ServiceTicket, TicketResult, char_op_error};
use flyff_common::rt::Reply;
use flyff_common::rt::telemetry::HealthState;
use flyff_common::{AccountId, CharacterId, ObjId, SessionId, UnixTime, WorldId};
use flyff_db::{AtomicBatch, AtomicKind, AtomicOutcome, BatchReply, DbOp};
use std::sync::Arc;

/// What social systems add to a character deletion (21 §3.6): friends, guild (a master's guild is disbanded),
/// campus, housing visitors. M1 has none ([`NoDeleteHooks`]); doc 21 registers its implementation in M6.
pub trait CharacterDeleteHooks: Send + 'static {
    /// May the character be deleted? `Err(CharOpError::Wartime)` while its guild is at war (12 §10.2 step 6).
    /// Read-only: nothing has changed when it refuses.
    fn check(
        &self,
        svc: &Services,
        ctx: &GameCtx<'_>,
        character: CharacterId,
    ) -> Result<(), CharOpError>;

    /// The panic-free commit part: updates the in-memory services and appends the ops that belong to the
    /// `CharacterDelete` batch after `SoftDeleteCharacter` (friends, guild member or disband, campus).
    fn apply(
        &mut self,
        svc: &mut Services,
        ctx: &mut GameCtx<'_>,
        character: CharacterId,
        ops: &mut Vec<DbOp>,
    );
}

/// No social side effects (the M1 behaviour).
#[derive(Debug, Default, Clone, Copy)]
pub struct NoDeleteHooks;

impl CharacterDeleteHooks for NoDeleteHooks {
    fn check(
        &self,
        _svc: &Services,
        _ctx: &GameCtx<'_>,
        _character: CharacterId,
    ) -> Result<(), CharOpError> {
        Ok(())
    }

    fn apply(
        &mut self,
        _svc: &mut Services,
        _ctx: &mut GameCtx<'_>,
        _character: CharacterId,
        _ops: &mut Vec<DbOp>,
    ) {
    }
}

fn shutting_down(ctx: &GameCtx<'_>) -> bool {
    matches!(
        ctx.telemetry.health.state(),
        HealthState::ShuttingDown | HealthState::Stopped
    )
}

fn directory_error(e: DirectoryError) -> CharOpError {
    match e {
        DirectoryError::NameTaken => CharOpError::NameTaken,
        DirectoryError::SlotTaken => CharOpError::SlotTaken,
        DirectoryError::NotFound => CharOpError::NotFound,
    }
}

/// `GameInput::CreateCharacter` (12 §9.5 steps 1-5).
pub fn on_create_character(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    template: &NewCharacterTemplate,
    options: BuildOptions,
    account: AccountId,
    req: Box<CreateCharacterRequest>,
    reply: Reply<Result<CharacterId, CharOpError>>,
) {
    let answer = |reply: Reply<Result<CharacterId, CharOpError>>, r| {
        let _ = reply.send(r);
    };
    if shutting_down(ctx) {
        return answer(reply, Err(CharOpError::ShuttingDown));
    }
    // the lobby validated the request; this thread re-checks what it owns, and refuses a malformed request
    if let Err(e) = req.validate() {
        tracing::warn!(account = account.0, "create refused: {e}");
        return answer(
            reply,
            Err(match e {
                crate::persist::new_character::RequestError::Slot(_) => CharOpError::SlotTaken,
                _ => CharOpError::Internal,
            }),
        );
    }
    // 1. the directory: name free, slot free, fewer than three characters (a full account has no free slot)
    if svc.presence.name_taken(&req.name) {
        return answer(reply, Err(CharOpError::NameTaken));
    }
    let slots = svc.presence.account_slots(account);
    debug_assert_eq!(slots.len(), SLOTS);
    if slots.get(usize::from(req.slot)).is_none_or(Option::is_some) {
        return answer(reply, Err(CharOpError::SlotTaken));
    }
    // 2. reserve name and slot under the ticket of the commit
    let ticket = ctx.tickets.issue_untracked();
    if let Err(e) = svc
        .presence
        .reserve_pending(&req.name, account, req.slot, ticket)
    {
        return answer(reply, Err(directory_error(e)));
    }
    // 3. build the record (fresh serials, start position, starter kit)
    let cx = BuildContext {
        account,
        now: UnixTime(ctx.clock.unix_ms.0.div_euclid(1000)),
        options,
    };
    let record = {
        let mut port = GamePersist {
            persist: &*ctx.persist,
            tickets: &mut *ctx.tickets,
        };
        let mut next_serial = || svc.serials.next(&mut port).map_err(|e| e.to_string());
        build(
            template,
            ctx.data,
            &req,
            &cx,
            &mut next_serial,
            ctx.rng.world(WorldId(template.world)),
        )
    };
    let record = match record {
        Ok(r) => r,
        Err(e) => {
            svc.presence.release_pending(ticket);
            tracing::error!(account = account.0, name = %req.name, "cannot build the new character: {e}");
            let _: &BuildError = &e;
            return answer(reply, Err(CharOpError::Internal));
        }
    };
    // 4. one atomic batch; the continuation completes the reservation
    let entry = DirEntry::new(
        CharacterId(0),
        account,
        req.name.clone(),
        req.slot,
        req.sex,
        template.job,
        u8::try_from(template.level).unwrap_or(u8::MAX),
    );
    ctx.tickets.register(
        ticket,
        Pending::Service(ServiceTicket::new(move |svc, _ctx, result| {
            finish_create(svc, ticket, entry, result, reply);
        })),
    );
    ctx.persist.atomic(AtomicBatch {
        reply: BatchReply::Game(ticket),
        ..AtomicBatch::of_ops(
            AtomicKind::CharacterCreate,
            vec![DbOp::CreateCharacter(Box::new(record))],
        )
    });
}

/// The `DbReply` of a create: release the reservation, insert the character, answer the lobby (12 §9.5 step 5).
fn finish_create(
    svc: &mut Services,
    ticket: flyff_db::Ticket,
    mut entry: DirEntry,
    result: TicketResult,
    reply: Reply<Result<CharacterId, CharOpError>>,
) {
    svc.presence.release_pending(ticket);
    let answer = match result {
        TicketResult::Committed(AtomicOutcome::CharacterCreated(id)) => {
            entry.id = id;
            svc.presence.insert_created(entry);
            Ok(id)
        }
        TicketResult::Failed(e) => Err(char_op_error(&e)),
        other => {
            tracing::error!(
                ?other,
                "CharacterCreate completed with an unexpected result"
            );
            Err(CharOpError::Internal)
        }
    };
    // a lobby that gave up after 10 s dropped its receiver; the character exists and shows in the next list
    let _ = reply.send(answer);
}

/// `GameInput::DeleteCharacter` (12 §10.2 steps 5-6 and the commit).
pub fn on_delete_character(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    hooks: &mut dyn CharacterDeleteHooks,
    account: AccountId,
    character: CharacterId,
    reply: Reply<Result<(), CharOpError>>,
) {
    if shutting_down(ctx) {
        let _ = reply.send(Err(CharOpError::ShuttingDown));
        return;
    }
    // active, owned by the account, not in the world
    let Some(entry) = svc.presence.get(character) else {
        let _ = reply.send(Err(CharOpError::NotFound));
        return;
    };
    if entry.account != account || entry.state != flyff_db::CharacterState::Active {
        let _ = reply.send(Err(CharOpError::NotFound));
        return;
    }
    if entry.online.is_some() {
        let _ = reply.send(Err(CharOpError::Online));
        return;
    }
    // the social veto: nothing has changed when it refuses
    if let Err(e) = hooks.check(svc, ctx, character) {
        let _ = reply.send(Err(e));
        return;
    }
    // commit: the directory first (name and slot free at once), then the hooks, then the ops
    let now = UnixTime(ctx.clock.unix_ms.0.div_euclid(1000));
    svc.presence.mark_deleted(character, now);
    let mut ops = vec![DbOp::SoftDeleteCharacter {
        character_id: character,
        at: now,
    }];
    hooks.apply(svc, ctx, character, &mut ops);
    let ticket = ctx.tickets.issue(Pending::Service(ServiceTicket::new(
        move |svc, _ctx, result| {
            let answer = match result {
                TicketResult::Committed(_) => Ok(()),
                TicketResult::Failed(e) => {
                    // the database refused: the character is still there
                    if !svc.presence.restore(character) {
                        tracing::error!(
                            character = character.0,
                            "a failed delete could not be undone in the directory"
                        );
                    }
                    Err(char_op_error(&e))
                }
                other => {
                    tracing::error!(
                        ?other,
                        "CharacterDelete completed with an unexpected result"
                    );
                    Err(CharOpError::Internal)
                }
            };
            let _ = reply.send(answer);
        },
    )));
    ctx.persist.atomic(AtomicBatch {
        reply: BatchReply::Game(ticket),
        ..AtomicBatch::of_ops(AtomicKind::CharacterDelete, ops)
    });
}

/// The `SessionRules` of the lobby's character operations. Everything but create and delete is the wrapped
/// rules' business (the M1 stub now; doc 15's join and doc 12's leave rule later).
pub struct CharacterRules {
    inner: Box<dyn SessionRules>,
    template: Arc<NewCharacterTemplate>,
    options: BuildOptions,
    hooks: Box<dyn CharacterDeleteHooks>,
}

impl CharacterRules {
    /// Wraps `inner` with the character operations of `template`; no delete hooks, default document capacities.
    pub fn new(inner: Box<dyn SessionRules>, template: NewCharacterTemplate) -> Self {
        Self {
            inner,
            template: Arc::new(template),
            options: BuildOptions::default(),
            hooks: Box::new(NoDeleteHooks),
        }
    }

    /// The document capacities of the init-only `features.*` keys (13 §6.3, §6.5).
    #[must_use]
    pub fn with_options(mut self, options: BuildOptions) -> Self {
        self.options = options;
        self
    }

    /// Installs the social delete hooks (doc 21 in M6; the e2e build's war-time test hook).
    #[must_use]
    pub fn with_hooks(mut self, hooks: Box<dyn CharacterDeleteHooks>) -> Self {
        self.hooks = hooks;
        self
    }

    /// The template in use.
    pub fn template(&self) -> &NewCharacterTemplate {
        &self.template
    }
}

impl SessionRules for CharacterRules {
    fn join(
        &mut self,
        svc: &mut Services,
        ctx: &mut GameCtx<'_>,
        info: JoinInfo,
    ) -> Result<JoinDone, JoinFailReason> {
        self.inner.join(svc, ctx, info)
    }

    fn joined(&mut self, svc: &mut Services, ctx: &mut GameCtx<'_>, player: ObjId) {
        self.inner.joined(svc, ctx, player);
    }

    fn reattached(
        &mut self,
        svc: &mut Services,
        ctx: &mut GameCtx<'_>,
        player: ObjId,
        session: SessionId,
    ) {
        self.inner.reattached(svc, ctx, player, session);
    }

    fn leave_mode(
        &mut self,
        ctx: &GameCtx<'_>,
        player: ObjId,
        trigger: &LeaveTrigger,
    ) -> LeaveMode {
        self.inner.leave_mode(ctx, player, trigger)
    }

    fn finish_leave(&mut self, svc: &mut Services, ctx: &mut GameCtx<'_>, player: ObjId) {
        self.inner.finish_leave(svc, ctx, player);
    }

    fn create_character(
        &mut self,
        svc: &mut Services,
        ctx: &mut GameCtx<'_>,
        account: AccountId,
        req: Box<CreateCharacterRequest>,
        reply: Reply<Result<CharacterId, CharOpError>>,
    ) {
        let template = Arc::clone(&self.template);
        on_create_character(svc, ctx, &template, self.options, account, req, reply);
    }

    fn delete_character(
        &mut self,
        svc: &mut Services,
        ctx: &mut GameCtx<'_>,
        account: AccountId,
        character: CharacterId,
        reply: Reply<Result<(), CharOpError>>,
    ) {
        on_delete_character(svc, ctx, &mut *self.hooks, account, character, reply);
    }
}
