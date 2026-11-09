//! The one-off importers (Plan/11 WP-PROTO-04 / WP-PROTO-05): annex 11a sections 2-4 and 7 to `legacy_index.csv`, and
//! ref/99 appendix B to `ui_send_sites.csv`. They parse the markdown tables of the plan documents, check the
//! row counts against the totals table of the annex and are idempotent (the CSV is a pure function of the markdown).

use crate::csv::{Dir, HTTP_FILE, LegacyRow, Ns, SiteStatus, Status, UiSite};
use anyhow::{Context, Result, bail};
use std::collections::BTreeSet;

/// Splits a markdown table row into trimmed cells; `None` for separators and non-table lines.
fn cells(line: &str) -> Option<Vec<String>> {
    let t = line.trim();
    if !t.starts_with('|') || t.starts_with("|---") || t.starts_with("| ---") {
        return None;
    }
    let inner = t.strip_prefix('|')?.strip_suffix('|').unwrap_or(&t[1..]);
    Some(inner.split('|').map(|c| c.trim().to_owned()).collect())
}

/// The define name of a "Legacy name" cell: the first word (`ITEMTRANSY (`PACHETTYPE_`)` is `ITEMTRANSY`).
fn define_name(cell: &str) -> String {
    cell.split_whitespace().next().unwrap_or(cell).to_owned()
}

/// The text between the first pair of backticks, or the trimmed input.
fn first_tick(s: &str) -> &str {
    let mut it = s.split('`');
    it.next();
    it.next().unwrap_or(s).trim()
}

/// All backtick-quoted words of a cell.
fn ticks(s: &str) -> Vec<String> {
    s.split('`')
        .enumerate()
        .filter(|(i, w)| i % 2 == 1 && !w.is_empty())
        .map(|(_, w)| w.trim().to_owned())
        .collect()
}

fn dash(s: &str) -> String {
    if s == "—" || s == "-" {
        String::new()
    } else {
        s.to_owned()
    }
}

/// `port — rename scroll` -> (`port`, `rename scroll`).
fn status_note(cell: &str) -> (String, String) {
    match cell.split_once(" — ") {
        Some((s, n)) => (s.trim().to_owned(), n.trim().to_owned()),
        None => (cell.trim().to_owned(), String::new()),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    ClientPackets,
    ServerPackets,
    Snapshots,
    Internal,
}

/// Deviations of the transcribed schema from the message names of annex 11a, applied while importing so that the CSV
/// names the message that exists: `(legacy name, direction, annex message, schema message, reason)`. Each is also
/// recorded in `crates/flyff-proto/NOTES.md` and in the R1 ADR.
pub const RENAMES: &[(&str, Dir, &str, &str, &str)] = &[(
    "PIERCING",
    Dir::C2S,
    "Piercing",
    "PiercingReq",
    "item_types.proto already owns the CPiercing type `Piercing`; the C2S request takes the Req suffix (Plan/11 section 5.1 rule 3)",
)];

/// Result of [`import_11a`].
#[derive(Debug)]
pub struct Imported {
    pub rows: Vec<LegacyRow>,
    /// Row counts per section, `(rows, port, merge, split, drop, internal)`, in the order §2, §3, §4, §7.
    pub counts: [[usize; 6]; 4],
}

/// Parses annex 11a sections 2, 3, 4 and 7 into the 1081 rows of `legacy_index.csv` and verifies them against the
/// totals table of section 1.
pub fn import_11a(md: &str) -> Result<Imported> {
    let mut section: Option<Section> = None;
    let mut rows: Vec<LegacyRow> = Vec::new();
    let mut counts = [[0usize; 6]; 4];
    let mut totals: Vec<(usize, Vec<usize>)> = Vec::new();
    let mut in_totals = false;
    for (n, line) in md.lines().enumerate() {
        let lineno = n + 1;
        if let Some(h) = line.strip_prefix("## ") {
            in_totals = h.starts_with("1.");
            section = if h.starts_with("2.") {
                Some(Section::ClientPackets)
            } else if h.starts_with("3.") {
                Some(Section::ServerPackets)
            } else if h.starts_with("4.") {
                Some(Section::Snapshots)
            } else if h.starts_with("7.") {
                Some(Section::Internal)
            } else {
                None
            };
            continue;
        }
        let Some(c) = cells(line) else { continue };
        if in_totals {
            if let Some(first) = c.first().filter(|f| f.starts_with('§')) {
                let sec = first
                    .chars()
                    .nth(1)
                    .and_then(|d| d.to_digit(10))
                    .unwrap_or(0) as usize;
                let nums: Vec<usize> = c[1..]
                    .iter()
                    .map(|x| {
                        x.split_whitespace()
                            .next()
                            .and_then(|w| w.parse().ok())
                            .unwrap_or(0)
                    })
                    .collect();
                // §7 prints "207 internal, 78 unused, 25 dead, 5 dead feature (...)" in the last cell
                let mut last = c.last().cloned().unwrap_or_default();
                let mut v = nums;
                if sec == 7 {
                    let mut internal = 0usize;
                    let mut dropped = 0usize;
                    for part in last.split(',') {
                        let part = part.trim();
                        let num: usize = part
                            .split_whitespace()
                            .next()
                            .and_then(|w| w.parse().ok())
                            .unwrap_or(0);
                        if part.contains("internal") {
                            internal += num;
                        } else {
                            dropped += num;
                        }
                    }
                    last.clear();
                    v = vec![v[0], 0, 0, 0, dropped, internal];
                } else {
                    v.push(0);
                }
                totals.push((sec, v));
            }
            continue;
        }
        let Some(sec) = section else { continue };
        if c.first().is_some_and(|f| f == "Legacy name") {
            continue;
        }
        let row = match sec {
            Section::Internal => {
                if c.len() != 4 {
                    bail!(
                        "11a line {lineno}: {} cells in a section 7 row, want 4",
                        c.len()
                    );
                }
                let (status, note) = status_note(&c[3]);
                let status = match status.as_str() {
                    "internal" => Status::Internal,
                    "drop" => Status::Drop,
                    other => bail!("11a line {lineno}: section 7 status `{other}`"),
                };
                let hex = first_tick(&c[1]);
                LegacyRow {
                    ns: Ns::Packet,
                    id: u32::from_str_radix(hex, 16)
                        .with_context(|| format!("11a line {lineno}: id `{hex}`"))?,
                    name: define_name(&c[0]),
                    dir: Dir::None,
                    messages: Vec::new(),
                    file: String::new(),
                    ms: String::new(),
                    doc: String::new(),
                    status,
                    note,
                }
            }
            _ => {
                if c.len() != 8 {
                    bail!(
                        "11a line {lineno}: {} cells in a table row, want 8",
                        c.len()
                    );
                }
                let hex = first_tick(&c[1]);
                let ns = match (sec, hex.len()) {
                    (Section::Snapshots, 4) | (Section::ClientPackets, 4) => Ns::Snapshot,
                    (Section::ClientPackets | Section::ServerPackets, 8) => Ns::Packet,
                    _ => bail!("11a line {lineno}: id `{hex}` has the wrong width for its section"),
                };
                let dir = if c[2].starts_with("C→S") {
                    Dir::C2S
                } else if c[2].starts_with("S→C") {
                    Dir::S2C
                } else {
                    bail!("11a line {lineno}: direction `{}`", c[2]);
                };
                let (status, note) = status_note(&c[7]);
                let status = match status.as_str() {
                    "port" => Status::Port,
                    "merge" => Status::Merge,
                    "split" => Status::Split,
                    "drop" => Status::Drop,
                    other => bail!("11a line {lineno}: status `{other}`"),
                };
                let mut messages = ticks(&c[3]);
                let mut file = dash(&c[4]);
                if let Some(m) = messages
                    .first()
                    .filter(|m| m.contains(' ') && m.contains('/'))
                {
                    // `POST /api/login` (HTTP JSON, doc 12): an endpoint, not a proto message
                    if file != HTTP_FILE {
                        bail!("11a line {lineno}: endpoint `{m}` without the HTTP file cell");
                    }
                    messages = vec![m.clone()];
                } else if file == HTTP_FILE {
                    bail!("11a line {lineno}: HTTP row without an endpoint");
                }
                if status == Status::Drop {
                    messages.clear();
                    file.clear();
                }
                LegacyRow {
                    ns,
                    id: u32::from_str_radix(hex, 16)
                        .with_context(|| format!("11a line {lineno}: id `{hex}`"))?,
                    name: define_name(&c[0]),
                    dir,
                    messages,
                    file,
                    ms: dash(&c[5]),
                    doc: dash(&c[6]),
                    status,
                    note,
                }
            }
        };
        let si = match sec {
            Section::ClientPackets => 0,
            Section::ServerPackets => 1,
            Section::Snapshots => 2,
            Section::Internal => 3,
        };
        counts[si][0] += 1;
        counts[si][match row.status {
            Status::Port => 1,
            Status::Merge => 2,
            Status::Split => 3,
            Status::Drop => 4,
            Status::Internal => 5,
        }] += 1;
        rows.push(row);
    }
    for row in &mut rows {
        for (name, dir, annex, schema, why) in RENAMES {
            if row.name == *name && row.dir == *dir {
                for m in &mut row.messages {
                    if m == annex {
                        *m = (*schema).to_owned();
                        row.note = if row.note.is_empty() {
                            format!("annex name `{annex}`: {why}")
                        } else {
                            format!("{}; annex name `{annex}`: {why}", row.note)
                        };
                    }
                }
            }
        }
    }
    // the totals table (section 1)
    let mut problems = Vec::new();
    if totals.len() != 4 {
        problems.push(format!(
            "the totals table has {} rows, want 4",
            totals.len()
        ));
    }
    for (sec, want) in &totals {
        let Some(si) = [2usize, 3, 4, 7].iter().position(|s| s == sec) else {
            continue;
        };
        let have = counts[si];
        // want = [rows, port, merge, split, drop, internal]
        if have.to_vec() != *want {
            problems.push(format!(
                "section {sec}: counted {have:?}, the totals table says {want:?}"
            ));
        }
    }
    let mut seen = BTreeSet::new();
    for r in &rows {
        if !seen.insert((r.ns, r.id, r.name.clone(), r.dir)) {
            problems.push(format!("duplicate row {r}"));
        }
    }
    if !problems.is_empty() {
        bail!(
            "annex 11a does not match its own totals:\n  {}",
            problems.join("\n  ")
        );
    }
    Ok(Imported { rows, counts })
}

/// Parses ref/99 appendix B (the client send-site index) into `ui_send_sites.csv` rows. `index` resolves the ids to
/// the milestone of the message that replaces them and detects dropped packets.
pub fn import_ui_sites(md: &str, index: &[LegacyRow]) -> Result<Vec<UiSite>> {
    let start = md
        .lines()
        .position(|l| l.starts_with("## Appendix B"))
        .context("ref/99 has no `## Appendix B` heading")?;
    let mut out = Vec::new();
    let mut problems = Vec::new();
    let mut in_table = false;
    for (n, line) in md.lines().enumerate().skip(start + 1) {
        let lineno = n + 1;
        if line.starts_with("## ") {
            break;
        }
        let Some(c) = cells(line) else {
            // a separator row stays inside the table; any other text ends it
            if in_table && !line.trim().is_empty() && !line.trim_start().starts_with('|') {
                break;
            }
            continue;
        };
        if c.first().is_some_and(|f| f.starts_with("Send function")) {
            in_table = true;
            continue;
        }
        if !in_table {
            continue;
        }
        if c.len() != 3 {
            bail!(
                "ref/99 line {lineno}: {} cells in an appendix B row, want 3",
                c.len()
            );
        }
        let send_fn = first_tick(&c[0]).to_owned();
        // drop parenthetical remarks (they may contain commas), then split the id list
        let mut ids_text = String::new();
        let mut depth = 0;
        for ch in c[1].chars() {
            match ch {
                '(' => depth += 1,
                ')' if depth > 0 => depth -= 1,
                other if depth == 0 => ids_text.push(other),
                _ => {}
            }
        }
        let mut wanted: Vec<(String, Option<u32>)> = Vec::new();
        for part in ids_text.split(',') {
            let mut w = part.split_whitespace();
            let (Some(name), Some(hex)) = (w.next(), w.next()) else {
                continue;
            };
            let Some(hex) = hex.strip_prefix("0x").or_else(|| hex.strip_prefix("0X")) else {
                continue;
            };
            let id = u32::from_str_radix(hex, 16)
                .with_context(|| format!("ref/99 line {lineno}: id `{hex}`"))?;
            wanted.push((name.to_owned(), Some(id)));
        }
        if wanted.is_empty() {
            // `SendHdr`: the packets are named in brackets in the call sites: `WndField.cpp:2790[SORT_INVENTORY]`
            for site in c[2].split(',') {
                if let (Some(a), Some(b)) = (site.find('['), site.find(']'))
                    && a < b
                {
                    wanted.push((site[a + 1..b].to_owned(), None));
                }
            }
        }
        if wanted.is_empty() {
            problems.push(format!("ref/99 line {lineno}: `{send_fn}` names no packet"));
            continue;
        }
        let mut ids = Vec::new();
        let mut milestones: Vec<&str> = Vec::new();
        let mut live = false;
        for (name, id) in wanted {
            let row = index.iter().find(|r| {
                r.ns == Ns::Packet
                    && r.dir != Dir::S2C
                    && id.is_none_or(|i| r.id == i)
                    && r.name.eq_ignore_ascii_case(&name)
            });
            let row = row.or_else(|| {
                // the same name with a different spelling of the id is a mismatch, reported below
                index.iter().find(|r| {
                    r.ns == Ns::Packet && r.dir != Dir::S2C && id.is_some_and(|i| r.id == i)
                })
            });
            let Some(row) = row else {
                problems.push(format!("ref/99 line {lineno}: `{send_fn}` sends {name} {id:?}, which is not in the index"));
                continue;
            };
            if !row.name.eq_ignore_ascii_case(&name) {
                problems.push(format!(
                    "ref/99 line {lineno}: `{send_fn}` calls id {:08X} {name}, the index names it {}",
                    row.id, row.name
                ));
            }
            ids.push((row.name.clone(), row.id));
            if row.status.is_mapped() {
                live = true;
                if !row.ms.is_empty() {
                    milestones.push(row.ms.as_str());
                }
            }
        }
        milestones.sort_by_key(|m| milestone_rank(m));
        out.push(UiSite {
            send_fn,
            legacy_ids: ids,
            call_sites: c[2].clone(),
            milestone: milestones
                .first()
                .map(|m| (*m).to_owned())
                .unwrap_or_default(),
            status: if live {
                SiteStatus::Todo
            } else {
                SiteStatus::Dropped
            },
        });
    }
    if !problems.is_empty() {
        bail!(
            "ref/99 appendix B does not match the index:\n  {}",
            problems.join("\n  ")
        );
    }
    if out.is_empty() {
        bail!("ref/99 appendix B has no rows");
    }
    Ok(out)
}

/// `M3` -> 3 (unknown spellings sort last).
pub fn milestone_rank(m: &str) -> u32 {
    m.strip_prefix('M')
        .and_then(|n| n.parse().ok())
        .unwrap_or(u32::MAX)
}
