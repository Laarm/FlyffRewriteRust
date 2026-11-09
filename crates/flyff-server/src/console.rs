//! The stdin console (`flyff-console` thread, Plan/10 §13.1). It reads lines and prints the result of each to
//! stdout; EOF ends only this thread, never the process (a server under systemd has a closed stdin).
//!
//! The grammar and the execution are `admin::lines` (the same ones `POST /admin/api/console` uses): the runtime
//! words, doc 23's console words and, for everything else, a GM line with `AdminActor::Console`. The thread only
//! does the I/O; every command runs on the tokio runtime through [`AdminOps`].

// CLI output is this module's job.
#![allow(clippy::print_stdout)]

use crate::admin::AdminOps;
use crate::admin::lines;
use crate::cli::account::SysTerminal;
use crate::config::AdminConfig;
use flyff_game::input::AdminActor;
use std::io::{BufRead, IsTerminal};
use std::sync::Arc;

/// Decides whether the console thread starts (`admin.console`: `"auto"` = stdin is a TTY).
pub fn enabled(cfg: &AdminConfig) -> bool {
    match cfg.console.as_str() {
        "stdin" => true,
        "off" => false,
        _ => std::io::stdin().is_terminal(),
    }
}

/// Everything the console needs.
pub struct ConsoleCtx {
    /// The admin surfaces.
    pub ops: Arc<AdminOps>,
    /// The runtime the commands run on.
    pub rt: tokio::runtime::Handle,
}

/// Starts the `flyff-console` thread.
pub fn spawn(ctx: ConsoleCtx) -> std::io::Result<std::thread::JoinHandle<()>> {
    std::thread::Builder::new()
        .name("flyff-console".to_owned())
        .spawn(move || {
            let stdin = std::io::stdin();
            let mut line = String::new();
            loop {
                line.clear();
                // the lock is released after every line: a password prompt of an `account` word reads stdin itself
                match stdin.lock().read_line(&mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => execute(&ctx, line.trim_end_matches(['\n', '\r'])),
                }
            }
            // EOF (e.g. under systemd): the thread ends, the server keeps running.
        })
}

fn execute(ctx: &ConsoleCtx, line: &str) {
    let reply = ctx.rt.block_on(lines::run_line(
        &ctx.ops,
        line,
        None,
        AdminActor::Console,
        &mut SysTerminal,
    ));
    for l in &reply.lines {
        println!("{l}");
    }
}
