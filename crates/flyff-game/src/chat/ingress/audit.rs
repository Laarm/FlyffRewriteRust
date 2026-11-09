//! Audit records of the chat and GM paths (Plan/23 §3.9, §6.12; 13 §25): `gm.chat` for every line typed by a
//! player with authority `>= G` and for their whispers and instant messages, `gm.command` for every dispatch
//! (see `crate::gm::audit`).

use super::super::Actor;
use crate::ctx::{GameCtx, PlayerRef, Services};
use flyff_common::CharacterId;
use flyff_common::gm::Authority;
use serde_json::{Value, json};

/// A recorder of the emitted audit events, switched on by tests (`Services.gm.tap`); the production writer is
/// `flyff_db::AuditLog` (WP-DB-09), which the helpers call as well (`emit_record`: the flat record becomes the
/// actor context and the `data` of the line).
#[derive(Debug, Default)]
pub struct AuditTap {
    /// Record the events below.
    pub enabled: bool,
    /// `(event, detail)` in emission order.
    pub events: Vec<(String, Value)>,
}

/// Emits one audit event (and records it when the tap is enabled).
pub fn emit(svc: &mut Services, ctx: &GameCtx<'_>, event: &'static str, detail: Value) {
    ctx.audit.emit_record(event, &detail);
    if svc.gm.tap.enabled {
        svc.gm.tap.events.push((event.to_owned(), detail));
    }
}

/// The common head of the records of one actor (`acc`, `chr`, `name`, `world`, `layer`, `pos`).
pub fn head(ctx: &GameCtx<'_>, a: &Actor) -> serde_json::Map<String, Value> {
    let acc = ctx.entry(a.who).map_or(0, |e| e.account.0);
    let mut m = serde_json::Map::new();
    m.insert("acc".to_owned(), json!(acc));
    m.insert("chr".to_owned(), json!(a.character.0));
    m.insert("name".to_owned(), json!(a.name));
    m.insert("world".to_owned(), json!(a.world.0));
    m.insert("layer".to_owned(), json!(a.layer.0));
    m.insert("pos".to_owned(), json!([a.pos.x, a.pos.y, a.pos.z]));
    m
}

/// `gm.chat {channel, text}` of `a` (23 §6.12).
pub fn gm_chat(svc: &mut Services, ctx: &GameCtx<'_>, a: &Actor, channel: &str, text: &str) {
    let mut rec = head(ctx, a);
    rec.insert(
        "data".to_owned(),
        json!({ "channel": channel, "text": text }),
    );
    emit(svc, ctx, "gm.chat", Value::Object(rec));
}

/// `audit_private` (23 §6.5, §6.12): a whisper or instant message. One record per side whose authority is
/// `>= G`: the sender (`whisper_out` / `say_out`) and, when online, the target (`whisper_in` / `say_in`).
pub fn audit_private(
    svc: &mut Services,
    ctx: &GameCtx<'_>,
    from: PlayerRef,
    to: CharacterId,
    kind: PrivateKind,
    text: &str,
) {
    let (out, inn) = match kind {
        PrivateKind::Whisper => ("whisper_out", "whisper_in"),
        PrivateKind::Im => ("say_out", "say_in"),
    };
    if let Some(a) = super::super::actor(ctx, from)
        && a.authority.at_least(Authority::LOG_CHATTING)
    {
        gm_chat(svc, ctx, &a, out, text);
    }
    let target = svc
        .presence
        .get(to)
        .and_then(|e| e.online)
        .and_then(|o| super::super::actor(ctx, PlayerRef(o.obj)));
    if let Some(t) = target
        && t.authority.at_least(Authority::LOG_CHATTING)
    {
        gm_chat(svc, ctx, &t, inn, text);
    }
}

/// The private channels of §6.5.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrivateKind {
    /// `/w`.
    Whisper,
    /// `/say` (messenger instant message).
    Im,
}
