//! The binary-format corpus runner of `flyff check` (Plan/30 §17.1, WP-FMT-12; Plan/10 §9.2): every binary
//! file the VFS serves (`.o3d .chr .ani .lnd .dyo .sfx .dds .tga .bmp .cur`, `overrides/` winning over the
//! Resource) goes through [`flyff_formats::corpus::check`] on a few threads, and the per-kind tallies are
//! compared with [`flyff_formats::corpus::REFERENCE_COUNTS`] on request.
//!
//! * The text kinds (`.wld .rgn .wld.cnt .pat .24l`) are only counted, never read here: the game-data loader
//!   parses them and `flyff check` already prints its diagnostics.
//! * An unreadable file is a [`CorpusFailure`]; the three files of
//!   [`flyff_formats::corpus::KNOWN_UNREADABLE`] carry `known: true` and are not findings.
//! * The report does not depend on the thread count: the items go through the crate's order-preserving
//!   `par_map` (std scoped threads), which returns the results in input order, and the VFS iterates sorted
//!   by path.
//! * A parser panic (a bug, the parsers promise not to) is a failure ("parser panicked"), never a crash of
//!   the caller. An I/O error while reading is a failure with its text.
//!
//! No clock here (the crate forbids `Instant`): the caller measures the elapsed time.

use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};

use flyff_formats::FormatKind;
use flyff_formats::corpus::{self, KindTally, RefMismatch, Verdict};
use flyff_vfs::{Vfs, VfsEntry};

use crate::util::{panic_message, par_map};

/// One binary file that did not parse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorpusFailure {
    /// The normalised VFS key (lower case, `/`).
    pub path: String,
    /// The format the file name says.
    pub kind: FormatKind,
    /// The parser error, the I/O error or "parser panicked: ...".
    pub error: String,
    /// The file is in [`flyff_formats::corpus::KNOWN_UNREADABLE`]: listed and counted, not a finding.
    pub known: bool,
}

/// The result of [`binary_corpus`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinaryCorpusReport {
    /// Worker threads requested (at least 1; 0 behaves as 1).
    pub threads: usize,
    /// Binary files read (the parsed kinds; the counted-only text kinds are not included).
    pub files: u64,
    /// Total size of those files in bytes (the sizes actually read).
    pub bytes: u64,
    /// Per-format counts, text kinds included (`files` only); only formats that occur have an entry.
    pub tallies: BTreeMap<FormatKind, KindTally>,
    /// Every file that did not parse, known or not, sorted by VFS path.
    pub failures: Vec<CorpusFailure>,
}

impl BinaryCorpusReport {
    /// The findings: unreadable files that are not known.
    pub fn unreadable(&self) -> impl Iterator<Item = &CorpusFailure> {
        self.failures.iter().filter(|f| !f.known)
    }

    /// The known unreadable files (fine, but listed).
    pub fn known(&self) -> impl Iterator<Item = &CorpusFailure> {
        self.failures.iter().filter(|f| f.known)
    }

    /// The differences from [`flyff_formats::corpus::REFERENCE_COUNTS`] (empty = the reference Resource).
    pub fn reference_mismatches(&self) -> Vec<RefMismatch> {
        corpus::compare_reference(&self.tallies)
    }
}

/// The outcome of one file.
struct Item {
    kind: FormatKind,
    tally: KindTally,
    bytes: u64,
    failure: Option<CorpusFailure>,
}

/// Runs the corpus check over every binary file of `vfs` on up to `threads` worker threads (`0` = 1).
pub fn binary_corpus(vfs: &Vfs, threads: usize) -> BinaryCorpusReport {
    run(vfs, threads, corpus::check)
}

/// [`binary_corpus`] with the checker injected (the tests make it panic).
fn run<C>(vfs: &Vfs, threads: usize, checker: C) -> BinaryCorpusReport
where
    C: Fn(FormatKind, &str, &[u8], &mut KindTally) -> Verdict + Sync,
{
    let threads = threads.max(1);
    let mut tallies: BTreeMap<FormatKind, KindTally> = BTreeMap::new();
    let mut work: Vec<(FormatKind, &VfsEntry)> = Vec::new();
    for entry in vfs.iter() {
        let Some(kind) = FormatKind::from_path(entry.path.as_str()) else {
            continue;
        };
        if kind.is_corpus_parsed() {
            work.push((kind, entry));
        } else {
            // Counted only: the game-data loader parses the text files.
            let _ = corpus::check(
                kind,
                entry.path.as_str(),
                &[],
                tallies.entry(kind).or_default(),
            );
        }
    }

    let items = par_map(&work, threads, |&(kind, entry)| {
        check_one(kind, entry, &checker)
    });

    let mut report = BinaryCorpusReport {
        threads,
        files: 0,
        bytes: 0,
        tallies,
        failures: Vec::new(),
    };
    for item in items {
        report.files += 1;
        report.bytes = report.bytes.saturating_add(item.bytes);
        report
            .tallies
            .entry(item.kind)
            .or_default()
            .merge(&item.tally);
        report.failures.extend(item.failure);
    }
    report
}

fn check_one<C>(kind: FormatKind, entry: &VfsEntry, checker: &C) -> Item
where
    C: Fn(FormatKind, &str, &[u8], &mut KindTally) -> Verdict,
{
    let path = entry.path.as_str();
    let mut tally = KindTally::default();
    let fail = |tally: &mut KindTally, error: String, bytes: u64| {
        tally.files = 1;
        tally.ok = 0;
        tally.known = 0;
        tally.failed = 1;
        tally.facts.clear();
        Item {
            kind,
            tally: std::mem::take(tally),
            bytes,
            failure: Some(CorpusFailure {
                path: path.to_owned(),
                kind,
                error,
                known: false,
            }),
        }
    };
    let bytes = match std::fs::read(&entry.real) {
        Ok(b) => b,
        Err(e) => return fail(&mut tally, e.to_string(), 0),
    };
    let size = bytes.len() as u64;
    let verdict = catch_unwind(AssertUnwindSafe(|| checker(kind, path, &bytes, &mut tally)));
    match verdict {
        Ok(Verdict::Ok) => Item {
            kind,
            tally,
            bytes: size,
            failure: None,
        },
        Ok(Verdict::Known(e)) => Item {
            kind,
            tally,
            bytes: size,
            failure: Some(CorpusFailure {
                path: path.to_owned(),
                kind,
                error: e.to_string(),
                known: true,
            }),
        },
        Ok(Verdict::Failed(e)) => Item {
            kind,
            tally,
            bytes: size,
            failure: Some(CorpusFailure {
                path: path.to_owned(),
                kind,
                error: e.to_string(),
                known: false,
            }),
        },
        Err(payload) => fail(
            &mut tally,
            format!("parser panicked: {}", panic_message(payload.as_ref())),
            size,
        ),
    }
}
