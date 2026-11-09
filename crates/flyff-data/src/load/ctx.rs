//! `LoadCtx`: everything a loader needs (Plan/14 §5.1): the VFS, the options, the diagnostics, the script
//! tables (after P1) and the partially built [`GameData`].
//!
//! Loaders read files only through [`LoadCtx::read_text`], which tracks the source (for the data
//! fingerprint) and maps decoding findings to diagnostics. A loader borrows the script tables and the
//! diagnostics as disjoint fields:
//!
//! ```text
//! let Some(text) = cx.read_text("propJob.inc", Need::Required) else { return };
//! let sctx = cx.script_ctx();                       // Arc<ScriptCtx>, a cheap clone
//! let file = cx.file_arc("propJob.inc");
//! let rows = parse_job_file(&text, &file, &sctx, &mut cx.diags);
//! cx.gd.jobs ... = rows;                            // `cx.gd` derefs to `GameData`
//! ```

use crate::client_data::ClientData;
use crate::game_data::{GameData, SourceInfo};
use crate::options::DataOptions;
use flyff_script::{DiagCode, Diagnostics, ScriptCtx, ScriptVariant, Severity};
use flyff_vfs::{Layer, SourceEncoding, SourceRecord, Vfs, VfsError, VfsNoteKind, hint_for};
use std::collections::BTreeMap;
use std::sync::Arc;

/// How serious a missing file is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Need {
    /// Fatal `FileMissingRequired` (14 §8.2).
    Required,
    /// Warning `FileMissingOptional` (a file that matters when absent: `Event.lua`, world folders).
    Warn,
    /// Info `FileMissingOptional` (absent by design: `WdEvent01.txt.txt`, `Letter1_ENG.inc`).
    Info,
    /// Nothing is reported (the caller reports).
    Silent,
}

/// The partially built `GameData`: every field is its default until a loader sets it (`GameData` is
/// `Default`), and the fields can be borrowed independently.
pub type GameDataBuilder = GameData;

/// The loader context (see the module documentation).
pub struct LoadCtx<'a> {
    /// The file index.
    pub vfs: &'a Vfs,
    /// The loading options.
    pub opts: &'a DataOptions,
    /// Everything reported so far.
    pub diags: Diagnostics,
    /// The define and string tables (empty until P1 has run), `Server` keyword rule.
    pub script: Arc<ScriptCtx>,
    /// The data under construction.
    pub gd: GameDataBuilder,
    /// The client-only data under construction (filled by P7; stays empty without client data).
    pub client: ClientData,
    sources: BTreeMap<String, SourceRecord>,
}

impl<'a> LoadCtx<'a> {
    /// A fresh context over `vfs` (nothing is read yet).
    pub fn new(vfs: &'a Vfs, opts: &'a DataOptions) -> Self {
        Self {
            vfs,
            opts,
            diags: Diagnostics::new(),
            script: Arc::new(ScriptCtx::empty(ScriptVariant::Server)),
            gd: GameDataBuilder::default(),
            client: ClientData::default(),
            sources: BTreeMap::new(),
        }
    }

    /// A clone of the script tables (so a `Script` can borrow them while `diags` is borrowed mutably).
    pub fn script_ctx(&self) -> Arc<ScriptCtx> {
        Arc::clone(&self.script)
    }

    /// The normalised path of `path` as an `Arc<str>` for `Script::new` and diagnostics.
    pub fn file_arc(&self, path: &str) -> Arc<str> {
        Arc::from(
            flyff_common::normalize_resource_path(path)
                .unwrap_or_else(|_| path.to_owned())
                .as_str(),
        )
    }

    /// `true` when `path` resolves in the VFS.
    pub fn exists(&self, path: &str) -> bool {
        self.vfs.exists(path)
    }

    /// Reports a diagnostic about `file` (a loader-level message without a scanner position).
    pub fn report(
        &mut self,
        severity: Severity,
        code: DiagCode,
        file: Option<&str>,
        line: u32,
        message: impl Into<String>,
    ) {
        let file = file.map(|f| self.file_arc(f));
        self.diags
            .report(severity, code, file.as_ref(), line, message);
    }

    /// Reads and decodes `path` (14 §2.6), records the source for the fingerprint and reports decoding
    /// findings. A missing file is reported according to `need`.
    pub fn read_text(&mut self, path: &str, need: Need) -> Option<String> {
        let key = match flyff_common::normalize_resource_path(path) {
            Ok(k) => k,
            Err(e) => {
                self.report(
                    Severity::Error,
                    DiagCode::FileMissingOptional,
                    Some(path),
                    0,
                    format!("invalid resource path {path:?}: {e}"),
                );
                return None;
            }
        };
        match self
            .vfs
            .read_text_with(path, hint_for(&key), self.opts.cp1252_lossy)
        {
            Ok(d) => {
                let rec = self.vfs.source_record(path, Some(d.encoding));
                self.sources.insert(rec.path.clone(), rec);
                self.report_decoding(&key, &d);
                Some(d.text)
            }
            Err(VfsError::NotFound(_)) => {
                self.sources
                    .insert(key.clone(), SourceRecord::missing(key.clone()));
                let (sev, code, what) = match need {
                    Need::Required => (
                        Severity::Fatal,
                        DiagCode::FileMissingRequired,
                        "required file is missing",
                    ),
                    Need::Warn => (
                        Severity::Warning,
                        DiagCode::FileMissingOptional,
                        "file is missing",
                    ),
                    Need::Info => (
                        Severity::Info,
                        DiagCode::FileMissingOptional,
                        "optional file is missing",
                    ),
                    Need::Silent => return None,
                };
                self.report(sev, code, Some(&key), 0, format!("{key}: {what}"));
                None
            }
            Err(e) => {
                self.report(
                    Severity::Error,
                    DiagCode::FileMissingOptional,
                    Some(&key),
                    0,
                    format!("{key}: {e}"),
                );
                None
            }
        }
    }

    fn report_decoding(&mut self, key: &str, d: &flyff_vfs::DecodedText) {
        if d.encoding == SourceEncoding::Cp949 {
            self.report(
                Severity::Info,
                DiagCode::EncodingFallback,
                Some(key),
                0,
                format!("{key}: not valid UTF-8, decoded as CP949"),
            );
        }
        if d.replaced_chars > 0 {
            let sev = if d.encoding == SourceEncoding::Cp949 {
                Severity::Info
            } else {
                Severity::Warning
            };
            self.report(
                sev,
                DiagCode::EncodingMalformed,
                Some(key),
                0,
                format!(
                    "{key}: {} malformed character(s) replaced",
                    d.replaced_chars
                ),
            );
        }
        if d.truncated_at_nul {
            let sev = if d.dropped_after_nul > 0 {
                Severity::Warning
            } else {
                Severity::Info
            };
            self.report(
                sev,
                DiagCode::TruncatedAtNul,
                Some(key),
                0,
                format!(
                    "{key}: UTF-16 text cut at its first NUL ({} character(s) dropped)",
                    d.dropped_after_nul
                ),
            );
        }
    }

    /// Maps the notes of the VFS index (case collisions, overrides, unreadable entries) to diagnostics.
    pub fn import_vfs_notes(&mut self) {
        for n in self.vfs.notes() {
            let (sev, code) = match n.kind {
                VfsNoteKind::CaseCollision => (Severity::Warning, DiagCode::VfsCaseCollision),
                VfsNoteKind::Override => (Severity::Info, DiagCode::VfsOverride),
                VfsNoteKind::Unreadable => (Severity::Warning, DiagCode::VfsUnreadable),
            };
            let file = n.path.as_ref().map(|p| p.as_str().to_owned());
            self.diags.report(
                sev,
                code,
                file.as_deref().map(Arc::<str>::from).as_ref(),
                0,
                n.message.clone(),
            );
        }
    }

    /// The source records collected so far (every file that was looked up).
    pub fn source_records(&self) -> impl Iterator<Item = &SourceRecord> {
        self.sources.values()
    }

    /// The serialisable form of the collected source records.
    pub fn source_infos(&self) -> BTreeMap<String, SourceInfo> {
        self.sources
            .iter()
            .map(|(k, r)| (k.clone(), source_info(r)))
            .collect()
    }
}

fn source_info(r: &SourceRecord) -> SourceInfo {
    SourceInfo {
        layer: r.layer.map(|l| {
            match l {
                Layer::Resource => "resource",
                Layer::Override => "override",
            }
            .to_owned()
        }),
        size: r.size,
        mtime_ns: r.mtime_ns,
        encoding: r.encoding.map(|e| format!("{e:?}").to_ascii_lowercase()),
    }
}
