//! The Lua and reload commands (Plan/23 §4.6 rows 87 and 95, §4.9 N14; WP-GM-25; PL-119): `/loadscript` (87),
//! `/Lua` (95) and the new `/reload` (N14). `/rec` (88) is `gm::rates::reload_constant`.
//!
//! * `/lua event` and `/lua rr` reload the Lua family synchronously through `crate::event::reload_scripts` (the body
//!   of `AdminCmd::ReloadScripts`), so the issuer of any surface (player, item, console, admin API) reads the
//!   outcome in the reply; a failed file keeps the previous version and says so. The original's database round
//!   trip (`SendEventLuaChanged`) is gone.
//! * `/loadscript` and `/reload config` need the server (the disk read of `scripts/npc/`, the async file
//!   reload of WP-OPS-16): they put a [`ServerRequest`] on `Services.gm.server_requests` and answer at once; the
//!   server's forwarder (`flyff-server` `admin/gm_reload.rs`) sends the result back as a `[System]` line to an
//!   in-world issuer.
//! * `/lua ms`, `/lua dps` and every other `/reload` word say that a restart is needed (PL-119: `GameData` is
//!   immutable); any other `/lua` word, or none, is silent (the original had no branch).

use crate::event::notice::cut_bytes;
use crate::event::reload_scripts;
use crate::gm::audit::issuer_label;
use crate::gm::requests::ServerRequest;
use crate::gm::{CmdArgs, CmdCtx, CmdDone, CmdError, CmdResult};
use crate::input::AdminOutput;
use flyff_common::CharacterId;
use flyff_lua::{ReloadTarget, ScriptFamily};

/// The reply for what only a restart can change.
pub const REQUIRES_RESTART: &str = "requires a restart";

/// The reply when the server cannot be asked (a disconnected sink, as in tests, or a full one).
pub const NOT_AVAILABLE: &str = "reload is not available here";

/// The legacy `AddText` of `/lua event`.
pub const EVENT_RELOAD_TEXT: &str = "Event.lua Reload...";

/// The immediate reply of `/loadscript` and `/reload dialog`; the result follows as a `[System]` line.
pub const DIALOG_REQUESTED: &str = "dialog scripts reload requested";

/// The immediate reply of `/reload config`; the result follows as a `[System]` line.
pub const CONFIG_REQUESTED: &str = "reloading flyff.toml ...";

/// The longest text of a `Text` message is 260 bytes (`notice.proto`); a Lua error can be longer.
const MAX_REPLY_BYTES: usize = 255;

/// Reloads one Lua family and prints the outcome: one `Text` per line of the report, or the refusal with the text of
/// the error (cut to the `Text` limit; the full error is in the log of the Lua host).
fn reload_family(cx: &mut CmdCtx<'_, '_>, family: ScriptFamily) -> CmdResult {
    match reload_scripts(cx.svc, cx.ctx, ReloadTarget::Family(family), None) {
        AdminOutput::Lines(lines) => {
            for line in lines {
                cx.out.text(cut_bytes(&line, MAX_REPLY_BYTES));
            }
            Ok(CmdDone::default())
        }
        AdminOutput::Error(e) => Err(CmdError::Refused(cut_bytes(&e, MAX_REPLY_BYTES).to_owned())),
        other => Err(CmdError::Internal(format!("the reload answered {other:?}"))),
    }
}

/// `/lua event`: the legacy text, the log line with the GM, then the reload of `Event.lua` (the state pass runs
/// again, the aggregates are recomputed).
fn lua_event(cx: &mut CmdCtx<'_, '_>) -> CmdResult {
    cx.out.text(EVENT_RELOAD_TEXT);
    // the legacy `Error( "Event.lua Reload... - %s", pUser->GetName() )`: the GM's character name, else the label
    let by = issuer_label(cx);
    let gm = cx
        .issuer
        .player()
        .and_then(|p| cx.ctx.entry(p))
        .and_then(|e| cx.svc.presence.get(e.character))
        .map_or_else(|| by.clone(), |d| d.name.clone());
    tracing::warn!(gm = %gm, by = %by, "Event.lua Reload...");
    reload_family(cx, ScriptFamily::Event)
}

/// Row 95 `/Lua (event | rr | ms | dps)` (`TextCmd_Lua`; the argument is lower-cased): `event` reloads `Event.lua`
/// (replies `Event.lua Reload...` and one line per reloaded family, or the refusal), `rr` reloads the rainbow-race
/// script (the typed config and the schedule follow; whether the race is on is the flag of id 25), `ms` and `dps`
/// need a restart; anything else is silent (`monsterraid` and `autoraid` belong to `__MONSTER_RAID`, which V15 does not
/// compile).
pub fn lua(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let word = args.token().to_ascii_lowercase();
    match word.as_str() {
        "event" => lua_event(cx),
        "rr" => reload_family(cx, ScriptFamily::RainbowRace),
        "ms" | "dps" => restart_needed(cx),
        _ => Ok(CmdDone::default()),
    }
}

fn restart_needed(cx: &mut CmdCtx<'_, '_>) -> CmdResult {
    cx.out.text(REQUIRES_RESTART);
    Ok(CmdDone::default())
}

/// Puts `make(notify, by)` on the server channel. `notify` is the character of a player or item issuer (also for the
/// character of the `as <name>` prefix: it is the console's line, so `None`); `by` is the issuer label of the audit
/// (`gm:<account id>`, `console`, `owner:local`, ...), never the account of the `as` character.
fn ask_server(
    cx: &mut CmdCtx<'_, '_>,
    make: fn(Option<CharacterId>, String) -> ServerRequest,
) -> bool {
    let notify = cx
        .issuer
        .player()
        .and_then(|p| cx.ctx.entry(p))
        .map(|e| e.character);
    let by = issuer_label(cx);
    cx.svc.gm.server_requests.send(make(notify, by))
}

/// The common part of `/loadscript` and `/reload dialog`.
fn load_dialogs(cx: &mut CmdCtx<'_, '_>) -> CmdResult {
    if ask_server(cx, |notify, by| ServerRequest::ReloadNpcScripts {
        notify,
        by,
    }) {
        cx.out.text(DIALOG_REQUESTED);
        Ok(CmdDone::default())
    } else {
        Err(CmdError::Refused(NOT_AVAILABLE.to_owned()))
    }
}

/// Row 87 `/loadscript (loscr)` (`TextCmd_LoadScript`): the server reads the transpiled dialogs of
/// `paths.npc_scripts_dir` and the game swaps the whole registry atomically; the old dialogs stay when a file does not
/// load. The legacy reloaded `WorldScript.dll`. The answer is `dialog scripts reload requested` now and
/// `[System] dialog scripts reloaded` (or `not reloaded: <error>`) from the server to an in-world issuer.
pub fn load_script(cx: &mut CmdCtx<'_, '_>, _args: &mut CmdArgs<'_>) -> CmdResult {
    load_dialogs(cx)
}

/// N14 `/reload (config | dialog | lua)` (new): `lua` is `/lua event`, `dialog` is `/loadscript`, `config` is the
/// file reload of WP-OPS-16 (the runtime-class keys of `flyff.toml` reach the game as one `ReloadLive`; the result
/// follows as a `[System]` line); any other word says that a restart is needed. No word is the usage line.
pub fn reload(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let word = args.token().to_ascii_lowercase();
    match word.as_str() {
        "" => Err(CmdError::Usage),
        "lua" => lua_event(cx),
        "dialog" => load_dialogs(cx),
        "config" => {
            if ask_server(cx, |notify, by| ServerRequest::ReloadConfig { notify, by }) {
                cx.out.text(CONFIG_REQUESTED);
                Ok(CmdDone::default())
            } else {
                Err(CmdError::Refused(NOT_AVAILABLE.to_owned()))
            }
        }
        _ => restart_needed(cx),
    }
}
