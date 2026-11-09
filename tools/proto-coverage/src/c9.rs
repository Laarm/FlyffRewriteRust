//! Rule C9: the client definition of done for the legacy send functions (Plan/02 section 9.6, WP-TEST-54).
//!
//! Every function of `ui_send_sites.csv` that the browser client sends must carry a marker comment in `web/src`:
//!
//! ```text
//! // legacy-send: SendChat
//! // legacy-send: SendMeleeAttack, SendMeleeAttack2
//! ```
//!
//! A marker is a whole line: blanks, `//`, blanks, `legacy-send:`, then one or more identifiers
//! (`[A-Za-z_][A-Za-z0-9_]*`) separated by commas, nothing else. A line that starts with `// legacy-send:` (or a near
//! miss such as `// legacy_send: X`) but does not fit that grammar is *malformed*; the same text anywhere else in a line
//! (a JSDoc quote, a trailing remark, a string) is prose and ignored. `*.d.ts` files, `*.test.ts(x)` files and
//! `node_modules` are not scanned: a test cannot make a send function "done".
//!
//! [`check`] is a pure function of the rows, the markers and the current milestone, so the unit fixtures need no tree:
//!
//! | finding | severity |
//! |---|---|
//! | a `done` row without a marker | error |
//! | a marker naming a `send_fn` that is not a row (the message names the nearest row by edit distance) | error |
//! | a malformed marker | error |
//! | a `todo` row without a marker whose milestone is at or below the current one | warning before M8, error from M8 |
//! | a `todo` row without a parsable milestone | error |
//! | a `todo` row that already has a marker (run `--sync-sites`) | warning |
//! | a marker on a `dropped` row | warning |

use crate::checks::{Finding, Severity};
use crate::csv::{SiteStatus, UiSite};
use crate::import::milestone_rank;
use anyhow::{Context, Result};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// The check id of the findings.
pub const CHECK: &str = "C9";
/// From this milestone on a due `todo` row is an error (before: a warning), Plan/02 section 9.6.
pub const ERROR_FROM_MILESTONE: u32 = 8;
/// The largest edit distance at which a marker name gets a suggestion.
pub const SUGGEST_DISTANCE: usize = 3;
/// The repository-relative name of the sites file in finding locations.
const SITES_FILE: &str = "proto/legacy/ui_send_sites.csv";

/// One marker line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Marker {
    /// Path relative to the repository root, `/` separated.
    pub file: String,
    /// 1-based line.
    pub line: usize,
    /// The names of the line, in order.
    pub names: Vec<String>,
}

/// A line that claims to be a marker but is not one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Malformed {
    /// Path relative to the repository root.
    pub file: String,
    /// 1-based line.
    pub line: usize,
    /// The trimmed line.
    pub text: String,
}

/// What a scan of `web/src` found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Markers {
    /// Well-formed markers in file and line order.
    pub found: Vec<Marker>,
    /// Malformed marker lines.
    pub malformed: Vec<Malformed>,
    /// Source files read.
    pub files_scanned: usize,
    /// `*.test.ts(x)` files that were not read.
    pub test_files_skipped: usize,
}

impl Markers {
    /// Every name of every marker line.
    pub fn names(&self) -> BTreeSet<&str> {
        self.found
            .iter()
            .flat_map(|m| m.names.iter().map(String::as_str))
            .collect()
    }

    /// Number of distinct source files that carry a marker.
    pub fn marker_files(&self) -> usize {
        self.found
            .iter()
            .map(|m| m.file.as_str())
            .collect::<BTreeSet<_>>()
            .len()
    }

    /// The first marker line of a name, as `file:line`.
    fn first_site(&self, name: &str) -> Option<String> {
        self.found
            .iter()
            .find(|m| m.names.iter().any(|n| n == name))
            .map(|m| format!("{}:{}", m.file, m.line))
    }
}

/// What one source line is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Line {
    /// Not a marker (and not trying to be one).
    Prose,
    /// A well-formed marker with its names.
    Marker(Vec<String>),
    /// Starts like a marker but does not fit the grammar.
    Malformed,
}

fn is_identifier(s: &str) -> bool {
    let mut c = s.chars();
    c.next()
        .is_some_and(|f| f.is_ascii_alphabetic() || f == '_')
        && c.all(|x| x.is_ascii_alphanumeric() || x == '_')
}

/// `legacy-send:` in any case, with `-`, `_`, a blank or nothing between the words and blanks before the colon: the
/// near misses that must not pass silently.
fn near_miss(rest: &str) -> bool {
    let Some(head) = rest.get(..6) else {
        return false;
    };
    if !head.eq_ignore_ascii_case("legacy") {
        return false;
    }
    let mut tail = &rest[6..];
    if tail.starts_with(['-', '_', ' ']) {
        tail = &tail[1..];
    }
    let Some(head) = tail.get(..4) else {
        return false;
    };
    head.eq_ignore_ascii_case("send") && tail[4..].trim_start().starts_with(':')
}

/// Classifies one line (see the module documentation for the grammar).
pub fn parse_line(line: &str) -> Line {
    let Some(rest) = line.trim().strip_prefix("//") else {
        return Line::Prose;
    };
    let rest = rest.trim_start();
    let Some(list) = rest.strip_prefix("legacy-send:") else {
        return if near_miss(rest) {
            Line::Malformed
        } else {
            Line::Prose
        };
    };
    let names: Vec<&str> = list.split(',').map(str::trim).collect();
    if names.iter().all(|n| is_identifier(n)) {
        Line::Marker(names.into_iter().map(str::to_owned).collect())
    } else {
        Line::Malformed
    }
}

/// Scans the text of one file.
pub fn scan_text(file: &str, text: &str, out: &mut Markers) {
    for (i, line) in text.lines().enumerate() {
        match parse_line(line) {
            Line::Prose => {}
            Line::Marker(names) => out.found.push(Marker {
                file: file.to_owned(),
                line: i + 1,
                names,
            }),
            Line::Malformed => out.malformed.push(Malformed {
                file: file.to_owned(),
                line: i + 1,
                text: line.trim().to_owned(),
            }),
        }
    }
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>, tests: &mut usize) -> Result<()> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .with_context(|| format!("read {}", dir.display()))?
        .filter_map(Result::ok)
        .collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for e in entries {
        let name = e.file_name().to_string_lossy().into_owned();
        let Ok(kind) = e.file_type() else { continue };
        if kind.is_dir() {
            if name != "node_modules" {
                walk(&e.path(), out, tests)?;
            }
        } else if kind.is_file()
            && (name.ends_with(".ts") || name.ends_with(".tsx"))
            && !name.ends_with(".d.ts")
        {
            if name.ends_with(".test.ts") || name.ends_with(".test.tsx") {
                *tests += 1;
            } else {
                out.push(e.path());
            }
        }
    }
    Ok(())
}

/// Scans `web_src` (a directory below `root`) for markers.
pub fn scan(root: &Path, web_src: &Path) -> Result<Markers> {
    let mut files = Vec::new();
    let mut out = Markers::default();
    walk(web_src, &mut files, &mut out.test_files_skipped)?;
    for path in files {
        let bytes = std::fs::read(&path).with_context(|| format!("read {}", path.display()))?;
        let rel = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        scan_text(&rel, &String::from_utf8_lossy(&bytes), &mut out);
        out.files_scanned += 1;
    }
    Ok(out)
}

/// Levenshtein distance of two strings (by `char`).
pub fn edit_distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut cur = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            let sub = prev[j] + usize::from(ca != *cb); // BOUNDS: prev and cur have b.len() + 1 cells
            let del = prev[j + 1] + 1; // BOUNDS: as above
            let ins = cur[j] + 1; // BOUNDS: cur has j + 1 cells at this point
            cur.push(sub.min(del).min(ins));
        }
        prev = cur;
    }
    prev[b.len()] // BOUNDS: prev has b.len() + 1 cells
}

/// The row name nearest to `name` (case-insensitive edit distance of at most [`SUGGEST_DISTANCE`]).
pub fn nearest<'a>(name: &str, rows: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    let lower = name.to_lowercase();
    let mut best: Option<(usize, &str)> = None;
    for row in rows {
        let d = edit_distance(&lower, &row.to_lowercase());
        if d <= SUGGEST_DISTANCE && best.is_none_or(|(b, _)| d < b) {
            best = Some((d, row));
        }
    }
    best.map(|(_, r)| r)
}

/// A marker or a malformed line as printed.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct MarkerRef {
    /// The name (unknown markers) or the line text (malformed ones).
    pub text: String,
    /// `web/src/...:LINE`.
    pub location: String,
    /// The nearest row name, for an unknown marker.
    pub suggestion: Option<String>,
}

/// The `c9` section of the report.
#[derive(Serialize, Clone, Debug, Default)]
pub struct Summary {
    /// False when the rule was not evaluated (no `web/src`).
    pub ran: bool,
    /// Why it was not evaluated.
    pub note: String,
    /// `M5`: the milestone the due rows are measured against.
    pub current_milestone: String,
    /// `M8`: from there on a due row is an error.
    pub error_from_milestone: String,
    /// Rows per status, then per milestone (`-` when the row has none).
    pub rows_by_status: BTreeMap<String, BTreeMap<String, usize>>,
    /// Marker lines found.
    pub marker_lines: usize,
    /// Distinct names in them.
    pub distinct_names: usize,
    /// Source files that carry a marker.
    pub marker_files: usize,
    /// Source files read.
    pub files_scanned: usize,
    /// Test files that were not read.
    pub test_files_skipped: usize,
    /// `todo` rows that already have a marker (`--sync-sites` turns them into `done`).
    pub todo_with_marker: Vec<String>,
    /// Markers naming no row.
    pub unknown_markers: Vec<MarkerRef>,
    /// Malformed marker lines.
    pub malformed_markers: Vec<MarkerRef>,
    /// `todo` rows without a marker per milestone.
    pub todo_by_milestone: BTreeMap<String, usize>,
    /// The names of the `todo` rows without a marker that are due by the current milestone, per milestone.
    pub due: BTreeMap<String, Vec<String>>,
}

impl Summary {
    /// The summary of a run that did not evaluate the rule.
    pub fn skipped(note: impl Into<String>) -> Summary {
        Summary {
            note: note.into(),
            ..Summary::default()
        }
    }
}

fn finding(
    severity: Severity,
    subject: impl Into<String>,
    detail: impl Into<String>,
    loc: Option<String>,
) -> Finding {
    Finding {
        check: CHECK,
        severity,
        subject: subject.into(),
        detail: detail.into(),
        loc,
    }
}

/// Rule C9 over the rows and the markers of a scan; `current` is the rank of the current milestone
/// (`PROTO_STUB_MAX_MILESTONE`, else `current_milestone` of `docs/wp-status.toml`).
pub fn check(sites: &[UiSite], markers: &Markers, current: u32) -> (Summary, Vec<Finding>) {
    let mut out = Vec::new();
    let names = markers.names();
    let rows: BTreeSet<&str> = sites.iter().map(|s| s.send_fn.as_str()).collect();
    let mut s = Summary {
        ran: true,
        current_milestone: format!("M{current}"),
        error_from_milestone: format!("M{ERROR_FROM_MILESTONE}"),
        marker_lines: markers.found.len(),
        distinct_names: names.len(),
        marker_files: markers.marker_files(),
        files_scanned: markers.files_scanned,
        test_files_skipped: markers.test_files_skipped,
        ..Summary::default()
    };

    for m in &markers.malformed {
        let location = format!("{}:{}", m.file, m.line);
        out.push(finding(
            Severity::Error,
            &m.text,
            format!(
                "malformed marker `{}`: write `// legacy-send: SendName` or `// legacy-send: SendA, SendB` (one line, identifiers separated by commas, nothing else)",
                m.text
            ),
            Some(location.clone()),
        ));
        s.malformed_markers.push(MarkerRef {
            text: m.text.clone(),
            location,
            suggestion: None,
        });
    }

    for m in &markers.found {
        for name in &m.names {
            if rows.contains(name.as_str()) {
                continue;
            }
            let location = format!("{}:{}", m.file, m.line);
            let suggestion = nearest(name, rows.iter().copied()).map(str::to_owned);
            let hint = match &suggestion {
                Some(near) => format!("; did you mean `{near}`?"),
                None => format!(
                    "; no send function is within edit distance {SUGGEST_DISTANCE} (add a row through an ADR when the packet is real)"
                ),
            };
            out.push(finding(
                Severity::Error,
                name,
                format!(
                    "{location}: the marker names `{name}`, which is not a send_fn of ui_send_sites.csv{hint}"
                ),
                Some(location.clone()),
            ));
            s.unknown_markers.push(MarkerRef {
                text: name.clone(),
                location,
                suggestion,
            });
        }
    }

    let due_is_error = current >= ERROR_FROM_MILESTONE;
    for (i, site) in sites.iter().enumerate() {
        let ms = if site.milestone.is_empty() {
            "-"
        } else {
            site.milestone.as_str()
        };
        *s.rows_by_status
            .entry(site.status.tag().to_owned())
            .or_default()
            .entry(ms.to_owned())
            .or_default() += 1;
        let marked = names.contains(site.send_fn.as_str());
        let row_loc = Some(format!("{SITES_FILE}:{}", i + 2));
        match site.status {
            SiteStatus::Done if !marked => out.push(finding(
                Severity::Error,
                &site.send_fn,
                "a `done` row has no `// legacy-send:` marker under web/src (write the marker above the send, or set the row back to `todo`)",
                row_loc,
            )),
            SiteStatus::Done => {}
            SiteStatus::Dropped if marked => out.push(finding(
                Severity::Warning,
                &site.send_fn,
                format!(
                    "a marker names a `dropped` row ({}): the packet has no replacement message",
                    markers.first_site(&site.send_fn).unwrap_or_default()
                ),
                markers.first_site(&site.send_fn),
            )),
            SiteStatus::Dropped => {}
            SiteStatus::Todo if marked => {
                s.todo_with_marker.push(site.send_fn.clone());
                out.push(finding(
                    Severity::Warning,
                    &site.send_fn,
                    format!(
                        "a marker exists ({}) but the row is still `todo`: run `cargo xtask coverage proto --sync-sites`",
                        markers.first_site(&site.send_fn).unwrap_or_default()
                    ),
                    row_loc,
                ));
            }
            SiteStatus::Todo => {
                *s.todo_by_milestone.entry(ms.to_owned()).or_default() += 1;
                let rank = milestone_rank(&site.milestone);
                if rank == u32::MAX {
                    out.push(finding(
                        Severity::Error,
                        &site.send_fn,
                        format!(
                            "a `todo` row needs a milestone of the form M<n>, found `{}`",
                            site.milestone
                        ),
                        row_loc,
                    ));
                } else if rank <= current {
                    s.due
                        .entry(site.milestone.clone())
                        .or_default()
                        .push(site.send_fn.clone());
                    out.push(finding(
                        if due_is_error {
                            Severity::Error
                        } else {
                            Severity::Warning
                        },
                        &site.send_fn,
                        format!(
                            "a `todo` row of {} is due at {} and has no `// legacy-send:` marker under web/src (an error from M{ERROR_FROM_MILESTONE})",
                            site.milestone, s.current_milestone
                        ),
                        row_loc,
                    ));
                }
            }
        }
    }
    (s, out)
}

/// `--sync-sites`: sets `done` on every `todo` row that has a marker (never the reverse) and returns the names.
pub fn sync(sites: &mut [UiSite], markers: &Markers) -> Vec<String> {
    let names = markers.names();
    let mut flipped = Vec::new();
    for site in sites.iter_mut() {
        if site.status == SiteStatus::Todo && names.contains(site.send_fn.as_str()) {
            site.status = SiteStatus::Done;
            flipped.push(site.send_fn.clone());
        }
    }
    flipped
}
