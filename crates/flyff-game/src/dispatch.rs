//! `GameInput` and `DbReply` dispatch (Plan/10 §4.6, §4.7, WP-OPS-06).
//!
//! Every variant of C1 is handled here as a skeleton: the *mechanics* (sink and slot lifecycle, the session
//! table, `NetEvent`s, panic scopes, urgent flushes) are the runtime's, the *policy* sits behind the traits of
//! [`crate::handlers`] and [`crate::session::rules`]. Scopes follow the table of 10 §4.6: `Join`/`Reattach`
//! bodies and `Client` handlers run under the session's `Player` scope; leaves, kicks, admin, shutdown and
//! `DbReply` continuations are unscoped (a panic there ends the process, 10 §14.4).

use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::handlers::Handlers;
use crate::input::{
    AdminActor, AdminCmd, AdminOutput, ClientMsg, GameInput, JoinFailReason, JoinRequest, NetEvent,
    ReattachResult,
};
use crate::outbox::FlushReport;
use crate::panic::HandlerScope;
use crate::session::{JoinDone, JoinInfo, LeaveTrigger, SessionEntry, begin_leave};
use crate::tick::{Game, RunState};
use crate::tickets::{FlushPurpose, Pending, TicketResult, char_op_error};
use flyff_common::rt::{InputKind, KickReason, LostCause, Reply, SessionSink};
use flyff_common::{AccountId, CharacterId, ObjId, SessionId};
use flyff_db::{AtomicOutcome, DbReply, Ticket};
use flyff_proto::pb;
use std::collections::BTreeMap;
use std::sync::atomic::Ordering;

/// Counters of the dispatcher (the owner of `/metrics` wires them, WP-OPS-18).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DispatchStats {
    /// In-world client messages received.
    pub client_msgs: u64,
    /// `Client` inputs of unknown or unbound sessions (`flyff_inbound_dropped_total{reason="no_session"}`).
    pub dropped_no_session: u64,
    /// `Client` inputs of tainted avatars.
    pub dropped_tainted: u64,
    /// `SessionLost` inputs of unknown or already unbound sessions.
    pub session_lost_ignored: u64,
    /// `ClockOffset` inputs of unknown sessions.
    pub clock_offset_ignored: u64,
    /// Joins committed.
    pub joins: u64,
    /// Joins refused (the sink is dropped, `JoinFailed` is emitted).
    pub joins_refused: u64,
    /// Re-attaches that found their avatar.
    pub reattaches: u64,
    /// `SessionLost{SlowConsumer}` inputs synthesised from flush reports.
    pub slow_consumer_losses: u64,
    /// Client messages without a handler, per message name.
    pub unhandled: BTreeMap<&'static str, u64>,
}

/// Name of an admin command for error texts and logs.
pub(crate) fn admin_name(cmd: &AdminCmd) -> &'static str {
    match cmd {
        AdminCmd::Status => "status",
        AdminCmd::SaveAll => "save-all",
        AdminCmd::SetRuntime { .. } => "set",
        AdminCmd::UnsetRuntime { .. } => "unset",
        AdminCmd::Digest => "digest",
        AdminCmd::CancelShutdown => "shutdown cancel",
        AdminCmd::RunMaintenance => "db maintenance now",
        AdminCmd::RunLine { .. } => "GM command line",
        AdminCmd::ListOnline { .. } => "online",
        AdminCmd::Kick { .. } => "kick",
        AdminCmd::Broadcast { .. } => "broadcast",
        AdminCmd::GetRates | AdminCmd::SetRate { .. } => "rates",
        AdminCmd::GetEvents | AdminCmd::SetEvent { .. } => "events",
        AdminCmd::AuthorityChanged { .. } => "authority change",
        AdminCmd::ReloadScripts { .. } => "script reload",
        AdminCmd::TeleportCharacter(_) => "teleport character",
        AdminCmd::Heal(_) => "heal",
        AdminCmd::Resurrect(_) => "resurrect",
        AdminCmd::KillObject(_) => "kill object",
        AdminCmd::DespawnObject(_) => "despawn object",
        AdminCmd::DespawnArea(_) => "despawn area",
        AdminCmd::SpawnMonsters(_) => "spawn monsters",
        AdminCmd::RespawnRegion(_) => "respawn region",
        AdminCmd::DropItem(_) => "drop item",
        AdminCmd::Mute(_) => "mute",
        AdminCmd::Unmute(_) => "unmute",
        AdminCmd::SetModeFlags(_) => "set mode flags",
        AdminCmd::NotifyCharacter(_) => "notify character",
        AdminCmd::EditCharacter(_) => "edit character",
        AdminCmd::OfflineCharacterEdit(_) => "offline character edit",
        AdminCmd::RemoveItem(_) => "remove item",
        AdminCmd::AdjustGold(_) => "adjust gold",
        AdminCmd::PetAction(_) => "pet action",
        AdminCmd::GrantBatch(_) => "grant batch",
        AdminCmd::CancelPendingGrant(_) => "cancel pending grant",
        AdminCmd::CancelPendingRemoval(_) => "cancel pending removal",
        AdminCmd::GuildCreate(_) => "guild create",
        AdminCmd::GuildEdit(_) => "guild edit",
        AdminCmd::GuildDisband(_) => "guild disband",
        AdminCmd::SetNoticeSchedule(_) => "set notice schedule",
        AdminCmd::SendNotice(_) => "send notice",
        AdminCmd::SetWeather(_) => "set weather",
        AdminCmd::RunJob(_) => "run job",
        AdminCmd::SetSpawnScale(_) => "set spawn scale",
    }
}

/// What an input is, for the observability of the dispatch: its kind (`flyff_game_inputs_total{kind}`), its name
/// (the proto name of a client message, else the name of the kind: the panic context and the overrun log), and the
/// session and character it is about.
fn input_identity(
    input: &GameInput,
) -> (
    InputKind,
    &'static str,
    Option<SessionId>,
    Option<CharacterId>,
) {
    let plain = |kind: InputKind, session: Option<SessionId>| (kind, kind.name(), session, None);
    match input {
        GameInput::Join(req) => (
            InputKind::Join,
            "join",
            Some(req.session),
            Some(req.character.identity.id),
        ),
        GameInput::Reattach { session, .. } => plain(InputKind::Reattach, Some(*session)),
        GameInput::Client { session, msg } => {
            (InputKind::Client, msg.case().name(), Some(*session), None)
        }
        GameInput::SessionLost { session, .. } => plain(InputKind::SessionLost, Some(*session)),
        GameInput::KickAccount { .. } => plain(InputKind::KickAccount, None),
        GameInput::CreateCharacter { .. } => plain(InputKind::CreateCharacter, None),
        GameInput::DeleteCharacter { .. } => plain(InputKind::DeleteCharacter, None),
        GameInput::Admin { .. } => plain(InputKind::Admin, None),
        GameInput::AdminView { .. } => plain(InputKind::AdminView, None),
        GameInput::ReloadLive(_) => plain(InputKind::ReloadLive, None),
        GameInput::Shutdown { .. } => plain(InputKind::Shutdown, None),
        GameInput::ClockOffset { session, .. } => plain(InputKind::ClockOffset, Some(*session)),
    }
}

impl Game {
    /// The handlers, the services and the context as disjoint borrows.
    pub(crate) fn split_handlers(&mut self) -> (&mut Handlers, &mut Services, GameCtx<'_>) {
        let (handlers, services, ctx) = self.split_all();
        (handlers, services, ctx)
    }

    /// Dispatches one C1 input (10 §4.6) and then executes the leaves it requested. Counts it by kind, names it in
    /// the panic context (session, character and proto name of a client message, `join` for a join) for the
    /// time it runs, and tells the overrun log how long it took.
    pub(crate) fn dispatch(&mut self, input: GameInput) {
        let (kind, name, session, character) = input_identity(&input);
        let character = character.or_else(|| session.and_then(|s| self.character_of(s)));
        self.telemetry.count_input(kind);
        crate::panic::set_unit(session, character, name);
        let started = self.telemetry.now_ns();
        self.dispatch_inner(input);
        let ns = self.telemetry.now_ns().saturating_sub(started);
        self.tick_window.slowest.input.note(name, ns);
        crate::panic::clear_unit();
    }

    fn dispatch_inner(&mut self, input: GameInput) {
        match input {
            GameInput::Join(req) => self.on_join(*req),
            GameInput::Reattach {
                account,
                session,
                sink,
                reply,
            } => self.on_reattach(account, session, sink, reply),
            GameInput::Client { session, msg } => self.on_client(session, msg),
            GameInput::SessionLost { session, cause } => self.on_session_lost(session, cause),
            GameInput::KickAccount {
                account,
                reason,
                reply,
            } => self.on_kick_account(account, reason, reply),
            GameInput::CreateCharacter {
                account,
                req,
                reply,
            } => {
                let (h, svc, mut ctx) = self.split_handlers();
                h.rules.create_character(svc, &mut ctx, account, req, reply);
            }
            GameInput::DeleteCharacter {
                account,
                character,
                reply,
            } => {
                let (h, svc, mut ctx) = self.split_handlers();
                h.rules
                    .delete_character(svc, &mut ctx, account, character, reply);
            }
            // answered when the flush behind the snapshots committed (10 §13.1)
            GameInput::Admin {
                cmd: AdminCmd::SaveAll,
                reply,
                ..
            } => self.admin_save_all(reply),
            // answered when the batch behind the command committed (offline edits, rename; 25 §4.6)
            GameInput::Admin { cmd, actor, reply } if crate::admin::actions::defers(&cmd) => {
                let (svc, mut ctx) = self.split();
                crate::admin::actions::run_deferred(svc, &mut ctx, cmd, actor, reply);
            }
            GameInput::Admin { cmd, actor, reply } => {
                let out = self.on_admin(cmd, actor);
                let _ = reply.send(out);
            }
            // admin subscriptions and read-only queries: not recorded, no change of world state (25 ADM-D4)
            GameInput::AdminView { conn, op } => self.on_admin_view(conn, op),
            GameInput::ReloadLive(cfg) => self.on_reload_live(cfg),
            GameInput::Shutdown {
                countdown_s,
                reason,
                message,
            } => {
                tracing::info!(
                    countdown_s,
                    ?reason,
                    message = message.as_deref().unwrap_or(""),
                    "shutdown requested"
                );
                self.on_shutdown_request(countdown_s, message);
            }
            GameInput::ClockOffset {
                session,
                offset_frames,
            } => self.on_clock_offset(session, offset_frames),
        }
        self.drain_leaves();
        self.drain_gm_requests();
    }

    // ---- join / re-attach ---------------------------------------------------------------------------------

    /// `Join` (10 §4.6, 12 §11.2): the sink is attached first, so a refusal or a panic of the hook can never
    /// leak it; the hook builds the avatar, the runtime commits the entry.
    fn on_join(&mut self, req: JoinRequest) {
        let JoinRequest {
            session,
            account,
            authority,
            character,
            sink,
            client_build,
        } = req;
        if !matches!(self.run_state, RunState::Running) {
            self.refuse_join(session, sink, JoinFailReason::ShuttingDown);
            return;
        }
        if self.sessions.player_of_account(account).is_some() {
            tracing::error!(
                session_id = session.0,
                account = account.0,
                "join refused: the account already has an avatar (invariant I3 broken)"
            );
            self.refuse_join(session, sink, JoinFailReason::AlreadyOnline);
            return;
        }
        if self.outbox.contains(session) {
            tracing::error!(
                session_id = session.0,
                "join refused: the session is already attached"
            );
            self.refuse_join(session, sink, JoinFailReason::Internal);
            return;
        }
        let play_time_base_s = character.row.play_time_s;
        let character_id = character.identity.id;
        self.outbox.attach(session, sink, None);
        let info = JoinInfo {
            session,
            account,
            authority,
            character,
            client_build: client_build.clone(),
        };
        let mut outcome: Option<Result<JoinDone, JoinFailReason>> = None;
        let completed = self.scoped(HandlerScope::Player(session), |g| {
            let (h, svc, mut ctx) = g.split_handlers();
            outcome = Some(h.rules.join(svc, &mut ctx, info));
        });
        match (completed, outcome) {
            (true, Some(Ok(done))) => {
                // Commit (unscoped, panic-free).
                let entry = SessionEntry::new(
                    done.player,
                    account,
                    character_id,
                    session,
                    authority,
                    client_build,
                    self.clock.tick,
                    self.clock.unix_ms,
                    play_time_base_s,
                );
                self.sessions.insert(entry);
                self.outbox.set_player(session, done.player);
                self.counters.joins += 1;
                {
                    let (h, svc, mut ctx) = self.split_handlers();
                    h.rules.joined(svc, &mut ctx, done.player);
                }
                tracing::debug!(
                    session_id = session.0,
                    player = done.player.0,
                    "join committed"
                );
            }
            (true, Some(Err(reason))) => {
                // Nothing may remain: drop the slot and the sink (the socket stays open, back to CharSelect).
                drop(self.outbox.remove(session));
                self.counters.joins_refused += 1;
                self.send_net(NetEvent::JoinFailed { session, reason });
            }
            (true, None) | (false, _) => {
                // A panic: `on_scoped_panic` closed the sink with `InternalError` and removed the slot.
                drop(self.outbox.remove(session));
                self.counters.joins_refused += 1;
                self.send_net(NetEvent::JoinFailed {
                    session,
                    reason: JoinFailReason::Internal,
                });
            }
        }
    }

    fn refuse_join(
        &mut self,
        session: SessionId,
        sink: Box<dyn SessionSink>,
        reason: JoinFailReason,
    ) {
        drop(sink);
        self.counters.joins_refused += 1;
        self.send_net(NetEvent::JoinFailed { session, reason });
    }

    /// `Reattach` (10 §4.6, 12 RA2): binds the new session to the avatar of the account and asks the world to
    /// write the join sequence from memory.
    fn on_reattach(
        &mut self,
        account: AccountId,
        session: SessionId,
        sink: Box<dyn SessionSink>,
        reply: Reply<ReattachResult>,
    ) {
        let Some(player) = self.sessions.player_of_account(account) else {
            // The lobby keeps using the socket: drop the (gated) sink without closing it.
            drop(sink);
            let _ = reply.send(ReattachResult::NoAvatar);
            return;
        };
        if self.sessions.player_of(session).is_some() || self.outbox.contains(session) {
            tracing::error!(
                session_id = session.0,
                "re-attach refused: the session is already in use"
            );
            drop(sink);
            let _ = reply.send(ReattachResult::NoAvatar);
            return;
        }
        let old_slot = self.outbox.slot_of(player);
        self.sessions.rebind(player, session);
        match old_slot {
            Some(old) => {
                let _ = self.outbox.rebind(old, session, sink);
            }
            None => self.outbox.attach(session, sink, Some(player)),
        }
        let completed = self.scoped(HandlerScope::Player(session), |g| {
            let (h, svc, mut ctx) = g.split_handlers();
            h.rules.reattached(svc, &mut ctx, player, session);
        });
        let result = if completed {
            self.counters.reattaches += 1;
            ReattachResult::Attached
        } else {
            ReattachResult::NoAvatar
        };
        let _ = reply.send(result);
    }

    // ---- client messages ----------------------------------------------------------------------------------

    fn on_client(&mut self, session: SessionId, msg: ClientMsg) {
        self.counters.client_msgs += 1;
        let Some(player) = self.sessions.player_of(session) else {
            self.counters.dropped_no_session += 1;
            tracing::debug!(
                session_id = session.0,
                "client message of an unknown or unbound session dropped"
            );
            return;
        };
        if self.sessions.get(player).is_some_and(|e| e.tainted) {
            self.counters.dropped_tainted += 1;
            return;
        }
        self.scoped(HandlerScope::Player(session), move |g| {
            let (h, svc, mut ctx) = g.split_handlers();
            h.client
                .route(svc, &mut ctx, PlayerRef(player), session, msg);
        });
    }

    fn on_clock_offset(&mut self, session: SessionId, offset_frames: i64) {
        match self
            .sessions
            .player_of(session)
            .and_then(|p| self.sessions.get_mut(p))
        {
            Some(e) => e.clock_offset_frames = offset_frames,
            None => self.counters.clock_offset_ignored += 1,
        }
    }

    // ---- leaving --------------------------------------------------------------------------------------------

    fn on_session_lost(&mut self, session: SessionId, cause: LostCause) {
        tracing::debug!(session_id = session.0, ?cause, "session lost");
        match self.sessions.player_of(session) {
            Some(player) => self.begin_leave(player, LeaveTrigger::SocketLost(cause)),
            None => {
                self.counters.session_lost_ignored += 1;
                // A slot that never got an avatar (a join that did not commit) must not linger.
                if self
                    .outbox
                    .slot(session)
                    .is_some_and(|s| s.player.is_none())
                {
                    drop(self.outbox.remove(session));
                }
            }
        }
    }

    fn on_kick_account(
        &mut self,
        account: AccountId,
        reason: KickReason,
        reply: Option<Reply<bool>>,
    ) {
        let player = self.sessions.player_of_account(account);
        if let Some(p) = player {
            self.begin_leave(p, LeaveTrigger::Kicked(reason));
        }
        if let Some(r) = reply {
            let _ = r.send(player.is_some());
        }
    }

    /// Starts a leave through the rules (unscoped).
    pub(crate) fn begin_leave(&mut self, player: ObjId, trigger: LeaveTrigger) {
        let (h, svc, mut ctx) = self.split_handlers();
        begin_leave(h.rules.as_mut(), svc, &mut ctx, player, trigger);
    }

    /// Executes the leave requests queued through `GameCtx::request_leave` (and the P18 linger check), in
    /// request order; a leave may queue further leaves.
    pub(crate) fn drain_leaves(&mut self) {
        let mut rounds = 0;
        while !self.leaves.is_empty() && rounds < 64 {
            rounds += 1;
            for (player, trigger) in std::mem::take(&mut self.leaves) {
                self.begin_leave(player, trigger);
            }
        }
        if !self.leaves.is_empty() {
            tracing::error!(
                pending = self.leaves.len(),
                "leave requests keep queueing more leaves: dropped"
            );
            self.leaves.clear();
        }
    }

    /// Player-scoped panic (10 §14.3): the session is poisoned: one `Kick{SERVER_ERROR}` frame straight to the
    /// sink, the close with `InternalError`, and the avatar is removed without a save.
    pub(crate) fn poison_session(&mut self, session: SessionId) {
        if let Some(sink) = self.outbox.remove(session) {
            let kick = flyff_proto::EncodedMsg::new(
                0,
                pb::Kick {
                    reason: pb::kick::Reason::ServerError as i32,
                    tid: 0,
                    detail: String::new(),
                    reconnect_after_ms: 0,
                },
            );
            let mut one = flyff_proto::SessionOutbox::new();
            one.push(&kick);
            for frame in one.take_frames(self.clock.frame0.0, usize::MAX) {
                let _ = sink.send_batch(frame);
            }
            sink.close(flyff_common::rt::CloseReason::InternalError);
        }
        if let Some(player) = self.sessions.player_of(session) {
            if let Some(e) = self.sessions.get_mut(player) {
                e.tainted = true;
            }
            self.leaves.push((player, LeaveTrigger::Tainted));
        }
        self.drain_leaves();
    }

    // ---- admin ----------------------------------------------------------------------------------------------

    fn on_admin(&mut self, cmd: AdminCmd, actor: AdminActor) -> AdminOutput {
        match cmd {
            AdminCmd::Status => AdminOutput::Status(self.status_snapshot()),
            AdminCmd::CancelShutdown => self.cancel_shutdown(),
            // the console's own commands (25 §11.5): one area file per group, stubs until their work packages land
            other if crate::admin::actions::handles(&other) => {
                let (svc, mut ctx) = self.split();
                crate::admin::actions::run(svc, &mut ctx, other, actor)
            }
            other => {
                if crate::admin_ops::is_builtin(&other) && !self.handlers.admin.handles(&other) {
                    return self.admin_builtin(other, actor);
                }
                let (h, svc, mut ctx) = self.split_handlers();
                h.admin.run(svc, &mut ctx, other, actor)
            }
        }
    }

    // ---- flush reports ---------------------------------------------------------------------------------------

    /// `on_report` of 10 §6.4: counters, and a deferred `SessionLost{SlowConsumer}` per full sink, processed at
    /// the start of the next tick (so a slow session's leave rule never runs in the middle of a flush).
    pub(crate) fn on_flush_report(&mut self, report: FlushReport) {
        self.telemetry
            .out_batches
            .fetch_add(report.frames, Ordering::Relaxed);
        self.telemetry
            .out_bytes
            .fetch_add(report.bytes, Ordering::Relaxed);
        for sid in report.full {
            self.telemetry
                .slow_consumers
                .fetch_add(1, Ordering::Relaxed);
            self.counters.slow_consumer_losses += 1;
            tracing::warn!(session_id = sid.0, "slow consumer: session lost");
            self.deferred.push_back(GameInput::SessionLost {
                session: sid,
                cause: LostCause::SlowConsumer,
            });
        }
        // Closed sockets: recorded only; the gateway reports the closed socket itself.
        for sid in report.closed {
            tracing::debug!(
                session_id = sid.0,
                "sink closed: the gateway reports the lost session"
            );
        }
    }

    // ---- DbReply (10 §4.7) -----------------------------------------------------------------------------------

    /// Turns one persistence completion into its continuation (unscoped): counted as an input of kind `db_reply`
    /// and named in the panic context while it runs.
    pub(crate) fn on_db_reply(&mut self, reply: DbReply) {
        self.telemetry.count_input(InputKind::DbReply);
        crate::panic::set_unit(None, None, InputKind::DbReply.name());
        let started = self.telemetry.now_ns();
        self.run_db_reply(reply);
        let ns = self.telemetry.now_ns().saturating_sub(started);
        self.tick_window.slowest.input.note("db_reply", ns);
        crate::panic::clear_unit();
    }

    fn run_db_reply(&mut self, reply: DbReply) {
        match reply {
            DbReply::Committed { ticket, outcome } => {
                self.complete_ticket(ticket, TicketResult::Committed(outcome));
            }
            DbReply::Failed { ticket, error } => {
                tracing::warn!(ticket = ticket.0, error = %error, "database command failed");
                self.complete_ticket(ticket, TicketResult::Failed(error));
            }
            DbReply::SerialBlock { ticket, block } => {
                self.complete_ticket(ticket, TicketResult::SerialBlock(block));
            }
            DbReply::ReadDone { ticket, result } => {
                self.complete_ticket(ticket, TicketResult::ReadDone(result));
            }
            DbReply::Degraded { reason, .. } => {
                tracing::error!(reason = %reason, "persistence degraded");
                self.telemetry
                    .health
                    .db_degraded
                    .store(true, Ordering::Relaxed);
                self.notice_gms(&format!("database writes are failing: {reason}"));
            }
            DbReply::Recovered => {
                tracing::info!("persistence recovered");
                self.telemetry
                    .health
                    .db_degraded
                    .store(false, Ordering::Relaxed);
                self.notice_gms("database writes recovered");
            }
        }
        self.drain_leaves();
    }

    fn complete_ticket(&mut self, ticket: Ticket, result: TicketResult) {
        let Some(pending) = self.tickets.take(ticket) else {
            tracing::warn!(ticket = ticket.0, "database reply for an unknown ticket");
            return;
        };
        match pending {
            Pending::CharacterCreate { reply } => {
                let answer = match result {
                    TicketResult::Committed(AtomicOutcome::CharacterCreated(id)) => Ok(id),
                    TicketResult::Failed(e) => Err(char_op_error(&e)),
                    _ => Err(crate::input::CharOpError::Internal),
                };
                let _ = reply.send(answer);
            }
            Pending::CharacterDelete { reply } => {
                let answer = match result {
                    TicketResult::Committed(_) => Ok(()),
                    TicketResult::Failed(e) => Err(char_op_error(&e)),
                    _ => Err(crate::input::CharOpError::Internal),
                };
                let _ = reply.send(answer);
            }
            Pending::SerialBlock => match result {
                TicketResult::SerialBlock(block) => self.services.serials.on_block(block),
                other => tracing::warn!(
                    ticket = ticket.0,
                    ?other,
                    "the serial block request failed; the allocator falls back to blocking reservations"
                ),
            },
            Pending::Flush(FlushPurpose::Shutdown) => self.on_shutdown_flush_done(ticket),
            Pending::Flush(FlushPurpose::SaveAll { reply, affected }) => {
                let _ = reply.send(match result {
                    TicketResult::Failed(e) => AdminOutput::Error(format!("save failed: {e}")),
                    _ => AdminOutput::Done { affected },
                });
            }
            Pending::Service(f) => {
                let (svc, mut ctx) = self.split();
                f.run(svc, &mut ctx, result);
            }
        }
    }

    /// `SystemNotice` to every online GM (authority `'L'` and above; 10 §4.7).
    pub(crate) fn notice_gms(&mut self, text: &str) {
        let targets: Vec<SessionId> = self
            .sessions
            .iter()
            .filter(|e| e.authority.is_gm())
            .filter_map(|e| e.session)
            .collect();
        for sid in targets {
            self.outbox.to_session(
                sid,
                0,
                pb::SystemNotice {
                    text: text.to_owned(),
                },
            );
        }
    }

    /// Publishes the session gauges after a tick (`players_in_world`, `players_lingering`).
    pub(crate) fn publish_session_gauges(&self) {
        self.telemetry
            .players_in_world
            .store(self.sessions.len() as u64, Ordering::Relaxed);
        self.telemetry
            .players_lingering
            .store(self.sessions.unbound_len() as u64, Ordering::Relaxed);
    }

    fn send_net(&self, ev: NetEvent) {
        let _ = self.net_events.send(ev);
    }
}
