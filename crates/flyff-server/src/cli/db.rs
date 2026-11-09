//! `flyff db backup|restore|check|vacuum` and `flyff character undelete` (Plan/10 §9.2, 13 §21, §23; WP-OPS-14).
//!
//! The library functions are `flyff_db::{backup, check, maintenance}`; this module is their command line: flags,
//! the confirmation of a restore, the text output and the exit codes of the table in 10 §9.2.
//!
//! | command | lock | exit codes |
//! |---|---|---|
//! | `db backup [--with-config]` | – (an own read connection; safe while the server runs) | 0, 1 |
//! | `db restore <FILE> [--yes]` | **X**: exits 75 while the server holds `data/flyff.db.lock` | 0, 1, 75 |
//! | `db check [--quick \| --full \| --serials \| --items]` | – | 0, 1 findings, 2 error |
//! | `db vacuum` | **X** | 0, 1, 75 |
//! | `character undelete <NAME \| ID> [--slot N]` | **X** | 0, 1, 75 |
//!
//! `character undelete` appends a `char.undelete` line to the audit log (actor `cli`).

#![allow(clippy::print_stdout, clippy::print_stderr)]

use super::{Cli, DbCmd, env_pairs, home_of, overrides_of, prepare::init_cli_logging};
use crate::audit::{Actor, AuditFile};
use crate::config::{self, Config};
use crate::data_load::{self, DataMode};
use crate::exit::{self, ExitStatusCode};
use flyff_common::UnixTime;
use flyff_db::backup::{BackupClock, SystemClock, cli_backup, inspect_database, restore};
use flyff_db::check::{CheckMode, CheckReport, cli_check};
use flyff_db::maintenance::{undelete_character, vacuum};
use flyff_db::{DbError, DbErrorKind, InstanceLock, PersistenceConfig};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// What every command needs: the home, the configuration and the persistence slice of it.
struct Setup {
    home: PathBuf,
    config: Config,
    persistence: PersistenceConfig,
}

impl Setup {
    fn db_path(&self) -> PathBuf {
        self.home.join(&self.persistence.database.path)
    }
}

fn setup(cli: &Cli) -> Result<Setup, ExitStatusCode> {
    init_cli_logging();
    let home = home_of(cli)?;
    let loaded = config::load(&home, &overrides_of(cli), &env_pairs(), &[]).map_err(|errs| {
        for e in errs {
            eprintln!("error: {e}");
        }
        exit::CONFIG
    })?;
    for w in &loaded.warnings {
        eprintln!("warning: {w}");
    }
    let persistence = loaded.config.persistence();
    Ok(Setup {
        home,
        config: loaded.config,
        persistence,
    })
}

fn now() -> UnixTime {
    UnixTime(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX)),
    )
}

fn clock() -> Arc<dyn BackupClock> {
    Arc::new(SystemClock::utc())
}

/// The exit code of a failed command that takes the instance lock (`X` in the table above).
fn locked_exit(e: &DbError) -> ExitStatusCode {
    match e.kind {
        DbErrorKind::Locked => exit::TEMPFAIL,
        DbErrorKind::InvalidConfig | DbErrorKind::InitOnlyMismatch => exit::CONFIG,
        _ => exit::FAILURE,
    }
}

fn human_bytes(n: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KiB", "MiB", "GiB"];
    let mut value = n as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{n} B")
    } else {
        format!("{value:.1} {}", UNITS.get(unit).copied().unwrap_or("B"))
    }
}

/// Runs `flyff db <cmd>` (`migrate` is in `cli.rs`).
pub fn run(cli: &Cli, cmd: &DbCmd) -> ExitStatusCode {
    match cmd {
        DbCmd::Backup { with_config } => backup(cli, *with_config),
        DbCmd::Restore { file, yes } => restore_cmd(cli, file, *yes),
        DbCmd::Check {
            quick: _,
            full,
            serials,
            items,
        } => {
            let mode = if *full {
                CheckMode::Full
            } else if *serials {
                CheckMode::Serials
            } else if *items {
                CheckMode::Items
            } else {
                CheckMode::Quick
            };
            check(cli, mode)
        }
        DbCmd::Vacuum => vacuum_cmd(cli),
        DbCmd::Migrate { .. } => exit::FAILURE, // handled by cli.rs
    }
}

// ---- backup ----------------------------------------------------------------------------------------------------

fn backup(cli: &Cli, with_config: bool) -> ExitStatusCode {
    let st = match setup(cli) {
        Ok(s) => s,
        Err(c) => return c,
    };
    match cli_backup(&st.persistence, &st.home, with_config, clock()) {
        Ok(done) => {
            println!(
                "backup:  {} ({})",
                done.path.display(),
                human_bytes(done.bytes)
            );
            if let Some(dir) = &done.config_dir {
                println!("config:  {}", dir.display());
            }
            exit::OK
        }
        Err(e) => {
            eprintln!("error: {e}");
            exit::FAILURE
        }
    }
}

// ---- restore ---------------------------------------------------------------------------------------------------

fn restore_cmd(cli: &Cli, file: &Path, yes: bool) -> ExitStatusCode {
    let st = match setup(cli) {
        Ok(s) => s,
        Err(c) => return c,
    };
    let db_path = st.db_path();
    // refuse before asking anything while a server runs
    match InstanceLock::is_held(&db_path) {
        Ok(true) => {
            eprintln!(
                "error: {} is in use by a running flyff: stop the server first",
                db_path.display()
            );
            return exit::TEMPFAIL;
        }
        Ok(false) => {}
        Err(e) => {
            eprintln!("error: {e}");
            return locked_exit(&e);
        }
    }
    let source = match inspect_database(file) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: {}: {}", file.display(), e.context);
            return exit::FAILURE;
        }
    };
    println!(
        "restore: {} (schema {}, {} accounts, {} characters, {} guilds, {} open mails, newest save {})",
        file.display(),
        source.user_version,
        source.accounts,
        source.characters,
        source.guilds,
        source.open_mails,
        source.newest_save_text()
    );
    if db_path.is_file() {
        match inspect_database(&db_path) {
            Ok(cur) => println!(
                "current: {} (schema {}, {} accounts, {} characters, {} guilds, {} open mails, newest save {})",
                db_path.display(),
                cur.user_version,
                cur.accounts,
                cur.characters,
                cur.guilds,
                cur.open_mails,
                cur.newest_save_text()
            ),
            Err(e) => println!(
                "current: {} is not readable ({}); it is moved aside, not deleted",
                db_path.display(),
                e.context
            ),
        }
    }
    if !yes
        && !confirm(
            "Everything the server saved after the backup time is lost; the audit log is not rewound. \
             A backup of the current database is written first.",
        )
    {
        eprintln!("aborted: nothing was changed");
        return exit::FAILURE;
    }
    match restore(&st.persistence, &st.home, file, now()) {
        Ok(report) => {
            if let Some(p) = &report.prerestore_backup {
                println!("prerestore backup: {}", p.display());
            }
            if let Some(p) = &report.moved_aside {
                println!("the unreadable database was moved to {}", p.display());
            }
            if let Some(p) = &report.premigrate_backup {
                println!("pre-migration backup: {}", p.display());
            }
            if !report.migrated.is_empty() {
                println!("migrations applied: {:?}", report.migrated);
            }
            let s = &report.summary;
            println!(
                "restored: schema {}, {} accounts, {} characters, {} guilds, {} open mails, newest save {}",
                s.user_version,
                s.accounts,
                s.characters,
                s.guilds,
                s.open_mails,
                s.newest_save_text()
            );
            println!("start the server normally");
            exit::OK
        }
        Err(e) => {
            eprintln!("error: {e}");
            locked_exit(&e)
        }
    }
}

/// Asks `question` on stderr and reads the answer from stdin: only `yes` confirms.
fn confirm(question: &str) -> bool {
    eprintln!("{question}");
    eprint!("Type 'yes' to continue: ");
    let _ = std::io::stderr().flush();
    let mut line = String::new();
    match std::io::stdin().lock().read_line(&mut line) {
        Ok(n) if n > 0 => line.trim().eq_ignore_ascii_case("yes"),
        _ => false,
    }
}

// ---- check -----------------------------------------------------------------------------------------------------

fn check(cli: &Cli, mode: CheckMode) -> ExitStatusCode {
    let st = match setup(cli) {
        Ok(s) => s,
        Err(c) => return c,
    };
    let result = if mode == CheckMode::Items {
        let paths = st.config.resolve_paths(&st.home);
        let data = match data_load::load(&st.config, &paths, DataMode::Load, None) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("error: db check --items needs the game data: {e}");
                return exit::DATA;
            }
        };
        let game = Arc::clone(&data.game);
        let judge = move |id: u32, count: u16| -> Option<String> {
            match game.items.get(id) {
                None => Some("unknown item id".to_owned()),
                Some(prop) => {
                    let max = prop.pack_max.max(1);
                    (count == 0 || u32::from(count) > max)
                        .then(|| format!("count {count} outside 1..={max}"))
                }
            }
        };
        cli_check(&st.persistence, &st.home, mode, Some(&judge))
    } else {
        cli_check(&st.persistence, &st.home, mode, None)
    };
    match result {
        Ok(report) => {
            print_report(mode, &report);
            if report.is_clean() {
                exit::OK
            } else {
                exit::FAILURE
            }
        }
        Err(e) => {
            eprintln!("error: {e}");
            exit::DATA
        }
    }
}

fn print_report(mode: CheckMode, report: &CheckReport) {
    println!(
        "db check ({}): schema {}",
        match mode {
            CheckMode::Quick => "quick",
            CheckMode::Full => "full",
            CheckMode::Serials => "serials",
            CheckMode::Items => "items",
        },
        report.user_version
    );
    for note in &report.notes {
        println!("  note: {note}");
    }
    if report.is_clean() {
        println!("  no findings");
        return;
    }
    let width = report
        .findings
        .iter()
        .map(|f| f.check.len())
        .max()
        .unwrap_or(5)
        .max(5);
    println!("  {:<width$}  finding", "check");
    for f in &report.findings {
        println!("  {:<width$}  {}", f.check, f.message);
    }
    if report.suppressed > 0 {
        println!("  ... and {} more findings", report.suppressed);
    }
    println!(
        "{} finding(s)",
        report.findings.len() as u64 + report.suppressed
    );
}

// ---- vacuum ----------------------------------------------------------------------------------------------------

fn vacuum_cmd(cli: &Cli) -> ExitStatusCode {
    let st = match setup(cli) {
        Ok(s) => s,
        Err(c) => return c,
    };
    match vacuum(&st.persistence, &st.home, &clock()) {
        Ok(r) => {
            println!("backup:  {}", r.backup.display());
            println!(
                "vacuum:  {} -> {}",
                human_bytes(r.bytes_before),
                human_bytes(r.bytes_after)
            );
            exit::OK
        }
        Err(e) => {
            eprintln!("error: {e}");
            locked_exit(&e)
        }
    }
}

// ---- character undelete ------------------------------------------------------------------------------------------

/// `flyff character undelete <NAME|ID> [--slot N]`.
pub fn undelete(cli: &Cli, target: &str, slot: Option<u8>) -> ExitStatusCode {
    let st = match setup(cli) {
        Ok(s) => s,
        Err(c) => return c,
    };
    match undelete_character(&st.persistence, &st.home, target, slot) {
        Ok(r) => {
            println!(
                "restored character {} (#{}, account #{}) into slot {}{}",
                r.name,
                r.character.0,
                r.account.0,
                r.slot,
                if r.slot == r.previous_slot {
                    String::new()
                } else {
                    format!(" (it was in slot {})", r.previous_slot)
                }
            );
            // `char.undelete` appended directly to the audit log, actor cli (Plan/10 §9.2 CLI audit rule)
            let audit = AuditFile::open(&st.home, &st.persistence.audit, now().0);
            audit.emit(
                "char.undelete",
                &Actor {
                    account: Some(r.account),
                    character: Some(r.character),
                    name: Some(r.name.clone()),
                    ip: None,
                },
                serde_json::json!({
                    "by": "cli",
                    "slot": r.slot,
                    "previous_slot": r.previous_slot,
                    "target_account": r.account.0,
                    "deleted_at": r.deleted_at.map(|t| t.0),
                }),
            );
            audit.flush();
            exit::OK
        }
        Err(e) => {
            eprintln!("error: {}", e.context);
            locked_exit(&e)
        }
    }
}
