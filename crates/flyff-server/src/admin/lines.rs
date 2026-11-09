//! The line matcher of the console grammar (Plan/10 §13.1, 23 §8.1): one parser and one executor behind the stdin
//! console (`flyff-console`), `POST /admin/api/console` and the `/admin` page's console box.
//!
//! A line is matched against the runtime words first (`help`, `status`, `sessions`, `save-all`, `set`, `unset`,
//! `reload`, `log`, `shutdown`, `backup`, `db release|checkpoint`, `digest`, `version`), then against the console
//! words of doc 23 (`online`, `kick`, `account ...`, `grant ...`, plus `rates`, `events` and `notice`, which are
//! this build's shorthands for the admin routes of the same names), and anything else is a GM line:
//! `[as <character>] [/]<command>`, which becomes `AdminCmd::RunLine` for doc 23's handler.

use super::json;
use super::ops::{AdminOps, OpsError, ShutdownReq};
use super::reload::ReloadBy;
use crate::cli::AccountCmd;
use crate::cli::account::{self, Env, Terminal};
use clap::Parser;
use flyff_common::AccountId;
use flyff_common::gm::Authority;
use flyff_db::{CheckpointMode, MaintenanceJob, queries};
use flyff_game::input::{AdminActor, AdminCmd, AdminOutput};

/// What a line produced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineReply {
    /// The lines to print.
    pub lines: Vec<String>,
    /// `false` when the command failed (the HTTP route reports `result: "error"`).
    pub ok: bool,
}

impl LineReply {
    fn ok(lines: Vec<String>) -> LineReply {
        LineReply { lines, ok: true }
    }

    fn one(line: impl Into<String>) -> LineReply {
        LineReply::ok(vec![line.into()])
    }

    fn err(line: impl Into<String>) -> LineReply {
        LineReply {
            lines: vec![format!("error: {}", line.into())],
            ok: false,
        }
    }
}

/// A parsed line.
#[derive(Clone, Debug, PartialEq)]
pub enum Word {
    /// Blank.
    Empty,
    /// `help`.
    Help,
    /// `status`.
    Status,
    /// `sessions [N]`.
    Sessions(Option<usize>),
    /// `save-all`.
    SaveAll,
    /// `set <key> <value>`.
    Set(String, serde_json::Value),
    /// `unset <key>`.
    Unset(String),
    /// `reload`.
    Reload,
    /// `log <filter>`.
    Log(String),
    /// `shutdown [seconds] [message...]`, `shutdown now`, `shutdown cancel`.
    Shutdown(ShutdownReq),
    /// `backup`.
    Backup,
    /// `db release <account>`.
    DbRelease(String),
    /// `db checkpoint [passive|restart|truncate]`.
    DbCheckpoint(CheckpointMode),
    /// `db maintenance now`: the daily retention job (Plan/13 §23).
    DbMaintenance,
    /// `digest`.
    Digest,
    /// `version`.
    Version,
    /// `online [N]`.
    Online(Option<u32>),
    /// `kick <name> [reason...]`.
    Kick(String),
    /// `account <args...>`.
    Account(Vec<String>),
    /// `grant <character> (item <itemId|"name"> [count] | gold <n>) [note...]`: the text after the word.
    Grant(String),
    /// `rates` / `rates <key> <value|default>`.
    Rates(Option<(String, Option<f64>)>),
    /// `events` / `events <id> <state>`.
    Events(Option<(u16, u8)>),
    /// `notice <text>`.
    Notice(String),
    /// A GM line, with the `as <character>` prefix when given.
    Gm {
        /// The line, with a leading `/`.
        line: String,
        /// `as <character>`.
        as_character: Option<String>,
    },
    /// A runtime or console word with unusable arguments.
    Usage(&'static str),
}

/// The text of `help`.
pub const HELP: &[&str] = &[
    "runtime: help | status | sessions [N] | reload | log <filter> | version | digest | save-all",
    "         set <key> <value> | unset <key>   (runtime keys: rates.*, events.*)",
    "         shutdown [seconds] [message...] | shutdown now | shutdown cancel",
    "         db release <account> | db checkpoint [passive|restart|truncate] | db maintenance now | backup",
    "console: online [N] | kick <account-or-character> [reason...] | notice <text>",
    "         rates [<key> <value>|<key> default] | events [<id> <state>]",
    "         account create|passwd|set-auth|list|ban ...   (as `flyff account`)",
    "other:   [as <character>] [/]<GM command>   (e.g. /help)",
];

/// The first word and the rest of a line.
fn split_first(line: &str) -> (&str, &str) {
    let t = line.trim_start();
    match t.find(char::is_whitespace) {
        Some(i) => (&t[..i], t[i..].trim_start()),
        None => (t, ""),
    }
}

/// Parses one console line.
pub fn parse(line: &str) -> Word {
    let line = line.trim();
    if line.is_empty() {
        return Word::Empty;
    }
    // a leading slash is a GM line, never a runtime word
    if line.starts_with('/') {
        return Word::Gm {
            line: line.to_owned(),
            as_character: None,
        };
    }
    let (first, rest) = split_first(line);
    let lower = first.to_ascii_lowercase();
    let args: Vec<&str> = rest.split_whitespace().collect();
    match lower.as_str() {
        "help" | "?" => Word::Help,
        "status" => Word::Status,
        "sessions" => match args.as_slice() {
            [] => Word::Sessions(None),
            [n] => n
                .parse()
                .map_or(Word::Usage("sessions [N]"), |n| Word::Sessions(Some(n))),
            _ => Word::Usage("sessions [N]"),
        },
        "save-all" | "saveall" => Word::SaveAll,
        "set" => match rest.split_once(char::is_whitespace) {
            Some((key, value)) if !value.trim().is_empty() => {
                let v = value.trim();
                Word::Set(
                    key.to_owned(),
                    serde_json::from_str(v)
                        .unwrap_or_else(|_| serde_json::Value::String(v.to_owned())),
                )
            }
            _ => Word::Usage("set <key> <value>"),
        },
        "unset" => match args.as_slice() {
            [key] => Word::Unset((*key).to_owned()),
            _ => Word::Usage("unset <key>"),
        },
        "reload" => Word::Reload,
        "log" => match args.first() {
            Some(f) => Word::Log((*f).to_owned()),
            None => Word::Usage("log <filter>"),
        },
        "shutdown" | "stop" => match args.first().map(|a| a.to_ascii_lowercase()).as_deref() {
            None => Word::Shutdown(ShutdownReq::default()),
            Some("now") => Word::Shutdown(ShutdownReq {
                countdown_s: Some(0),
                message: message_after(rest, 1),
                cancel: false,
            }),
            Some("cancel") => Word::Shutdown(ShutdownReq {
                cancel: true,
                ..ShutdownReq::default()
            }),
            Some(n) => n.parse::<u32>().map_or(
                Word::Usage("shutdown [seconds] [message...] | shutdown now | shutdown cancel"),
                |s| {
                    Word::Shutdown(ShutdownReq {
                        countdown_s: Some(s.min(3600)),
                        message: message_after(rest, 1),
                        cancel: false,
                    })
                },
            ),
        },
        "backup" => Word::Backup,
        "db" => match args.as_slice() {
            ["release", account] => Word::DbRelease((*account).to_owned()),
            ["maintenance", "now"] => Word::DbMaintenance,
            ["checkpoint"] => Word::DbCheckpoint(CheckpointMode::Passive),
            ["checkpoint", mode] => match mode.to_ascii_lowercase().as_str() {
                "passive" => Word::DbCheckpoint(CheckpointMode::Passive),
                "restart" => Word::DbCheckpoint(CheckpointMode::Restart),
                "truncate" => Word::DbCheckpoint(CheckpointMode::Truncate),
                _ => Word::Usage("db checkpoint [passive|restart|truncate]"),
            },
            _ => Word::Usage(
                "db release <account> | db checkpoint [passive|restart|truncate] | db maintenance now",
            ),
        },
        "digest" => Word::Digest,
        "version" => Word::Version,
        "online" => match args.as_slice() {
            [] => Word::Online(None),
            [n] => n
                .parse()
                .map_or(Word::Usage("online [N]"), |n| Word::Online(Some(n))),
            _ => Word::Usage("online [N]"),
        },
        "kick" => match args.first() {
            Some(name) => Word::Kick((*name).to_owned()),
            None => Word::Usage("kick <account-or-character> [reason...]"),
        },
        "account" => Word::Account(args.iter().map(|s| (*s).to_owned()).collect()),
        "grant" => Word::Grant(rest.to_owned()),
        "rates" => match args.as_slice() {
            [] => Word::Rates(None),
            [key, value] => {
                if value.eq_ignore_ascii_case("default") {
                    Word::Rates(Some(((*key).to_owned(), None)))
                } else {
                    value
                        .parse::<f64>()
                        .map_or(Word::Usage("rates [<key> <value>|<key> default]"), |v| {
                            Word::Rates(Some(((*key).to_owned(), Some(v))))
                        })
                }
            }
            _ => Word::Usage("rates [<key> <value>|<key> default]"),
        },
        "events" => match args.as_slice() {
            [] => Word::Events(None),
            [id, state] => match (id.parse::<u16>(), state.parse::<u8>()) {
                (Ok(i), Ok(s)) => Word::Events(Some((i, s))),
                _ => Word::Usage("events [<id> <state 0..255>]"),
            },
            _ => Word::Usage("events [<id> <state 0..255>]"),
        },
        "notice" | "broadcast" => {
            if rest.is_empty() {
                Word::Usage("notice <text>")
            } else {
                Word::Notice(rest.to_owned())
            }
        }
        "as" => {
            let (who, command) = split_first(rest);
            if who.is_empty() || command.is_empty() {
                Word::Usage("as <character> <GM command>")
            } else {
                Word::Gm {
                    line: with_slash(command),
                    as_character: Some(who.to_owned()),
                }
            }
        }
        _ => Word::Gm {
            line: with_slash(line),
            as_character: None,
        },
    }
}

/// `rest` without its first `skip` words, trimmed; `None` when nothing is left.
fn message_after(rest: &str, skip: usize) -> Option<String> {
    let mut r = rest.trim_start();
    for _ in 0..skip {
        r = match r.find(char::is_whitespace) {
            Some(i) => r[i..].trim_start(),
            None => "",
        };
    }
    (!r.is_empty()).then(|| r.to_owned())
}

fn with_slash(line: &str) -> String {
    if line.starts_with('/') {
        line.to_owned()
    } else {
        format!("/{line}")
    }
}

/// `account ...` tokens as the clap command of `flyff account`.
#[derive(Parser, Debug)]
#[command(name = "account", no_binary_name = true)]
struct AccountWord {
    #[command(subcommand)]
    cmd: AccountCmd,
}

/// `GET`-style terminal of the HTTP console: there is nothing to prompt.
pub struct NoTerminal;

impl Terminal for NoTerminal {
    fn is_tty(&self) -> bool {
        false
    }

    fn prompt_secret(&mut self, _prompt: &str) -> std::io::Result<String> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "no terminal",
        ))
    }

    fn read_stdin_line(&mut self) -> std::io::Result<String> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "no terminal",
        ))
    }
}

fn ops_error(e: OpsError) -> LineReply {
    LineReply::err(e.to_string())
}

/// Parses and executes one line.
pub async fn run_line(
    ops: &AdminOps,
    line: &str,
    as_character: Option<&str>,
    actor: AdminActor,
    term: &mut dyn Terminal,
) -> LineReply {
    let mut word = parse(line);
    if let Some(who) = as_character {
        match &mut word {
            Word::Gm {
                as_character: slot, ..
            } => {
                slot.get_or_insert_with(|| who.to_owned());
            }
            Word::Empty => {}
            // `as` only means something for a GM line
            _ => {
                word = Word::Gm {
                    line: with_slash(line.trim()),
                    as_character: Some(who.to_owned()),
                };
            }
        }
    }
    let reply = execute(ops, word, actor, term).await;
    if !line.trim().is_empty() {
        ops.audit_command(actor, line.trim(), if reply.ok { "ok" } else { "error" });
    }
    reply
}

/// The lowest authority letter that may run a word (Plan/25 §9.9, §1.4): the console words that read are open to a
/// staff member (`L`), `kick` and `notice` need `M` (`act.kick`, `act.broadcast`), every other word changes the
/// server, its rates or its accounts and needs `P`. A GM line is checked by the GM handler against the command
/// table with the staff member's own letter. The owners, the stdin console and the CLI act as `P`.
pub fn word_min_authority(word: &Word) -> Authority {
    match word {
        Word::Empty
        | Word::Usage(_)
        | Word::Help
        | Word::Version
        | Word::Status
        | Word::Sessions(_)
        | Word::Online(_)
        | Word::Rates(None)
        | Word::Events(None)
        | Word::Gm { .. } => Authority::GAMEMASTER,
        Word::Kick(_) | Word::Notice(_) => Authority::GAMEMASTER2,
        _ => Authority::ADMINISTRATOR,
    }
}

async fn execute(
    ops: &AdminOps,
    word: Word,
    actor: AdminActor,
    term: &mut dyn Terminal,
) -> LineReply {
    let need = word_min_authority(&word);
    if actor.authority() < need {
        return LineReply::err(format!(
            "this command needs authority {} (you act as {})",
            need.letter(),
            actor.authority().letter()
        ));
    }
    match word {
        Word::Empty => LineReply::ok(Vec::new()),
        Word::Usage(u) => LineReply::err(format!("usage: {u}")),
        Word::Help => LineReply::ok(HELP.iter().map(|s| (*s).to_owned()).collect()),
        Word::Version => LineReply::one(ops.version_line.clone()),
        Word::Status => match ops.status_snapshot().await {
            Ok(s) => {
                let mut lines = vec![json::status_line(&s)];
                lines.push(format!("sessions: {}", ops.session_counts()));
                LineReply::ok(lines)
            }
            Err(e) => ops_error(e),
        },
        Word::Sessions(limit) => {
            let n = limit.unwrap_or(20);
            let mut lines = vec![format!(
                "{:>6} {:<12} {:<22} {:<16} {:>6} {:>8}",
                "id", "state", "peer", "account", "age s", "queued"
            )];
            for s in ops.gateway_sessions(n) {
                lines.push(format!(
                    "{:>6} {:<12} {:<22} {:<16} {:>6} {:>8}",
                    s.id,
                    s.state,
                    s.peer,
                    s.account.as_deref().unwrap_or("-"),
                    s.age_s,
                    s.queued_bytes
                ));
            }
            match ops
                .online(limit.and_then(|n| u32::try_from(n).ok()), actor)
                .await
            {
                Ok(rows) => lines.extend(json::online_table(&rows)),
                Err(e) => lines.push(format!("error: {e}")),
            }
            LineReply::ok(lines)
        }
        Word::Online(limit) => match ops.online(limit, actor).await {
            Ok(rows) => LineReply::ok(json::online_table(&rows)),
            Err(e) => ops_error(e),
        },
        Word::SaveAll => game_lines(ops, AdminCmd::SaveAll, actor).await,
        Word::Digest => game_lines(ops, AdminCmd::Digest, actor).await,
        Word::Set(key, value) => game_lines(ops, AdminCmd::SetRuntime { key, value }, actor).await,
        Word::Unset(key) => game_lines(ops, AdminCmd::UnsetRuntime { key }, actor).await,
        Word::Rates(None) => game_lines(ops, AdminCmd::GetRates, actor).await,
        Word::Rates(Some((key, value))) => {
            game_lines(ops, AdminCmd::SetRate { key, value }, actor).await
        }
        Word::Events(None) => game_lines(ops, AdminCmd::GetEvents, actor).await,
        Word::Events(Some((id, state))) => {
            game_lines(ops, AdminCmd::SetEvent { id, state }, actor).await
        }
        Word::Notice(text) => game_lines(ops, AdminCmd::Broadcast { text }, actor).await,
        Word::Gm { line, as_character } => {
            game_lines(ops, AdminCmd::RunLine { line, as_character }, actor).await
        }
        Word::Reload => {
            let r = ops.reload(ReloadBy::from(actor)).await;
            LineReply {
                ok: r.errors.is_empty(),
                lines: super::reload::report_lines(&r),
            }
        }
        Word::Log(filter) => match ops.log.set_filter(&filter) {
            Ok(()) => LineReply::one(format!("log filter is now '{filter}' (not persisted)")),
            Err(e) => LineReply::err(format!("cannot set the log filter: {e:#}")),
        },
        Word::Shutdown(req) => match ops.shutdown(&req, actor) {
            Ok(Some(0)) => LineReply::one("shutting down now"),
            Ok(Some(n)) => LineReply::one(format!("shutting down in {n} s")),
            Ok(None) => LineReply::one("shutdown cancelled"),
            Err(e) => LineReply::err(e),
        },
        // a `manual` backup on a reader thread (13 §21.1): the writer is never stalled
        Word::Backup => match ops.persist.backup(flyff_db::BackupKind::Manual).await {
            Ok(path) => LineReply::one(format!("backup written: {}", path.display())),
            Err(e) => LineReply::err(format!("backup failed: {e}")),
        },
        Word::DbCheckpoint(mode) => {
            ops.persist.maintenance(MaintenanceJob::Checkpoint(mode));
            LineReply::one(format!("checkpoint ({}) queued", mode.pragma()))
        }
        Word::DbMaintenance => db_maintenance(ops, actor).await,
        Word::DbRelease(name) => {
            let Ok(n) = crate::lobby::accounts::AccountName::parse(&name) else {
                return LineReply::err("not an account name");
            };
            let wanted = n.as_str().to_owned();
            match ops
                .persist
                .read(move |c| queries::find_account(c, &wanted))
                .await
            {
                Ok(Some(rec)) => {
                    let id: AccountId = rec.id;
                    if ops.persist.is_poisoned(id) {
                        ops.persist.release_poisoned_account(id);
                        LineReply::one(format!("account {name} released"))
                    } else {
                        LineReply::one(format!("account {name} is not blocked"))
                    }
                }
                Ok(None) => LineReply::err("no such account"),
                Err(e) => LineReply::err(e.to_string()),
            }
        }
        Word::Kick(name) => match ops.kick(&name, actor).await {
            Ok(Some(n)) => LineReply::one(format!("kicked {n}")),
            Ok(None) => LineReply::err("not found"),
            Err(e) => ops_error(e),
        },
        Word::Grant(args) => match super::grants::console(ops, &args).await {
            Ok(text) => LineReply::one(text),
            Err(text) => LineReply::err(text),
        },
        Word::Account(args) => account_word(ops, &args, term).await,
    }
}

/// How long `db maintenance now` waits for the writer's report.
const MAINTENANCE_WAIT: std::time::Duration = std::time::Duration::from_secs(30);

/// `db maintenance now`: the game selects the characters whose retention ran out and queues the daily job
/// (`AdminCmd::RunMaintenance`); the writer runs it and the report is printed when it is there.
async fn db_maintenance(ops: &AdminOps, actor: AdminActor) -> LineReply {
    let runs_before = ops.persist.stats().maintenance_runs;
    let failures_before = ops.persist.stats().maintenance_failures;
    let selected = match ops.game(AdminCmd::RunMaintenance, actor).await {
        Ok(AdminOutput::Done { affected }) => affected,
        Ok(AdminOutput::Error(e)) => return LineReply::err(e),
        Ok(_) => return LineReply::err("unexpected answer of the game"),
        Err(e) => return ops_error(e),
    };
    let started = std::time::Instant::now();
    loop {
        let stats = ops.persist.stats();
        if stats.maintenance_failures > failures_before {
            return LineReply::err(
                "the maintenance run failed (see the log); it is idempotent, try again",
            );
        }
        if stats.maintenance_runs > runs_before {
            let mut lines = vec![format!("{selected} character(s) selected for the purge")];
            if let Some(r) = ops.persist.last_maintenance() {
                lines.push(format!(
                    "purged: characters {}, mails {}, notes {}, grants {}, removals {}, cancelled grants and removals {}, idempotency keys {}, guild bank log rows {}, couple proposals {}, guild rejoin cooldowns {}; vacuum {}; {} ms",
                    r.characters_purged,
                    r.mails_purged,
                    r.notes_purged,
                    r.grants_purged,
                    r.removals_purged,
                    r.cancelled_purged,
                    r.idempotency_purged,
                    r.guild_bank_log_trimmed,
                    r.couple_proposals_purged,
                    r.rejoin_cooldowns_purged,
                    if r.vacuumed { "done" } else { "skipped (a backup is running)" },
                    r.elapsed_ms
                ));
            }
            return LineReply::ok(lines);
        }
        if started.elapsed() > MAINTENANCE_WAIT {
            return LineReply::one(format!(
                "{selected} character(s) selected; the maintenance run is still going, see the log for its report"
            ));
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
}

async fn game_lines(ops: &AdminOps, cmd: AdminCmd, actor: AdminActor) -> LineReply {
    match ops.game(cmd, actor).await {
        Ok(AdminOutput::Error(e)) => LineReply::err(e),
        Ok(out) => LineReply::ok(json::output_lines(&out)),
        Err(e) => ops_error(e),
    }
}

/// `account create|passwd|set-auth|list|ban ...` on the console: the same grammar and rules as `flyff account`,
/// executed in-process by the server's [`AccountAdmin`](crate::lobby::accounts::AccountAdmin) (with its online
/// effects), never on the game thread.
async fn account_word(ops: &AdminOps, args: &[String], term: &mut dyn Terminal) -> LineReply {
    let parsed = AccountWord::try_parse_from(args.iter().map(String::as_str));
    let cmd = match parsed {
        Ok(w) => w.cmd,
        Err(e) => {
            return LineReply {
                lines: e.to_string().lines().map(str::to_owned).collect(),
                ok: false,
            };
        }
    };
    let cfg = ops.current.lock().clone();
    let env = Env {
        salt: cfg.auth.legacy_md5_salt.clone(),
        zone: ops.reload_source.zone_name.clone(),
        now: ops.core.clock.unix().0,
    };
    let op = match account::prepare(&cmd, &env, term) {
        Ok(op) => op,
        Err(f) => return LineReply::err(f.message),
    };
    match account::run_ops(&ops.accounts, &op, "console").await {
        Ok(done) => LineReply::ok(
            account::render(&op, &done)
                .lines()
                .map(str::to_owned)
                .collect(),
        ),
        Err(f) => LineReply::err(f.message),
    }
}
