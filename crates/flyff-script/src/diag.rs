//! Typed loader diagnostics (Plan/14 §8.1).
//!
//! The original reported problems with `Error()`/`SntxErr()` message boxes. Here every problem is a
//! [`Diagnostic`] with a [`Severity`], a stable [`DiagCode`], the file and the 1-based line. A
//! [`Diagnostics`] collects them (keeping at most [`Diagnostics::CAP`] items, but counting all).

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

/// How serious a diagnostic is (Plan/14 §8.2).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
pub enum Severity {
    /// Expected condition; counted, logged only with `--verbose`.
    Info,
    /// Tolerated oddity.
    Warning,
    /// The original reported it and continued.
    Error,
    /// Start-up fails.
    Fatal,
}

/// Stable diagnostic code (Plan/14 §8.1). The order is part of the contract (sorting of counts).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
pub enum DiagCode {
    // ---- VFS / files
    /// The Resource root is missing or has no `Masquerade.prj`.
    VfsMissingRoot,
    /// Two real files normalise to the same key.
    VfsCaseCollision,
    /// A file of `overrides/` shadows a Resource file.
    VfsOverride,
    /// A required file is missing.
    FileMissingRequired,
    /// An optional file is missing.
    FileMissingOptional,
    /// Text was not valid UTF-8 and was decoded as CP949.
    EncodingFallback,
    /// A malformed byte sequence was replaced by U+FFFD.
    EncodingMalformed,
    /// A UTF-16 text was cut at its first NUL (original `WideCharToMultiByte(-1)` behaviour).
    TruncatedAtNul,
    /// A `world/*/*.txt.txt` string table that is not in the hard-coded list is non-empty.
    UnlistedStringTable,
    // ---- scanner / script
    /// A `"` string did not end with a quote.
    UnterminatedString,
    /// A `"` string exceeded 2048 bytes.
    StringTooLong,
    /// An identifier in a numeric context is neither a string key nor a define ("X Not Found.").
    UnknownIdentifier,
    /// `atoi`/`_atoi64` saturated.
    NumberOverflow,
    /// A define name was defined twice (the first definition wins).
    DefineDuplicate,
    /// A `#define` without a usable numeric value was ignored.
    DefineIgnored,
    /// A define differs between two sources.
    DefineDrift,
    /// An `IDS_` key was defined twice (the first definition wins).
    StringDuplicate,
    /// A string-table line does not start with `IDS`.
    StringBadKey,
    /// The Client variant would have classified a substituted identifier as a keyword.
    ClientVariantDivergence,
    // ---- tables
    /// A prop id is invalid.
    PropIdInvalid,
    /// A prop id is duplicated.
    PropIdDuplicate,
    /// A prop id is out of range.
    PropIdOutOfRange,
    /// A prop row was skipped.
    PropRowSkipped,
    /// An index is out of range.
    IndexOutOfRange,
    /// A fixed capacity was exceeded.
    CapacityExceeded,
    /// A model id was registered twice.
    ModelDuplicate,
    /// A value is outside its documented range.
    ValueOutOfRange,
    /// A reference cannot be resolved.
    RefUnresolved,
    /// A shop references a missing item.
    ShopItemMissing,
    /// A chip cost is invalid.
    ChipCostInvalid,
    /// A vendor rule has no items.
    VendorRuleEmpty,
    /// A world folder is missing.
    WorldFolderMissing,
    /// A world title is unknown.
    WorldTitleUnknown,
    /// A referenced asset is missing.
    AssetMissing,
    // ---- lua
    /// Lua syntax error.
    LuaSyntax,
    /// Lua runtime error.
    LuaError,
    /// A Lua sandbox limit was hit.
    LuaLimit,
    // ---- additions (appended so that the order of the codes above stays stable)
    /// A file or directory of `Resource/` or `overrides/` could not be indexed (non-UTF-8 name,
    /// unreadable metadata, symlink loop).
    VfsUnreadable,
    // ---- text world formats (Plan/30 §9, §21; parsed by `flyff-formats::world_text`)
    /// A `.wld`/`.rgn`/`.wld.cnt`/`.pat` record was cut off by the end of the file and dropped (the
    /// original read zeros).
    WorldTextTruncated,
    /// A key, script, sound or revival-key string is longer than the original's 31-character buffer.
    WorldTextStringLong,
    /// A `title`/`desc` marker of a `region2`/`region3` record is a different token (the original does
    /// not verify it and continues identically).
    RgnUnexpectedToken,
    /// A respawn control marker that is neither 0, 1 nor 2.
    RespawnBadControl,
    /// A `.pat` structure the original would crash on or loop forever on.
    PatMalformed,
    /// A `.24l` light table with fewer than 24 complete rows (the missing values read as zero).
    L24Short,
    // ---- NPC dialog scripts (Plan/20 §5.5, §6.2; reported by `flyff-lua`)
    /// A dialog key line of `WorldDialog.txt` is empty: the NPC dialogs are disabled (the original refused to
    /// start).
    DialogKeyEmpty,
    /// An NPC script uses a `C.NAME` constant that is not defined in the Resource headers.
    NpcScriptUndefinedConstant,
    // ---- additions for the table grammars of WP-DATA-07
    /// A statement of a table grammar does not follow the grammar (the original refuses to start; the loader
    /// skips the construct).
    SyntaxError,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

impl fmt::Display for DiagCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

/// `Option<Arc<str>>` as a plain optional string (serde's `rc` feature is not enabled).
mod opt_arc_str {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::sync::Arc;

    pub fn serialize<S: Serializer>(v: &Option<Arc<str>>, s: S) -> Result<S::Ok, S::Error> {
        v.as_deref().serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Arc<str>>, D::Error> {
        Ok(Option::<String>::deserialize(d)?.map(Arc::from))
    }
}

/// One problem found while loading data (Plan/14 §8.1).
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Diagnostic {
    /// Severity.
    pub severity: Severity,
    /// Stable code.
    pub code: DiagCode,
    /// Normalised file path, when known.
    #[serde(with = "opt_arc_str")]
    pub file: Option<Arc<str>>,
    /// 1-based line, `0` when unknown.
    pub line: u32,
    /// Human-readable message.
    pub message: String,
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}[{}]", self.severity, self.code)?;
        if let Some(file) = &self.file {
            write!(f, " {file}")?;
            if self.line != 0 {
                write!(f, ":{}", self.line)?;
            }
        }
        write!(f, ": {}", self.message)
    }
}

/// A collection of diagnostics: the first [`Diagnostics::CAP`] items plus exact counts per
/// `(code, severity)` for all of them (Plan/14 §8.1).
#[derive(Clone, Default, Debug)]
pub struct Diagnostics {
    items: Vec<Diagnostic>,
    counts: BTreeMap<(DiagCode, Severity), u32>,
    dropped: u32,
}

/// A serialisable digest of a [`Diagnostics`] (Plan/14 §8.1).
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct DiagSummary {
    /// `"Error:UnknownIdentifier"` -> count.
    pub counts: BTreeMap<String, u32>,
    /// The first (at most 200) diagnostics.
    pub first: Vec<Diagnostic>,
}

impl Diagnostics {
    /// Maximum number of stored diagnostics; further ones are only counted.
    pub const CAP: usize = 20_000;
    /// Maximum number of diagnostics in a [`DiagSummary`].
    pub const SUMMARY_FIRST: usize = 200;

    /// An empty collection.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a diagnostic (always counted; stored while below [`Self::CAP`]).
    pub fn push(&mut self, d: Diagnostic) {
        let n = self.counts.entry((d.code, d.severity)).or_insert(0);
        *n = n.saturating_add(1);
        if self.items.len() < Self::CAP {
            self.items.push(d);
        } else {
            self.dropped = self.dropped.saturating_add(1);
        }
    }

    /// Adds a diagnostic built from its parts.
    pub fn report(
        &mut self,
        severity: Severity,
        code: DiagCode,
        file: Option<&Arc<str>>,
        line: u32,
        message: impl Into<String>,
    ) {
        self.push(Diagnostic {
            severity,
            code,
            file: file.cloned(),
            line,
            message: message.into(),
        });
    }

    /// The stored diagnostics, in report order.
    pub fn items(&self) -> &[Diagnostic] {
        &self.items
    }

    /// Number of diagnostics that were counted but not stored because of the cap.
    pub fn dropped(&self) -> u32 {
        self.dropped
    }

    /// Total number of reported diagnostics (stored or not).
    pub fn total(&self) -> u32 {
        self.counts.values().fold(0u32, |a, n| a.saturating_add(*n))
    }

    /// `true` when nothing was reported.
    pub fn is_empty(&self) -> bool {
        self.counts.is_empty()
    }

    /// Number of diagnostics with exactly this code and severity.
    pub fn count(&self, code: DiagCode, severity: Severity) -> u32 {
        self.counts.get(&(code, severity)).copied().unwrap_or(0)
    }

    /// Number of diagnostics with this code, at any severity.
    pub fn count_code(&self, code: DiagCode) -> u32 {
        self.counts
            .iter()
            .filter(|((c, _), _)| *c == code)
            .fold(0u32, |a, (_, n)| a.saturating_add(*n))
    }

    /// Number of diagnostics of at least `min` severity.
    pub fn count_at_least(&self, min: Severity) -> u32 {
        self.counts
            .iter()
            .filter(|((_, s), _)| *s >= min)
            .fold(0u32, |a, (_, n)| a.saturating_add(*n))
    }

    /// The highest severity reported, if any.
    pub fn max_severity(&self) -> Option<Severity> {
        self.counts.keys().map(|(_, s)| *s).max()
    }

    /// The exact counts per `(code, severity)`, sorted.
    pub fn counts(&self) -> &BTreeMap<(DiagCode, Severity), u32> {
        &self.counts
    }

    /// Appends all diagnostics of `other` (counts included, even those `other` did not store).
    pub fn extend(&mut self, other: Diagnostics) {
        let stored: BTreeMap<(DiagCode, Severity), u32> =
            other.items.iter().fold(BTreeMap::new(), |mut m, d| {
                let n: &mut u32 = m.entry((d.code, d.severity)).or_insert(0);
                *n = n.saturating_add(1);
                m
            });
        for d in other.items {
            self.push(d);
        }
        // Counts of diagnostics `other` had dropped (cap) are added without items.
        for (key, total) in other.counts {
            let have = stored.get(&key).copied().unwrap_or(0);
            let extra = total.saturating_sub(have);
            if extra > 0 {
                let n = self.counts.entry(key).or_insert(0);
                *n = n.saturating_add(extra);
                self.dropped = self.dropped.saturating_add(extra);
            }
        }
    }

    /// A serialisable digest: counts per `"Severity:Code"` and the first diagnostics.
    pub fn summary(&self) -> DiagSummary {
        let counts = self
            .counts
            .iter()
            .map(|((code, sev), n)| {
                let mut key = sev.to_string();
                key.push(':');
                key.push_str(&code.to_string());
                (key, *n)
            })
            .collect();
        DiagSummary {
            counts,
            first: self
                .items
                .iter()
                .take(Self::SUMMARY_FIRST)
                .cloned()
                .collect(),
        }
    }
}
