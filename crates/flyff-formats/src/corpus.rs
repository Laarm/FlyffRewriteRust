//! Format classification of paths and byte buffers (Plan/30 §1.2, WP-FMT-01: `FormatKind::from_path`
//! and `sniff`) and the corpus checker of WP-FMT-12 (Plan/30 §17.1): [`check`] parses one file with the
//! corpus parse mode of its kind and tallies the result, [`REFERENCE_COUNTS`] is the single table of the
//! numbers measured on the original `Resource/`, [`compare_reference`] compares a run with it and
//! [`KNOWN_UNREADABLE`] lists the three real files that no parser reads (and that are fine).
//!
//! The runner that walks a VFS lives in `flyff_assets::check::binary_corpus`; this module is wasm-safe
//! (no file system, threads or clocks) and never panics on any input.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::error::{ErrorKind, FormatError, FormatWarning};
use crate::{
    AniOptions, FormatKind, KeyMode, LandOptions, LayerMode, O3dOptions, ani, bmp, chr, cur, dds,
    dyo, lnd, o3d, objects, sfx, tga,
};

/// Extension (lower case, without the dot) to format.
const EXTENSIONS: &[(&str, FormatKind)] = &[
    ("o3d", FormatKind::O3d),
    ("chr", FormatKind::Chr),
    ("ani", FormatKind::Ani),
    ("lnd", FormatKind::Lnd),
    ("dyo", FormatKind::Dyo),
    ("sfx", FormatKind::Sfx),
    ("dds", FormatKind::Dds),
    ("tga", FormatKind::Tga),
    ("bmp", FormatKind::Bmp),
    ("cur", FormatKind::Cur),
    ("wld", FormatKind::Wld),
    ("rgn", FormatKind::Rgn),
    ("pat", FormatKind::Pat),
    ("24l", FormatKind::L24),
];

impl FormatKind {
    /// Classifies a file by its name (ASCII case-insensitive; `/` and `\` both separate directories).
    ///
    /// `.wld.cnt` is [`FormatKind::Cnt`] (matched before the plain extension, which would otherwise be
    /// `.cnt`); a bare `.cnt` is not a Flyff format and yields `None`.
    pub fn from_path(path: &str) -> Option<FormatKind> {
        let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
        if ends_with_ignore_ascii_case(name, ".wld.cnt") {
            return Some(FormatKind::Cnt);
        }
        let dot = name.rfind('.')?;
        let ext = name.get(dot + 1..)?;
        EXTENSIONS
            .iter()
            .find(|(e, _)| ext.eq_ignore_ascii_case(e))
            .map(|&(_, kind)| kind)
    }
}

fn ends_with_ignore_ascii_case(s: &str, suffix: &str) -> bool {
    s.len() >= suffix.len()
        && s.as_bytes()
            .get(s.len() - suffix.len()..)
            .is_some_and(|tail| tail.eq_ignore_ascii_case(suffix.as_bytes()))
}

/// Recognises a format from its leading magic: `"DDS "` is [`FormatKind::Dds`], `"SFX0."` is
/// [`FormatKind::Sfx`]; everything else (the other formats have no magic) is `None`.
pub fn sniff(bytes: &[u8]) -> Option<FormatKind> {
    if bytes.starts_with(b"DDS ") {
        Some(FormatKind::Dds)
    } else if bytes.starts_with(b"SFX0.") {
        Some(FormatKind::Sfx)
    } else {
        None
    }
}

// ---- WP-FMT-12: the corpus checker ---------------------------------------------------------------------

impl FormatKind {
    /// The short name used in reports and in the reference table (`o3d chr ani lnd dyo sfx dds tga bmp cur wld
    /// rgn wld.cnt pat 24l fsk1 fmo1`).
    pub fn label(self) -> &'static str {
        match self {
            FormatKind::O3d => "o3d",
            FormatKind::Chr => "chr",
            FormatKind::Ani => "ani",
            FormatKind::Lnd => "lnd",
            FormatKind::Dyo => "dyo",
            FormatKind::Sfx => "sfx",
            FormatKind::Dds => "dds",
            FormatKind::Tga => "tga",
            FormatKind::Bmp => "bmp",
            FormatKind::Cur => "cur",
            FormatKind::Wld => "wld",
            FormatKind::Rgn => "rgn",
            FormatKind::Cnt => "wld.cnt",
            FormatKind::Pat => "pat",
            FormatKind::L24 => "24l",
            FormatKind::PackedSkeleton => "fsk1",
            FormatKind::PackedMotion => "fmo1",
        }
    }

    /// `true` for the kinds [`check`] parses (the binary formats of Plan/30 §17.1). The text kinds
    /// (`wld rgn wld.cnt pat 24l`, parsed by the game-data loader) and the packed blobs are only counted.
    pub fn is_corpus_parsed(self) -> bool {
        matches!(
            self,
            FormatKind::O3d
                | FormatKind::Chr
                | FormatKind::Ani
                | FormatKind::Lnd
                | FormatKind::Dyo
                | FormatKind::Sfx
                | FormatKind::Dds
                | FormatKind::Tga
                | FormatKind::Bmp
                | FormatKind::Cur
        )
    }
}

/// The counts of one format over a corpus run (Plan/30 §17.1). `files = ok + known + failed` for the parsed
/// kinds; the counted-only kinds only fill `files`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KindTally {
    /// Files seen.
    pub files: u64,
    /// Files that parsed (warnings included).
    pub ok: u64,
    /// Files that did not parse and are listed in [`KNOWN_UNREADABLE`].
    pub known: u64,
    /// Files that did not parse and are not known: the findings.
    pub failed: u64,
    /// Per-kind facts: `version:21`, `warn:O3dNameMismatch`, `format:DXT1`, ... A fact counts files (or
    /// warnings, entries, records where the key says so, see [`REFERENCE_COUNTS`]); a fact that never
    /// occurred has no entry (missing = 0).
    pub facts: BTreeMap<String, u64>,
}

impl KindTally {
    /// Adds `other` to `self` (sums every counter and every fact).
    pub fn merge(&mut self, other: &KindTally) {
        self.files = self.files.saturating_add(other.files);
        self.ok = self.ok.saturating_add(other.ok);
        self.known = self.known.saturating_add(other.known);
        self.failed = self.failed.saturating_add(other.failed);
        for (k, v) in &other.facts {
            self.add_fact(k, *v);
        }
    }

    /// Adds `n` to the fact `key` (nothing for `n == 0`, so absent facts stay absent).
    pub fn add_fact(&mut self, key: &str, n: u64) {
        if n == 0 {
            return;
        }
        match self.facts.get_mut(key) {
            Some(v) => *v = v.saturating_add(n),
            None => {
                self.facts.insert(key.to_owned(), n);
            }
        }
    }
}

/// The outcome of [`check`] for one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// The file parsed (or its kind is only counted).
    Ok,
    /// The file did not parse and is a [`KNOWN_UNREADABLE`] entry.
    Known(FormatError),
    /// The file did not parse: a finding.
    Failed(FormatError),
}

/// The class of an error, so a [`KNOWN_UNREADABLE`] entry matches the failure it documents and nothing else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorClass {
    /// [`ErrorKind::LegacyLayout`].
    LegacyLayout,
    /// [`ErrorKind::BadMagic`].
    BadMagic,
}

impl ErrorClass {
    /// `true` when `kind` belongs to this class.
    pub fn matches(self, kind: &ErrorKind) -> bool {
        match self {
            ErrorClass::LegacyLayout => matches!(kind, ErrorKind::LegacyLayout),
            ErrorClass::BadMagic => matches!(kind, ErrorKind::BadMagic { .. }),
        }
    }
}

/// A file of the original `Resource/` that no parser reads and that is fine (never loaded by the original
/// game either).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KnownUnreadable {
    /// The normalised VFS key (lower case, `/`).
    pub path: &'static str,
    /// The format the file name says.
    pub kind: FormatKind,
    /// The error class it fails with; another failure of the same file is a finding.
    pub class: ErrorClass,
    /// Why it is fine.
    pub reason: &'static str,
}

/// The three unreadable files of the reference `Resource/` (Plan/30 §17.1): listed and counted, not findings.
/// A file at the same key that is readable (an `overrides/` replacement) becomes a [`compare_reference`] row.
pub const KNOWN_UNREADABLE: &[KnownUnreadable] = &[
    KnownUnreadable {
        path: "model/part_malehead.o3d",
        kind: FormatKind::O3d,
        class: ErrorClass::LegacyLayout,
        reason: "pre-v20 head model, never loaded by the original (Plan/30 §6.6)",
    },
    KnownUnreadable {
        path: "model/part_femalehead.o3d",
        kind: FormatKind::O3d,
        class: ErrorClass::LegacyLayout,
        reason: "pre-v20 head model, never loaded by the original (Plan/30 §6.6)",
    },
    KnownUnreadable {
        path: "sfx/texture/sfx_sklacrbowautoshot01.dds",
        kind: FormatKind::Dds,
        class: ErrorClass::BadMagic,
        reason: "an SFX0. effect file with a .dds name, never loaded as a texture",
    },
];

/// The [`KNOWN_UNREADABLE`] entry for `path` that `error` matches, if any.
pub fn known_unreadable(
    path: &str,
    kind: FormatKind,
    error: &ErrorKind,
) -> Option<&'static KnownUnreadable> {
    KNOWN_UNREADABLE
        .iter()
        .find(|k| k.kind == kind && k.path == path && k.class.matches(error))
}

/// Parses `bytes` with the corpus parse mode of `kind` and adds the result to `tally` (`files` always; then
/// `ok` plus the facts, `known` or `failed`). `path` is the normalised VFS key (the `.o3d` header name is
/// compared with its file name; [`KNOWN_UNREADABLE`] is matched on it).
///
/// Parse modes: `.o3d` geometry skipped with the file name, `.ani` keys skipped, `.lnd` layers skipped, the
/// others in full. Every warning becomes the fact `warn:<WarningCode>`. The text kinds and the packed blobs
/// are only counted (`files`), their `Verdict` is [`Verdict::Ok`].
pub fn check(kind: FormatKind, path: &str, bytes: &[u8], tally: &mut KindTally) -> Verdict {
    tally.files = tally.files.saturating_add(1);
    if !kind.is_corpus_parsed() {
        return Verdict::Ok;
    }
    match parse_facts(kind, path, bytes) {
        Ok(facts) => {
            tally.ok = tally.ok.saturating_add(1);
            for (key, n) in &facts {
                tally.add_fact(key, *n);
            }
            Verdict::Ok
        }
        Err(e) => {
            if known_unreadable(path, kind, &e.kind).is_some() {
                tally.known = tally.known.saturating_add(1);
                Verdict::Known(e)
            } else {
                tally.failed = tally.failed.saturating_add(1);
                Verdict::Failed(e)
            }
        }
    }
}

type Facts = Vec<(String, u64)>;

fn fact(facts: &mut Facts, key: impl Into<String>, n: u64) {
    if n > 0 {
        facts.push((key.into(), n));
    }
}

fn flag(facts: &mut Facts, key: &str, on: bool) {
    fact(facts, key, u64::from(on));
}

fn warnings(facts: &mut Facts, w: &[FormatWarning]) {
    for w in w {
        fact(facts, format!("warn:{:?}", w.code), 1);
    }
}

fn count(n: usize) -> u64 {
    u64::try_from(n).unwrap_or(u64::MAX)
}

fn sfx_label(v: sfx::SfxVersion) -> &'static str {
    match v {
        sfx::SfxVersion::Old => "old",
        sfx::SfxVersion::V01 => "0.1",
        sfx::SfxVersion::V02 => "0.2",
        sfx::SfxVersion::V03 => "0.3",
    }
}

fn dds_label(f: dds::DdsFormat) -> &'static str {
    match f {
        dds::DdsFormat::Bc1 => "DXT1",
        dds::DdsFormat::Bc2 => "DXT3",
        dds::DdsFormat::Bc3 => "DXT5",
        dds::DdsFormat::A1R5G5B5 => "A1R5G5B5",
        dds::DdsFormat::X1R5G5B5 => "X1R5G5B5",
        dds::DdsFormat::R5G6B5 => "R5G6B5",
        dds::DdsFormat::A8R8G8B8 => "A8R8G8B8",
        dds::DdsFormat::OtherRgb { .. } => "other",
    }
}

/// The facts of one successfully parsed file.
fn parse_facts(kind: FormatKind, path: &str, bytes: &[u8]) -> Result<Facts, FormatError> {
    let mut f = Facts::new();
    match kind {
        FormatKind::O3d => {
            let name = path.rsplit('/').next().unwrap_or(path);
            let p = o3d::parse(bytes, &O3dOptions::skip().with_file_name(name))?;
            let o = &p.value;
            fact(&mut f, format!("version:{}", o.version), 1);
            flag(&mut f, "lod", o.lod != 0);
            flag(&mut f, "collision", o.collision.is_some());
            flag(&mut f, "embedded", o.bones.is_some());
            flag(&mut f, "animated", o.max_frame > 0);
            flag(&mut f, "events", !o.events.is_empty());
            warnings(&mut f, &p.warnings);
        }
        FormatKind::Chr => {
            let p = chr::parse(bytes)?;
            fact(&mut f, format!("version:{}", p.value.version), 1);
            warnings(&mut f, &p.warnings);
        }
        FormatKind::Ani => {
            let p = ani::parse(
                bytes,
                &AniOptions {
                    keys: KeyMode::Skip,
                },
            )?;
            let a = &p.value;
            fact(&mut f, format!("version:{}", a.version), 1);
            flag(&mut f, "attr_files", a.attrs.iter().any(|m| m.flags != 0));
            let with = |bit: u32| count(a.attrs.iter().filter(|m| m.flags & bit != 0).count());
            fact(&mut f, "attr_hit", with(crate::MA_HIT));
            fact(&mut f, "attr_sound", with(crate::MA_SOUND));
            fact(&mut f, "attr_quake", with(crate::MA_QUAKE));
            warnings(&mut f, &p.warnings);
        }
        FormatKind::Lnd => {
            let p = lnd::parse(
                bytes,
                &LandOptions {
                    layers: LayerMode::Skip,
                },
            )?;
            let l = &p.value;
            fact(&mut f, format!("version:{}", l.version), 1);
            fact(&mut f, "objects", count(l.objects.len()));
            fact(&mut f, "sfx", count(l.sfx.as_ref().map_or(0, Vec::len)));
            warnings(&mut f, &p.warnings);
        }
        FormatKind::Dyo => {
            let p = dyo::parse(bytes)?;
            let n = |t: u32| count(p.value.records.iter().filter(|r| r.obj_type == t).count());
            fact(&mut f, "movers", n(objects::OT_MOVER));
            fact(&mut f, "ctrls", n(objects::OT_CTRL));
            warnings(&mut f, &p.warnings);
        }
        FormatKind::Sfx => {
            let p = sfx::parse(bytes)?;
            fact(&mut f, format!("version:{}", sfx_label(p.value.version)), 1);
            fact(&mut f, "parts", count(p.value.parts.len()));
            warnings(&mut f, &p.warnings);
        }
        FormatKind::Dds => {
            let p = dds::view(bytes)?;
            fact(&mut f, format!("format:{}", dds_label(p.value.format)), 1);
            flag(&mut f, "cube", dds::facts(&p.value).cube);
            warnings(&mut f, &p.warnings);
        }
        FormatKind::Tga => {
            let p = tga::parse(bytes)?;
            if let Ok(h) = tga::header(bytes) {
                fact(
                    &mut f,
                    format!("type{}_bpp{}", h.image_type, h.bits_per_pixel),
                    1,
                );
            }
            warnings(&mut f, &p.warnings);
        }
        FormatKind::Bmp => {
            let p = bmp::parse(bytes)?;
            if let Ok(h) = bmp::header(bytes) {
                fact(&mut f, format!("bpp{}", h.bits_per_pixel), 1);
            }
            warnings(&mut f, &p.warnings);
        }
        FormatKind::Cur => {
            let p = cur::parse(bytes)?;
            warnings(&mut f, &p.warnings);
        }
        FormatKind::Wld
        | FormatKind::Rgn
        | FormatKind::Cnt
        | FormatKind::Pat
        | FormatKind::L24
        | FormatKind::PackedSkeleton
        | FormatKind::PackedMotion => {}
    }
    Ok(f)
}

/// The reference numbers of one format (Plan/30 §17.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KindReference {
    /// The format.
    pub kind: FormatKind,
    /// Files in the Resource.
    pub files: u64,
    /// Files that parse.
    pub ok: u64,
    /// Files in [`KNOWN_UNREADABLE`].
    pub known: u64,
    /// The facts (sorted by key; a fact that is not listed must not occur).
    pub facts: &'static [(&'static str, u64)],
}

/// The single table of the numbers measured on the original `Resource/` (Plan/30 §17.1; replaces the golden
/// file of Plan/60 §4.1), pinned by the `tests/res_*.rs` corpus tests. Sorted by [`FormatKind`], so a new
/// data set or format is one row. `failed` is 0 everywhere.
///
/// Fact keys: `version:N` (files), `warn:<WarningCode>` (warnings), o3d `lod collision embedded animated
/// events` (files), ani `attr_files` (files with a flagged entry) and `attr_hit attr_sound attr_quake`
/// (entries), lnd `objects sfx` (records), dyo `movers ctrls` (records), sfx `parts`, dds `format:X`
/// and `cube` (files), tga `typeN_bppM` and bmp `bppN` (files). The text kinds are counted only.
pub const REFERENCE_COUNTS: &[KindReference] = &[
    KindReference {
        kind: FormatKind::O3d,
        files: 4198,
        ok: 4196,
        known: 2,
        facts: &[
            ("animated", 130),
            ("collision", 1144),
            ("embedded", 35),
            ("events", 856),
            ("lod", 1917),
            ("version:20", 1376),
            ("version:21", 1200),
            ("version:22", 1620),
            ("warn:O3dNameMismatch", 6),
        ],
    },
    KindReference {
        kind: FormatKind::Chr,
        files: 301,
        ok: 301,
        known: 0,
        facts: &[
            ("version:4", 32),
            ("version:5", 32),
            ("version:6", 14),
            ("version:7", 223),
        ],
    },
    KindReference {
        kind: FormatKind::Ani,
        files: 2925,
        ok: 2925,
        known: 0,
        facts: &[
            ("attr_files", 1534),
            ("attr_hit", 653),
            ("attr_quake", 18),
            ("attr_sound", 1488),
            ("version:10", 2925),
        ],
    },
    KindReference {
        kind: FormatKind::Lnd,
        files: 1357,
        ok: 1357,
        known: 0,
        facts: &[
            ("objects", 68_987),
            ("sfx", 2597),
            ("version:2", 469),
            ("version:3", 888),
        ],
    },
    KindReference {
        kind: FormatKind::Dyo,
        files: 33,
        ok: 33,
        known: 0,
        facts: &[("ctrls", 10), ("movers", 264)],
    },
    KindReference {
        kind: FormatKind::Sfx,
        files: 733,
        ok: 733,
        known: 0,
        facts: &[
            ("parts", 4466),
            ("version:0.1", 76),
            ("version:0.2", 422),
            ("version:0.3", 198),
            ("version:old", 37),
        ],
    },
    KindReference {
        kind: FormatKind::Dds,
        files: 11_903,
        ok: 11_902,
        known: 1,
        facts: &[
            ("cube", 1),
            ("format:A1R5G5B5", 3809),
            ("format:A8R8G8B8", 200),
            ("format:DXT1", 7066),
            ("format:DXT3", 691),
            ("format:DXT5", 18),
            ("format:R5G6B5", 114),
            ("format:X1R5G5B5", 4),
            ("warn:DdsPartialMipChain", 3),
        ],
    },
    KindReference {
        kind: FormatKind::Tga,
        files: 854,
        ok: 854,
        known: 0,
        facts: &[
            ("type10_bpp32", 8),
            ("type2_bpp24", 235),
            ("type2_bpp32", 611),
        ],
    },
    KindReference {
        kind: FormatKind::Bmp,
        files: 551,
        ok: 551,
        known: 0,
        facts: &[("bpp24", 543), ("bpp32", 3), ("bpp8", 5)],
    },
    KindReference {
        kind: FormatKind::Cur,
        files: 23,
        ok: 23,
        known: 0,
        facts: &[],
    },
    KindReference {
        kind: FormatKind::Wld,
        files: 33,
        ok: 0,
        known: 0,
        facts: &[],
    },
    KindReference {
        kind: FormatKind::Rgn,
        files: 33,
        ok: 0,
        known: 0,
        facts: &[],
    },
    KindReference {
        kind: FormatKind::Cnt,
        files: 15,
        ok: 0,
        known: 0,
        facts: &[],
    },
    KindReference {
        kind: FormatKind::Pat,
        files: 15,
        ok: 0,
        known: 0,
        facts: &[],
    },
];

/// One difference between a corpus run and [`REFERENCE_COUNTS`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefMismatch {
    /// The format.
    pub kind: FormatKind,
    /// `files`, `ok`, `known`, `failed` or a fact key.
    pub what: String,
    /// The count found.
    pub found: u64,
    /// The reference count (0 for a fact or kind the table does not know).
    pub reference: u64,
}

impl fmt::Display for RefMismatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {}: found {}, reference {}",
            self.kind.label(),
            self.what,
            self.found,
            self.reference
        )
    }
}

/// Compares the tallies of a run with [`REFERENCE_COUNTS`]: `files`, `ok`, `known`, `failed` (reference 0)
/// and the union of the fact keys per kind (a missing key counts 0, so an unexpected warning or kind is a
/// row, and so is a kind or fact that is absent from the run). Rows come ordered by kind, then
/// `files ok known failed`, then the fact keys; an empty result means the run equals the reference.
pub fn compare_reference(tallies: &BTreeMap<FormatKind, KindTally>) -> Vec<RefMismatch> {
    let kinds: BTreeSet<FormatKind> = tallies
        .keys()
        .copied()
        .chain(REFERENCE_COUNTS.iter().map(|r| r.kind))
        .collect();
    let empty = KindTally::default();
    let mut rows = Vec::new();
    for kind in kinds {
        let found = tallies.get(&kind).unwrap_or(&empty);
        let reference = REFERENCE_COUNTS.iter().find(|r| r.kind == kind);
        let (r_files, r_ok, r_known) = reference.map_or((0, 0, 0), |r| (r.files, r.ok, r.known));
        let mut push = |what: &str, found: u64, reference: u64| {
            if found != reference {
                rows.push(RefMismatch {
                    kind,
                    what: what.to_owned(),
                    found,
                    reference,
                });
            }
        };
        push("files", found.files, r_files);
        push("ok", found.ok, r_ok);
        push("known", found.known, r_known);
        push("failed", found.failed, 0);
        let ref_facts: &[(&str, u64)] = reference.map_or(&[], |r| r.facts);
        let keys: BTreeSet<&str> = found
            .facts
            .keys()
            .map(String::as_str)
            .chain(ref_facts.iter().map(|(k, _)| *k))
            .collect();
        for key in keys {
            let want = ref_facts
                .iter()
                .find(|(k, _)| *k == key)
                .map_or(0, |(_, v)| *v);
            push(key, found.facts.get(key).copied().unwrap_or(0), want);
        }
    }
    rows
}
