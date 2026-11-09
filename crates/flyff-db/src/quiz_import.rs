//! The library behind `flyff quiz import` (WP-DB-15, Plan/13 §2.3, §12, 22 §15.1, 10 §9.2): imports quiz questions
//! into `quiz_questions` while the server runs.
//!
//! The TOML format and its parser belong to the quiz task (WP-EVENT-19), the CLI wiring to WP-OPS-17; this module is
//! the persistence half. [`import`] is ONE `BEGIN IMMEDIATE` transaction over the same op bodies as
//! `DbOp::UpsertQuizQuestion` and `DbOp::PruneQuizQuestions` (one invalid record rolls everything back, the error
//! names its index); [`import_into`] opens its own short connection like every CLI command that writes (13 §2.3 allows
//! it while the server runs: the busy timeout serialises it with the writer thread) and **never creates a database**.
//!
//! The import is idempotent: a question is identified by its natural key `(quiz_type, question)`, so the same file
//! twice inserts nothing the second time (`unchanged`). A changed answer, choice list, prize or `enabled` flag is an
//! update that keeps the id and `used_at`. With `replace` the questions of that type that are not in the file are
//! deleted; the other type is never touched.

use crate::config::DbConfig;
use crate::error::{DbError, DbErrorKind};
use crate::model::{QuizQuestionRecord, QuizType};
use crate::repo::houses::{self, QuizUpsert};
use rusqlite::{Connection, TransactionBehavior};
use std::path::Path;

/// What an import did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct QuizImportReport {
    /// New questions.
    pub inserted: u32,
    /// Existing questions whose choices, answer, prizes or `enabled` flag changed.
    pub updated: u32,
    /// Existing questions that already had these values.
    pub unchanged: u32,
    /// Questions deleted by `replace`.
    pub pruned: u32,
}

const ERROR_PREFIX: &str = "quiz import: record ";

/// The record index an [`import`] error names (`Invalid` for a record that fails
/// [`QuizQuestionRecord::validate`], and the errors of its write), `None` for any other error.
pub fn error_index(err: &DbError) -> Option<usize> {
    let rest = err.context.strip_prefix(ERROR_PREFIX)?;
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

fn at_record(index: usize, err: DbError) -> DbError {
    DbError::new(err.kind, format!("{ERROR_PREFIX}{index}: {}", err.context))
}

/// Imports `questions` into the open database `conn` in one `BEGIN IMMEDIATE` transaction. Every record is validated
/// first ([`QuizQuestionRecord::validate`], `Invalid` naming the index: nothing is written then); `replace =
/// Some(t)` deletes the questions of type `t` that are not in `questions` before the upserts and leaves the other type
/// alone. An error rolls everything back.
pub fn import(
    conn: &mut Connection,
    questions: &[QuizQuestionRecord],
    replace: Option<QuizType>,
) -> Result<QuizImportReport, DbError> {
    for (index, q) in questions.iter().enumerate() {
        q.validate().map_err(|why| {
            DbError::new(
                DbErrorKind::Invalid,
                format!("{ERROR_PREFIX}{index}: {why}"),
            )
        })?;
    }
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let mut report = QuizImportReport::default();
    if let Some(quiz_type) = replace {
        let keep: Vec<String> = questions
            .iter()
            .filter(|q| q.quiz_type == quiz_type)
            .map(|q| q.question.clone())
            .collect();
        report.pruned = houses::prune_quiz_questions(&tx, quiz_type, &keep)?;
    }
    for (index, q) in questions.iter().enumerate() {
        match houses::upsert_quiz_question(&tx, q).map_err(|e| at_record(index, e))? {
            QuizUpsert::Inserted(_) => report.inserted += 1,
            QuizUpsert::Updated => report.updated += 1,
            QuizUpsert::Unchanged => report.unchanged += 1,
        }
    }
    tx.commit()?;
    Ok(report)
}

/// Opens the database file at `db_path` with its own short connection ([`crate::conn::open_writer`]: the pragmas and
/// the `busy_timeout` of `cfg`, WITHOUT the instance lock, so it works while the server runs) and runs [`import`].
/// A missing file is `NotFound` (nothing is created), a fresh or older schema `Unsupported` ("start the server once or
/// run `flyff db migrate`"), a newer one `NewerSchema`. A SQLite `BUSY` after the timeout is `Transient`.
pub fn import_into(
    db_path: &Path,
    cfg: &DbConfig,
    questions: &[QuizQuestionRecord],
    replace: Option<QuizType>,
) -> Result<QuizImportReport, DbError> {
    if !db_path.is_file() {
        return Err(DbError::new(
            DbErrorKind::NotFound,
            format!("no database at {}", db_path.display()),
        ));
    }
    if std::fs::metadata(db_path)?.len() == 0 {
        // `open_writer` would stamp an empty file as a new database: an import never creates one
        return Err(DbError::new(
            DbErrorKind::Unsupported,
            format!(
                "{} is empty: start the server once or run `flyff db migrate`",
                db_path.display()
            ),
        ));
    }
    let opened = crate::conn::open_writer(db_path, cfg)?;
    let mut conn = opened.conn;
    let version = crate::migrate::user_version(&conn)?;
    crate::migrate::guard_version(version)?;
    if opened.fresh || version < i64::from(crate::migrate::latest_version()) {
        return Err(DbError::new(
            DbErrorKind::Unsupported,
            format!(
                "{} is at schema v{version}, expected v{}: start the server once or run `flyff db migrate`",
                db_path.display(),
                crate::migrate::latest_version()
            ),
        ));
    }
    import(&mut conn, questions, replace)
}
