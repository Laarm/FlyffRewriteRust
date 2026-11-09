//! The `gm.command` audit record (Plan/23 §3.9, 13 §25): every dispatch with an item, console or admin issuer,
//! and every player dispatch where the command was found (any result, `denied` included) or where the player's
//! authority is `>= L`. Unknown commands typed by players below `L` are not audited (noise).

use super::{CmdCtx, CommandSpec, Issuer};
use crate::chat::actor;
use crate::chat::ingress::audit as chat_audit;
use flyff_common::gm::Authority;
use serde_json::{Value, json};

/// Lines are truncated to 1 KiB in the record.
const MAX_LINE_BYTES: usize = 1024;

fn truncated(line: &str) -> &str {
    if line.len() <= MAX_LINE_BYTES {
        return line;
    }
    let mut end = MAX_LINE_BYTES;
    while end > 0 && !line.is_char_boundary(end) {
        end -= 1;
    }
    line.get(..end).unwrap_or("")
}

fn record(
    cx: &mut CmdCtx<'_, '_>,
    cmd: Option<&CommandSpec>,
    line: &str,
    result: &str,
    target: Option<&str>,
) {
    let a = cx.self_player.and_then(|p| actor(cx.ctx, p));
    let mut rec = match &a {
        Some(a) => chat_audit::head(cx.ctx, a),
        None => serde_json::Map::new(),
    };
    let item = match cx.issuer {
        Issuer::Item { item, .. } => Some(item),
        _ => None,
    };
    let acting = match cx.issuer {
        Issuer::Console | Issuer::Admin { .. } => a.as_ref().map(|a| a.name.clone()),
        _ => None,
    };
    rec.insert(
        "data".to_owned(),
        json!({
            "issuer": cx.issuer.tag(),
            "line": truncated(line),
            "cmd": cmd.map(|c| c.name.to_ascii_lowercase()),
            "auth": cx.authority.letter().to_string(),
            "as": acting,
            "item": item,
            "target": target,
            "result": result,
        }),
    );
    chat_audit::emit(cx.svc, cx.ctx, "gm.command", Value::Object(rec));
}

/// Audits the dispatch of a found command.
pub fn command(
    cx: &mut CmdCtx<'_, '_>,
    spec: &CommandSpec,
    line: &str,
    result: &str,
    target: Option<&str>,
) {
    record(cx, Some(spec), line, result, target);
}

/// Audits a command token that is not in the registry (only for GM-level and non-player issuers).
pub fn unknown(cx: &mut CmdCtx<'_, '_>, line: &str) {
    let noisy =
        matches!(cx.issuer, Issuer::Player { .. }) && !cx.authority.at_least(Authority::GAMEMASTER);
    if !noisy {
        record(cx, None, line, "unknown", None);
    }
}

/// Audits a GM effect that has its own event name (`account.authority`, `account.ban`, `account.unban`,
/// `item.create`, ...): the head of the issuing character (when there is one) and `data`.
pub fn event(cx: &mut CmdCtx<'_, '_>, ev: &'static str, data: Value) {
    let a = cx.self_player.and_then(|p| actor(cx.ctx, p));
    let mut rec = match &a {
        Some(a) => chat_audit::head(cx.ctx, a),
        None => serde_json::Map::new(),
    };
    rec.insert("data".to_owned(), data);
    chat_audit::emit(cx.svc, cx.ctx, ev, Value::Object(rec));
}

/// Who a persisted GM action is attributed to (`account_bans.created_by`, the `by` of the audit events):
/// `gm:<account id>` for a player or item issuer, `console`, `owner:local`, `owner:token` or `staff:<account>` (the
/// audit actor of 25 §1.1) for the operator surfaces.
pub fn issuer_label(cx: &CmdCtx<'_, '_>) -> String {
    match cx.issuer {
        Issuer::Player { player, .. } | Issuer::Item { player, .. } => {
            let acc = cx.ctx.entry(player).map_or(0, |e| e.account.0);
            format!("gm:{acc}")
        }
        Issuer::Console => "console".to_owned(),
        Issuer::Admin { actor } => actor.label(),
    }
}

/// The character the shutdown trigger and the audit name as the actor (`CharacterId(0)` without one).
pub fn issuer_character(cx: &CmdCtx<'_, '_>) -> flyff_common::CharacterId {
    match cx.issuer {
        Issuer::Player { player, .. } | Issuer::Item { player, .. } => cx
            .ctx
            .entry(player)
            .map_or(flyff_common::CharacterId(0), |e| e.character),
        _ => flyff_common::CharacterId(0),
    }
}
