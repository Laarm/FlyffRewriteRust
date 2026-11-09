//! The world-aware session rules (Plan/12 §11-§12, 15 §9, §18; WP-AUTH-09 game side, WP-AUTH-15, WP-WORLD-09,
//! WP-WORLD-13): [`WorldRules`] implements [`SessionRules`] over the real worlds.
//!
//! * **join** (12 §11.2, 15 §9): the load normalisation (13 §17.4), the placement with its fallbacks, the player
//!   enters the world's add queue, the save timer starts, the post-placement teleports are requested and the join
//!   batch of 11 §2.5 is written into the session's queue; everything leaves in the `ServerBatch` of the tick of
//!   the `Join` (the spawns of the visible objects are the `add_to_view` of the same tick's `_add`). With **no
//!   world loaded at all** (a test server with empty data) the M1 stub join of the wrapped rules is used.
//! * **the leave rule** (12 §12.2): [`leave_mode`] is the single implementation: immediate in a safe region, for
//!   GMs, in the guild-war world 202, when not in a world and for every shutdown, tainted and GM-kick / ban
//!   leave; otherwise the avatar lingers `gameplay.timewait_close_s` (10 s), shortened or lengthened by the
//!   `ReqLeave` penalty time ([`on_req_leave`]). The linger keeps the avatar in the world (attackable, no sink).
//! * **finish_leave** (12 §12.5): `destroy_player`, the final snapshot (`Logout`, `ToCharSelect` or `Shutdown`
//!   with the location of `destroy_player`; none for a tainted session), `remove_player`, the entry and the slot,
//!   `NetEvent::Left{final_gen}`.
//! * **re-attach** (12 §13, 15 §9.3): the join sequence again from memory.
//!
//! Character creation and deletion are not here: [`super::CharacterRules`] wraps this type.

use super::rules::{
    JoinDone, JoinInfo, LeaveMode, LeaveTrigger, SessionRules, StubRules, finish_leave_stub,
};
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::input::{AfterLeave, CharOpError, CreateCharacterRequest, JoinFailReason, NetEvent};
use crate::persist::load::{LoadHook, LoadOptions, apply_character_load};
use crate::persist::save::save_player;
use crate::social::hooks as social;
use crate::world::join::{
    DefaultJoinPolicy, JoinPolicy, JoinRegistry, SavedLocation, gc1to1_enabled, place_join,
    replay_for_reattach, write_join_batch,
};
use crate::world::logout::{LogoutHooks, NoLogoutHooks, destroy_player, remove_player};
use crate::world::{ReplaceKind, region};
use flyff_common::defs::define::RA_SAFETY;
use flyff_common::defs::world::WI_WORLD_GUILDWAR;
use flyff_common::math::Vec3;
use flyff_common::rt::{KickReason, Reply};
use flyff_common::{AccountId, CharacterId, GameMs, LayerId, ObjId, SessionId, WorldId};
use flyff_db::SaveReason;
use flyff_sim::types::OBJSTAF_ACC;

/// `GameMs` deadline of a linger started at `now` (`gameplay.timewait_close_s`).
fn linger_deadline(now: GameMs, timewait_close_s: u32) -> GameMs {
    GameMs(now.0.saturating_add(u64::from(timewait_close_s) * 1000))
}

/// The leave rule (12 §12.2, port of `CUserMng::RemoveUserFromCacheMsg`, `src:WORLDSERVER/User.cpp:3945-3980`):
///
/// ```text
/// Shutdown | Tainted | Kicked(GmKick) | Kicked(Banned) => Immediate          // FIX (PL-041)
/// not in a world | RA_SAFETY | authority >= GAMEMASTER | world 202 => Immediate
/// leave_penalty_at: None => LingerUntil(now + timewait), Some(t) if t <= now => Immediate, Some(t) => LingerUntil(t)
/// ```
///
/// `Kicked(DuplicateLogin)`, `SocketLost` and `ReturnToCharSelect` follow the legacy rule (KEEP: a takeover cannot
/// be used to escape combat).
pub fn leave_mode(ctx: &GameCtx<'_>, player: ObjId, trigger: &LeaveTrigger) -> LeaveMode {
    match trigger {
        LeaveTrigger::Shutdown
        | LeaveTrigger::Tainted
        | LeaveTrigger::LingerExpired
        | LeaveTrigger::Kicked(KickReason::GmKick | KickReason::Banned) => {
            return LeaveMode::Immediate;
        }
        _ => {}
    }
    let Some(entry) = ctx.sessions.get(player) else {
        return LeaveMode::Immediate;
    };
    let in_world = ctx
        .worlds
        .core(player)
        .is_some_and(|c| !matches!(c.link, crate::world::LinkState::Detached) && c.world.0 != 0);
    let m = ctx.worlds.player(player);
    let safe = m.is_some_and(|m| region::is_region_attr(region::region_attr_of(m), RA_SAFETY));
    let guild_war = m.is_some_and(|m| m.core.world.0 == WI_WORLD_GUILDWAR);
    if !in_world || safe || entry.authority.is_gm() || guild_war {
        return LeaveMode::Immediate;
    }
    let now = ctx.clock.game_ms;
    match entry.leave_penalty_at {
        None => LeaveMode::LingerUntil(linger_deadline(
            now,
            ctx.cfg.rules.gameplay.timewait_close_s,
        )),
        Some(t) if t <= now => LeaveMode::Immediate,
        Some(t) => LeaveMode::LingerUntil(t),
    }
}

/// `ReqLeave` (12 §12.3): the first request starts the penalty time `now + timewait_close_s`; later requests
/// change nothing. The session stays bound and active; nothing is sent back.
pub fn on_req_leave(ctx: &mut GameCtx<'_>, who: PlayerRef) {
    let deadline = linger_deadline(ctx.clock.game_ms, ctx.cfg.rules.gameplay.timewait_close_s);
    if let Some(e) = ctx.sessions.get_mut(who.0) {
        e.leave_penalty_at.get_or_insert(deadline);
    }
}

/// `ReturnToCharSelect` (12 §12.4): the leave of the same session with the sink detached and kept open.
pub fn on_return_to_char_select(ctx: &mut GameCtx<'_>, who: PlayerRef) {
    ctx.request_leave(who.0, LeaveTrigger::ReturnToCharSelect);
}

/// The session rules over the real worlds (see the module docs).
pub struct WorldRules {
    inner: Box<dyn SessionRules>,
    /// The join sequence (15 §9.2): doc 21/22/16 contributors register here.
    pub registry: JoinRegistry,
    /// The predicates of doc 22 for the placement.
    pub policy: Box<dyn JoinPolicy>,
    /// The cleanups of the logout (15 §18.2).
    pub logout: Box<dyn LogoutHooks>,
    /// The normalisation steps of docs 17/19/20/22 (13 §17.4).
    pub load_hooks: Vec<Box<dyn LoadHook>>,
}

impl std::fmt::Debug for WorldRules {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorldRules")
            .field("registry", &self.registry)
            .finish_non_exhaustive()
    }
}

impl Default for WorldRules {
    fn default() -> Self {
        WorldRules::new()
    }
}

impl WorldRules {
    /// The rules with the standard join contributors, no doc 22 policy and no logout hooks; a game without a
    /// world falls back to the M1 stub.
    pub fn new() -> WorldRules {
        WorldRules {
            inner: Box::new(StubRules::new()),
            registry: JoinRegistry::standard(),
            policy: Box::new(DefaultJoinPolicy),
            logout: Box::new(NoLogoutHooks),
            load_hooks: Vec::new(),
        }
    }

    /// Installs the logout hooks of docs 16-23.
    pub fn with_logout(mut self, hooks: Box<dyn LogoutHooks>) -> WorldRules {
        self.logout = hooks;
        self
    }

    /// Installs doc 22's placement predicates.
    pub fn with_policy(mut self, policy: Box<dyn JoinPolicy>) -> WorldRules {
        self.policy = policy;
        self
    }
}

impl SessionRules for WorldRules {
    fn join(
        &mut self,
        svc: &mut Services,
        ctx: &mut GameCtx<'_>,
        info: JoinInfo,
    ) -> Result<JoinDone, JoinFailReason> {
        if ctx.worlds.is_empty() {
            // no world is loaded (a server without game data): the M1 join stub
            return self.inner.join(svc, ctx, info);
        }
        let row = &info.character.row;
        let saved = SavedLocation {
            world: WorldId(row.world_id),
            layer: LayerId(row.layer),
            pos: Vec3::new(row.pos[0], row.pos[1], row.pos[2]),
        };
        // ---- guild house (doc 22 §12.2, task guild-house): the admission facts that need the services
        self.policy
            .prepare(svc, ctx, info.character.identity.id, saved);
        let placement = place_join(
            ctx.worlds,
            saved,
            info.character.identity.id,
            gc1to1_enabled(ctx.live),
            self.policy.as_ref(),
        );
        if ctx.worlds.world(placement.world).is_none() {
            // Madrigal itself is missing: only with broken data
            return Err(JoinFailReason::InvalidWorld);
        }
        let now = ctx.clock.game_ms;
        let max_pack = u16::try_from(ctx.cfg.rules.gameplay.max_pack_item).unwrap_or(u16::MAX);
        let (built, report) = apply_character_load(
            &info.character,
            info.authority,
            ctx.data,
            now,
            ctx.rng.world(placement.world),
            LoadOptions { max_pack },
            &mut self.load_hooks,
        );
        if !report.repairs.is_empty()
            || report.items_repaired > 0
            || !info.character.repairs.is_empty()
        {
            tracing::warn!(
                character = info.character.identity.id.0,
                repairs = ?report.repairs,
                items = report.items_repaired,
                "join: the character load was repaired"
            );
            // `persist.repair` (13 §25.2): what the loader had to fix, for the operator
            ctx.audit.emit(
                flyff_db::AuditEvent::new(
                    flyff_db::audit::ev::PERSIST_REPAIR,
                    serde_json::json!({
                        "character": info.character.identity.id.0,
                        "numeric": report.repairs.iter().map(|r| format!("{r:?}")).collect::<Vec<_>>(),
                        "items_repaired": report.items_repaired,
                        "containers": info.character.repairs.iter().map(|r| format!("{r:?}")).collect::<Vec<_>>(),
                    }),
                )
                .by(flyff_db::AuditActor {
                    account_id: Some(info.account),
                    character_id: Some(info.character.identity.id),
                    name: Some(info.character.identity.name.clone()),
                    ..flyff_db::AuditActor::default()
                }),
            );
        }
        let mut mover = built.mover;
        // 16 §13.6, §14.3, 17 §7.9: the stats of what the player wears, its running premium items and buffs
        let mut join_fx = crate::stats::join::apply_loaded(svc, ctx, &mut mover);
        // 15 §9.1 step 1: angle 0 (the constructor did), the flying acceleration flag is cleared
        mover.sim.act.flags.0 &= !OBJSTAF_ACC;
        mover.core.pos = placement.pos;
        mover.sim.pos = placement.pos;
        let id = ctx
            .worlds
            .spawn_mover(placement.world, placement.layer, mover)
            .map_err(|e| {
                tracing::error!("join: the placement was refused: {e}");
                JoinFailReason::InvalidWorld
            })?;
        // a joined player has not been through `ProcessRegion` yet: its first process evaluates the attribute
        if let Some(c) = ctx.worlds.store_mut().core_mut(id) {
            c.pos_changed = true;
        }
        svc.saves.track(id, now);
        // ---- quests (doc 20 §9.2, task quest-core): the audit lines of the load normalisation
        crate::quest::emit_load_notes(ctx, PlayerRef(id));
        // post-placement teleports run at this tick's `_replace`, last request wins
        for t in &placement.teleports {
            if let Err(e) = ctx
                .worlds
                .replace(id, t.world, t.pos, LayerId(0), ReplaceKind::Normal)
            {
                tracing::warn!(player = id.0, "join teleport refused: {e}");
            }
        }
        if placement.instance_go_out {
            self.policy.instance_go_out(svc, ctx, PlayerRef(id));
        }
        // 21 §9.1: the friend list of the joiner exists before the batch (`AddFriendGameJoin`) and before the presence
        crate::social::messenger::on_join(svc, &info.character);
        // 21 §9.6: the undelivered offline notes of the joiner (`Tag` is written with the batch)
        crate::social::messenger::notes::on_join(svc, &info.character);
        // ---- housing (doc 22 §13, task housing): the mini room (layer, furniture) from the load; the friend list exists
        crate::event::housing::on_join(svc, ctx, PlayerRef(id), &info.character);
        // 21 §4.3: the party slot comes online before the batch, so the joiner's own `PartyMember` lists it online
        crate::social::party::on_reconnect(svc, ctx, PlayerRef(id));
        // 21 §5.12: the guild and war mirrors before the batch, so that `SpawnSelf` carries `id_guild` / `id_war`
        crate::social::guild::attach(svc, ctx, PlayerRef(id));
        // ---- guild house (doc 22 §12, task guild-house): the furniture DSTs, the offline rest points, the room check
        crate::event::guild_house::on_join(svc, ctx, PlayerRef(id));
        // 21 §11-§13: the campus mirrors, no pending proposal and the honor recheck before the batch (`OnJoin`)
        crate::social::relation::on_join(svc, ctx, PlayerRef(id));
        // `DPDatabaseClient.cpp:725`: no cheer points and the first one in 60 minutes, before the batch so that
        // `SpawnSelf` carries the rest (a re-attach keeps the running timer)
        crate::social::cheer::on_join(ctx, PlayerRef(id));
        write_join_batch(&self.registry, svc, ctx, id, info.session, false);
        // what the buffs of the first process left to do (an expired `BUFF_TOGIFT`...): after the join batch
        for mut f in join_fx.follow.drain(..) {
            f.rebind(ObjId::NULL, id);
            f.run(svc, ctx);
        }
        // 19 §12.6.4 (PL-079): a protected awakening that waits for its choice is offered again
        crate::item::upgrade::handlers::resend_pending(ctx, PlayerRef(id), info.session);
        // 13 §17.4 step 8 / 19 §20.4 (WP-ECON-09): the open grants and removals of the load wait for `joined` (the
        // session entry does not exist yet)
        crate::econ::grants::stash(svc, &info.character);
        Ok(JoinDone { player: id })
    }

    fn joined(&mut self, svc: &mut Services, ctx: &mut GameCtx<'_>, player: ObjId) {
        if ctx.worlds.player(player).is_some() {
            social::on_player_joined(svc, ctx, PlayerRef(player));
            // 19 §20.5: the summoned system pet is announced again after the join batch
            crate::item::pets::system::after_join(svc, ctx, PlayerRef(player));
            // ---- housing (doc 22 §13, task housing): the furniture buffs, the papering of a room the join placed him in
            crate::event::housing::after_join(svc, ctx, PlayerRef(player));
            // 19 §20.4 (WP-ECON-09): the removals of the load, then its grants (ordinary item messages after the batch)
            crate::econ::grants::on_joined(svc, ctx, PlayerRef(player));
        } else {
            self.inner.joined(svc, ctx, player);
        }
    }

    fn reattached(
        &mut self,
        svc: &mut Services,
        ctx: &mut GameCtx<'_>,
        player: ObjId,
        session: SessionId,
    ) {
        if ctx.worlds.player(player).is_none() {
            self.inner.reattached(svc, ctx, player, session);
            return;
        }
        replay_for_reattach(&self.registry, svc, ctx, player, session);
        // ---- guild house (doc 22 §12.7, task guild-house): the rest gauge returns with the next second
        crate::event::guild_house::rest::resend(ctx, PlayerRef(player));
        crate::item::upgrade::handlers::resend_pending(ctx, PlayerRef(player), session);
        // the directory entry follows the new session
        social::on_player_joined(svc, ctx, PlayerRef(player));
        // 19 §20.5: the summoned system pet is announced to the new session
        crate::item::pets::system::after_join(svc, ctx, PlayerRef(player));
        // ---- housing (doc 22 §13, task housing): a player inside a room needs the papering set again
        crate::event::housing::after_reattach(svc, ctx, PlayerRef(player));
    }

    fn leave_mode(
        &mut self,
        ctx: &GameCtx<'_>,
        player: ObjId,
        trigger: &LeaveTrigger,
    ) -> LeaveMode {
        leave_mode(ctx, player, trigger)
    }

    fn finish_leave(&mut self, svc: &mut Services, ctx: &mut GameCtx<'_>, player: ObjId) {
        let Some(entry) = ctx.sessions.get(player) else {
            return;
        };
        let (tainted, next, shutdown) = (
            entry.tainted,
            entry.leave_next.unwrap_or(AfterLeave::Close),
            entry.leaving_for_shutdown,
        );
        if ctx.worlds.player(player).is_none() {
            // an avatar of the M1 stub (no world): nothing to destroy or save
            svc.saves.untrack(player);
            finish_leave_stub(ctx, player);
            return;
        }
        let who = PlayerRef(player);
        let reason = if next == AfterLeave::CharSelect {
            SaveReason::ToCharSelect
        } else if shutdown {
            SaveReason::Shutdown
        } else {
            SaveReason::Logout
        };
        // 12 §12.5 step 1-2: destroy_player, then the final snapshot with its location (never for a tainted
        // session: its state is not trusted, and the hooks are not run for it either)
        let final_gen = if tainted {
            svc.saves.last_gen(player)
        } else {
            let loc = destroy_player(self.logout.as_mut(), svc, ctx, who);
            save_player(svc, ctx, player, reason, Some(loc))
                .unwrap_or_else(|| svc.saves.last_gen(player))
        };
        // `session.logout` with what the stay produced (13 §25.2); the entry and the mover still exist here
        let reason = ctx
            .sessions
            .get(player)
            .and_then(|e| e.leave_reason)
            .unwrap_or("disconnect");
        crate::audit::session_logout(ctx, who, reason, final_gen);
        // the directory goes offline while the entry still exists (21 §3.4 leave step 6)
        social::on_player_leaving(svc, ctx, who);
        // step 3: the despawn, the entry and the slot
        remove_player(self.logout.as_mut(), svc, ctx, who);
        svc.saves.untrack(player);
        let Some(entry) = ctx.sessions.remove(player) else {
            return;
        };
        if let Some(sid) = ctx.outbox.slot_of(player) {
            let _ = ctx.outbox.remove(sid);
        }
        ctx.emit(NetEvent::Left {
            session: entry.last_session.unwrap_or(SessionId(0)),
            account: entry.account,
            character: entry.character,
            final_gen,
            next,
        });
    }

    fn create_character(
        &mut self,
        svc: &mut Services,
        ctx: &mut GameCtx<'_>,
        account: AccountId,
        req: Box<CreateCharacterRequest>,
        reply: Reply<Result<CharacterId, CharOpError>>,
    ) {
        self.inner.create_character(svc, ctx, account, req, reply);
    }

    fn delete_character(
        &mut self,
        svc: &mut Services,
        ctx: &mut GameCtx<'_>,
        account: AccountId,
        character: CharacterId,
        reply: Reply<Result<(), CharOpError>>,
    ) {
        self.inner
            .delete_character(svc, ctx, account, character, reply);
    }
}
