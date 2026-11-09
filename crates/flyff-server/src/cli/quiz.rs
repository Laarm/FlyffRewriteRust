//! `flyff quiz import <FILE> [--type N] [--replace]` (Plan/10 §9.2, 22 §15.1; WP-OPS-17).
//!
//! The file format and its parser are `flyff_game::event::quiz::import` (a pure library function, line numbers in
//! every error); the write is `flyff_db::quiz_import::import_into` (ONE `BEGIN IMMEDIATE` transaction on the
//! command's own short connection). The command takes no instance lock (lock column "–" of the §9.2 table; 13 §2.3
//! allows the write while the server runs: SQLite serialises it with the writer thread through `busy_timeout`).
//!
//! | outcome | exit | output |
//! |---|---|---|
//! | imported | 0 | `quiz import: <FILE>: <n> question(s)[, <k> skipped (other type)]: inserted a, updated b, unchanged c, pruned d` |
//! | unreadable file, a malformed file (every error is listed as `error: <FILE>:<line>: <message>`, nothing is imported and the database is not even opened), no database, an old schema, SQLite busy | 1 | `error: ...` |
//! | a bad configuration | 78 | the configuration errors |
//! | clap usage errors (`--type` outside 1..=2, `--replace` without `--type`) | 64 | clap's message |
//!
//! `--type N` imports only the entries of that type (an entry may leave out its `type` key then) and `--replace` also
//! deletes the questions of that type that are not in the file (the other type is never touched; the ids of the
//! questions that stay are kept). The same file twice reports `inserted n` and then `unchanged n`. The command appends
//! a `quiz.import` line to the audit log (actor `cli`, Plan/10 §9.2 "CLI audit rule").

#![allow(clippy::print_stdout, clippy::print_stderr)]

use super::{Cli, QuizCmd, env_pairs, home_of, overrides_of, prepare::init_cli_logging};
use crate::audit::{Actor, AuditFile};
use crate::config;
use crate::exit::{self, ExitStatusCode};
use flyff_db::quiz_import::{QuizImportReport, error_index, import_into};
use flyff_db::{DbError, DbErrorKind, PersistenceConfig, QuizQuestionRecord, QuizType};
use flyff_game::event::quiz::import::{ParsedQuestion, QuizParseError, parse};
use std::io::Write;
use std::path::Path;

/// `flyff quiz ...` (the subcommand body of [`super::run`]).
pub fn run(cli: &Cli, cmd: &QuizCmd) -> ExitStatusCode {
    match cmd {
        QuizCmd::Import {
            file,
            quiz_type,
            replace,
        } => import(cli, file, *quiz_type, *replace),
    }
}

/// What an import needs from the command line.
struct Job<'a> {
    /// The file as typed (the name that errors and the audit line carry).
    file: &'a Path,
    /// `--type`.
    only: Option<QuizType>,
    /// `--replace`.
    replace: bool,
}

fn import(cli: &Cli, file: &Path, quiz_type: Option<u32>, replace: bool) -> ExitStatusCode {
    init_cli_logging();
    let home = match home_of(cli) {
        Ok(h) => h,
        Err(c) => return c,
    };
    let loaded = match config::load(&home, &overrides_of(cli), &env_pairs(), &[]) {
        Ok(l) => l,
        Err(errs) => {
            for e in errs {
                eprintln!("error: {e}");
            }
            return exit::CONFIG;
        }
    };
    for w in &loaded.warnings {
        eprintln!("warning: {w}");
    }
    let persistence = loaded.config.persistence();
    // clap limits `--type` to 1..=2; a caller that builds the command itself gets the usage code
    let only = match quiz_type {
        None => None,
        Some(code) => match u8::try_from(code).ok().and_then(QuizType::from_code) {
            Some(t) => Some(t),
            None => {
                eprintln!("error: --type must be 1 (OX) or 2 (four choices), not {code}");
                return exit::USAGE;
            }
        },
    };
    let job = Job {
        file,
        only,
        replace,
    };
    match execute(
        &home,
        &persistence,
        &job,
        &mut std::io::stdout(),
        &mut std::io::stderr(),
    ) {
        Ok(_) => exit::OK,
        Err(code) => code,
    }
}

/// Reads and parses the file, imports it and writes the audit line; the report of the import, or the exit code of the
/// failure. `out` and `err` are stdout and stderr (tests capture them).
fn execute(
    home: &Path,
    persistence: &PersistenceConfig,
    job: &Job<'_>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<QuizImportReport, ExitStatusCode> {
    let name = job.file.display().to_string();
    if job.replace && job.only.is_none() {
        let _ = writeln!(err, "error: --replace needs --type");
        return Err(exit::USAGE);
    }
    let text = match std::fs::read_to_string(job.file) {
        Ok(t) => t,
        Err(e) => {
            let _ = writeln!(err, "error: cannot read {name}: {e}");
            return Err(exit::FAILURE);
        }
    };
    let parsed = match parse(&text, job.only) {
        Ok(p) => p,
        Err(errors) => {
            for line in parse_failure_lines(&name, &errors) {
                let _ = writeln!(err, "{line}");
            }
            return Err(exit::FAILURE);
        }
    };
    let records: Vec<QuizQuestionRecord> =
        parsed.questions.iter().map(|q| q.record.clone()).collect();
    let db_path = home.join(&persistence.database.path);
    let replace_type = if job.replace { job.only } else { None };
    let report = match import_into(&db_path, &persistence.database, &records, replace_type) {
        Ok(r) => r,
        Err(e) => {
            for line in db_failure_lines(&name, &db_path, &e, &parsed.questions) {
                let _ = writeln!(err, "{line}");
            }
            return Err(exit::FAILURE);
        }
    };
    let _ = writeln!(
        out,
        "{}",
        success_line(&name, records.len(), parsed.skipped_other_type, &report)
    );
    // `quiz.import` appended directly to the audit log, actor cli (Plan/10 §9.2 CLI audit rule)
    let audit = AuditFile::open(home, &persistence.audit, now_secs());
    let event = flyff_db::audit::content::quiz_import(&name, &report, replace_type);
    audit.emit(event.ev, &Actor::none(), event.data);
    audit.flush();
    Ok(report)
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// The stderr lines of a file that did not parse: one `error: <FILE>:<line>: <message>` per problem and the summary.
fn parse_failure_lines(file: &str, errors: &[QuizParseError]) -> Vec<String> {
    let mut lines: Vec<String> = errors
        .iter()
        .map(|e| format!("error: {file}:{}: {}", e.line, e.message))
        .collect();
    lines.push(format!(
        "error: nothing imported ({} error(s))",
        errors.len()
    ));
    lines
}

/// The stdout line of a finished import.
fn success_line(file: &str, questions: usize, skipped: usize, r: &QuizImportReport) -> String {
    let skipped = if skipped > 0 {
        format!(", {skipped} skipped (other type)")
    } else {
        String::new()
    };
    format!(
        "quiz import: {file}: {questions} question(s){skipped}: inserted {}, updated {}, unchanged {}, pruned {}",
        r.inserted, r.updated, r.unchanged, r.pruned
    )
}

/// The stderr lines of a database refusal. An `Invalid` error at record `i` (the second line of defence behind the
/// parser) is printed with the line of record `i` in the file.
fn db_failure_lines(
    file: &str,
    db_path: &Path,
    err: &DbError,
    questions: &[ParsedQuestion],
) -> Vec<String> {
    let mut lines = Vec::new();
    match err.kind {
        DbErrorKind::NotFound => lines.push(format!(
            "error: no database at {} (run `flyff db migrate` or start the server once)",
            db_path.display()
        )),
        DbErrorKind::Invalid => {
            let located = error_index(err).and_then(|i| questions.get(i));
            match located {
                Some(q) => {
                    let why = err
                        .context
                        .split_once(": ")
                        .and_then(|(_, rest)| rest.split_once(": "))
                        .map_or(err.context.as_str(), |(_, why)| why);
                    lines.push(format!("error: {file}:{}: {why}", q.line));
                }
                None => lines.push(format!("error: {}", err.context)),
            }
        }
        DbErrorKind::Transient => lines.push(format!(
            "error: {} (the database is busy: try again in a moment)",
            err.context
        )),
        _ => lines.push(format!("error: {}", err.context)),
    }
    lines.push("error: nothing imported".to_owned());
    lines
}
