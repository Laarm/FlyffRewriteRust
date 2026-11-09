//! The forwarder of the game-to-server requests of the GM commands (Plan/23 §4.6, §4.9; Plan/20 §3.9; WP-GM-25).
//!
//! The game thread never touches the disk and cannot await, so `/loadscript` (and `/reload dialog`) and
//! `/reload config` put a [`ServerRequest`] on the one channel [`flyff_game::gm::ServerRequestSink`] and answer the
//! GM at once. [`spawn`] runs the task that handles the requests in order:
//!
//! * `ReloadNpcScripts`: reads `scripts/npc/` of the resolved `paths.npc_scripts_dir` on a blocking thread
//!   ([`ScriptSources::read`]) and sends the sources to the game as `AdminCmd::ReloadScripts{Npc, Some(..)}`, which
//!   swaps the whole dialog registry atomically (the old dialogs stay when a file does not load);
//! * `ReloadConfig`: awaits [`AdminOps::reload`] with [`ReloadBy::Gm`] directly (it takes its own lock and does its
//!   own blocking load; it is never wrapped in `spawn_blocking`). The runtime-class keys reach the game as one
//!   `ReloadLive` that is queued before the call returns, so the `NotifyCharacter` below arrives after the
//!   GM's `GameRate`.
//!
//! The outcome is one line: [`npc_lines`] for the dialogs, [`summary_line`] for the configuration. An in-world
//! issuer (`notify`) gets it as a `[System]` chat line (`NotifyCharacter`, [`notify_text`] makes the text fit and
//! carry no control characters); the console and the admin API read it in the log (`tracing::info!` with `by`), the
//! configuration reload also in the `server.config_reload` audit record.

use crate::admin::ops::{AdminOps, OpsError};
use crate::admin::reload::{ReloadBy, summary_line};
use flyff_common::CharacterId;
use flyff_game::admin::actions::moderation::{MAX_NOTIFY_BYTES, NotifyCharacter, NotifyStyle};
use flyff_game::gm::ServerRequest;
use flyff_game::input::{AdminActor, AdminCmd, AdminOutput};
use flyff_lua::{ReloadTarget, ScriptSources};
use std::sync::Arc;

/// The text of a successful dialog reload.
pub const DIALOGS_RELOADED: &str = "dialog scripts reloaded";

/// The lines that tell the GM how the dialog reload ended. `result` is the answer of the game, or the text of what
/// stopped it before (the read error of `scripts/npc/`, the [`OpsError`] of a game that did not answer). The old
/// dialogs stay in force whenever the line says "not reloaded".
pub fn npc_lines(result: &Result<AdminOutput, String>) -> Vec<String> {
    let line = match result {
        Ok(AdminOutput::Lines(_)) => DIALOGS_RELOADED.to_owned(),
        Ok(AdminOutput::Error(e)) | Err(e) => format!("dialog scripts not reloaded: {e}"),
        Ok(other) => format!("dialog scripts not reloaded: unexpected answer {other:?}"),
    };
    vec![line]
}

/// `text` fit for `NotifyCharacter`: every control character (the line breaks of a Lua or TOML error) becomes a space
/// and the text is cut to [`MAX_NOTIFY_BYTES`] on a character boundary, because `notify_character` refuses a longer
/// text and any control character.
pub fn notify_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len().min(MAX_NOTIFY_BYTES));
    for c in text.chars() {
        let c = if c.is_control() { ' ' } else { c };
        if out.len() + c.len_utf8() > MAX_NOTIFY_BYTES {
            break;
        }
        out.push(c);
    }
    out
}

/// The sources of `scripts/npc/` (read on a blocking thread) sent to the game; every failure is text.
async fn reload_dialogs(ops: &AdminOps) -> Result<AdminOutput, String> {
    // the same resolved path `runtime.rs` reads at the start (an absolute `FLYFF__PATHS__NPC_SCRIPTS_DIR` included);
    // the lock is not held across an await
    let dir = {
        let current = ops.current.lock();
        current.resolve_paths(&ops.reload_source.home).npc_scripts
    };
    let shown = dir.display().to_string();
    let sources = match tokio::task::spawn_blocking(move || ScriptSources::read(&dir)).await {
        Ok(Ok(sources)) => sources,
        Ok(Err(e)) => return Err(format!("cannot read {shown}: {e}")),
        Err(e) => return Err(format!("reading {shown} failed: {e}")),
    };
    ops.game(
        AdminCmd::ReloadScripts {
            target: ReloadTarget::Npc,
            npc_sources: Some(sources),
        },
        AdminActor::Console,
    )
    .await
    .map_err(|e: OpsError| e.to_string())
}

/// Tells the in-world issuer the lines as `[System]` chat lines, one `NotifyCharacter` each. An issuer who left
/// since gets nothing (the per-target failure is not an error here).
async fn deliver(ops: &AdminOps, notify: Option<CharacterId>, lines: &[String]) {
    let Some(character) = notify else {
        return;
    };
    for line in lines {
        let cmd = AdminCmd::NotifyCharacter(NotifyCharacter {
            characters: vec![character],
            text: notify_text(line),
            style: NotifyStyle::System,
        });
        if let Err(e) = ops.game(cmd, AdminActor::Console).await {
            tracing::warn!(
                character = character.0,
                "the reload result was not delivered: {e}"
            );
            return;
        }
    }
}

async fn handle(ops: &AdminOps, request: ServerRequest) {
    match request {
        ServerRequest::ReloadNpcScripts { notify, by } => {
            let lines = npc_lines(&reload_dialogs(ops).await);
            for line in &lines {
                tracing::info!(by = %by, "{line}");
            }
            deliver(ops, notify, &lines).await;
        }
        ServerRequest::ReloadConfig { notify, by } => {
            let report = ops.reload(ReloadBy::Gm(by.clone())).await;
            let line = summary_line(&report);
            tracing::info!(by = %by, "GM config reload: {line}");
            deliver(ops, notify, &[line]).await;
        }
    }
}

/// Starts the task that handles the requests of the GM commands in order. It ends when every sender is gone (the
/// game dropped its sink).
pub fn spawn(rt: &tokio::runtime::Handle, ops: Arc<AdminOps>, rx: flume::Receiver<ServerRequest>) {
    rt.spawn(async move {
        while let Ok(request) = rx.recv_async().await {
            handle(&ops, request).await;
        }
    });
}
