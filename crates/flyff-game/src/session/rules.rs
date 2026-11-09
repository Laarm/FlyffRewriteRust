//! The seams of the session life cycle (Plan/10 §4.6, 12 §11–§13, 15 §9, WP-OPS-06).
//!
//! The runtime ([`crate::dispatch`]) owns the *mechanics* of every lobby-originated input: attaching and
//! detaching sinks, the session table, `NetEvent`s, panic scopes. The *policy* is behind [`SessionRules`],
//! which doc 12 (`on_join` invariants, the leave rule, `finish_leave`, characters) and doc 15 (the join driver
//! `world::join_player`, the re-attach view) implement; [`StubRules`] is the M1 stub used until they land
//! (12 §11.2 "M1 join stub", §12.7 "M1 subset").
//!
//! A leave is requested with [`crate::GameCtx::request_leave`] (or by the runtime itself) and executed by the
//! runtime right after the requesting handler returns, in request order (see [`begin_leave`]).

use crate::ctx::{GameCtx, Services};
use crate::input::{
    AfterLeave, Authority, CharOpError, CreateCharacterRequest, JoinFailReason, NetEvent,
};
use crate::world::ObjIdAllocator;
use flyff_common::rt::{CloseReason, KickReason, LostCause, Reply};
use flyff_common::{AccountId, CharacterId, GameMs, ObjId, SaveGen, SessionId};
use flyff_db::CharacterLoad;
use flyff_proto::pb;

/// Why an avatar leaves (12 §12.2 trigger column).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeaveTrigger {
    /// The socket ended (`GameInput::SessionLost`).
    SocketLost(LostCause),
    /// `KickAccount`: duplicate login, GM kick or ban (the sink gets a `Kick` frame and is closed).
    Kicked(KickReason),
    /// `ReturnToCharSelect`: the socket stays open and goes back to the lobby.
    ReturnToCharSelect,
    /// The shutdown countdown ended (everyone leaves immediately).
    Shutdown,
    /// A player-scoped panic (14.3): immediate, no snapshot.
    Tainted,
    /// The linger deadline of P18 (`SessionTable::pop_due_lingers`): finish at once.
    LingerExpired,
}

impl LeaveTrigger {
    /// The `reason` of the `session.logout` audit line (Plan/13 §25.2).
    pub fn audit_reason(&self) -> &'static str {
        match self {
            LeaveTrigger::SocketLost(_) | LeaveTrigger::LingerExpired => "disconnect",
            LeaveTrigger::Kicked(KickReason::DuplicateLogin) => "takeover",
            LeaveTrigger::Kicked(KickReason::Banned) => "ban",
            LeaveTrigger::Kicked(_) => "kick",
            LeaveTrigger::ReturnToCharSelect => "char_select",
            LeaveTrigger::Shutdown => "shutdown",
            LeaveTrigger::Tainted => "panic",
        }
    }
}

/// What the leave rule decided (12 §12.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeaveMode {
    /// `finish_leave` in the same tick.
    Immediate,
    /// The avatar stays (attackable, no sink) until this `GameMs`; a re-attach cancels it.
    LingerUntil(GameMs),
}

/// A `GameInput::Join` without its sink: the runtime owns the sink from the moment the request arrives (it is
/// attached to the `Outbox` slot of the session before the hook runs, so a panic or a refusal cannot leak it).
pub struct JoinInfo {
    /// Session of the socket.
    pub session: SessionId,
    /// Account.
    pub account: AccountId,
    /// Account authority.
    pub authority: Authority,
    /// Loaded character.
    pub character: Box<CharacterLoad>,
    /// `Hello.client_build`.
    pub client_build: String,
}

/// A successful join: the avatar exists in the world (or in the stub: nowhere).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JoinDone {
    /// The avatar's `ObjId`.
    pub player: ObjId,
}

/// The policy hooks of the session life cycle. Every method has the M1 behaviour of [`StubRules`] as its
/// default except [`SessionRules::join`], which refuses until a world exists.
///
/// All hooks run on the game thread. `join` and `reattached` run under the `Player` panic scope of the session
/// (their pushes are staged and dropped on a panic, the session is poisoned); the others run unscoped.
pub trait SessionRules: Send + 'static {
    /// Doc 12 `session::on_join` (invariants) + doc 15 `world::join_player` (validate, create the mover, write
    /// the join batch with `ctx.outbox.to_session(info.session, ..)`). On `Ok` the runtime commits: it inserts
    /// the `SessionEntry` and binds the avatar to the session's `Outbox` slot. On `Err` nothing may remain
    /// inserted; the runtime drops the sink and emits `NetEvent::JoinFailed` (the socket stays open).
    fn join(
        &mut self,
        svc: &mut Services,
        ctx: &mut GameCtx<'_>,
        info: JoinInfo,
    ) -> Result<JoinDone, JoinFailReason> {
        let _ = (svc, ctx, info);
        Err(JoinFailReason::Internal)
    }

    /// Called by the runtime right after a successful `join` was committed (the `SessionEntry` exists and the
    /// avatar is bound): the services that need the entry (the player directory's online mark, doc 21's join
    /// notices) run here. Unscoped; the default does nothing.
    fn joined(&mut self, svc: &mut Services, ctx: &mut GameCtx<'_>, player: ObjId) {
        let _ = (svc, ctx, player);
    }

    /// Doc 15 re-attach view (15 §9.3): writes the full join sequence from memory for the avatar that was just
    /// re-bound to `session`. The table entry and the outbox slot are already bound.
    fn reattached(
        &mut self,
        svc: &mut Services,
        ctx: &mut GameCtx<'_>,
        player: ObjId,
        session: SessionId,
    ) {
        let _ = (svc, ctx, player, session);
    }

    /// Doc 12 leave rule (12 §12.2) for the avatar whose session was just unbound.
    fn leave_mode(
        &mut self,
        ctx: &GameCtx<'_>,
        player: ObjId,
        trigger: &LeaveTrigger,
    ) -> LeaveMode {
        let _ = (ctx, player, trigger);
        LeaveMode::Immediate
    }

    /// Doc 12 `session::finish_leave` (12 §12.5): destroy the player, final snapshot, remove the entry and
    /// the slot, emit `NetEvent::Left`. The default is the M1 subset (12 §12.7): no snapshot, `final_gen` =
    /// `SaveGen(0)`.
    fn finish_leave(&mut self, svc: &mut Services, ctx: &mut GameCtx<'_>, player: ObjId) {
        let _ = svc;
        finish_leave_stub(ctx, player);
    }

    /// `GameInput::CreateCharacter` (R1): validate, build the starter kit, emit the atomic batch and register the
    /// ticket with `reply`. The default answers with an internal error.
    fn create_character(
        &mut self,
        svc: &mut Services,
        ctx: &mut GameCtx<'_>,
        account: AccountId,
        req: Box<CreateCharacterRequest>,
        reply: Reply<Result<CharacterId, CharOpError>>,
    ) {
        let _ = (svc, ctx, account, req);
        let _ = reply.send(Err(CharOpError::Internal));
    }

    /// `GameInput::DeleteCharacter` (R1).
    fn delete_character(
        &mut self,
        svc: &mut Services,
        ctx: &mut GameCtx<'_>,
        account: AccountId,
        character: CharacterId,
        reply: Reply<Result<(), CharOpError>>,
    ) {
        let _ = (svc, ctx, account, character);
        let _ = reply.send(Err(CharOpError::Internal));
    }
}

/// The M1 subset of the leave end (12 §12.7): remove the entry and the slot, report `Left` with the gen that
/// was already flushed (`SaveGen(0)`: nothing can have changed in M1).
pub fn finish_leave_stub(ctx: &mut GameCtx<'_>, player: ObjId) {
    let Some(entry) = ctx.sessions.remove(player) else {
        return;
    };
    // The slot of a lingering or just-unbound avatar is still keyed by its old session.
    if let Some(sid) = ctx.outbox.slot_of(player) {
        let _ = ctx.outbox.remove(sid);
    }
    // The session id this avatar was last bound to is what the lobby correlates (`Left.session`).
    let session = entry.last_session.unwrap_or(SessionId(0));
    crate::audit::session_logout_stub(ctx, &entry);
    ctx.emit(NetEvent::Left {
        session,
        account: entry.account,
        character: entry.character,
        final_gen: SaveGen::ZERO,
        next: entry.leave_next.unwrap_or(AfterLeave::Close),
    });
}

/// The M1 stub: accepts every join and places the avatar into no world (12 §11.2 "M1 join stub"); leaves are
/// immediate and snapshot-less. Doc 12/15 replace it through `GameInit.handlers`.
#[derive(Debug, Default)]
pub struct StubRules {
    ids: ObjIdAllocator,
}

impl StubRules {
    /// A stub with a fresh id allocator.
    pub fn new() -> Self {
        Self::default()
    }
}

impl SessionRules for StubRules {
    fn join(
        &mut self,
        _svc: &mut Services,
        _ctx: &mut GameCtx<'_>,
        _info: JoinInfo,
    ) -> Result<JoinDone, JoinFailReason> {
        match self.ids.alloc() {
            Some(player) => Ok(JoinDone { player }),
            None => Err(JoinFailReason::Internal),
        }
    }
}

/// Starts a leave (12 §12.2, the runtime part): unbind the session, handle the sink per trigger, ask
/// [`SessionRules::leave_mode`], then finish at once or set the linger deadline. Called by the runtime for
/// `SessionLost`, `KickAccount`, the leave queue and the P18 linger check.
pub fn begin_leave(
    rules: &mut dyn SessionRules,
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    player: ObjId,
    trigger: LeaveTrigger,
) {
    let Some(entry) = ctx.sessions.get(player) else {
        return;
    };
    let bound = entry.session;
    let had_deadline = entry.linger_until;
    // 1. Every trigger unbinds first: later inputs for the session find nothing and are ignored.
    if let Some(sid) = bound {
        ctx.sessions.unbind(sid);
    }
    if let Some(e) = ctx.sessions.get_mut(player) {
        e.leave_next = Some(match trigger {
            LeaveTrigger::ReturnToCharSelect => AfterLeave::CharSelect,
            _ => e.leave_next.unwrap_or(AfterLeave::Close),
        });
        if trigger == LeaveTrigger::Shutdown {
            e.leaving_for_shutdown = true;
        }
        // the audit reason: the first trigger names it, a shutdown or a panic overrides it
        let reason = trigger.audit_reason();
        if matches!(trigger, LeaveTrigger::Shutdown | LeaveTrigger::Tainted) {
            e.leave_reason = Some(reason);
        } else {
            e.leave_reason.get_or_insert(reason);
        }
    }
    // 2. Sink handling of the trigger.
    match (trigger, bound) {
        (LeaveTrigger::Kicked(reason), Some(sid)) => {
            let detail = ctx
                .sessions
                .get_mut(player)
                .and_then(|e| e.kick_detail.take())
                .unwrap_or_default();
            kick_sink_with(ctx, sid, reason, detail);
        }
        (LeaveTrigger::ReturnToCharSelect, Some(sid)) => {
            // The lobby keeps using the socket: drop the game's sink reference without closing it.
            drop(ctx.outbox.remove(sid));
        }
        _ => {}
    }
    // 3. The leave rule.
    let mode = match trigger {
        LeaveTrigger::Shutdown | LeaveTrigger::Tainted | LeaveTrigger::LingerExpired => {
            LeaveMode::Immediate
        }
        _ => rules.leave_mode(ctx, player, &trigger),
    };
    match mode {
        LeaveMode::Immediate => rules.finish_leave(svc, ctx, player),
        LeaveMode::LingerUntil(t) => {
            // A second trigger never extends a running linger.
            let until = had_deadline.map_or(t, |old| old.min(t));
            ctx.sessions.set_linger(player, Some(until));
            if let Some(sid) = bound {
                ctx.outbox.set_lingering(sid);
            }
        }
    }
}

/// Pushes `Kick{reason}` (urgent), flushes the session, takes its sink and closes it with `Kicked(reason)`
/// (10 §7.7). Used for `KickAccount` and by the shutdown step.
pub fn kick_sink(ctx: &mut GameCtx<'_>, sid: SessionId, reason: KickReason) {
    kick_sink_with(ctx, sid, reason, String::new());
}

/// [`kick_sink`] with the `Kick.detail` text (`/kickall <message>`, at most 255 bytes).
pub fn kick_sink_with(ctx: &mut GameCtx<'_>, sid: SessionId, reason: KickReason, detail: String) {
    ctx.outbox.to_session(
        sid,
        0,
        pb::Kick {
            reason: pb::kick::Reason::from(reason) as i32,
            tid: 0,
            detail,
            reconnect_after_ms: 0,
        },
    );
    ctx.outbox.mark_urgent(sid);
    let _ = ctx
        .outbox
        .flush_now(sid, ctx.clock.frame0, ctx.cfg.max_batch_bytes);
    if let Some(sink) = ctx.outbox.remove(sid) {
        sink.close(CloseReason::Kicked(reason));
    }
}
