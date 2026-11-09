//! `target/reports/proto-coverage.{md,json}` (Plan/11 section 11 outputs, with the rule C9 section of WP-TEST-54).

use crate::c9;
use crate::checks::{Checked, Severity};
use crate::csv::{Dir, LegacyRow, Ns, Status, UiSite};
use crate::model::Model;
use serde::Serialize;
use std::collections::BTreeMap;
use std::fmt::Write as _;

/// One finding as printed.
#[derive(Serialize, Debug, Clone)]
pub struct FindingOut {
    pub check: String,
    pub severity: String,
    pub subject: String,
    pub detail: String,
    pub location: Option<String>,
}

/// One legacy row as printed.
#[derive(Serialize, Debug, Clone)]
pub struct RowOut {
    pub ns: String,
    pub id: String,
    pub name: String,
    pub dir: String,
    pub status: String,
    pub messages: String,
    pub file: String,
    pub ms: String,
    pub note: String,
}

/// Messages of one file.
#[derive(Serialize, Debug, Clone, Default)]
pub struct FileCount {
    pub c2s: usize,
    pub s2c: usize,
    pub stubs: usize,
}

/// The JSON report.
#[derive(Serialize, Debug)]
pub struct Report {
    pub ok: bool,
    pub errors: usize,
    pub warnings: usize,
    pub stub_max_milestone: String,
    pub envelope_c2s: usize,
    pub envelope_s2c: usize,
    pub extension_ids: usize,
    pub reserved_c2s: Vec<(u32, u32)>,
    pub reserved_s2c: Vec<(u32, u32)>,
    pub legacy_rows: usize,
    pub rows_by_status: BTreeMap<String, usize>,
    /// §2 C->S, §3 S->C packet, §4 snapshot, §7 no client endpoint: rows and statuses.
    pub rows_by_set: BTreeMap<String, BTreeMap<String, usize>>,
    pub messages_by_file: BTreeMap<String, FileCount>,
    pub messages_by_milestone: BTreeMap<String, usize>,
    pub stubs: Vec<(String, String, String)>,
    pub dropped: Vec<RowOut>,
    pub send_functions: usize,
    pub send_sites_by_status: BTreeMap<String, usize>,
    pub send_notes: Vec<(String, String, String, String, String)>,
    /// Rule C9: the markers of `web/src` against the rows.
    pub c9: c9::Summary,
    pub findings: Vec<FindingOut>,
    pub rows: Vec<RowOut>,
}

fn row_out(r: &LegacyRow) -> RowOut {
    RowOut {
        ns: r.ns.tag().to_owned(),
        id: r.id_hex(),
        name: r.name.clone(),
        dir: r.dir.tag().to_owned(),
        status: r.status.tag().to_owned(),
        messages: r.messages.join(" + "),
        file: r.file.clone(),
        ms: r.ms.clone(),
        note: r.note.clone(),
    }
}

/// The set a row belongs to in the annex totals table.
pub fn set_of(r: &LegacyRow) -> &'static str {
    match (r.ns, r.dir) {
        (_, Dir::None) => "§7 no client endpoint",
        (Ns::Snapshot, Dir::S2C) => "§4 SNAPSHOTTYPE S->C",
        (Ns::Packet, Dir::S2C) => "§3 PACKETTYPE S->C",
        (_, Dir::C2S) => "§2 C->S",
    }
}

/// Builds the report.
pub fn build(
    model: &Model,
    index: &[LegacyRow],
    sites: &[UiSite],
    checked: &Checked,
    stub_max: &str,
) -> Report {
    let mut by_status: BTreeMap<String, usize> = BTreeMap::new();
    let mut by_set: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
    for r in index {
        *by_status.entry(r.status.tag().to_owned()).or_default() += 1;
        let set = by_set.entry(set_of(r).to_owned()).or_default();
        *set.entry("rows".to_owned()).or_default() += 1;
        *set.entry(r.status.tag().to_owned()).or_default() += 1;
    }
    let mut files: BTreeMap<String, FileCount> = BTreeMap::new();
    let mut by_ms: BTreeMap<String, usize> = BTreeMap::new();
    for (dir, cases) in [(Dir::C2S, &model.client), (Dir::S2C, &model.server)] {
        for c in cases {
            let Some(m) = model.messages.get(&c.message) else {
                continue;
            };
            let f = files.entry(m.file.clone()).or_default();
            if dir == Dir::C2S {
                f.c2s += 1;
            } else {
                f.s2c += 1;
            }
            if m.stub {
                f.stubs += 1;
            }
            *by_ms.entry(m.milestone.clone()).or_default() += 1;
        }
    }
    let mut site_status: BTreeMap<String, usize> = BTreeMap::new();
    for s in sites {
        *site_status.entry(s.status.tag().to_owned()).or_default() += 1;
    }
    let findings: Vec<FindingOut> = checked
        .findings
        .iter()
        .map(|f| FindingOut {
            check: f.check.to_owned(),
            severity: match f.severity {
                Severity::Error => "error",
                Severity::Warning => "warning",
            }
            .to_owned(),
            subject: f.subject.clone(),
            detail: f.detail.clone(),
            location: f.loc.clone(),
        })
        .collect();
    Report {
        ok: checked.errors() == 0,
        errors: checked.errors(),
        warnings: checked.findings.len() - checked.errors(),
        stub_max_milestone: stub_max.to_owned(),
        envelope_c2s: model.client.len(),
        envelope_s2c: model.server.len(),
        extension_ids: model
            .messages
            .values()
            .filter(|m| m.ext_id.is_some())
            .count(),
        reserved_c2s: model.client_reserved.clone(),
        reserved_s2c: model.server_reserved.clone(),
        legacy_rows: index.len(),
        rows_by_status: by_status,
        rows_by_set: by_set,
        messages_by_file: files,
        messages_by_milestone: by_ms,
        stubs: checked
            .stubs
            .iter()
            .map(|s| (s.name.clone(), s.file.clone(), s.milestone.clone()))
            .collect(),
        dropped: index
            .iter()
            .filter(|r| matches!(r.status, Status::Drop | Status::Internal))
            .map(row_out)
            .collect(),
        send_functions: sites.len(),
        send_sites_by_status: site_status,
        send_notes: checked
            .send_notes
            .iter()
            .map(|n| {
                (
                    n.send_fn.clone(),
                    format!("{} 0x{:08X}", n.packet, n.id),
                    n.status.to_owned(),
                    n.replacement.clone(),
                    n.milestone.clone(),
                )
            })
            .collect(),
        c9: checked.c9.clone(),
        findings,
        rows: index.iter().map(row_out).collect(),
    }
}

/// Renders the markdown report.
pub fn markdown(r: &Report) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "# Proto coverage report\n");
    let _ = writeln!(
        s,
        "Result: **{}** ({} errors, {} warnings). `PROTO_STUB_MAX_MILESTONE` = {}.\n",
        if r.ok { "green" } else { "RED" },
        r.errors,
        r.warnings,
        r.stub_max_milestone
    );
    let _ = writeln!(s, "## Envelope\n");
    let _ = writeln!(
        s,
        "* {} C->S and {} S->C envelope messages, {} with an extension id, {} stubs.",
        r.envelope_c2s,
        r.envelope_s2c,
        r.extension_ids,
        r.stubs.len()
    );
    let _ = writeln!(
        s,
        "* reserved C->S tags {:?}, reserved S->C tags {:?}.\n",
        r.reserved_c2s, r.reserved_s2c
    );
    let _ = writeln!(s, "## Legacy index ({} rows)\n", r.legacy_rows);
    let _ = writeln!(
        s,
        "| set | rows | port | merge | split | drop | internal |\n|---|---|---|---|---|---|---|"
    );
    for (set, c) in &r.rows_by_set {
        let g = |k: &str| c.get(k).copied().unwrap_or(0);
        let _ = writeln!(
            s,
            "| {set} | {} | {} | {} | {} | {} | {} |",
            g("rows"),
            g("port"),
            g("merge"),
            g("split"),
            g("drop"),
            g("internal")
        );
    }
    let _ = writeln!(
        s,
        "\n## Messages by file\n\n| file | C->S | S->C | stubs |\n|---|---|---|---|"
    );
    for (f, c) in &r.messages_by_file {
        let _ = writeln!(s, "| {f}.proto | {} | {} | {} |", c.c2s, c.s2c, c.stubs);
    }
    let _ = writeln!(s, "\n## Messages by milestone\n");
    let line: Vec<String> = r
        .messages_by_milestone
        .iter()
        .map(|(m, n)| format!("{m} {n}"))
        .collect();
    let _ = writeln!(s, "{}\n", line.join(", "));
    let _ = writeln!(s, "## Stubs ({})\n", r.stubs.len());
    if r.stubs.is_empty() {
        let _ = writeln!(s, "none\n");
    }
    for (n, f, m) in &r.stubs {
        let _ = writeln!(s, "* {n} ({f}.proto, {m})");
    }
    let _ = writeln!(s, "## Findings ({})\n", r.findings.len());
    if r.findings.is_empty() {
        let _ = writeln!(s, "none\n");
    }
    for f in &r.findings {
        let _ = writeln!(
            s,
            "* {} {} `{}`: {}{}",
            f.severity,
            f.check,
            f.subject,
            f.detail,
            f.location
                .as_ref()
                .map_or(String::new(), |l| format!(" ({l})"))
        );
    }
    let _ = writeln!(s, "\n## UI send sites ({} functions)\n", r.send_functions);
    let line: Vec<String> = r
        .send_sites_by_status
        .iter()
        .map(|(m, n)| format!("{m} {n}"))
        .collect();
    let _ = writeln!(s, "{}\n", line.join(", "));
    let _ = writeln!(
        s,
        "Send functions whose packet is dropped or merged (replacement for the client work packages):\n"
    );
    let _ = writeln!(
        s,
        "| send function | packet | status | replacement | MS |\n|---|---|---|---|---|"
    );
    for (f, p, st, rep, ms) in &r.send_notes {
        let _ = writeln!(s, "| {f} | {p} | {st} | {} | {ms} |", rep.replace('|', "/"));
    }
    c9_section(&mut s, &r.c9);
    let _ = writeln!(s, "\n## Dropped and internal ids ({})\n", r.dropped.len());
    let _ = writeln!(
        s,
        "| ns | id | name | dir | status | reason |\n|---|---|---|---|---|---|"
    );
    for d in &r.dropped {
        let _ = writeln!(
            s,
            "| {} | {} | {} | {} | {} | {} |",
            d.ns,
            d.id,
            d.name,
            d.dir,
            d.status,
            d.note.replace('|', "/")
        );
    }
    let _ = writeln!(s, "\n## Every legacy row ({})\n", r.rows.len());
    let _ = writeln!(
        s,
        "| ns | id | name | dir | status | message | file | MS |\n|---|---|---|---|---|---|---|---|"
    );
    for d in &r.rows {
        let _ = writeln!(
            s,
            "| {} | {} | {} | {} | {} | {} | {} | {} |",
            d.ns, d.id, d.name, d.dir, d.status, d.messages, d.file, d.ms
        );
    }
    s
}

/// The rule C9 section: the matrix of rows, the markers found, what is open.
fn c9_section(s: &mut String, c: &c9::Summary) {
    let _ = writeln!(s, "\n## Rule C9: UI send-site markers\n");
    if !c.ran {
        let _ = writeln!(s, "Not evaluated: {}\n", c.note);
        return;
    }
    let _ = writeln!(
        s,
        "Current milestone {} (a `todo` row at or below it is a warning, an error from {}).",
        c.current_milestone, c.error_from_milestone
    );
    let _ = writeln!(
        s,
        "{} marker lines name {} distinct send functions in {} files ({} source files read, {} test files skipped).\n",
        c.marker_lines, c.distinct_names, c.marker_files, c.files_scanned, c.test_files_skipped
    );
    let mut milestones: Vec<&String> = c
        .rows_by_status
        .values()
        .flat_map(BTreeMap::keys)
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    milestones.sort_by_key(|m| (crate::import::milestone_rank(m), (*m).clone()));
    let _ = write!(s, "| status |");
    for m in &milestones {
        let _ = write!(s, " {m} |");
    }
    let _ = writeln!(s, " total |");
    let _ = write!(s, "|---|");
    for _ in &milestones {
        let _ = write!(s, "---|");
    }
    let _ = writeln!(s, "---|");
    for (status, by_ms) in &c.rows_by_status {
        let _ = write!(s, "| {status} |");
        for m in &milestones {
            let _ = write!(s, " {} |", by_ms.get(*m).copied().unwrap_or(0));
        }
        let _ = writeln!(s, " {} |", by_ms.values().sum::<usize>());
    }
    let _ = writeln!(s);
    let list = |items: &[String]| {
        if items.is_empty() {
            "none".to_owned()
        } else {
            items.join(", ")
        }
    };
    let _ = writeln!(
        s,
        "* `todo` rows that already have a marker (`cargo xtask coverage proto --sync-sites`): {}",
        list(&c.todo_with_marker)
    );
    let _ = writeln!(s, "* unknown markers: {}", c.unknown_markers.len());
    for m in &c.unknown_markers {
        let _ = writeln!(
            s,
            "  * `{}` at {}{}",
            m.text,
            m.location,
            m.suggestion
                .as_ref()
                .map_or(String::new(), |n| format!(" (did you mean `{n}`?)"))
        );
    }
    let _ = writeln!(s, "* malformed markers: {}", c.malformed_markers.len());
    for m in &c.malformed_markers {
        let _ = writeln!(s, "  * `{}` at {}", m.text, m.location);
    }
    let line: Vec<String> = c
        .todo_by_milestone
        .iter()
        .map(|(m, n)| format!("{m} {n}"))
        .collect();
    let _ = writeln!(
        s,
        "* `todo` rows without a marker per milestone: {}",
        if line.is_empty() {
            "none".to_owned()
        } else {
            line.join(", ")
        }
    );
    let _ = writeln!(s, "* due by {} without a marker:", c.current_milestone);
    if c.due.is_empty() {
        let _ = writeln!(s, "  * none");
    }
    for (m, names) in &c.due {
        let _ = writeln!(s, "  * {m}: {}", names.join(", "));
    }
    let _ = writeln!(s);
}
