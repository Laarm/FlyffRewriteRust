//! The `ScriptDlg` request handler (Plan/20 §7.2): throttle, range, session validation, the dialog run, the
//! dialog-condition hook, the one `DialogOps` response and the error policy (§3.8).
//!
//! Handler scope `Player`. The steps keep the order of the original (src:WORLDSERVER/DPSrvr.cpp:921-991):
//!
//! 1. **throttle (VR-061)**: a request within `gameplay.npc_script_throttle_ms` (400) of the last one is dropped
//!    silently; otherwise the stamp is set — before any other check, so a request that is rejected later still
//!    counts,
//! 2. the key is at most 255 bytes,
//! 3. the NPC is a live NPC mover in the player's world **and layer**,
//! 4. **range**: `‖Δ‖² > range²` (3-D, 32 m squared = 1024; exactly 1024 is accepted) drops the request and the
//!    session of that NPC,
//! 5. the NPC has a script (`NpcDef.dialog_key` and a loaded file; 88 NPCs have a dialog key but no script, KQ-26),
//! 6. an empty key is `#init`,
//! 7. **session validation (VR-060)**: `#init` opens a new session, any other key must be one the last responses
//!    offered, compared as `(key, param, quest_id)`,
//! 8. the transient offers are cleared and the sequence number grows,
//! 9. `run_dialog`,
//! 10. the dialog-condition hook of the quests,
//! 11. one `DialogOps` message if there are operations; they are applied to the session; `exit` ends it,
//! 12. a Lua fault discards the operations, sends `DialogOps{[exit]}` and ends the session.

use super::host::DialogHost;
use super::session::DialogSession;
use crate::chat::send_to;
use crate::ctx::{GameCtx, PlayerRef, Services};
use flyff_common::ObjId;
use flyff_proto::pb;
use flyff_proto::pb::dialog_op::Op;

/// The longest key a client may send (`SCRIPTDLG` reads at most 256 bytes).
pub const MAX_KEY_LEN: usize = 255;

/// The counters behind `flyff_dialog_requests_total{result}` (Plan/20 §3.10).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DialogStats {
    /// Requests that ran.
    pub ok: u64,
    /// Dropped by the 400 ms throttle.
    pub throttled: u64,
    /// Dropped by the range (or world / layer) check.
    pub range: u64,
    /// The NPC has no script (or the dialogs are disabled).
    pub noscript: u64,
    /// Dropped by the session (not offered), by the key length, or the NPC is no NPC.
    pub rejected: u64,
    /// Requests that ended in a Lua fault (the window was closed).
    pub faulted: u64,
}

/// `ScriptDlg{npc_obj_id, key, param, quest_id}` (Plan/20 §7.2).
pub fn on_script_dlg(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, m: pb::ScriptDlg) {
    let now = ctx.clock.game_ms;
    // 1: throttle (VR-061), stamp before any other check
    let throttle = u64::from(ctx.cfg.rules.gameplay.npc_script_throttle_ms);
    let (p_world, p_layer, p_pos) = {
        let Some(mover) = ctx.worlds.player_mut(who.0) else {
            return;
        };
        let (world, layer, pos) = (mover.core.world, mover.core.layer, mover.core.pos);
        let Some(p) = mover.player_mut() else { return };
        if p.npc
            .script_req_at
            .is_some_and(|last| now.0 < last.0.saturating_add(throttle))
        {
            svc.npc.stats.throttled += 1;
            return;
        }
        p.npc.script_req_at = Some(now);
        (world, layer, pos)
    };
    // 2: key length
    if m.key.len() > MAX_KEY_LEN {
        svc.npc.stats.rejected += 1;
        return;
    }
    // 3: the NPC: a live NPC mover in the player's world and layer
    let npc = ObjId(m.npc_obj_id);
    let Some(nm) = ctx.worlds.mover(npc) else {
        svc.npc.stats.rejected += 1;
        return;
    };
    let Some(npc_ext) = nm.npc() else {
        svc.npc.stats.rejected += 1;
        return;
    };
    if nm.core.deleted || nm.core.world != p_world || nm.core.layer != p_layer {
        svc.npc.stats.range += 1;
        return;
    }
    let character_key_exact = npc_ext.character_key.clone();
    let character_key = character_key_exact.to_ascii_lowercase();
    let npc_pos = nm.core.pos;
    // 4: range
    let range = ctx.cfg.rules.gameplay.npc_menu_range;
    let d = p_pos - npc_pos;
    if d.x * d.x + d.y * d.y + d.z * d.z > range * range {
        svc.npc.stats.range += 1;
        drop_session_of(ctx, who, npc);
        return;
    }
    // 5: the script
    let Some(rt) = ctx.lua.npc_runtime() else {
        svc.npc.stats.noscript += 1;
        return;
    };
    let script = (!rt.dialog_keys().is_disabled())
        .then(|| ctx.data.npc(&character_key))
        .flatten()
        .and_then(|def| def.dialog_key.as_deref())
        .and_then(|k| rt.script(k));
    let Some(script) = script else {
        svc.npc.stats.noscript += 1;
        return;
    };
    // 6-8: the key, the session
    let key: &str = if m.key.is_empty() { "#init" } else { &m.key };
    let seq = {
        let Some(p) = ctx
            .worlds
            .player_mut(who.0)
            .and_then(|mover| mover.player_mut())
        else {
            return;
        };
        if key == "#init" {
            p.npc.dialog = Some(DialogSession::new(npc, &character_key));
        } else {
            let accepted =
                p.npc.dialog.as_ref().is_some_and(|s| {
                    s.npc == npc && s.accepts(key, m.param as u32, m.quest_id as u32)
                });
            if !accepted {
                tracing::debug!(
                    player = who.0.0,
                    npc = npc.0,
                    key,
                    "ScriptDlg: the key was not offered by the open dialog: rejected (VR-060)"
                );
                svc.npc.stats.rejected += 1;
                return;
            }
        }
        let Some(s) = p.npc.dialog.as_mut() else {
            return;
        };
        s.transient.clear();
        s.seq = s.seq.wrapping_add(1);
        s.seq
    };
    svc.npc.stats.ok += 1;
    // 9: run
    let Some(mut host) = DialogHost::new(svc, ctx, rt, Some(who), npc, script) else {
        return;
    };
    let result = host.run_dialog(key, i64::from(m.param), i64::from(m.quest_id));
    let ops = host.take_outcome().ops;
    // 10: the dialog-condition hook (runs even when the script has no function for the key)
    let preds = std::sync::Arc::clone(&svc.npc.predicates);
    let matched = ctx
        .worlds
        .player_mut(who.0)
        .and_then(|mover| mover.player_mut())
        .and_then(|p| {
            // the NPC key as stored: `strcmp` against `SetEndCondDialog` (FIX: the lower-cased key never matched a
            // mixed-case key such as `MaFl_Dior`)
            crate::quest::dialog_condition_hook(
                preds.as_ref(),
                &mut p.quests,
                &character_key_exact,
                key,
            )
            .and_then(|q| p.quests.find(u32::from(q)).copied())
        });
    if let Some(record) = matched {
        send_to(
            ctx,
            who,
            pb::SetQuest {
                quest: Some(super::quest_ops::quest_msg(&record)),
            },
        );
    }
    // 11-12: the response
    match result {
        Ok(()) => {
            if ops.is_empty() {
                return;
            }
            let exit = ctx
                .worlds
                .player_mut(who.0)
                .and_then(|mover| mover.player_mut())
                .is_some_and(|p| {
                    let exit = p.npc.dialog.as_mut().is_some_and(|s| s.apply_all(&ops));
                    if exit {
                        p.npc.dialog = None;
                    }
                    exit
                });
            let _ = exit;
            send_to(
                ctx,
                who,
                pb::DialogOps {
                    npc_obj_id: npc.0,
                    ops,
                    session_seq: seq,
                },
            );
        }
        Err(e) => {
            svc.npc.stats.faulted += 1;
            tracing::error!(
                player = who.0.0,
                npc = npc.0,
                key,
                error = %e,
                "ScriptDlg: the NPC function failed; the dialog is closed (PL-106)"
            );
            if let Some(p) = ctx
                .worlds
                .player_mut(who.0)
                .and_then(|mover| mover.player_mut())
            {
                p.npc.dialog = None;
            }
            send_to(
                ctx,
                who,
                pb::DialogOps {
                    npc_obj_id: npc.0,
                    ops: vec![pb::DialogOp {
                        op: Some(Op::Exit(pb::DialogEmpty {})),
                    }],
                    session_seq: seq,
                },
            );
        }
    }
}

/// Ends the player's dialog when it belongs to `npc`.
fn drop_session_of(ctx: &mut GameCtx<'_>, who: PlayerRef, npc: ObjId) {
    if let Some(p) = ctx
        .worlds
        .player_mut(who.0)
        .and_then(|mover| mover.player_mut())
        && p.npc.dialog.as_ref().is_some_and(|s| s.npc == npc)
    {
        p.npc.dialog = None;
    }
}

/// Session drop hook for world or layer changes, death and NPC removal (Plan/20 §7.4): the dialog of the player
/// ends. A stale session is harmless in any case (a request must name the NPC of the session and pass the range
/// and layer checks), so the hook is a cleanup, not a guard.
pub fn end_dialog(ctx: &mut GameCtx<'_>, who: PlayerRef) {
    if let Some(p) = ctx
        .worlds
        .player_mut(who.0)
        .and_then(|mover| mover.player_mut())
    {
        p.npc.dialog = None;
    }
}
