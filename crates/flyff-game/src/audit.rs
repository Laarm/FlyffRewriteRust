//! Game-side audit emitters (Plan/13 §25, WP-DB-09): small helpers that fill the actor context of an event from
//! the avatar, so a system emits `audit::emit_for(ctx, who, ev::TRADE_COMPLETE, json!({..}))` in its commit block
//! and the line carries `acc`, `chr`, `name`, `world`, `layer` and `pos` like the plan's example.
//!
//! The writer is `flyff_db::AuditLog` (`GameCtx.audit`): emitting never blocks the tick and never reads the wall
//! clock in game logic (the timestamp is taken by the audit event itself, for the output only).

use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::panic::{PanicContext, PanicInfo};
use crate::persist::snapshot::play_time_s;
use crate::session::SessionEntry;
use flyff_common::SaveGen;
use flyff_db::audit::ev;
use flyff_db::{AuditActor, AuditEvent, AuditLog};
use serde_json::{Value, json};

/// The actor context of a live player: account, character, name, world, layer, position.
pub fn actor_of(ctx: &GameCtx<'_>, who: PlayerRef) -> Option<AuditActor> {
    let m = ctx.player(who)?;
    let p = m.player()?;
    Some(AuditActor {
        account_id: ctx.entry(who).map(|e| e.account),
        character_id: Some(p.ident.character_id),
        name: Some(p.ident.name.clone()),
        ip: None,
        world: Some(m.core.world.0),
        layer: Some(m.core.layer.0),
        pos: Some([m.core.pos.x, m.core.pos.y, m.core.pos.z]),
    })
}

/// Emits `ev` with `data` for the avatar `who` (the actor context is filled from the avatar; an unknown avatar
/// gives a line without context).
pub fn emit_for(ctx: &GameCtx<'_>, who: PlayerRef, ev: &'static str, data: Value) {
    let mut event = AuditEvent::new(ev, data);
    if let Some(actor) = actor_of(ctx, who) {
        event = event.by(actor);
    }
    ctx.audit.emit(event);
}

/// `session.logout` (Plan/13 §25.2, 12 §19.1): the character left the world. `reason` is the leave trigger
/// (`disconnect`, `char_select`, `takeover`, `kick`, `ban`, `shutdown`, `panic`); the data carries what the
/// session produced: the played seconds, the gold, level and job at the end, and the generation of the final
/// save.
pub fn session_logout(ctx: &GameCtx<'_>, who: PlayerRef, reason: &str, final_gen: SaveGen) {
    let Some(entry) = ctx.entry(who) else {
        return;
    };
    let (gold, level, job) = ctx.player(who).map_or((0, 0, 0), |m| {
        (m.player().map_or(0, |p| p.items.gold), m.mv.level, m.mv.job)
    });
    emit_for(
        ctx,
        who,
        ev::SESSION_LOGOUT,
        json!({
            "character": entry.character.0,
            "play_s": play_time_s(entry, ctx.clock.unix_ms),
            "gold": gold,
            "level": level,
            "job": job,
            "reason": reason,
            "final_gen": final_gen.0,
        }),
    );
}

/// P14 `server.ccu` (Plan/10 §11, 13 §25.2): every 9 000 ticks (10 minutes) the number of connected sessions
/// (`online`, every open WebSocket session), the avatars in the world with a socket (`in_world`) and the avatars
/// that linger without one (`lingering`).
pub fn ccu(_svc: &mut Services, ctx: &mut GameCtx<'_>) {
    let total = ctx.sessions.len();
    let lingering = ctx.sessions.unbound_len();
    ctx.audit.emit(AuditEvent::new(
        ev::SERVER_CCU,
        json!({
            "online": ctx.telemetry.sessions.load(std::sync::atomic::Ordering::Relaxed),
            "in_world": total.saturating_sub(lingering),
            "lingering": lingering,
        }),
    ));
}

/// `server.panic` (Plan/10 §11, §14.3, §14.4): a caught panic. `scope` is `player`, `multiparty` or `unscoped`;
/// `ctx` is the [`PanicContext`] read BEFORE anything else ran; `actor` is the account and character of the session
/// that was being handled, when there was one. The data carries the scope, the session and character ids, `msg`
/// (the proto name of the client message, the kind of the input or the name of the job or phase), the panic
/// location, the tick, the phase and the panic text cut to 300 characters.
pub fn server_panic(
    audit: &AuditLog,
    actor: Option<AuditActor>,
    scope: &str,
    ctx: &PanicContext,
    info: &PanicInfo,
) {
    let mut event = AuditEvent::new(
        ev::SERVER_PANIC,
        json!({
            "scope": scope,
            "session_id": ctx.session.map(|s| s.0),
            "char_id": ctx.character.map(|c| c.0),
            "msg": ctx.what,
            "location": info.location,
            "tick": ctx.tick,
            "phase": ctx.phase,
            "panic": info.message.chars().take(300).collect::<String>(),
        }),
    );
    if let Some(actor) = actor {
        event = event.by(actor);
    }
    audit.emit(event);
}

/// `session.logout` of an avatar that never entered a world (the M1 stub rules, a test server without data): no
/// level, job or gold exist to report.
pub fn session_logout_stub(ctx: &GameCtx<'_>, entry: &SessionEntry) {
    ctx.audit.emit(
        AuditEvent::new(
            ev::SESSION_LOGOUT,
            json!({
                "character": entry.character.0,
                "play_s": play_time_s(entry, ctx.clock.unix_ms),
                "reason": entry.leave_reason.unwrap_or("disconnect"),
                "final_gen": 0,
            }),
        )
        .by(AuditActor {
            account_id: Some(entry.account),
            character_id: Some(entry.character),
            ..AuditActor::default()
        }),
    );
}
