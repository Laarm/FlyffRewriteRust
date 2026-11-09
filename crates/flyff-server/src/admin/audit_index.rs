//! The admin audit viewer's index and query engine (Plan/25 §10.2, 13 §25; WP-ADMIN-29).
//!
//! **Source.** `logs/audit/audit-YYYY-MM-DD.jsonl` (UTC day files, one JSON object per line, written by
//! `flyff_db::audit`). **Index.** One file per day, `logs/audit/index/YYYY-MM-DD.idx`, with per-key postings (line
//! offsets, ascending, varint deltas) for the event name, the actor, the character ids, the account ids and the item
//! serials of every line, plus a table of blocks of [`BLOCK_LINES`] lines with their `ts` range. It is built lazily on
//! the first query that touches a day, extended incrementally while the day is today, rewritten at most once a minute
//! for today (once for a finished day) and deleted with its JSONL file at retention (`flyff_db::audit::sweep_dir`).
//! A damaged, foreign or stale index is rebuilt, never trusted.
//!
//! **What a line is about** ([`LineKeys`], the one definition both paths use):
//!
//! | key | where in the line |
//! |---|---|
//! | `ts` | top-level `ts` (unsigned integer) |
//! | `ev` | top-level `ev` |
//! | actors (lower case) | `data.actor`, `data.by` (strings; `staff:joe`, `owner:local`, `cli`) |
//! | characters | top-level `chr`; anywhere in `data` an unsigned integer under one of [`CHAR_KEYS`], or the integers of an array under one of [`CHAR_ARRAY_KEYS`] |
//! | accounts | top-level `acc`; the same for [`ACC_KEYS`] / [`ACC_ARRAY_KEYS`] |
//! | serials | anywhere in `data`: an unsigned integer under `serial`, the integers of an array under `serials` |
//!
//! [`scan_line`] extracts them in one pass without building a JSON tree (it is what makes a 1,000,000-line day
//! indexable in about a second); [`reference_keys`] does the same over a `serde_json::Value` and is what the tests
//! compare it with. A query ([`Filter`]) is a time range, an event filter (names and group prefixes), an actor, a
//! character, an account, a serial and a free-text substring; it is answered from the postings when any of the first
//! five is set ([`AuditStore::query`] with `use_index`) or by scanning the day file (the reference, and the fallback).
//! Both give the same lines in the same order (newest first), which `indexed_queries_equal_a_full_scan` checks on a
//! 1,000,000-line fixture.

use flyff_db::calendar;
use parking_lot::Mutex;
use std::collections::{BTreeMap, VecDeque};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use xxhash_rust::xxh3::xxh3_64;

/// Lines per block of the index (the unit of time-range skipping).
pub const BLOCK_LINES: usize = 1024;
/// The index of a past day is kept in memory for the next query; this many days at most.
const CACHE_DAYS: usize = 3;
/// The index of today is written to disk at most this often (milliseconds).
const PERSIST_EVERY_MS: i64 = 60_000;
/// A chunk of the day file read at once.
const CHUNK: usize = 4 << 20;
/// The magic of an index file.
const MAGIC: &[u8; 4] = b"FAIX";
/// The format version.
const VERSION: u8 = 1;
/// Bytes before the end of the indexed part that are hashed to notice a replaced file.
const TAIL_HASH_BYTES: u64 = 4096;

/// Keys under which an integer in `data` is a character id.
pub const CHAR_KEYS: &[&str] = &[
    "chr",
    "character",
    "character_id",
    "target_chr",
    "with_chr",
    "receiver",
    "victim",
    "killer",
    "seller",
    "buyer",
    "master",
    "member",
    "owner",
    "target",
    "from_chr",
];
/// Keys under which an array of integers in `data` is a list of character ids.
pub const CHAR_ARRAY_KEYS: &[&str] = &["characters", "recipients", "members"];
/// Keys under which an integer in `data` is an account id.
pub const ACC_KEYS: &[&str] = &["acc", "account_id", "target_account"];
/// Keys under which an array of integers in `data` is a list of account ids.
pub const ACC_ARRAY_KEYS: &[&str] = &["accounts"];
/// Keys under which an integer in `data` is an item serial.
pub const SERIAL_KEYS: &[&str] = &["serial"];
/// Keys under which an array of integers in `data` is a list of item serials.
pub const SERIAL_ARRAY_KEYS: &[&str] = &["serials"];
/// Keys of `data` (top level only) that name who did it.
pub const ACTOR_KEYS: &[&str] = &["actor", "by"];

// ---- what a line is about -------------------------------------------------------------------------------------------

/// The keys of one audit line (see the module documentation).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LineKeys {
    /// `ts` in milliseconds (0 when the line has none).
    pub ts: i64,
    /// `ev`.
    pub ev: String,
    /// The actors, lower case, sorted, without duplicates.
    pub actors: Vec<String>,
    /// The character ids, sorted, without duplicates.
    pub chrs: Vec<u32>,
    /// The account ids, sorted, without duplicates.
    pub accs: Vec<u32>,
    /// The item serials, sorted, without duplicates.
    pub serials: Vec<u64>,
}

impl LineKeys {
    fn finish(&mut self) {
        self.actors.sort();
        self.actors.dedup();
        self.chrs.sort_unstable();
        self.chrs.dedup();
        self.accs.sort_unstable();
        self.accs.dedup();
        self.serials.sort_unstable();
        self.serials.dedup();
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Ctx {
    /// The top-level object of the line.
    Top,
    /// Inside `data`; the number is the depth (1 = the keys of `data` itself).
    Data(u8),
    /// A value nobody looks into.
    Skip,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    None,
    Ts,
    Ev,
    Chr,
    Acc,
    Serial,
    ChrArr,
    AccArr,
    SerialArr,
    Actor,
}

/// The single-pass scanner behind [`scan_line`].
struct Scan<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> Scan<'a> {
    fn ws(&mut self) {
        while let Some(c) = self.b.get(self.i) {
            if matches!(c, b' ' | b'\t' | b'\r' | b'\n') {
                self.i += 1;
            } else {
                break;
            }
        }
    }

    fn peek(&self) -> Option<u8> {
        self.b.get(self.i).copied()
    }

    /// At `"`: skips the string and returns its content (between the quotes) and whether it has escapes.
    fn string(&mut self) -> Option<(&'a [u8], bool)> {
        if self.peek() != Some(b'"') {
            return None;
        }
        let start = self.i + 1;
        let mut j = start;
        let mut escaped = false;
        loop {
            match *self.b.get(j)? {
                b'"' => break,
                b'\\' => {
                    escaped = true;
                    j += 2;
                }
                _ => j += 1,
            }
        }
        self.i = j + 1;
        Some((self.b.get(start..j)?, escaped))
    }

    /// The text of a string token, unescaped when it needs it.
    fn text(raw: &[u8], escaped: bool) -> Option<String> {
        if !escaped {
            return std::str::from_utf8(raw).ok().map(str::to_owned);
        }
        let mut quoted = Vec::with_capacity(raw.len() + 2);
        quoted.push(b'"');
        quoted.extend_from_slice(raw);
        quoted.push(b'"');
        serde_json::from_slice::<String>(&quoted).ok()
    }

    /// At a number: its value when it is an unsigned integer that fits `u64`, `Ok(None)` for any other number;
    /// `Err` for a malformed one (a leading zero, a lone minus).
    fn number(&mut self) -> Result<Option<u64>, ()> {
        let start = self.i;
        if self.peek() == Some(b'-') {
            self.i += 1;
        }
        let digits_from = self.i;
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.i += 1;
        }
        let digits = self.i - digits_from;
        if digits == 0 {
            return Err(());
        }
        if digits > 1 && self.b.get(digits_from) == Some(&b'0') {
            return Err(());
        }
        let mut other = self.b.get(start) == Some(&b'-');
        if self.peek() == Some(b'.') {
            other = true;
            self.i += 1;
            let f = self.i;
            while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                self.i += 1;
            }
            if self.i == f {
                return Err(());
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            other = true;
            self.i += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.i += 1;
            }
            let f = self.i;
            while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                self.i += 1;
            }
            if self.i == f {
                return Err(());
            }
        }
        if other {
            return Ok(None);
        }
        let text = std::str::from_utf8(self.b.get(digits_from..digits_from + digits).ok_or(())?)
            .map_err(|_| ())?;
        Ok(text.parse::<u64>().ok())
    }

    fn literal(&mut self, word: &[u8]) -> Option<()> {
        if self.b.get(self.i..self.i + word.len())? == word {
            self.i += word.len();
            Some(())
        } else {
            None
        }
    }

    fn object(&mut self, ctx: Ctx, out: &mut LineKeys) -> Option<()> {
        self.i += 1; // {
        self.ws();
        if self.peek() == Some(b'}') {
            self.i += 1;
            return Some(());
        }
        loop {
            self.ws();
            let (raw, esc) = self.string()?;
            self.ws();
            if self.peek() != Some(b':') {
                return None;
            }
            self.i += 1;
            self.ws();
            let key_text;
            let key: &str = if esc {
                key_text = Self::text(raw, true)?;
                &key_text
            } else {
                std::str::from_utf8(raw).ok()?
            };
            let (kind, next) = classify(ctx, key);
            self.value(next, kind, out)?;
            self.ws();
            match self.peek()? {
                b',' => self.i += 1,
                b'}' => {
                    self.i += 1;
                    return Some(());
                }
                _ => return None,
            }
        }
    }

    fn array(&mut self, ctx: Ctx, kind: Kind, out: &mut LineKeys) -> Option<()> {
        self.i += 1; // [
        self.ws();
        if self.peek() == Some(b']') {
            self.i += 1;
            return Some(());
        }
        let element = match kind {
            Kind::ChrArr => Kind::Chr,
            Kind::AccArr => Kind::Acc,
            Kind::SerialArr => Kind::Serial,
            _ => Kind::None,
        };
        loop {
            self.ws();
            self.value(ctx, element, out)?;
            self.ws();
            match self.peek()? {
                b',' => self.i += 1,
                b']' => {
                    self.i += 1;
                    return Some(());
                }
                _ => return None,
            }
        }
    }

    fn value(&mut self, ctx: Ctx, kind: Kind, out: &mut LineKeys) -> Option<()> {
        match self.peek()? {
            b'{' => {
                let inner = match ctx {
                    Ctx::Data(d) => Ctx::Data(d.saturating_add(1)),
                    other => other,
                };
                self.object(inner, out)
            }
            b'[' => {
                let inner = match ctx {
                    Ctx::Data(d) => Ctx::Data(d.saturating_add(1)),
                    other => other,
                };
                self.array(inner, kind, out)
            }
            b'"' => {
                let (raw, esc) = self.string()?;
                match kind {
                    Kind::Ev => out.ev = Self::text(raw, esc)?,
                    Kind::Actor => {
                        let t = Self::text(raw, esc)?;
                        out.actors.push(t.to_lowercase());
                    }
                    _ => {}
                }
                Some(())
            }
            b't' => self.literal(b"true"),
            b'f' => self.literal(b"false"),
            b'n' => self.literal(b"null"),
            b'-' | b'0'..=b'9' => {
                let n = self.number().ok()?;
                if let Some(n) = n {
                    match kind {
                        Kind::Ts => out.ts = i64::try_from(n).unwrap_or(0),
                        Kind::Chr => {
                            if let Ok(v) = u32::try_from(n) {
                                out.chrs.push(v);
                            }
                        }
                        Kind::Acc => {
                            if let Ok(v) = u32::try_from(n) {
                                out.accs.push(v);
                            }
                        }
                        Kind::Serial => out.serials.push(n),
                        _ => {}
                    }
                }
                Some(())
            }
            _ => None,
        }
    }
}

/// What the value of `key` is, and the context of the value's own keys.
fn classify(ctx: Ctx, key: &str) -> (Kind, Ctx) {
    match ctx {
        Ctx::Top => match key {
            "ts" => (Kind::Ts, Ctx::Skip),
            "ev" => (Kind::Ev, Ctx::Skip),
            "acc" => (Kind::Acc, Ctx::Skip),
            "chr" => (Kind::Chr, Ctx::Skip),
            // entering the object adds one: its own keys are depth 1
            "data" => (Kind::None, Ctx::Data(0)),
            _ => (Kind::None, Ctx::Skip),
        },
        Ctx::Data(d) => {
            let kind = if CHAR_KEYS.contains(&key) {
                Kind::Chr
            } else if CHAR_ARRAY_KEYS.contains(&key) {
                Kind::ChrArr
            } else if ACC_KEYS.contains(&key) {
                Kind::Acc
            } else if ACC_ARRAY_KEYS.contains(&key) {
                Kind::AccArr
            } else if SERIAL_KEYS.contains(&key) {
                Kind::Serial
            } else if SERIAL_ARRAY_KEYS.contains(&key) {
                Kind::SerialArr
            } else if d == 1 && ACTOR_KEYS.contains(&key) {
                Kind::Actor
            } else {
                Kind::None
            };
            (kind, Ctx::Data(d))
        }
        Ctx::Skip => (Kind::None, Ctx::Skip),
    }
}

/// Extracts the keys of one line in a single pass. `None` for anything that is not one JSON object.
pub fn scan_line(line: &[u8]) -> Option<LineKeys> {
    let mut s = Scan { b: line, i: 0 };
    s.ws();
    if s.peek() != Some(b'{') {
        return None;
    }
    let mut out = LineKeys::default();
    s.object(Ctx::Top, &mut out)?;
    s.ws();
    if s.i != line.len() {
        return None;
    }
    out.finish();
    Some(out)
}

/// The same keys from a parsed JSON tree: the definition [`scan_line`] is tested against.
pub fn reference_keys(line: &[u8]) -> Option<LineKeys> {
    let v: serde_json::Value = serde_json::from_slice(line).ok()?;
    let obj = v.as_object()?;
    let mut out = LineKeys::default();
    if let Some(ts) = obj.get("ts").and_then(serde_json::Value::as_u64) {
        out.ts = i64::try_from(ts).unwrap_or(0);
    }
    if let Some(ev) = obj.get("ev").and_then(serde_json::Value::as_str) {
        out.ev = ev.to_owned();
    }
    for (key, list) in [("acc", &mut out.accs), ("chr", &mut out.chrs)] {
        if let Some(n) = obj
            .get(key)
            .and_then(serde_json::Value::as_u64)
            .and_then(|n| u32::try_from(n).ok())
        {
            list.push(n);
        }
    }
    if let Some(data) = obj.get("data") {
        walk(data, 1, &mut out);
    }
    out.finish();
    Some(out)
}

fn walk(v: &serde_json::Value, depth: u8, out: &mut LineKeys) {
    use serde_json::Value;
    match v {
        Value::Object(m) => {
            for (k, x) in m {
                let k = k.as_str();
                let uint = x.as_u64();
                let u32v = uint.and_then(|n| u32::try_from(n).ok());
                if CHAR_KEYS.contains(&k) {
                    out.chrs.extend(u32v);
                } else if ACC_KEYS.contains(&k) {
                    out.accs.extend(u32v);
                } else if SERIAL_KEYS.contains(&k) {
                    out.serials.extend(uint);
                } else if depth == 1
                    && ACTOR_KEYS.contains(&k)
                    && let Value::String(s) = x
                {
                    out.actors.push(s.to_lowercase());
                }
                if let Value::Array(a) = x {
                    let ints = a.iter().filter_map(Value::as_u64);
                    if CHAR_ARRAY_KEYS.contains(&k) {
                        out.chrs.extend(ints.filter_map(|n| u32::try_from(n).ok()));
                    } else if ACC_ARRAY_KEYS.contains(&k) {
                        out.accs.extend(ints.filter_map(|n| u32::try_from(n).ok()));
                    } else if SERIAL_ARRAY_KEYS.contains(&k) {
                        out.serials.extend(ints);
                    }
                }
                walk(x, depth.saturating_add(1), out);
            }
        }
        Value::Array(a) => {
            for x in a {
                walk(x, depth.saturating_add(1), out);
            }
        }
        _ => {}
    }
}

// ---- event groups ---------------------------------------------------------------------------------------------------

/// The event groups of the viewer's filter (Plan/25 §10.2): name and the event-name prefixes it covers.
pub const GROUPS: &[(&str, &[&str])] = &[
    ("account", &["account.", "session."]),
    (
        "character",
        &["char.", "quest.", "skill.", "pet.", "honor.", "exp_box."],
    ),
    ("item", &["item.", "grant.", "removal."]),
    (
        "economy",
        &[
            "shop.", "trade.", "vendor.", "mail.", "bank.", "pocket.", "tax.",
        ],
    ),
    ("guild", &["guild.", "siege.", "gc1to1."]),
    // `gm.command` is listed here first: the console's own commands belong to the admin group (it is also a `gm.` event)
    ("admin", &["admin.", "gm.command"]),
    ("gm", &["gm."]),
    ("server", &["server.", "persist.", "client."]),
];

/// The prefixes of group `name`, `None` for an unknown group.
pub fn group_prefixes(name: &str) -> Option<&'static [&'static str]> {
    GROUPS.iter().find(|(g, _)| *g == name).map(|(_, p)| *p)
}

/// The group an event belongs to (`"other"` when none).
pub fn group_of(ev: &str) -> &'static str {
    GROUPS
        .iter()
        .find(|(_, prefixes)| prefixes.iter().any(|p| ev.starts_with(p)))
        .map_or("other", |(g, _)| g)
}

// ---- the filter -----------------------------------------------------------------------------------------------------

/// Event names and name prefixes a line must match (any of them); empty = every event.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EventFilter {
    /// Exact event names (`admin.kick`).
    pub names: Vec<String>,
    /// Name prefixes (`admin.`); a prefix without a trailing `.` matches the exact name only when it is one.
    pub prefixes: Vec<String>,
}

impl EventFilter {
    /// `true` when no event restricts the query.
    pub fn is_empty(&self) -> bool {
        self.names.is_empty() && self.prefixes.is_empty()
    }

    /// `true` when `ev` passes.
    pub fn matches(&self, ev: &str) -> bool {
        self.is_empty()
            || self.names.iter().any(|n| n == ev)
            || self.prefixes.iter().any(|p| ev.starts_with(p.as_str()))
    }

    /// The filter of event groups by name (unknown groups are ignored).
    pub fn of_groups(groups: &[String]) -> EventFilter {
        let mut f = EventFilter::default();
        for g in groups {
            if let Some(prefixes) = group_prefixes(g) {
                f.prefixes.extend(prefixes.iter().map(|p| (*p).to_owned()));
            }
        }
        f
    }
}

/// A query of the audit viewer (Plan/25 §10.2).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Filter {
    /// Inclusive lower bound of `ts` in ms.
    pub from_ms: i64,
    /// Exclusive upper bound of `ts` in ms (`i64::MAX` = open).
    pub to_ms: i64,
    /// Event names and groups.
    pub events: EventFilter,
    /// The actor (`staff:joe`), compared case-insensitively.
    pub actor: Option<String>,
    /// A character id.
    pub character: Option<u32>,
    /// An account id.
    pub account: Option<u32>,
    /// An item serial.
    pub serial: Option<u64>,
    /// A substring of the raw line, compared case-insensitively.
    pub text: Option<String>,
}

impl Filter {
    /// A filter over `[from_ms, to_ms)` with nothing else set.
    pub fn range(from_ms: i64, to_ms: i64) -> Filter {
        Filter {
            from_ms,
            to_ms,
            ..Filter::default()
        }
    }

    fn lowered(&self) -> Lowered<'_> {
        Lowered {
            f: self,
            actor: self.actor.as_ref().map(|a| a.to_lowercase()),
            text: self.text.as_ref().map(|t| t.to_lowercase()),
        }
    }

    /// `true` when postings can narrow the query (an event, actor, character, account or serial is set).
    pub fn indexable(&self) -> bool {
        !self.events.is_empty()
            || self.actor.is_some()
            || self.character.is_some()
            || self.account.is_some()
            || self.serial.is_some()
    }
}

struct Lowered<'a> {
    f: &'a Filter,
    actor: Option<String>,
    text: Option<String>,
}

impl Lowered<'_> {
    /// The whole filter against the keys and the raw line.
    fn matches(&self, k: &LineKeys, raw: &[u8]) -> bool {
        let f = self.f;
        if k.ts < f.from_ms || k.ts >= f.to_ms {
            return false;
        }
        if !f.events.matches(&k.ev) {
            return false;
        }
        if let Some(a) = &self.actor
            && !k.actors.iter().any(|x| x == a)
        {
            return false;
        }
        if f.character.is_some_and(|c| !k.chrs.contains(&c))
            || f.account.is_some_and(|c| !k.accs.contains(&c))
            || f.serial.is_some_and(|c| !k.serials.contains(&c))
        {
            return false;
        }
        match &self.text {
            Some(t) => contains_ci(raw, t),
            None => true,
        }
    }
}

/// Case-insensitive substring of a UTF-8 line (`needle` is lower case).
fn contains_ci(hay: &[u8], needle: &str) -> bool {
    let text = String::from_utf8_lossy(hay);
    if text.is_ascii() && needle.is_ascii() {
        // the common case without allocating a lower-cased copy
        let n = needle.as_bytes();
        if n.is_empty() {
            return true;
        }
        let h = text.as_bytes();
        return h.windows(n.len()).any(|w| w.eq_ignore_ascii_case(n));
    }
    text.to_lowercase().contains(needle)
}

// ---- the per-day index ----------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Block {
    offset: u64,
    lines: u32,
    min_ts: i64,
    max_ts: i64,
}

/// The index of one day file (see the module documentation).
#[derive(Debug, Default)]
pub struct DayIndex {
    day: i64,
    indexed_len: u64,
    lines: u64,
    blocks: Vec<Block>,
    ev: BTreeMap<String, Vec<u64>>,
    actor: BTreeMap<String, Vec<u64>>,
    chr: BTreeMap<u32, Vec<u64>>,
    acc: BTreeMap<u32, Vec<u64>>,
    serial: BTreeMap<u64, Vec<u64>>,
    tail_hash: u64,
    /// Lines were added since the index was written.
    dirty: bool,
    /// When the index was last written (0 = not by this process).
    persisted_at_ms: i64,
}

impl DayIndex {
    fn new(day: i64) -> DayIndex {
        DayIndex {
            day,
            ..DayIndex::default()
        }
    }

    /// Lines indexed so far.
    pub fn lines(&self) -> u64 {
        self.lines
    }

    /// Bytes of the day file the index covers.
    pub fn indexed_len(&self) -> u64 {
        self.indexed_len
    }

    fn add_line(&mut self, offset: u64, keys: Option<&LineKeys>) {
        let ts = keys.map_or(0, |k| k.ts);
        let new_block = self
            .blocks
            .last()
            .is_none_or(|b| b.lines as usize >= BLOCK_LINES);
        if new_block {
            self.blocks.push(Block {
                offset,
                lines: 0,
                min_ts: i64::MAX,
                max_ts: i64::MIN,
            });
        }
        if let Some(b) = self.blocks.last_mut() {
            b.lines += 1;
            b.min_ts = b.min_ts.min(ts);
            b.max_ts = b.max_ts.max(ts);
        }
        self.lines += 1;
        let Some(k) = keys else {
            return;
        };
        push_str(&mut self.ev, &k.ev, offset);
        for a in &k.actors {
            push_str(&mut self.actor, a, offset);
        }
        for c in &k.chrs {
            self.chr.entry(*c).or_default().push(offset);
        }
        for c in &k.accs {
            self.acc.entry(*c).or_default().push(offset);
        }
        for s in &k.serials {
            self.serial.entry(*s).or_default().push(offset);
        }
    }

    /// Indexes the complete lines of `file` from `indexed_len` to `file_len`.
    fn extend(&mut self, file: &mut File, file_len: u64) -> std::io::Result<()> {
        file.seek(SeekFrom::Start(self.indexed_len))?;
        let mut pos = self.indexed_len;
        let mut carry: Vec<u8> = Vec::new();
        let mut buf = vec![0_u8; CHUNK];
        while pos + (carry.len() as u64) < file_len {
            let want = usize::try_from((file_len - pos - carry.len() as u64).min(CHUNK as u64))
                .unwrap_or(CHUNK);
            let n = file.read(buf.get_mut(..want).unwrap_or(&mut []))?;
            if n == 0 {
                break;
            }
            carry.extend_from_slice(buf.get(..n).unwrap_or(&[]));
            let mut start = 0;
            while let Some(nl) = carry
                .get(start..)
                .and_then(|s| s.iter().position(|b| *b == b'\n'))
            {
                let end = start + nl;
                let line = carry.get(start..end).unwrap_or(&[]);
                let keys = scan_line(line);
                self.add_line(pos + start as u64, keys.as_ref());
                start = end + 1;
            }
            pos += start as u64;
            carry.drain(..start);
        }
        // `carry` holds an incomplete last line (or nothing): it is not indexed yet
        if pos > self.indexed_len {
            self.dirty = true;
        }
        self.indexed_len = pos;
        self.tail_hash = tail_hash_of(file, pos)?;
        Ok(())
    }

    fn block_of(&self, offset: u64) -> Option<&Block> {
        let i = self.blocks.partition_point(|b| b.offset <= offset);
        self.blocks.get(i.checked_sub(1)?)
    }

    /// The offsets of the lines that match every set indexable dimension of `f` (`None` = no dimension is set).
    fn candidates(&self, f: &Filter, low: &Lowered<'_>) -> Option<Vec<u64>> {
        let mut dims: Vec<Vec<u64>> = Vec::new();
        if !f.events.is_empty() {
            let lists: Vec<&Vec<u64>> = self
                .ev
                .iter()
                .filter(|(name, _)| f.events.matches(name))
                .map(|(_, l)| l)
                .collect();
            dims.push(union(&lists));
        }
        if let Some(a) = &low.actor {
            dims.push(self.actor.get(a).cloned().unwrap_or_default());
        }
        if let Some(c) = f.character {
            dims.push(self.chr.get(&c).cloned().unwrap_or_default());
        }
        if let Some(c) = f.account {
            dims.push(self.acc.get(&c).cloned().unwrap_or_default());
        }
        if let Some(s) = f.serial {
            dims.push(self.serial.get(&s).cloned().unwrap_or_default());
        }
        if dims.is_empty() {
            return None;
        }
        dims.sort_by_key(Vec::len);
        let mut it = dims.into_iter();
        let mut acc = it.next().unwrap_or_default();
        for next in it {
            acc = intersect(&acc, &next);
            if acc.is_empty() {
                break;
            }
        }
        Some(acc)
    }

    // ---- file format ----

    fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(1 << 20);
        out.extend_from_slice(MAGIC);
        out.push(VERSION);
        out.extend_from_slice(&self.day.to_le_bytes());
        out.extend_from_slice(&self.indexed_len.to_le_bytes());
        out.extend_from_slice(&self.lines.to_le_bytes());
        out.extend_from_slice(&self.tail_hash.to_le_bytes());
        put_u32(&mut out, self.blocks.len());
        for b in &self.blocks {
            out.extend_from_slice(&b.offset.to_le_bytes());
            out.extend_from_slice(&b.lines.to_le_bytes());
            out.extend_from_slice(&b.min_ts.to_le_bytes());
            out.extend_from_slice(&b.max_ts.to_le_bytes());
        }
        put_map(&mut out, &self.ev, |o, k| put_str(o, k));
        put_map(&mut out, &self.actor, |o, k| put_str(o, k));
        put_map(&mut out, &self.chr, |o, k| {
            o.extend_from_slice(&k.to_le_bytes())
        });
        put_map(&mut out, &self.acc, |o, k| {
            o.extend_from_slice(&k.to_le_bytes())
        });
        put_map(&mut out, &self.serial, |o, k| {
            o.extend_from_slice(&k.to_le_bytes());
        });
        let sum = xxh3_64(&out);
        out.extend_from_slice(&sum.to_le_bytes());
        out
    }

    fn decode(bytes: &[u8]) -> Option<DayIndex> {
        let body_len = bytes.len().checked_sub(8)?;
        let (body, sum) = bytes.split_at(body_len);
        if xxh3_64(body) != u64::from_le_bytes(sum.try_into().ok()?) {
            return None;
        }
        let mut r = Reader { b: body, i: 0 };
        if r.take(4)? != MAGIC || r.u8()? != VERSION {
            return None;
        }
        let mut idx = DayIndex {
            day: r.i64()?,
            indexed_len: r.u64()?,
            lines: r.u64()?,
            tail_hash: r.u64()?,
            ..DayIndex::default()
        };
        let n = r.u32()? as usize;
        idx.blocks.reserve(n.min(1 << 20));
        for _ in 0..n {
            idx.blocks.push(Block {
                offset: r.u64()?,
                lines: r.u32()?,
                min_ts: r.i64()?,
                max_ts: r.i64()?,
            });
        }
        idx.ev = get_map(&mut r, |r| r.string())?;
        idx.actor = get_map(&mut r, |r| r.string())?;
        idx.chr = get_map(&mut r, |r| r.u32())?;
        idx.acc = get_map(&mut r, |r| r.u32())?;
        idx.serial = get_map(&mut r, |r| r.u64())?;
        (r.i == body.len()).then_some(idx)
    }
}

fn push_str(map: &mut BTreeMap<String, Vec<u64>>, key: &str, offset: u64) {
    if let Some(list) = map.get_mut(key) {
        list.push(offset);
    } else {
        map.insert(key.to_owned(), vec![offset]);
    }
}

/// The sorted union of sorted lists.
fn union(lists: &[&Vec<u64>]) -> Vec<u64> {
    match lists {
        [] => Vec::new(),
        [one] => (*one).clone(),
        many => {
            let mut all: Vec<u64> = many.iter().flat_map(|l| l.iter().copied()).collect();
            all.sort_unstable();
            all.dedup();
            all
        }
    }
}

/// The sorted intersection of two sorted lists.
fn intersect(a: &[u64], b: &[u64]) -> Vec<u64> {
    let mut out = Vec::with_capacity(a.len().min(b.len()));
    let (mut i, mut j) = (0, 0);
    while let (Some(x), Some(y)) = (a.get(i), b.get(j)) {
        match x.cmp(y) {
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
            std::cmp::Ordering::Equal => {
                out.push(*x);
                i += 1;
                j += 1;
            }
        }
    }
    out
}

fn put_u32(out: &mut Vec<u8>, n: usize) {
    out.extend_from_slice(&u32::try_from(n).unwrap_or(u32::MAX).to_le_bytes());
}

fn put_str(out: &mut Vec<u8>, s: &str) {
    let b = s.as_bytes();
    let n = b.len().min(usize::from(u16::MAX));
    out.extend_from_slice(&u16::try_from(n).unwrap_or(u16::MAX).to_le_bytes());
    out.extend_from_slice(b.get(..n).unwrap_or(&[]));
}

fn put_varint(out: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        out.push((v as u8 & 0x7f) | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

fn put_map<K>(out: &mut Vec<u8>, map: &BTreeMap<K, Vec<u64>>, put_key: impl Fn(&mut Vec<u8>, &K)) {
    put_u32(out, map.len());
    for (k, list) in map {
        put_key(out, k);
        put_u32(out, list.len());
        let mut prev = 0_u64;
        for v in list {
            put_varint(out, v.wrapping_sub(prev));
            prev = *v;
        }
    }
}

/// A bounds-checked reader over an index file (the file is untrusted).
struct Reader<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let s = self.b.get(self.i..self.i.checked_add(n)?)?;
        self.i += n;
        Some(s)
    }
    fn u8(&mut self) -> Option<u8> {
        self.take(1)?.first().copied()
    }
    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes(self.take(2)?.try_into().ok()?))
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn u64(&mut self) -> Option<u64> {
        Some(u64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }
    fn i64(&mut self) -> Option<i64> {
        Some(i64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }
    fn string(&mut self) -> Option<String> {
        let n = usize::from(self.u16()?);
        String::from_utf8(self.take(n)?.to_vec()).ok()
    }
    fn varint(&mut self) -> Option<u64> {
        let mut v = 0_u64;
        let mut shift = 0;
        loop {
            let b = self.u8()?;
            v |= u64::from(b & 0x7f).checked_shl(shift)?;
            if b & 0x80 == 0 {
                return Some(v);
            }
            shift += 7;
            if shift > 63 {
                return None;
            }
        }
    }
}

fn get_map<K: Ord>(
    r: &mut Reader<'_>,
    key: impl Fn(&mut Reader<'_>) -> Option<K>,
) -> Option<BTreeMap<K, Vec<u64>>> {
    let n = r.u32()? as usize;
    let mut map = BTreeMap::new();
    for _ in 0..n {
        let k = key(r)?;
        let count = r.u32()? as usize;
        // a count larger than the bytes left is a damaged file
        if count > r.b.len().saturating_sub(r.i) {
            return None;
        }
        let mut list = Vec::with_capacity(count);
        let mut prev = 0_u64;
        for _ in 0..count {
            prev = prev.checked_add(r.varint()?)?;
            list.push(prev);
        }
        map.insert(k, list);
    }
    Some(map)
}

/// The hash of the bytes just before `end` (up to [`TAIL_HASH_BYTES`]): a replaced or truncated file changes it.
fn tail_hash_of(file: &mut File, end: u64) -> std::io::Result<u64> {
    let from = end.saturating_sub(TAIL_HASH_BYTES);
    file.seek(SeekFrom::Start(from))?;
    let mut buf = vec![0_u8; usize::try_from(end - from).unwrap_or(0)];
    file.read_exact(&mut buf)?;
    Ok(xxh3_64(&buf))
}

// ---- the store ------------------------------------------------------------------------------------------------------

/// A position in the result list: continue with the lines before `offset` of `day`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cursor {
    /// UTC day index.
    pub day: i64,
    /// Byte offset of the last line returned (exclusive upper bound of the next page).
    pub offset: u64,
}

impl Cursor {
    /// `"<day>.<offset>"`.
    pub fn encode(&self) -> String {
        format!("{}.{}", self.day, self.offset)
    }

    /// Parses [`Cursor::encode`].
    pub fn decode(text: &str) -> Option<Cursor> {
        let (d, o) = text.split_once('.')?;
        Some(Cursor {
            day: d.parse().ok()?,
            offset: o.parse().ok()?,
        })
    }
}

/// One matching line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hit {
    /// UTC day index of the file.
    pub day: i64,
    /// Byte offset of the line in the day file (with `day`, its identity).
    pub offset: u64,
    /// `ts` of the line.
    pub ts_ms: i64,
    /// The raw line (without the newline).
    pub line: String,
    /// The keys of the line.
    pub keys: LineKeys,
}

/// Which lines of one day a call wants: those before `upper` (the offset of a cursor), at most `want`.
#[derive(Clone, Copy)]
struct Window {
    upper: u64,
    want: usize,
}

/// One page of a query.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Page {
    /// The matches, newest first.
    pub hits: Vec<Hit>,
    /// Where the next page starts; `None` at the end.
    pub next: Option<Cursor>,
    /// `true` when the postings answered (not a scan).
    pub indexed: bool,
    /// Lines read from files (0 for a pure postings answer).
    pub examined: u64,
}

/// The audit directory and the indexes of the days that were queried lately.
pub struct AuditStore {
    dir: PathBuf,
    cache: Mutex<VecDeque<(i64, Arc<Mutex<DayIndex>>)>>,
}

impl std::fmt::Debug for AuditStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuditStore")
            .field("dir", &self.dir)
            .finish_non_exhaustive()
    }
}

impl AuditStore {
    /// A store over `dir` (`logs/audit`).
    pub fn new(dir: impl Into<PathBuf>) -> AuditStore {
        AuditStore {
            dir: dir.into(),
            cache: Mutex::new(VecDeque::new()),
        }
    }

    /// The audit directory.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn day_file(&self, day: i64) -> PathBuf {
        self.dir
            .join(format!("audit-{}.jsonl", calendar::date_string(day)))
    }

    fn index_file(&self, day: i64) -> PathBuf {
        flyff_db::audit::index_path(&self.dir, &calendar::date_string(day))
    }

    /// The days that have a JSONL file, newest first.
    pub fn days(&self) -> Vec<i64> {
        let mut days: Vec<i64> = std::fs::read_dir(&self.dir)
            .map(|rd| {
                rd.filter_map(Result::ok)
                    .filter_map(|e| {
                        let name = e.file_name();
                        name.to_str()
                            .and_then(|n| n.strip_prefix("audit-"))
                            .and_then(|n| n.strip_suffix(".jsonl"))
                            .and_then(calendar::parse_date)
                    })
                    .collect()
            })
            .unwrap_or_default();
        days.sort_unstable_by(|a, b| b.cmp(a));
        days
    }

    /// The index of `day`, loaded, extended or built as needed. `now_ms` decides when today's index is persisted.
    fn index(&self, day: i64, now_ms: i64) -> std::io::Result<Arc<Mutex<DayIndex>>> {
        let slot = {
            let mut cache = self.cache.lock();
            if let Some(pos) = cache.iter().position(|(d, _)| *d == day) {
                let entry = cache.remove(pos);
                match entry {
                    Some(e) => {
                        let slot = Arc::clone(&e.1);
                        cache.push_front(e);
                        slot
                    }
                    None => Arc::new(Mutex::new(DayIndex::new(day))),
                }
            } else {
                let slot = Arc::new(Mutex::new(self.load_or_new(day)));
                cache.push_front((day, Arc::clone(&slot)));
                cache.truncate(CACHE_DAYS);
                slot
            }
        };
        {
            let mut idx = slot.lock();
            let path = self.day_file(day);
            let mut file = File::open(&path)?;
            let len = file.metadata()?.len();
            // the file shrank or was replaced: start again
            let stale = len < idx.indexed_len
                || (idx.indexed_len > 0
                    && tail_hash_of(&mut file, idx.indexed_len).ok() != Some(idx.tail_hash));
            if stale {
                *idx = DayIndex::new(day);
            }
            if len > idx.indexed_len {
                idx.extend(&mut file, len)?;
            }
            let finished = day < calendar::utc_day_of_ms(now_ms);
            if idx.dirty && (finished || now_ms - idx.persisted_at_ms >= PERSIST_EVERY_MS) {
                match self.persist(&idx) {
                    Ok(()) => idx.dirty = false,
                    Err(e) => {
                        tracing::warn!(day, error = %e, "the audit index could not be written")
                    }
                }
                // a failed write is tried again after the persist interval, not at every query
                idx.persisted_at_ms = now_ms;
            }
        }
        Ok(slot)
    }

    fn load_or_new(&self, day: i64) -> DayIndex {
        let loaded = std::fs::read(self.index_file(day))
            .ok()
            .and_then(|b| DayIndex::decode(&b))
            .filter(|i| i.day == day);
        loaded.unwrap_or_else(|| DayIndex::new(day))
    }

    fn persist(&self, idx: &DayIndex) -> std::io::Result<()> {
        let path = self.index_file(idx.day);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("idx.tmp");
        {
            let mut f = File::create(&tmp)?;
            f.write_all(&idx.encode())?;
            f.flush()?;
        }
        std::fs::rename(&tmp, &path)
    }

    /// Forgets the in-memory indexes (tests that damage the files).
    pub fn forget(&self) {
        self.cache.lock().clear();
    }

    /// One page of the matches of `f`, newest first, from `cursor` on. `use_index = false` scans the day files (the
    /// reference the index is tested against; the answer is the same).
    pub fn query(
        &self,
        f: &Filter,
        cursor: Option<Cursor>,
        limit: usize,
        now_ms: i64,
        use_index: bool,
    ) -> std::io::Result<Page> {
        let limit = limit.clamp(1, 5000);
        let low = f.lowered();
        let first_day = calendar::utc_day_of_ms(f.from_ms.max(0));
        let last_day = if f.to_ms == i64::MAX {
            i64::MAX
        } else {
            calendar::utc_day_of_ms(f.to_ms.saturating_sub(1).max(0))
        };
        let mut page = Page {
            indexed: use_index && f.indexable(),
            ..Page::default()
        };
        let mut want = limit + 1;
        for day in self.days() {
            if day < first_day || day > last_day {
                continue;
            }
            let upper = match cursor {
                Some(c) if c.day < day => continue,
                Some(c) if c.day == day => c.offset,
                _ => u64::MAX,
            };
            let hits = if use_index {
                self.day_indexed(
                    day,
                    f,
                    &low,
                    Window { upper, want },
                    now_ms,
                    &mut page.examined,
                )?
            } else {
                self.day_scan(day, &low, upper, want, &mut page.examined)?
            };
            want -= hits.len();
            page.hits.extend(hits);
            if want == 0 {
                break;
            }
        }
        if page.hits.len() > limit {
            page.hits.truncate(limit);
            page.next = page.hits.last().map(|h| Cursor {
                day: h.day,
                offset: h.offset,
            });
        }
        Ok(page)
    }

    fn day_indexed(
        &self,
        day: i64,
        f: &Filter,
        low: &Lowered<'_>,
        window: Window,
        now_ms: i64,
        examined: &mut u64,
    ) -> std::io::Result<Vec<Hit>> {
        let Window { upper, want } = window;
        let slot = self.index(day, now_ms)?;
        let idx = slot.lock();
        let mut file = File::open(self.day_file(day))?;
        let mut hits = Vec::new();
        match idx.candidates(f, low) {
            Some(list) => {
                let end = list.partition_point(|o| *o < upper);
                for offset in list.get(..end).unwrap_or(&[]).iter().rev() {
                    if let Some(b) = idx.block_of(*offset)
                        && (b.max_ts < f.from_ms || b.min_ts >= f.to_ms)
                    {
                        continue;
                    }
                    let line = read_line(&mut file, *offset, idx.indexed_len)?;
                    *examined += 1;
                    let keys = scan_line(&line).unwrap_or_default();
                    if low.matches(&keys, &line) {
                        hits.push(hit(day, *offset, line, keys));
                        if hits.len() >= want {
                            break;
                        }
                    }
                }
            }
            None => {
                for b in idx.blocks.iter().rev() {
                    if b.offset >= upper || b.max_ts < f.from_ms || b.min_ts >= f.to_ms {
                        continue;
                    }
                    let next = block_end(&idx, b);
                    for (offset, line) in read_block(&mut file, b.offset, next)?.into_iter().rev() {
                        if offset >= upper {
                            continue;
                        }
                        *examined += 1;
                        let keys = scan_line(&line).unwrap_or_default();
                        if low.matches(&keys, &line) {
                            hits.push(hit(day, offset, line, keys));
                            if hits.len() >= want {
                                return Ok(hits);
                            }
                        }
                    }
                }
            }
        }
        Ok(hits)
    }

    fn day_scan(
        &self,
        day: i64,
        low: &Lowered<'_>,
        upper: u64,
        want: usize,
        examined: &mut u64,
    ) -> std::io::Result<Vec<Hit>> {
        let mut file = File::open(self.day_file(day))?;
        let len = file.metadata()?.len();
        let mut keep: VecDeque<Hit> = VecDeque::with_capacity(want.min(1024));
        let mut pos = 0_u64;
        let mut carry: Vec<u8> = Vec::new();
        let mut buf = vec![0_u8; CHUNK];
        while pos + (carry.len() as u64) < len {
            let n = file.read(&mut buf)?;
            if n == 0 {
                break;
            }
            carry.extend_from_slice(buf.get(..n).unwrap_or(&[]));
            let mut start = 0;
            while let Some(nl) = carry
                .get(start..)
                .and_then(|s| s.iter().position(|b| *b == b'\n'))
            {
                let end = start + nl;
                let offset = pos + start as u64;
                if offset < upper {
                    let line = carry.get(start..end).unwrap_or(&[]);
                    *examined += 1;
                    let keys = scan_line(line).unwrap_or_default();
                    if low.matches(&keys, line) {
                        if keep.len() >= want {
                            keep.pop_front();
                        }
                        keep.push_back(hit(day, offset, line.to_vec(), keys));
                    }
                }
                start = end + 1;
            }
            pos += start as u64;
            carry.drain(..start);
        }
        Ok(keep.into_iter().rev().collect())
    }
}

fn hit(day: i64, offset: u64, line: Vec<u8>, keys: LineKeys) -> Hit {
    Hit {
        day,
        offset,
        ts_ms: keys.ts,
        line: String::from_utf8_lossy(&line).into_owned(),
        keys,
    }
}

/// The end offset of block `b` (the start of the next block, or the end of the indexed part).
fn block_end(idx: &DayIndex, b: &Block) -> u64 {
    let i = idx.blocks.partition_point(|x| x.offset <= b.offset);
    idx.blocks.get(i).map_or(idx.indexed_len, |n| n.offset)
}

/// The line that starts at `offset` (without its newline).
fn read_line(file: &mut File, offset: u64, limit: u64) -> std::io::Result<Vec<u8>> {
    file.seek(SeekFrom::Start(offset))?;
    let mut out = Vec::with_capacity(256);
    let mut buf = [0_u8; 1024];
    let mut left = limit.saturating_sub(offset);
    while left > 0 {
        let want = usize::try_from(left.min(buf.len() as u64)).unwrap_or(buf.len());
        let n = file.read(buf.get_mut(..want).unwrap_or(&mut []))?;
        if n == 0 {
            break;
        }
        let chunk = buf.get(..n).unwrap_or(&[]);
        if let Some(nl) = chunk.iter().position(|b| *b == b'\n') {
            out.extend_from_slice(chunk.get(..nl).unwrap_or(&[]));
            return Ok(out);
        }
        out.extend_from_slice(chunk);
        left -= n as u64;
    }
    Ok(out)
}

/// The lines of the byte range `[from, to)` with their offsets.
fn read_block(file: &mut File, from: u64, to: u64) -> std::io::Result<Vec<(u64, Vec<u8>)>> {
    file.seek(SeekFrom::Start(from))?;
    let mut buf = vec![0_u8; usize::try_from(to.saturating_sub(from)).unwrap_or(0)];
    file.read_exact(&mut buf)?;
    let mut out = Vec::with_capacity(BLOCK_LINES);
    let mut start = 0;
    while let Some(nl) = buf
        .get(start..)
        .and_then(|s| s.iter().position(|b| *b == b'\n'))
    {
        let end = start + nl;
        out.push((
            from + start as u64,
            buf.get(start..end).unwrap_or(&[]).to_vec(),
        ));
        start = end + 1;
    }
    Ok(out)
}

// ---- rows for the API -----------------------------------------------------------------------------------------------

/// Keys whose values are personal data (IPs, e-mail addresses): removed for an identity without `view.pii` (25 §12).
pub const PII_KEYS: &[&str] = &["ip", "email", "last_login_ip", "peer", "addr", "remote_ip"];

/// Removes the PII keys from every object of `v`.
pub fn redact_pii(v: &mut serde_json::Value) {
    match v {
        serde_json::Value::Object(m) => {
            m.retain(|k, _| !PII_KEYS.contains(&k.as_str()));
            m.values_mut().for_each(redact_pii);
        }
        serde_json::Value::Array(a) => a.iter_mut().for_each(redact_pii),
        _ => {}
    }
}

/// A one-line description of an audit record for the table and the CSV export.
pub fn summarize(ev: &str, data: &serde_json::Value) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(state) = data.get("state").and_then(serde_json::Value::as_str) {
        parts.push(state.to_owned());
    }
    if let Some(reason) = data.get("reason").and_then(serde_json::Value::as_str) {
        parts.push(format!("reason: {reason}"));
    }
    if let Some(obj) = data.as_object() {
        for (k, v) in obj {
            if matches!(k.as_str(), "state" | "reason" | "actor" | "rid") || parts.len() >= 6 {
                continue;
            }
            let text = match v {
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Number(n) => n.to_string(),
                serde_json::Value::Bool(b) => b.to_string(),
                serde_json::Value::Array(a) if a.len() <= 4 && a.iter().all(|x| x.is_number()) => {
                    v.to_string()
                }
                _ => continue,
            };
            if text.chars().count() <= 60 {
                parts.push(format!("{k}={text}"));
            }
        }
    }
    let text = parts.join(", ");
    if text.is_empty() {
        ev.to_owned()
    } else {
        text.chars().take(200).collect()
    }
}
