//! `/help` (N01, 23 §4.9): the commands the issuer may use, or the usage of one. The lines are `ChatText` (kind
//! `TEXT_CHAT`, BFIX1 #006): a list of 26 to 127 lines belongs in the chat box, the centre notices keep only 6. The browser
//! answers `/help` itself with a window; this handler stays for the admin console, the admin API and other clients.

use crate::gm::{CmdArgs, CmdCtx, CmdDone, CmdError, CmdResult, UNKNOWN_COMMAND};
use std::sync::Arc;

/// `/help [command]`: without an argument lists the commands visible to the issuer (`status` port or client,
/// `min_auth <=` authority) sorted by minimum letter then name, as `"/name (aliases) usage"` lines; with an
/// argument prints usage and help line of that command (unknown or invisible: the 23 §3.6 reply).
pub fn help(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let reg = Arc::clone(&cx.svc.gm.registry);
    let arg = args.token().to_owned();
    if arg.is_empty() {
        let list = reg.visible_to(cx.authority);
        cx.out
            .chat_text(format!("{} commands available:", list.len()));
        for s in list {
            let mut line = format!("/{}", s.name);
            if !s.aliases.is_empty() {
                line.push_str(&format!(" ({})", s.aliases.join(", ")));
            }
            if !s.usage.is_empty() {
                line.push(' ');
                line.push_str(s.usage);
            }
            cx.out.chat_text(line);
        }
        return Ok(CmdDone::default());
    }
    let visible = reg
        .lookup(arg.trim_start_matches('/'))
        .filter(|s| cx.authority.at_least(s.min_auth))
        .filter(|s| {
            matches!(
                s.status,
                crate::gm::CmdStatus::Port | crate::gm::CmdStatus::Client
            )
        });
    match visible {
        Some(s) => {
            cx.out
                .chat_text(format!("/{} {}", s.name, s.usage).trim_end().to_owned());
            cx.out.chat_text(s.help);
            Ok(CmdDone::on(s.name))
        }
        None => Err(CmdError::Refused(UNKNOWN_COMMAND.to_owned())),
    }
}
