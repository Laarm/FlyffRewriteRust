//! The quiz import file: format, parser and validator (Plan/22 §15.1, WP-EVENT-19; the command line is WP-OPS-17's
//! `flyff quiz import`, the persistence half is `flyff_db::quiz_import`).
//!
//! A pure library function: text in, records out. No game state, no SQL, no file access and no panic on operator
//! input, so the `flyff` CLI and a later admin import route call it alike.
//!
//! ```toml
//! [[question]]
//! type = "4c"                  # "ox" | "4c" (optional when the caller names the type)
//! question = "Which city is the capital of Madrigal's east?"
//! choices = ["Flaris", "Saint Morning", "Darkon", "Ricis"]   # exactly 4 for 4c, absent for ox
//! answer = 1                   # ox: 1 = O, 2 = X; 4c: 1..=4
//! enabled = true               # optional, default true
//! ```
//!
//! The only top-level key is the array of tables `[[question]]`; unknown keys are errors. EVERY error of the file is
//! collected (not only the first), each with the line of the entry's `[[question]]` header (a syntax error: the
//! line TOML names; a stray top-level key: its own line).

use flyff_db::{QuizQuestionRecord, QuizType};
use std::collections::BTreeMap;
use std::fmt;
use toml::de::{DeTable, DeValue};

/// `MAX_QUIZ_SIZE` (`Quiz.h`): the longest text of a question sent to the client, choices included, in bytes.
pub const MAX_SENT_BYTES: usize = 1024;

/// The keys an entry may carry (`deny_unknown_fields`).
const ENTRY_KEYS: [&str; 5] = ["type", "question", "choices", "answer", "enabled"];

/// One problem of an import file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuizParseError {
    /// 1-based line of the entry's `[[question]]` header (a syntax error: the line TOML names).
    pub line: usize,
    /// What is wrong, in the operator's words.
    pub message: String,
}

impl fmt::Display for QuizParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for QuizParseError {}

/// One valid entry of the file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedQuestion {
    /// 1-based line of the entry's `[[question]]` header: the CLI maps a database error at record `i` back to it.
    pub line: usize,
    /// The record (`id` 0, no prize, never used), already [`QuizQuestionRecord::validate`]d.
    pub record: QuizQuestionRecord,
}

/// What [`parse`] found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ParsedQuestions {
    /// The entries to import, in file order.
    pub questions: Vec<ParsedQuestion>,
    /// Entries of the other type when `only` was given (skipped, not validated).
    pub skipped_other_type: usize,
}

/// `"ox"` or `"4c"`: how the file and the messages spell a type.
pub const fn type_label(t: QuizType) -> &'static str {
    match t {
        QuizType::Ox => "ox",
        QuizType::FourChoice => "4c",
    }
}

/// The text the quiz sends for a question (`QuizQuestion.question`): the stored question with every `|` turned into a
/// line break (the legacy authoring convention; legacy `QUIZLIST::Serialize` made it `\r\n`, the browser gets `\n`),
/// and for a four-choice question `"\n"` plus the choices joined by `"\n"` (legacy `usp_Quiz_Load` joined question
/// and answers with `|`).
pub fn sent_text(q: &QuizQuestionRecord) -> String {
    let mut text = q.question.replace('|', "\n");
    if q.quiz_type == QuizType::FourChoice {
        for choice in &q.choices {
            text.push('\n');
            text.push_str(choice);
        }
    }
    text
}

/// Parses an import file. `only` is the type the caller imports (`--type`): an entry without `type` then defaults to
/// it and an entry of the other type is skipped and counted; without `only` every entry names its `type`.
///
/// # Errors
/// Every problem of the file, ascending by line: a TOML syntax error (one error), a top-level key other than
/// `[[question]]`, a malformed entry (the keys, the limits of [`QuizQuestionRecord::validate`], a sent text over
/// [`MAX_SENT_BYTES`]), a `(type, question)` twice (the second names the first), a file without any entry, a file
/// without an entry of the selected type.
pub fn parse(text: &str, only: Option<QuizType>) -> Result<ParsedQuestions, Vec<QuizParseError>> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let doc = match DeTable::parse(text) {
        Ok(doc) => doc.into_inner(),
        Err(e) => {
            let line = e.span().map_or(1, |s| line_of(text, s.start));
            return Err(vec![QuizParseError {
                line,
                message: e.message().to_owned(),
            }]);
        }
    };
    let mut errors = Vec::new();
    let mut entries: Vec<(usize, &DeTable<'_>)> = Vec::new();
    for (key, value) in &doc {
        if key_str(key) != "question" {
            errors.push(QuizParseError {
                line: line_of(text, key.span().start),
                message: format!(
                    "unknown top-level key `{}` (the file holds only [[question]] entries)",
                    key_str(key)
                ),
            });
            continue;
        }
        match value.as_ref() {
            DeValue::Array(items) => {
                for item in items {
                    match item.as_ref() {
                        DeValue::Table(table) => {
                            entries.push((entry_line(text, item.span().start), table));
                        }
                        other => errors.push(QuizParseError {
                            line: line_of(text, item.span().start),
                            message: format!(
                                "each `question` must be a table, not {}",
                                other.type_str()
                            ),
                        }),
                    }
                }
            }
            _ => errors.push(QuizParseError {
                line: line_of(text, key.span().start),
                message: "write the entries as [[question]], one table per question".to_owned(),
            }),
        }
    }
    let mut out = ParsedQuestions::default();
    let mut seen: BTreeMap<(u8, String), usize> = BTreeMap::new();
    for (line, table) in entries {
        match read_entry(table, only) {
            Entry::Skipped => out.skipped_other_type += 1,
            Entry::Bad(messages) => {
                errors.extend(
                    messages
                        .into_iter()
                        .map(|message| QuizParseError { line, message }),
                );
            }
            Entry::Ok(record) => {
                let key = (record.quiz_type.code(), record.question.clone());
                if let Some(first) = seen.get(&key) {
                    errors.push(QuizParseError {
                        line,
                        message: format!(
                            "duplicate of the {} question on line {first}",
                            type_label(record.quiz_type)
                        ),
                    });
                } else {
                    seen.insert(key, line);
                    out.questions.push(ParsedQuestion { line, record });
                }
            }
        }
    }
    if errors.is_empty() && out.questions.is_empty() {
        errors.push(QuizParseError {
            line: 1,
            message: match only {
                Some(t) if out.skipped_other_type > 0 => {
                    format!("no [[question]] of type {} in the file", type_label(t))
                }
                _ => "no [[question]] in the file".to_owned(),
            },
        });
    }
    if errors.is_empty() {
        Ok(out)
    } else {
        errors.sort_by_key(|e| e.line);
        Err(errors)
    }
}

enum Entry {
    Ok(QuizQuestionRecord),
    Skipped,
    Bad(Vec<String>),
}

/// Reads one `[[question]]` table; every problem of the entry is returned, not only the first.
fn read_entry(table: &DeTable<'_>, only: Option<QuizType>) -> Entry {
    let quiz_type = match read_type(table, only) {
        Ok(t) => t,
        Err(why) => return Entry::Bad(vec![why]),
    };
    if only.is_some_and(|t| t != quiz_type) {
        return Entry::Skipped;
    }
    let mut problems = Vec::new();
    for (key, _) in table {
        if !ENTRY_KEYS.contains(&key_str(key)) {
            problems.push(format!(
                "unknown key `{}` (allowed: {})",
                key_str(key),
                ENTRY_KEYS.join(", ")
            ));
        }
    }
    let question = match str_value(table, "question") {
        Ok(Some(q)) if q.trim().is_empty() => {
            problems.push("the question must not be blank".to_owned());
            String::new()
        }
        Ok(Some(q)) => q,
        Ok(None) => {
            problems.push("missing key `question`".to_owned());
            String::new()
        }
        Err(why) => {
            problems.push(why);
            String::new()
        }
    };
    let choices = match read_choices(table) {
        Ok(c) => c,
        Err(why) => {
            problems.push(why);
            Vec::new()
        }
    };
    let answer = match read_answer(table, quiz_type) {
        Ok(a) => a,
        Err(why) => {
            problems.push(why);
            0
        }
    };
    let enabled = match table.get("enabled").map(|v| v.as_ref()) {
        None => true,
        Some(DeValue::Boolean(b)) => *b,
        Some(other) => {
            problems.push(format!(
                "`enabled` must be true or false, not {}",
                other.type_str()
            ));
            true
        }
    };
    if !problems.is_empty() {
        return Entry::Bad(problems);
    }
    let record = QuizQuestionRecord {
        id: 0,
        quiz_type,
        question,
        choices,
        answer,
        prize_item_id: 0,
        prize_count: 0,
        enabled,
        used_at: None,
    };
    match check_record(&record) {
        Ok(()) => Entry::Ok(record),
        Err(why) => Entry::Bad(vec![why]),
    }
}

/// The limits of the record plus the ones of the client: single-line, non-blank choices and a sent text within
/// [`MAX_SENT_BYTES`].
fn check_record(record: &QuizQuestionRecord) -> Result<(), String> {
    record.validate()?;
    for (i, choice) in record.choices.iter().enumerate() {
        if choice.trim().is_empty() {
            return Err(format!("choice {} must not be blank", i + 1));
        }
        if choice.contains(['\n', '\r']) {
            return Err(format!("choice {} must be a single line", i + 1));
        }
    }
    let sent = sent_text(record).len();
    if sent > MAX_SENT_BYTES {
        return Err(format!(
            "the question and its choices are {sent} bytes when sent to the client, at most {MAX_SENT_BYTES} fit"
        ));
    }
    Ok(())
}

/// The entry's type: its `type` key, else `only`.
fn read_type(table: &DeTable<'_>, only: Option<QuizType>) -> Result<QuizType, String> {
    match table.get("type").map(|v| v.as_ref()) {
        None => only.ok_or_else(|| "missing key `type` (\"ox\" or \"4c\")".to_owned()),
        Some(DeValue::String(s)) => match s.to_ascii_lowercase().as_str() {
            "ox" => Ok(QuizType::Ox),
            "4c" => Ok(QuizType::FourChoice),
            _ => Err(format!("unknown type \"{s}\" (expected \"ox\" or \"4c\")")),
        },
        Some(other) => Err(format!(
            "`type` must be the string \"ox\" or \"4c\", not {}",
            other.type_str()
        )),
    }
}

/// The name of a key.
fn key_str<'a>(key: &'a toml::Spanned<std::borrow::Cow<'_, str>>) -> &'a str {
    key.get_ref()
}

fn str_value(table: &DeTable<'_>, key: &str) -> Result<Option<String>, String> {
    match table.get(key).map(|v| v.as_ref()) {
        None => Ok(None),
        Some(DeValue::String(s)) => Ok(Some(s.to_string())),
        Some(other) => Err(format!(
            "`{key}` must be a string, not {}",
            other.type_str()
        )),
    }
}

fn read_choices(table: &DeTable<'_>) -> Result<Vec<String>, String> {
    match table.get("choices").map(|v| v.as_ref()) {
        None => Ok(Vec::new()),
        Some(DeValue::Array(items)) => items
            .iter()
            .enumerate()
            .map(|(i, item)| match item.as_ref() {
                DeValue::String(s) => Ok(s.to_string()),
                other => Err(format!(
                    "choice {} must be a string, not {}",
                    i + 1,
                    other.type_str()
                )),
            })
            .collect(),
        Some(other) => Err(format!(
            "`choices` must be an array of strings, not {}",
            other.type_str()
        )),
    }
}

fn read_answer(table: &DeTable<'_>, quiz_type: QuizType) -> Result<u8, String> {
    let range = match quiz_type {
        QuizType::Ox => "1 = O, 2 = X",
        QuizType::FourChoice => "1..=4",
    };
    match table.get("answer").map(|v| v.as_ref()) {
        None => Err(format!("missing key `answer` ({range})")),
        Some(DeValue::Integer(i)) => {
            let n = i64::from_str_radix(i.as_str(), i.radix()).ok();
            match n.and_then(|n| u8::try_from(n).ok()) {
                Some(a) => Ok(a),
                None => Err(format!(
                    "the answer {} is out of range ({range})",
                    i.as_str()
                )),
            }
        }
        Some(other) => Err(format!(
            "`answer` must be an integer ({range}), not {}",
            other.type_str()
        )),
    }
}

/// 1-based line of a byte offset.
fn line_of(text: &str, offset: usize) -> usize {
    let end = offset.min(text.len());
    1 + text.as_bytes()[..end]
        .iter()
        .filter(|&&b| b == b'\n')
        .count()
}

/// The line of an entry: the line of its span start, moved back to its `[[question]]` header when the span starts at
/// the entry's first key (the header line, skipping blank and comment lines in between).
fn entry_line(text: &str, start: usize) -> usize {
    let start = start.min(text.len());
    let line = line_of(text, start);
    let line_start = text.as_bytes()[..start]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |i| i + 1);
    if !text[line_start..start].trim().is_empty() {
        return line;
    }
    let mut n = line;
    for previous in text[..line_start].lines().rev() {
        n -= 1;
        let t = previous.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        return if t.starts_with("[[") { n } else { line };
    }
    line
}
