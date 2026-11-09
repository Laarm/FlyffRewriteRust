//! The effects of an item operation: messages and audit lines (Plan/19 §4.2, §21).
//!
//! The core item functions mutate one `&mut Mover` and must not borrow the worlds while they send, so (like
//! `stats`) they append to an [`Fx`]; the caller flushes it afterwards through the `GameCtx`
//! ([`Fx::flush`]: handlers, timers) or through the world driver's sink ([`Fx::flush_sink`]: the pickup in the
//! tick). The order of the appends is the order of the legacy sends.

use crate::chat::ingress::notice::defined_text;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::follow::Followup;
use crate::mover::Mover;
use crate::stats::Out;
use crate::world::SpawnSink;
use flyff_common::ObjId;
use flyff_db::audit::ev;
use flyff_db::{AuditActor, AuditEvent, AuditLog};
use flyff_proto::pb;
use serde_json::{Value, json};

/// One audit line to emit with the player's actor context.
#[derive(Clone, Debug, PartialEq)]
pub struct AuditLine {
    /// The event name (`flyff_db::audit::ev`).
    pub ev: &'static str,
    /// The event data.
    pub data: Value,
}

/// Messages, audit lines and the urgent-save request an item function produced.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Fx {
    /// The messages in order.
    pub out: Out,
    /// The audit lines in order.
    pub audit: Vec<AuditLine>,
    /// A snapshot should be taken at the end of the tick (13 §17.1 `mark_urgent`).
    pub urgent: bool,
    /// What the mover-level code could not do itself (hurt another mover, teleport, create an item...):
    /// run by [`Fx::flush`] after the messages, in order.
    pub follow: Vec<Followup>,
}

impl Fx {
    /// No effects.
    pub fn new() -> Fx {
        Fx::default()
    }

    /// `AddDefinedText(tid, args...)` to the owner.
    pub fn defined(&mut self, tid: u32, args: Vec<String>) {
        self.out.to_self(defined_text(tid, args));
    }

    /// `AddDefinedText(tid)` to the owner.
    pub fn text(&mut self, tid: u32) {
        self.defined(tid, Vec::new());
    }

    /// `DefinedText` with chat display (`display: DISPLAY_CHAT`), the loot-text copy of §4.2.
    pub fn defined_chat(&mut self, tid: u32, args: Vec<String>) {
        let mut t = defined_text(tid, args);
        t.display = pb::defined_text::Display::Chat as i32;
        self.out.to_self(t);
    }

    /// Queues an audit line.
    pub fn audit(&mut self, ev: &'static str, data: Value) {
        self.audit.push(AuditLine { ev, data });
    }

    /// Asks for an urgent snapshot of the owner.
    pub fn mark_urgent(&mut self) {
        self.urgent = true;
    }

    /// Queues a follow-up action.
    pub fn then(&mut self, f: Followup) {
        self.follow.push(f);
    }

    /// `true` when nothing was produced.
    pub fn is_empty(&self) -> bool {
        self.out.is_empty() && self.audit.is_empty() && !self.urgent && self.follow.is_empty()
    }

    /// Sends the messages through the outbox, emits the audit lines with the avatar's context and marks the
    /// avatar for an urgent snapshot when asked.
    pub fn flush(self, svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
        self.flush_for(svc, ctx, who.0);
    }

    /// [`Fx::flush`] for any mover: a monster's messages reach the players around it; audit lines and the
    /// urgent snapshot only exist for a player.
    pub fn flush_for(self, svc: &mut Services, ctx: &mut GameCtx<'_>, subject: ObjId) {
        let Fx {
            out,
            audit,
            urgent,
            follow,
        } = self;
        out.flush(ctx, subject);
        if ctx.worlds.player(subject).is_some() {
            let who = PlayerRef(subject);
            for line in audit {
                crate::audit::emit_for(ctx, who, line.ev, line.data);
            }
            if urgent {
                svc.saves.mark_urgent(subject);
            }
        }
        for f in follow {
            f.run(svc, ctx);
        }
    }

    /// The world-phase variant: the messages go through the sink (`m` is the player, borrowed by the caller
    /// from the object store); audit lines are emitted into `audit_log`. The urgent request is dropped (the
    /// world phase cannot reach the save scheduler; the periodic snapshot covers it).
    pub fn flush_sink(self, sink: &mut dyn SpawnSink, m: &Mover, audit_log: &AuditLog) {
        let Fx { out, audit, .. } = self;
        out.flush_sink(sink, m);
        let actor = actor_of_mover(m);
        for line in audit {
            let mut e = AuditEvent::new(line.ev, line.data);
            if let Some(a) = &actor {
                e = e.by(a.clone());
            }
            audit_log.emit(e);
        }
    }
}

/// The audit actor of a player mover without the session record (the account comes from its identity).
pub fn actor_of_mover(m: &Mover) -> Option<AuditActor> {
    let p = m.player()?;
    Some(AuditActor {
        account_id: Some(p.ident.account_id),
        character_id: Some(p.ident.character_id),
        name: Some(p.ident.name.clone()),
        ip: None,
        world: Some(m.core.world.0),
        layer: Some(m.core.layer.0),
        pos: Some([m.core.pos.x, m.core.pos.y, m.core.pos.z]),
    })
}

/// The item part of an audit line: the record JSON without `obj_id` (`AuditEvent::item`).
pub fn item_json(it: &super::model::Item) -> Value {
    let mut v = serde_json::to_value(it.to_record(0)).unwrap_or(Value::Null);
    if let Some(o) = v.as_object_mut() {
        o.remove("obj_id");
    }
    v
}

/// `item.create{item, count, source}`.
pub fn audit_create(fx: &mut Fx, it: &super::model::Item, count: u32, source: &str) {
    fx.audit(
        ev::ITEM_CREATE,
        json!({ "item": item_json(it), "count": count, "source": source }),
    );
}
