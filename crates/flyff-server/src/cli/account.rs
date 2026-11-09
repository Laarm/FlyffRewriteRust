//! `flyff account create|passwd|set-auth|list|ban` (Plan/12 §5.5, WP-AUTH-19): the single account CLI.
//!
//! **Execution modes** (single-instance lock `data/flyff.db.lock`, detected by the non-blocking `flock` of
//! `flyff_db::open`):
//!
//! * *offline* (lock acquired): the CLI keeps the lock for the duration of the command (a concurrent `serve`
//!   exits 75), opens the database like `serve` does (migrations included), runs the operation through
//!   [`AccountAdmin`] (no online effects) and shuts the persistence actor down gracefully. The audit lines go to
//!   the CLI's own appender (`logs/audit/`, 10 §9.2).
//! * *online* (the lock is held by a running server): the operation is sent to the admin API on loopback
//!   (`http://127.0.0.1:<port>`; the port is the one the server wrote to `data/admin.addr`, else that of
//!   `net.bind`), so the server's `AccountAdmin` performs the writes through its own
//!   persistence actor (D5 single writer) and applies the online effects (kicks, token revocation, authority
//!   updates). A stale lock file whose `flock` is not held counts as offline.
//!
//! The password is read as 12 §5.5 says: prompted twice without echo on a terminal, one line from stdin with
//! `--password-stdin`, or generated with `--random` and printed once (`password: ...`). The plaintext never
//! leaves this process: the CLI sends `pwhash = md5_hex_lower(auth.legacy_md5_salt + password)`; it appears in no
//! argv, log or audit line.
//!
//! **Exit codes**: 0 success; 1 invalid value (name, level, password, date, e-mail) or operation refused; 2 not
//! found / already exists / no active ban; 3 database or lock error, or the server is unreachable in online mode;
//! 4 the admin API refused the request (401/403); 64 clap usage error.

#![allow(clippy::print_stdout, clippy::print_stderr)]

use super::admin_client::{AdminClient, ClientError, Reply, loopback_addr, url_encode};
use super::{AccountCmd, Cli, env_pairs, home_of, overrides_of, prepare::init_cli_logging};
use crate::audit::AuditFile;
use crate::config::{self, Config};
use crate::exit::{self, ExitStatusCode};
use crate::lobby::accounts::{
    AccountAdmin, AccountFilter, AccountName, AdminError, GmPolicy, NewAccount, PASSWORD_MAX,
    PASSWORD_MIN,
};
use crate::lobby::auth::{PasswordHasher, PwHash, legacy_pwhash};
use crate::lobby::bootstrap_admin::{PASSWORD_LEN, random_password};
use crate::lobby::online::LobbyClock;
use chrono::{DateTime, LocalResult, NaiveDate, NaiveDateTime, TimeZone};
use flyff_common::UnixTime;
use flyff_common::gm::Authority;
use flyff_db::{CreatedVia, DbErrorKind, ShutdownMode};
use serde_json::{Value, json};
use std::io::{BufRead, IsTerminal};
use std::path::PathBuf;
use std::time::Duration;

/// A refused command line value (exit 1) or a failed step (the code is the exit status).
#[derive(Debug, PartialEq, Eq)]
pub struct CliFailure {
    /// The process exit status.
    pub code: ExitStatusCode,
    /// What to tell the operator.
    pub message: String,
}

impl CliFailure {
    fn invalid(message: impl Into<String>) -> CliFailure {
        CliFailure {
            code: 1,
            message: message.into(),
        }
    }
}

/// A validated command: everything that needs no database is checked before one is opened.
#[derive(Clone, Debug, PartialEq)]
pub enum Op {
    /// `create`.
    Create {
        /// Normalised name.
        name: String,
        /// The client-side hash.
        pw: PwHash,
        /// Authority (default `F`).
        auth: Authority,
        /// Optional e-mail.
        email: Option<String>,
        /// The generated password to print once (`--random`).
        generated: Option<String>,
    },
    /// `passwd`.
    Passwd {
        /// Normalised name.
        name: String,
        /// The client-side hash.
        pw: PwHash,
        /// The generated password to print once (`--random`).
        generated: Option<String>,
    },
    /// `set-auth`.
    SetAuth {
        /// Normalised name.
        name: String,
        /// The new level.
        level: Authority,
        /// A character of the account instead of the account itself.
        character: Option<String>,
    },
    /// `list`.
    List {
        /// The filter.
        filter: AccountFilter,
        /// JSON output.
        json: bool,
    },
    /// `ban`.
    Ban {
        /// Normalised name.
        name: String,
        /// The end (`None` = permanent).
        until: Option<UnixTime>,
        /// The reason.
        reason: String,
    },
    /// `ban --lift`.
    Unban {
        /// Normalised name.
        name: String,
    },
}

/// Where passwords come from (the terminal, or a fake in tests).
pub trait Terminal: Send {
    /// Standard input is a terminal.
    fn is_tty(&self) -> bool;
    /// A line read without echo.
    fn prompt_secret(&mut self, prompt: &str) -> std::io::Result<String>;
    /// One line of standard input (`--password-stdin`), without its line break.
    fn read_stdin_line(&mut self) -> std::io::Result<String>;
}

/// The real terminal.
pub struct SysTerminal;

impl Terminal for SysTerminal {
    fn is_tty(&self) -> bool {
        std::io::stdin().is_terminal()
    }

    fn prompt_secret(&mut self, prompt: &str) -> std::io::Result<String> {
        crate::sys::read_line_no_echo(prompt)
    }

    fn read_stdin_line(&mut self) -> std::io::Result<String> {
        let mut line = String::new();
        std::io::stdin().lock().read_line(&mut line)?;
        while line.ends_with(['\n', '\r']) {
            line.pop();
        }
        Ok(line)
    }
}

/// What the command line says about the environment (the loaded configuration).
#[derive(Clone, Debug)]
pub struct Env {
    /// `auth.legacy_md5_salt`.
    pub salt: String,
    /// The IANA name of `server.timezone` (`"local"` resolved).
    pub zone: String,
    /// Now, Unix seconds.
    pub now: i64,
}

// ---- parsing ---------------------------------------------------------------------------------------------------

/// `90m`, `12h`, `7d` (n >= 1) or `perm` (`None`).
pub fn parse_duration(text: &str) -> Result<Option<i64>, CliFailure> {
    let t = text.trim().to_ascii_lowercase();
    if t == "perm" || t == "permanent" {
        return Ok(None);
    }
    let bad = || {
        CliFailure::invalid(format!(
            "'{text}' is not a duration: use <n>m, <n>h, <n>d (n >= 1) or perm"
        ))
    };
    let (num, unit) = t.split_at(t.len().saturating_sub(1));
    let n: i64 = num.parse().map_err(|_| bad())?;
    if n < 1 {
        return Err(bad());
    }
    let secs = match unit {
        "m" => 60,
        "h" => 3600,
        "d" => 86_400,
        _ => return Err(bad()),
    };
    n.checked_mul(secs).map(Some).ok_or_else(bad)
}

/// `YYYY-MM-DD[THH:MM]` in the server time zone, or RFC 3339 with an offset; the result must lie after `now`.
pub fn parse_until(text: &str, zone: &str, now: i64) -> Result<i64, CliFailure> {
    let t = text.trim();
    let bad = || {
        CliFailure::invalid(format!(
            "'{text}' is not a date: use YYYY-MM-DD, YYYY-MM-DDTHH:MM (server time zone {zone}) or RFC 3339 with an offset"
        ))
    };
    let at = if let Ok(d) = DateTime::parse_from_rfc3339(t) {
        d.timestamp()
    } else {
        let naive = if let Ok(n) = NaiveDateTime::parse_from_str(t, "%Y-%m-%dT%H:%M") {
            n
        } else if let Ok(d) = NaiveDate::parse_from_str(t, "%Y-%m-%d") {
            d.and_hms_opt(0, 0, 0).ok_or_else(bad)?
        } else {
            return Err(bad());
        };
        let tz: chrono_tz::Tz = zone
            .parse()
            .map_err(|_| CliFailure::invalid(format!("unknown time zone '{zone}'")))?;
        match tz.from_local_datetime(&naive) {
            LocalResult::Single(d) => d.timestamp(),
            LocalResult::Ambiguous(first, _) => first.timestamp(),
            LocalResult::None => return Err(bad()),
        }
    };
    if at <= now {
        return Err(CliFailure::invalid(format!(
            "'{text}' is not in the future"
        )));
    }
    Ok(at)
}

fn account_name(raw: &str) -> Result<String, CliFailure> {
    AccountName::parse(raw)
        .map(|n| n.as_str().to_owned())
        .map_err(|_| {
            CliFailure::invalid(
                "account names are 3 to 16 characters: a letter, then letters and digits",
            )
        })
}

fn level_of(raw: &str) -> Result<Authority, CliFailure> {
    match Authority::parse_token(raw) {
        Ok(a) if crate::lobby::accounts::writable_authority(a) => Ok(a),
        _ => Err(CliFailure::invalid(format!(
            "'{raw}' is not an authority level (D F G H J L M N O P, AUTH_*, player, logchat, journalist, helper, gm1, gm2, gm3, operator, admin)"
        ))),
    }
}

/// Reads the password as `flags` say and hashes it. Returns the hash and the generated text (`--random`).
fn password_of(
    term: &mut dyn Terminal,
    salt: &str,
    from_stdin: bool,
    random: bool,
) -> Result<(PwHash, Option<String>), CliFailure> {
    let (plain, generated) = if random {
        let pw = random_password(PASSWORD_LEN)
            .map_err(|e| CliFailure::invalid(format!("the OS random generator failed: {e}")))?;
        (pw.clone(), Some(pw))
    } else if from_stdin {
        let line = term
            .read_stdin_line()
            .map_err(|e| CliFailure::invalid(format!("cannot read the password: {e}")))?;
        (line, None)
    } else if term.is_tty() {
        let first = term
            .prompt_secret("Password: ")
            .map_err(|e| CliFailure::invalid(format!("cannot read the password: {e}")))?;
        let second = term
            .prompt_secret("Repeat password: ")
            .map_err(|e| CliFailure::invalid(format!("cannot read the password: {e}")))?;
        if first != second {
            return Err(CliFailure::invalid("the passwords do not match"));
        }
        (first, None)
    } else {
        return Err(CliFailure::invalid(
            "no password: use --password-stdin or --random (standard input is not a terminal)",
        ));
    };
    let len = plain.len();
    if !(PASSWORD_MIN..=PASSWORD_MAX).contains(&len) {
        return Err(CliFailure::invalid(format!(
            "a password has {PASSWORD_MIN} to {PASSWORD_MAX} bytes (UTF-8)"
        )));
    }
    Ok((legacy_pwhash(salt, &plain), generated))
}

/// Validates a parsed command into an [`Op`] (no database, no network).
pub fn prepare(cmd: &AccountCmd, env: &Env, term: &mut dyn Terminal) -> Result<Op, CliFailure> {
    match cmd {
        AccountCmd::Create {
            name,
            auth,
            password_stdin,
            random,
            email,
        } => {
            let name = account_name(name)?;
            let auth = match auth {
                Some(a) => level_of(a)?,
                None => Authority::GENERAL,
            };
            if let Some(mail) = email
                && !crate::lobby::accounts::valid_email(mail)
            {
                return Err(CliFailure::invalid("the e-mail address is malformed"));
            }
            let (pw, generated) = password_of(term, &env.salt, *password_stdin, *random)?;
            Ok(Op::Create {
                name,
                pw,
                auth,
                email: email.clone(),
                generated,
            })
        }
        AccountCmd::Passwd {
            name,
            password_stdin,
            random,
        } => {
            let name = account_name(name)?;
            let (pw, generated) = password_of(term, &env.salt, *password_stdin, *random)?;
            Ok(Op::Passwd {
                name,
                pw,
                generated,
            })
        }
        AccountCmd::SetAuth {
            name,
            level,
            character,
        } => Ok(Op::SetAuth {
            name: account_name(name)?,
            level: level_of(level)?,
            character: character.clone(),
        }),
        AccountCmd::List {
            name,
            gm,
            online,
            limit,
            json,
        } => Ok(Op::List {
            filter: AccountFilter {
                name_substr: name.clone(),
                gm_only: *gm,
                online_only: *online,
                limit: (*limit).clamp(1, 100_000),
            },
            json: *json,
        }),
        AccountCmd::Ban {
            name,
            for_,
            until,
            lift,
            reason,
        } => {
            let name = account_name(name)?;
            if *lift {
                return Ok(Op::Unban { name });
            }
            let until = match (for_, until) {
                (Some(d), None) => parse_duration(d)?.map(|secs| UnixTime(env.now + secs)),
                (None, Some(date)) => Some(UnixTime(parse_until(date, &env.zone, env.now)?)),
                _ => {
                    return Err(CliFailure::invalid(
                        "give exactly one of --for, --until or --lift",
                    ));
                }
            };
            Ok(Op::Ban {
                name,
                until,
                reason: reason.clone().unwrap_or_default(),
            })
        }
    }
}

// ---- results ---------------------------------------------------------------------------------------------------

/// What a successful operation reports.
#[derive(Clone, Debug, PartialEq)]
pub enum Done {
    /// `create`: id and name.
    Created {
        /// Account id.
        id: u32,
        /// Name.
        name: String,
    },
    /// `passwd`, `ban`, `ban --lift`.
    Changed,
    /// `set-auth`: whether a `[gm]` grant overrides the stored value.
    Authority {
        /// `[gm]` overrides it.
        overridden: bool,
    },
    /// `list`: the rows as JSON objects (the `AccountSummary` shape).
    Listed(Vec<Value>),
}

/// Prints the result of `op`, exactly as the plan's examples show.
pub fn render(op: &Op, done: &Done) -> String {
    let mut out = String::new();
    match (op, done) {
        (
            Op::Create {
                name,
                auth,
                generated,
                ..
            },
            Done::Created { id, .. },
        ) => {
            out.push_str(&format!(
                "account {name} created (id {id}, authority {auth})\n"
            ));
            if let Some(pw) = generated {
                out.push_str(&format!("password: {pw}\n"));
            }
        }
        (
            Op::Passwd {
                name, generated, ..
            },
            _,
        ) => {
            out.push_str(&format!("password of {name} changed\n"));
            if let Some(pw) = generated {
                out.push_str(&format!("password: {pw}\n"));
            }
        }
        (
            Op::SetAuth {
                name,
                level,
                character,
            },
            Done::Authority { overridden },
        ) => {
            match character {
                Some(c) => out.push_str(&format!(
                    "authority of character {c} of {name} is now {level}\n"
                )),
                None => out.push_str(&format!("authority of {name} is now {level}\n")),
            }
            if *overridden {
                out.push_str("warning: overridden by [gm] config\n");
            }
        }
        (Op::Ban { name, until, .. }, _) => match until {
            None => out.push_str(&format!("{name} is banned permanently\n")),
            Some(u) => out.push_str(&format!("{name} is banned until {}\n", rfc3339(u.0))),
        },
        (Op::Unban { name }, _) => out.push_str(&format!("lifted the ban of {name}\n")),
        (Op::List { json: true, .. }, Done::Listed(rows)) => {
            out.push_str(&serde_json::to_string_pretty(rows).unwrap_or_else(|_| "[]".to_owned()));
            out.push('\n');
        }
        (Op::List { .. }, Done::Listed(rows)) => out.push_str(&table(rows)),
        _ => {}
    }
    out
}

fn rfc3339(t: i64) -> String {
    DateTime::from_timestamp(t, 0).map_or_else(
        || t.to_string(),
        |d| d.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
    )
}

fn cell(v: &Value, key: &str) -> String {
    match v.get(key) {
        None | Some(Value::Null) => "-".to_owned(),
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
    }
}

fn table(rows: &[Value]) -> String {
    const COLS: [(&str, &str); 10] = [
        ("id", "id"),
        ("name", "name"),
        ("authority", "auth"),
        ("created_at", "created"),
        ("created_via", "via"),
        ("last_login_at", "last login"),
        ("last_login_ip", "last ip"),
        ("banned_until", "ban"),
        ("characters", "chars"),
        ("online", "online"),
    ];
    let mut grid: Vec<Vec<String>> = vec![COLS.iter().map(|(_, h)| (*h).to_owned()).collect()];
    for r in rows {
        grid.push(
            COLS.iter()
                .map(|(k, _)| match *k {
                    "created_at" | "last_login_at" => r
                        .get(*k)
                        .and_then(Value::as_i64)
                        .map_or_else(|| "-".to_owned(), rfc3339),
                    "banned_until" => match r.get(*k) {
                        Some(Value::Number(n)) => {
                            n.as_i64().map_or_else(|| "-".to_owned(), rfc3339)
                        }
                        Some(Value::String(s)) => s.clone(),
                        _ => "-".to_owned(),
                    },
                    "online" => match cell(r, k).as_str() {
                        "in_world" => "world".to_owned(),
                        "char_select" => "select".to_owned(),
                        _ => "-".to_owned(),
                    },
                    _ => cell(r, k),
                })
                .collect(),
        );
    }
    let widths: Vec<usize> = (0..COLS.len())
        .map(|c| {
            grid.iter()
                .map(|row| row.get(c).map_or(0, |s| s.chars().count()))
                .max()
                .unwrap_or(0)
        })
        .collect();
    let mut out = String::new();
    for row in &grid {
        let line: Vec<String> = row
            .iter()
            .zip(&widths)
            .map(|(s, w)| format!("{s:<w$}"))
            .collect();
        out.push_str(line.join("  ").trim_end());
        out.push('\n');
    }
    out
}

// ---- execution -------------------------------------------------------------------------------------------------

fn offline_failure(e: &AdminError) -> CliFailure {
    CliFailure {
        code: i32::from(e.exit_code()),
        message: match e {
            AdminError::NotFound => {
                "no such account (or the character does not belong to it)".to_owned()
            }
            AdminError::Exists => "the account already exists".to_owned(),
            AdminError::NoActiveBan => "the account has no active ban".to_owned(),
            other => other.to_string(),
        },
    }
}

/// Runs `op` through `admin`: offline (the CLI holds the instance lock; no online effects) or in-process on the
/// server (the console words; the online effects apply). `by` is the actor of the audit events (`cli`, `console`).
pub async fn run_ops(admin: &AccountAdmin, op: &Op, by: &str) -> Result<Done, CliFailure> {
    match op {
        Op::Create {
            name,
            pw,
            auth,
            email,
            ..
        } => admin
            .create_as(
                NewAccount {
                    name,
                    pw,
                    authority: *auth,
                    via: CreatedVia::Cli,
                    ip: None,
                    email: email.as_deref(),
                },
                by,
            )
            .await
            .map(|id| Done::Created {
                id: id.0,
                name: name.clone(),
            }),
        Op::Passwd { name, pw, .. } => admin
            .set_password(name, pw, by)
            .await
            .map(|()| Done::Changed),
        Op::SetAuth {
            name,
            level,
            character,
        } => admin
            .set_authority(name, *level, character.as_deref(), by)
            .await
            .map(|overridden| Done::Authority { overridden }),
        Op::List { filter, .. } => admin.list(filter.clone()).await.map(|rows| {
            Done::Listed(
                rows.iter()
                    .filter_map(|r| serde_json::to_value(r).ok())
                    .collect(),
            )
        }),
        Op::Ban {
            name,
            until,
            reason,
        } => admin
            .ban(name, *until, reason, by)
            .await
            .map(|()| Done::Changed),
        Op::Unban { name } => admin.unban(name, by).await.map(|()| Done::Changed),
    }
    .map_err(|e| offline_failure(&e))
}

fn http_failure(r: &Reply) -> CliFailure {
    let code = match r.status {
        400 => 1,
        404 | 409 => 2,
        401 | 403 => 4,
        _ => 3,
    };
    let message = match r.status {
        401 | 403 => format!(
            "the server refused the request ({}): {}",
            r.status,
            r.text.trim()
        ),
        _ => r.detail().to_owned(),
    };
    CliFailure { code, message }
}

/// Runs `op` through the admin API of the running server.
pub fn run_online(client: &AdminClient, op: &Op) -> Result<Done, CliFailure> {
    let by = "cli";
    let send = |method: &str, path: &str, body: Option<&Value>| -> Result<Reply, CliFailure> {
        client.request(method, path, body).map_err(|e| match e {
            ClientError::Unreachable { .. } | ClientError::BadResponse => CliFailure {
                code: 3,
                message: e.to_string(),
            },
        })
    };
    let reply = match op {
        Op::Create {
            name,
            pw,
            auth,
            email,
            ..
        } => send(
            "POST",
            "/admin/api/accounts",
            Some(&json!({
                "name": name, "pwhash": pw.as_str(), "auth": auth.letter().to_string(),
                "email": email, "by": by,
            })),
        )?,
        Op::Passwd { name, pw, .. } => send(
            "PUT",
            &format!("/admin/api/accounts/{name}/password"),
            Some(&json!({"pwhash": pw.as_str(), "by": by})),
        )?,
        Op::SetAuth {
            name,
            level,
            character,
        } => send(
            "PUT",
            &format!("/admin/api/accounts/{name}/authority"),
            Some(&json!({"level": level.letter().to_string(), "character": character, "by": by})),
        )?,
        Op::List { filter, .. } => {
            let mut q = format!("limit={}", filter.limit);
            if let Some(n) = &filter.name_substr {
                q.push_str(&format!("&name={}", url_encode(n)));
            }
            if filter.gm_only {
                q.push_str("&gm=true");
            }
            if filter.online_only {
                q.push_str("&online=true");
            }
            send("GET", &format!("/admin/api/accounts?{q}"), None)?
        }
        Op::Ban {
            name,
            until,
            reason,
        } => send(
            "POST",
            &format!("/admin/api/accounts/{name}/ban"),
            Some(&json!({"until": until.map(|u| u.0), "reason": reason, "by": by})),
        )?,
        Op::Unban { name } => send(
            "DELETE",
            &format!("/admin/api/accounts/{name}/ban"),
            Some(&json!({"by": by})),
        )?,
    };
    if !(200..300).contains(&reply.status) {
        return Err(http_failure(&reply));
    }
    Ok(match op {
        Op::Create { name, .. } => Done::Created {
            id: reply
                .body
                .get("id")
                .and_then(Value::as_u64)
                .and_then(|n| u32::try_from(n).ok())
                .unwrap_or(0),
            name: name.clone(),
        },
        Op::SetAuth { .. } => Done::Authority {
            overridden: reply
                .body
                .get("overridden_by_gm_config")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        },
        Op::List { .. } => Done::Listed(reply.body.as_array().cloned().unwrap_or_default()),
        _ => Done::Changed,
    })
}

// ---- the command -----------------------------------------------------------------------------------------------

struct Setup {
    home: PathBuf,
    cfg: Config,
    env: Env,
}

fn setup(cli: &Cli) -> Result<Setup, ExitStatusCode> {
    let home = home_of(cli)?;
    let loaded = match config::load(&home, &overrides_of(cli), &env_pairs(), &[]) {
        Ok(l) => l,
        Err(errs) => {
            for e in errs {
                eprintln!("error: {e}");
            }
            return Err(exit::CONFIG);
        }
    };
    let cfg = loaded.config;
    let zone = flyff_game::WallZone::resolve(&cfg.server.timezone)
        .map_or_else(|_| "UTC".to_owned(), |z| z.name().to_owned());
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));
    Ok(Setup {
        env: Env {
            salt: cfg.auth.legacy_md5_salt.clone(),
            zone,
            now,
        },
        home,
        cfg,
    })
}

/// `flyff account ...`: parse-time validation, mode detection, execution, output, exit code.
pub fn run(cli: &Cli, cmd: &AccountCmd) -> ExitStatusCode {
    run_with(cli, cmd, &mut SysTerminal)
}

/// [`run`] with an injected terminal (tests).
pub fn run_with(cli: &Cli, cmd: &AccountCmd, term: &mut dyn Terminal) -> ExitStatusCode {
    init_cli_logging();
    let st = match setup(cli) {
        Ok(s) => s,
        Err(code) => return code,
    };
    let op = match prepare(cmd, &st.env, term) {
        Ok(op) => op,
        Err(f) => {
            eprintln!("error: {}", f.message);
            return f.code;
        }
    };
    match execute(&st, &op) {
        Ok(done) => {
            print!("{}", render(&op, &done));
            exit::OK
        }
        Err(f) => {
            eprintln!("error: {}", f.message);
            f.code
        }
    }
}

fn execute(st: &Setup, op: &Op) -> Result<Done, CliFailure> {
    let persist_cfg = st.cfg.persistence();
    match flyff_db::open(&persist_cfg, &st.home) {
        Ok(db) => {
            // offline: this process holds the instance lock for the length of the command
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| CliFailure {
                    code: 3,
                    message: format!("cannot start the runtime: {e}"),
                })?;
            let gm = st.cfg.gm.typed().unwrap_or_default();
            let audit = AuditFile::open(&st.home, &persist_cfg.audit, st.env.now);
            let admin = AccountAdmin::new(
                db.handle.clone(),
                PasswordHasher::default(),
                LobbyClock::system(),
                st.env.salt.clone(),
                GmPolicy {
                    cfg: std::sync::Arc::new(gm),
                    servers_authorize: st.cfg.features.servers_authorize,
                },
                None,
            )
            .with_audit(audit);
            let result = rt.block_on(run_ops(&admin, op, "cli"));
            drop(admin);
            if let Err(e) = db
                .persistence
                .shutdown(ShutdownMode::Graceful, Duration::from_secs(30))
            {
                tracing::error!("persistence did not shut down cleanly: {e}");
                if result.is_ok() {
                    return Err(CliFailure {
                        code: 3,
                        message: format!("the database did not close cleanly: {e}"),
                    });
                }
            }
            result
        }
        Err(e) if e.kind == DbErrorKind::Locked => {
            // a running server holds the lock: talk to its admin API on loopback
            let data_dir = st.cfg.resolve_paths(&st.home).data;
            // the running server wrote the address it really listens on (`--bind`, `:0`); else `net.bind`
            let announced = std::fs::read_to_string(data_dir.join(crate::admin::ADMIN_ADDR_FILE))
                .ok()
                .and_then(|t| loopback_addr(t.trim()));
            let Some(addr) = announced.or_else(|| loopback_addr(&st.cfg.net.bind)) else {
                return Err(CliFailure {
                    code: 3,
                    message: format!("net.bind '{}' is not an address", st.cfg.net.bind),
                });
            };
            run_online(&AdminClient::new(addr, &data_dir), op)
        }
        Err(e) => Err(CliFailure {
            code: 3,
            message: format!("cannot open the database: {e}"),
        }),
    }
}
