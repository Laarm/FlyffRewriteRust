//! The checks C1-C8 of Plan/11 section 11 and rule C9 of Plan/02 section 9.6 (in [`crate::c9`]). Every check is a pure
//! function of the [`Model`] (the descriptor set), the legacy index, the UI send sites and, for C9, the markers a scan
//! of `web/src` found, and returns [`Finding`]s; nothing here touches the file system.

use crate::c9;
use crate::csv::{Dir, LegacyRow, Ns, SiteStatus, Status, UiSite};
use crate::import::milestone_rank;
use crate::model::{Message, Model};
use std::collections::{BTreeMap, BTreeSet};

/// Extension ids live in this range (D3).
pub const EXT_ID_RANGE: std::ops::RangeInclusive<u32> = 0xFE00_0000..=0xFEFF_FFFF;

/// The 64-bit fields that carry bit-packed words and therefore stay `bigint` in TypeScript (Plan/11 section 4.3):
/// the allowlist of check C7. A new entry needs a reason in `crates/flyff-proto/NOTES.md`.
pub const BIT_PACKED: &[&str] = &[
    "flyff.v1.CallTheRoll.attendance_bits",
    "flyff.v1.ItemElem.random_opt_item_id",
    "flyff.v1.ItemSelectAwakeningValueNtf.new_random_option",
    "flyff.v1.QueryEquipEntry.random_opt_item_id",
    "flyff.v1.UpdateItemEx.value",
];

/// The tag block of every feature file, identical in both envelopes (Plan/11 section 4.2).
pub const FILE_BLOCKS: &[(&str, u32, u32)] = &[
    ("session", 16, 47),
    ("lobby", 48, 79),
    ("world", 80, 111),
    ("movement", 112, 143),
    ("character", 144, 191),
    ("combat", 192, 223),
    ("skill", 224, 255),
    ("pvp", 256, 287),
    ("inventory", 288, 319),
    ("upgrade", 320, 351),
    ("shop", 352, 383),
    ("trade", 384, 415),
    ("bank", 416, 447),
    ("mail", 448, 479),
    ("pet", 480, 511),
    ("npc", 512, 543),
    ("quest", 544, 575),
    ("chat", 576, 607),
    ("notice", 608, 639),
    ("party", 640, 671),
    ("guild", 672, 719),
    ("guild_war", 720, 751),
    ("guild_siege", 752, 783),
    ("messenger", 784, 815),
    ("relation", 816, 847),
    ("event", 848, 879),
    ("lord", 880, 911),
    ("secret_room", 912, 943),
    ("housing", 944, 975),
    ("gm", 976, 1007),
    ("taskbar", 1008, 1039),
];

/// Hot `ClientEnvelope` tags 1-15 (one-byte key; 15 is reserved; Plan/11 section 3.3).
pub const HOT_C2S: &[(u32, &str)] = &[
    (1, "PlayerMoveState"),
    (2, "DestPosReq"),
    (3, "SetDestObjReq"),
    (4, "MeleeAttackReq"),
    (5, "UseSkillReq"),
    (6, "ChatReq"),
    (7, "MotionReq"),
    (8, "SetTargetReq"),
    (9, "DoUseItem"),
    (10, "MagicAttackReq"),
    (11, "RangeAttackReq"),
    (12, "MoveItemReq"),
    (13, "ServerTimeReq"),
    (14, "ScriptDlg"),
];

/// Hot `ServerMsg` tags 2-15 (tag 1 is `obj_id`).
pub const HOT_S2C: &[(u32, &str)] = &[
    (2, "MoverStateNtf"),
    (3, "MoverKeyframe"),
    (4, "DestPosNtf"),
    (5, "SetDestObjNtf"),
    (6, "Damage"),
    (7, "SetPointParam"),
    (8, "MeleeAttackNtf"),
    (9, "DelObj"),
    (10, "SpawnNpc"),
    (11, "SpawnPlayer"),
    (12, "SpawnItem"),
    (13, "MotionNtf"),
    (14, "ProjectileSpawn"),
    (15, "ChatNtf"),
];

/// Severity of a finding.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Warning,
    Error,
}

/// One finding of a check.
#[derive(Clone, Debug)]
pub struct Finding {
    /// `C1` .. `C9`.
    pub check: &'static str,
    pub severity: Severity,
    /// What it is about: a legacy row, a message, a field, a tag.
    pub subject: String,
    pub detail: String,
    /// `proto/flyff/v1/x.proto:LINE` when the subject is a message or a field in the sources.
    pub loc: Option<String>,
}

fn err(check: &'static str, subject: impl Into<String>, detail: impl Into<String>) -> Finding {
    Finding {
        check,
        severity: Severity::Error,
        subject: subject.into(),
        detail: detail.into(),
        loc: None,
    }
}

fn warn(check: &'static str, subject: impl Into<String>, detail: impl Into<String>) -> Finding {
    Finding {
        severity: Severity::Warning,
        ..err(check, subject, detail)
    }
}

/// Which envelope a message sits in.
fn direction_of(model: &Model, message: &str) -> Option<Dir> {
    let in_c = model.client.iter().any(|c| c.message == message);
    let in_s = model.server.iter().any(|c| c.message == message);
    match (in_c, in_s) {
        (true, false) => Some(Dir::C2S),
        (false, true) => Some(Dir::S2C),
        _ => None,
    }
}

/// The file of the rows that map to a transport container instead of an envelope variant.
pub const CONTAINER_FILE: &str = "envelope.proto";

/// True when the cell looks like `POST /api/login` (an HTTP endpoint).
fn is_endpoint(s: &str) -> bool {
    let mut w = s.split_whitespace();
    matches!(w.next(), Some("GET" | "POST" | "PUT" | "DELETE" | "PATCH"))
        && w.next().is_some_and(|p| p.starts_with('/'))
}

/// C1: every mapped legacy row is carried by a message of the listed name, file and direction; every drop row has a
/// reason; every HTTP row names an endpoint.
pub fn c1(model: &Model, index: &[LegacyRow]) -> Vec<Finding> {
    let mut out = Vec::new();
    for row in index {
        let subject = row.to_string();
        match row.status {
            Status::Drop | Status::Internal => {
                if row.note.trim().is_empty() {
                    out.push(err(
                        "C1",
                        &subject,
                        format!("a {} row needs a reason", row.status.tag()),
                    ));
                }
            }
            Status::Port | Status::Merge | Status::Split => {
                if row.messages.is_empty() {
                    out.push(err("C1", &subject, "a mapped row names no message"));
                    continue;
                }
                if row.is_http() {
                    if row.messages.len() != 1 || !is_endpoint(&row.messages[0]) {
                        out.push(err(
                            "C1",
                            &subject,
                            format!(
                                "an HTTP row must name `METHOD /path`, not `{}`",
                                row.messages.join("+")
                            ),
                        ));
                    }
                    continue;
                }
                if row.file == CONTAINER_FILE {
                    // the transport containers (ClientEnvelope, ServerBatch) stand for the legacy SNAPSHOT / JOIN
                    // containers; they exist but carry no (flyff.legacy_id) (C3 forbids it on non-variants)
                    for name in &row.messages {
                        if model.top(name).is_none_or(|m| m.file != "envelope") {
                            out.push(err(
                                "C1",
                                &subject,
                                format!("the container `{name}` is not defined in envelope.proto"),
                            ));
                        }
                    }
                    continue;
                }
                let mut carried = false;
                let mut listed_ok = 0;
                for name in &row.messages {
                    let Some(msg) = model.top(name) else {
                        out.push(err(
                            "C1",
                            &subject,
                            format!("the message `{name}` does not exist"),
                        ));
                        continue;
                    };
                    if format!("{}.proto", msg.file) != row.file {
                        out.push(err(
                            "C1",
                            &subject,
                            format!(
                                "`{name}` is in {}.proto, the index says {}",
                                msg.file, row.file
                            ),
                        ));
                        continue;
                    }
                    let envelope = model.envelope_names(row.dir);
                    if !envelope.contains(&name.as_str()) {
                        out.push(err(
                            "C1",
                            &subject,
                            format!("`{name}` is not in the {} envelope", row.dir.tag()),
                        ));
                        continue;
                    }
                    listed_ok += 1;
                    if msg
                        .legacy_ids
                        .iter()
                        .any(|l| l.ns == row.ns && l.value == row.id && l.name == row.name)
                    {
                        carried = true;
                    }
                    // a merged message arrives with the earliest of its ids; only a message that comes LATER than the
                    // index requires is a problem
                    if milestone_rank(&msg.milestone) > milestone_rank(&row.ms)
                        && !row.ms.is_empty()
                        && !msg.milestone.is_empty()
                    {
                        out.push(warn(
                            "C1",
                            &subject,
                            format!(
                                "milestone {} in the index, {} on `{name}`",
                                row.ms, msg.milestone
                            ),
                        ));
                    }
                }
                if listed_ok > 0 && !carried {
                    out.push(err(
                        "C1",
                        &subject,
                        format!(
                            "no message of `{}` carries (flyff.legacy_id) {} 0x{} {}",
                            row.messages.join("`, `"),
                            row.ns.tag(),
                            row.id_hex(),
                            row.name
                        ),
                    ));
                }
            }
        }
    }
    out
}

/// C2: every `(flyff.legacy_id)` of the schema matches a row of the index.
pub fn c2(model: &Model, index: &[LegacyRow]) -> Vec<Finding> {
    let mut out = Vec::new();
    for msg in model.messages.values() {
        let dir = direction_of(model, &msg.full_name).or(msg.dir);
        for lid in &msg.legacy_ids {
            let subject = format!(
                "{} {} 0x{:X} {}",
                msg.name,
                lid.ns.tag(),
                lid.value,
                lid.name
            );
            let same_id: Vec<&LegacyRow> = index
                .iter()
                .filter(|r| r.ns == lid.ns && r.id == lid.value)
                .collect();
            if same_id.is_empty() {
                out.push(err("C2", subject, "the id is not in legacy_index.csv"));
                continue;
            }
            let named: Vec<&LegacyRow> = same_id
                .iter()
                .copied()
                .filter(|r| r.name == lid.name)
                .collect();
            if named.is_empty() {
                let others: Vec<&str> = same_id.iter().map(|r| r.name.as_str()).collect();
                out.push(err(
                    "C2",
                    subject,
                    format!("the index names this id {}", others.join(" / ")),
                ));
                continue;
            }
            let Some(row) = named.iter().find(|r| dir.is_none_or(|d| r.dir == d)) else {
                let dirs: Vec<&str> = named.iter().map(|r| r.dir.tag()).collect();
                out.push(err(
                    "C2",
                    subject,
                    format!(
                        "the message is {} but the index row is {}",
                        dir.map_or("-", Dir::tag),
                        dirs.join(" / ")
                    ),
                ));
                continue;
            };
            if !row.status.is_mapped() {
                out.push(err(
                    "C2",
                    subject,
                    format!("the index lists this id as `{}`", row.status.tag()),
                ));
                continue;
            }
            if row.is_http() {
                out.push(err(
                    "C2",
                    subject,
                    "the index maps this id to an HTTP endpoint, not to a message",
                ));
                continue;
            }
            if !msg.nested && !row.messages.iter().any(|m| m == &msg.name) {
                out.push(err(
                    "C2",
                    subject,
                    format!("the index maps this id to `{}`", row.messages.join("`, `")),
                ));
            }
        }
    }
    out
}

/// C3: every envelope variant has a legacy id or an extension id; extension ids are unique and in range; other
/// messages carry legacy ids only with `sub`.
pub fn c3(model: &Model) -> Vec<Finding> {
    let mut out = Vec::new();
    let in_envelope: BTreeSet<&str> = model
        .client
        .iter()
        .chain(&model.server)
        .map(|c| c.message.as_str())
        .collect();
    for case in model.client.iter().chain(&model.server) {
        match model.messages.get(&case.message) {
            None => out.push(err(
                "C3",
                &case.message,
                "an envelope variant names a message that does not exist",
            )),
            Some(m) if m.legacy_ids.is_empty() && m.ext_id.is_none() => {
                out.push(err(
                    "C3",
                    &m.name,
                    "an envelope message needs (flyff.legacy_id) or (flyff.ext_id)",
                ));
            }
            Some(_) => {}
        }
    }
    let mut seen: BTreeMap<u32, &str> = BTreeMap::new();
    for m in model.messages.values() {
        if let Some(e) = m.ext_id {
            if !EXT_ID_RANGE.contains(&e) {
                out.push(err(
                    "C3",
                    &m.name,
                    format!("(flyff.ext_id) 0x{e:08X} is outside 0xFE000000..0xFEFFFFFF"),
                ));
            }
            if let Some(prev) = seen.insert(e, m.name.as_str()) {
                out.push(err(
                    "C3",
                    &m.name,
                    format!("(flyff.ext_id) 0x{e:08X} is also used by {prev}"),
                ));
            }
        }
        if !in_envelope.contains(m.full_name.as_str()) {
            for l in m.legacy_ids.iter().filter(|l| l.sub.is_empty()) {
                out.push(err(
                    "C3",
                    &m.name,
                    format!("a message outside the envelopes may carry (flyff.legacy_id) {} only with `sub` set", l.name),
                ));
            }
        }
    }
    out
}

/// C4: envelope tags: hot ranges, file blocks, no duplicates, reserved ranges unused.
pub fn c4(model: &Model) -> Vec<Finding> {
    let mut out = Vec::new();
    for (dir, cases, hot, reserved) in [
        (Dir::C2S, &model.client, HOT_C2S, &model.client_reserved),
        (Dir::S2C, &model.server, HOT_S2C, &model.server_reserved),
    ] {
        let mut used: BTreeMap<u32, &str> = BTreeMap::new();
        for case in cases {
            let name = model
                .messages
                .get(&case.message)
                .map_or(case.message.as_str(), |m| m.name.as_str());
            let subject = format!("{} {} tag {}", dir.tag(), name, case.tag);
            if let Some(prev) = used.insert(case.tag, name) {
                out.push(err(
                    "C4",
                    &subject,
                    format!("the tag is also used by {prev}"),
                ));
            }
            for &(lo, hi) in reserved {
                if (lo..=hi).contains(&case.tag) {
                    out.push(err(
                        "C4",
                        &subject,
                        format!("the tag is inside the reserved range {lo}..{hi}"),
                    ));
                }
            }
            if case.tag <= 15 {
                match hot.iter().find(|(t, _)| *t == case.tag) {
                    Some((_, want)) if *want == name => {}
                    Some((_, want)) => out.push(err("C4", &subject, format!("hot tag {} belongs to {want}", case.tag))),
                    None => out.push(err("C4", &subject, "tags 1-15 are the hot range and hold only the messages of Plan/11 section 3.3")),
                }
                continue;
            }
            let Some(msg) = model.messages.get(&case.message) else {
                continue;
            };
            match FILE_BLOCKS.iter().find(|(f, ..)| *f == msg.file) {
                None => out.push(err(
                    "C4",
                    &subject,
                    format!("{}.proto has no tag block", msg.file),
                )),
                Some(&(_, lo, hi)) if !(lo..=hi).contains(&case.tag) => {
                    out.push(err(
                        "C4",
                        &subject,
                        format!(
                            "the tag is outside the block {lo}..{hi} of {}.proto",
                            msg.file
                        ),
                    ));
                }
                Some(_) => {}
            }
        }
        for (tag, want) in hot {
            if !used.contains_key(tag) {
                out.push(err(
                    "C4",
                    format!("{} tag {tag}", dir.tag()),
                    format!("hot tag {tag} should carry {want}"),
                ));
            }
        }
    }
    out
}

/// C5: each envelope message is in exactly one envelope and agrees with its `(flyff.dir)`.
pub fn c5(model: &Model) -> Vec<Finding> {
    let mut out = Vec::new();
    let mut seen: BTreeMap<&str, Vec<Dir>> = BTreeMap::new();
    for (dir, cases) in [(Dir::C2S, &model.client), (Dir::S2C, &model.server)] {
        for c in cases {
            seen.entry(c.message.as_str()).or_default().push(dir);
        }
    }
    for (message, dirs) in seen {
        let name = model
            .messages
            .get(message)
            .map_or(message, |m| m.name.as_str());
        if dirs.len() != 1 {
            let list: Vec<&str> = dirs.iter().map(|d| d.tag()).collect();
            out.push(err(
                "C5",
                name,
                format!(
                    "the message appears {} times ({})",
                    dirs.len(),
                    list.join(", ")
                ),
            ));
            continue;
        }
        match model.messages.get(message).and_then(|m| m.dir) {
            None => out.push(err("C5", name, "an envelope message needs (flyff.dir)")),
            Some(d) if d != dirs[0] => {
                out.push(err(
                    "C5",
                    name,
                    format!(
                        "(flyff.dir) says {} but the message is in the {} envelope",
                        d.tag(),
                        dirs[0].tag()
                    ),
                ));
            }
            Some(_) => {}
        }
    }
    out
}

/// A stub message of the schema.
#[derive(Clone, Debug)]
pub struct Stub {
    pub name: String,
    pub file: String,
    pub milestone: String,
}

/// C6: the stub list, and an error for every stub whose milestone is not past `stub_max` (a milestone rank).
pub fn c6(model: &Model, stub_max: u32) -> (Vec<Stub>, Vec<Finding>) {
    let mut stubs = Vec::new();
    let mut out = Vec::new();
    for m in model.messages.values().filter(|m| m.stub) {
        stubs.push(Stub {
            name: m.name.clone(),
            file: m.file.clone(),
            milestone: m.milestone.clone(),
        });
        if milestone_rank(&m.milestone) <= stub_max {
            out.push(err(
                "C6",
                &m.name,
                format!("a stub of milestone {} (PROTO_STUB_MAX_MILESTONE is M{stub_max}): transcribe it", m.milestone),
            ));
        }
    }
    stubs.sort_by(|a, b| {
        (milestone_rank(&a.milestone), &a.name).cmp(&(milestone_rank(&b.milestone), &b.name))
    });
    (stubs, out)
}

fn pascal_case(s: &str) -> bool {
    s.split('.').all(|part| {
        let mut ch = part.chars();
        ch.next().is_some_and(|c| c.is_ascii_uppercase()) && ch.all(|c| c.is_ascii_alphanumeric())
    })
}

/// C7: schema lint: no `map<>`, ascending declarations, legacy numbers below 100, no `jstype`, js-safe 64-bit fields.
pub fn c7(model: &Model) -> Vec<Finding> {
    let mut out = Vec::new();
    let mut allow_used: BTreeSet<&str> = BTreeSet::new();
    for m in model.messages.values() {
        if !pascal_case(&m.name) {
            out.push(err("C7", &m.name, "message names are PascalCase"));
        }
        let mut last = 0;
        for f in &m.fields {
            let subject = format!("{}.{}", m.name, f.name);
            if f.is_map {
                out.push(err(
                    "C7",
                    &subject,
                    "map<> fields are forbidden (nondeterministic order)",
                ));
            }
            if f.number <= last {
                out.push(err(
                    "C7",
                    &subject,
                    format!(
                        "field {} is declared after field {last}: declare in ascending order",
                        f.number
                    ),
                ));
            }
            last = f.number;
            if f.has_legacy_type && f.number >= 100 {
                out.push(err(
                    "C7",
                    &subject,
                    "a legacy field is numbered 1..99; numbers from 100 are for added fields",
                ));
            }
            if f.has_jstype {
                out.push(err(
                    "C7",
                    &subject,
                    "no field sets `jstype` (protobuf-es maps 64-bit fields to bigint)",
                ));
            }
            if f.is_64_bit {
                let full = format!("{}.{}", m.full_name, f.name);
                let packed = BIT_PACKED.iter().find(|p| **p == full);
                match (f.js_safe, packed) {
                    (false, None) => out.push(err(
                        "C7",
                        &subject,
                        "a 64-bit field needs (flyff.js_safe_number) = true",
                    )),
                    (true, Some(_)) => out.push(err(
                        "C7",
                        &subject,
                        "a bit-packed 64-bit field must not carry (flyff.js_safe_number)",
                    )),
                    _ => {}
                }
                if let Some(p) = packed {
                    allow_used.insert(p);
                }
            }
        }
    }
    for p in BIT_PACKED {
        if !allow_used.contains(p) {
            out.push(warn(
                "C7",
                *p,
                "the bit-packed allowlist names a field that is not in the schema",
            ));
        }
    }
    out
}

/// One line of the C8 report: a send function whose packet was dropped or merged.
#[derive(Clone, Debug)]
pub struct SendSiteNote {
    pub send_fn: String,
    pub packet: String,
    pub id: u32,
    /// `drop`, `merge`, `split`, `port` or `internal`.
    pub status: &'static str,
    /// The message (or endpoint) that replaces the packet, or the reason of the drop.
    pub replacement: String,
    pub milestone: String,
}

/// C8: every packet a UI send function uses is a C->S row of the index; the notes list the dropped / merged ones.
pub fn c8(index: &[LegacyRow], sites: &[UiSite]) -> (Vec<SendSiteNote>, Vec<Finding>) {
    let mut notes = Vec::new();
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for site in sites {
        if !seen.insert(site.send_fn.as_str()) {
            out.push(err(
                "C8",
                &site.send_fn,
                "the send function is listed twice",
            ));
        }
        if site.legacy_ids.is_empty() {
            out.push(err(
                "C8",
                &site.send_fn,
                "the send function names no packet",
            ));
        }
        let mut any_live = false;
        for (name, id) in &site.legacy_ids {
            let row = index.iter().find(|r| {
                r.ns == Ns::Packet && r.id == *id && r.name == *name && r.dir != Dir::S2C
            });
            let Some(row) = row else {
                out.push(err("C8", &site.send_fn, format!("sends PACKETTYPE {name} 0x{id:08X}, which is not a C->S row of legacy_index.csv")));
                continue;
            };
            if row.status.is_mapped() {
                any_live = true;
            }
            if matches!(
                row.status,
                Status::Merge | Status::Drop | Status::Internal | Status::Split
            ) {
                notes.push(SendSiteNote {
                    send_fn: site.send_fn.clone(),
                    packet: name.clone(),
                    id: *id,
                    status: row.status.tag(),
                    replacement: if row.status.is_mapped() {
                        row.messages.join(", ")
                    } else {
                        row.note.clone()
                    },
                    milestone: row.ms.clone(),
                });
            }
        }
        if !any_live && site.status != SiteStatus::Dropped && !site.legacy_ids.is_empty() {
            out.push(warn(
                "C8",
                &site.send_fn,
                "every packet of the function is dropped but the site is not marked `dropped`",
            ));
        }
    }
    (notes, out)
}

/// Settings of a run.
#[derive(Clone, Debug, Default)]
pub struct Options {
    /// Rank of `PROTO_STUB_MAX_MILESTONE` (`M3` = 3); C9 measures the due `todo` rows against it as well.
    pub stub_max: u32,
    /// The markers of a scan of `web/src` (rule C9); `None` skips C9 (the C1-C8 fixtures, a tree without `web/src`).
    pub markers: Option<c9::Markers>,
}

/// The outcome of all checks.
#[derive(Debug)]
pub struct Checked {
    pub findings: Vec<Finding>,
    pub stubs: Vec<Stub>,
    pub send_notes: Vec<SendSiteNote>,
    /// The C9 section of the report.
    pub c9: c9::Summary,
}

impl Checked {
    /// Number of errors.
    pub fn errors(&self) -> usize {
        self.findings
            .iter()
            .filter(|f| f.severity == Severity::Error)
            .count()
    }
}

/// Runs C1-C9.
pub fn run_all(model: &Model, index: &[LegacyRow], sites: &[UiSite], opts: &Options) -> Checked {
    let mut findings = Vec::new();
    findings.extend(c1(model, index));
    findings.extend(c2(model, index));
    findings.extend(c3(model));
    findings.extend(c4(model));
    findings.extend(c5(model));
    let (stubs, f6) = c6(model, opts.stub_max);
    findings.extend(f6);
    findings.extend(c7(model));
    let (send_notes, f8) = c8(index, sites);
    findings.extend(f8);
    let c9 = match &opts.markers {
        Some(markers) => {
            let (summary, f9) = c9::check(sites, markers, opts.stub_max);
            findings.extend(f9);
            summary
        }
        None => c9::Summary::skipped("no web/src was scanned: rule C9 was not evaluated"),
    };
    Checked {
        findings,
        stubs,
        send_notes,
        c9,
    }
}

/// Message lookup helper for reports.
pub fn message_of<'a>(model: &'a Model, full: &str) -> Option<&'a Message> {
    model.messages.get(full)
}
