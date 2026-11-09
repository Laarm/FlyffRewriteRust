//! The inputs of a run (Plan/20 §5.1): the C++ sources, `WorldDialog.txt`, the define headers and the NPC
//! dialog bindings of `character*.inc`.
//!
//! [`Inputs`] is plain data, so tests can build it by hand; [`Inputs::from_dirs`] reads the real layout
//! (`<source>/WORLDDIALOG/*`, `<resource>/WorldDialog.txt`, `<resource>/define*.h`, `character*.inc` through
//! `flyff_data::load::load_all` when the Resource tree has a `Masquerade.prj`).

use crate::error::{ErrorClass, Finding};
use flyff_common::normalize_resource_path;
use flyff_data::dialog_text::parse_dialog_text;
use flyff_script::load_pre_files;
use flyff_vfs::{Vfs, VfsConfig, hint_for};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// A source file: normalised text and the SHA-256 of the raw bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceFile {
    /// The name shown in messages and `_transpile.toml` (`WORLDDIALOG/NpcScript.cpp`).
    pub name: String,
    /// Normalised text (no BOM, LF line ends).
    pub text: String,
    /// Lower-case hex SHA-256 of the raw file bytes.
    pub sha256: String,
}

impl SourceFile {
    /// A file from raw bytes.
    pub fn from_bytes(name: &str, raw: &[u8]) -> SourceFile {
        SourceFile {
            name: name.to_owned(),
            text: crate::source::normalise(raw),
            sha256: sha256_hex(raw),
        }
    }
}

/// One NPC of `character*.inc` that has a dialog file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NpcBinding {
    /// The NPC key (`MaFl_Mikyel`).
    pub key: String,
    /// `m_szDialog` (`MaFl_Mikyel.txt`).
    pub dialog_file: String,
    /// The script name: the file name minus its last four characters, lower-cased.
    pub dialog_key: String,
}

/// Everything a run reads.
#[derive(Clone, Debug)]
pub struct Inputs {
    /// `WORLDDIALOG/NpcScript.cpp`.
    pub npc_script_cpp: SourceFile,
    /// `WORLDDIALOG/NpcScriptHelper.cpp` (`g_tables`).
    pub helper_cpp: SourceFile,
    /// `WORLDDIALOG/NpcScript.h` (cross-check of the helper table).
    pub npc_script_h: Option<SourceFile>,
    /// `WORLDDIALOG/FunctionsInfo.h` (cross-check of the table-only names).
    pub functions_info_h: Option<SourceFile>,
    /// `WorldDialog.txt` lines (`GetDialogText` semantics, see `flyff_data::dialog_text`).
    pub dialog_lines: Vec<String>,
    /// SHA-256 of the raw `WorldDialog.txt`.
    pub dialog_sha256: String,
    /// Every define of the headers.
    pub defines: BTreeMap<String, i32>,
    /// The NPCs with a dialog file; `None` when `character*.inc` could not be loaded.
    pub npcs: Option<Vec<NpcBinding>>,
    /// `(name, sha256)` of the `character*.inc` files the NPC list came from (empty without `npcs`).
    pub npc_files: Vec<(String, String)>,
    /// Why `npcs` is `None`, for the report.
    pub npcs_note: Option<String>,
}

impl Inputs {
    /// `GetDialogText(id)`: the line, or `""` outside the table.
    pub fn text(&self, id: i64) -> &str {
        usize::try_from(id)
            .ok()
            .and_then(|i| self.dialog_lines.get(i))
            .map_or("", String::as_str)
    }

    /// Reads the real layout. Missing required files are `input` findings.
    pub fn from_dirs(source: &Path, resource: &Path) -> Result<Inputs, Vec<Finding>> {
        let mut findings = Vec::new();
        let dialog_dir =
            find_ci(source, "WORLDDIALOG").unwrap_or_else(|| source.join("WORLDDIALOG"));
        let mut read = |name: &str, required: bool| -> Option<SourceFile> {
            let path = find_ci(&dialog_dir, name);
            match path.as_ref().map(|p| (p, std::fs::read(p))) {
                Some((_, Ok(raw))) => {
                    Some(SourceFile::from_bytes(&format!("WORLDDIALOG/{name}"), &raw))
                }
                Some((p, Err(e))) => {
                    findings.push(Finding::new(
                        ErrorClass::Input,
                        name,
                        0,
                        format!("cannot read {}: {e}", p.display()),
                    ));
                    None
                }
                None => {
                    if required {
                        findings.push(Finding::new(
                            ErrorClass::Input,
                            name,
                            0,
                            format!("{} not found in {}", name, dialog_dir.display()),
                        ));
                    }
                    None
                }
            }
        };
        let npc_script_cpp = read("NpcScript.cpp", true);
        let helper_cpp = read("NpcScriptHelper.cpp", true);
        let npc_script_h = read("NpcScript.h", false);
        let functions_info_h = read("FunctionsInfo.h", false);

        let vfs = match Vfs::open_lenient(&VfsConfig::new(resource)) {
            Ok(v) => Some(v),
            Err(e) => {
                findings.push(Finding::new(
                    ErrorClass::Input,
                    "Resource",
                    0,
                    format!(
                        "cannot open the Resource folder {}: {e}",
                        resource.display()
                    ),
                ));
                None
            }
        };
        let (Some(npc_script_cpp), Some(helper_cpp), Some(vfs)) = (npc_script_cpp, helper_cpp, vfs)
        else {
            return Err(findings);
        };

        let (dialog_lines, dialog_sha256) = match read_dialog(&vfs) {
            Ok(v) => v,
            Err(f) => {
                findings.push(f);
                return Err(findings);
            }
        };
        let pre = load_pre_files(|name| read_text(&vfs, name));
        let defines: BTreeMap<String, i32> = pre
            .defines
            .iter()
            .map(|(name, value)| (name.to_owned(), value))
            .collect();
        let (npcs, npc_files, npcs_note) = load_npcs(&vfs);
        if !findings.is_empty() {
            return Err(findings);
        }
        Ok(Inputs {
            npc_script_cpp,
            helper_cpp,
            npc_script_h,
            functions_info_h,
            dialog_lines,
            dialog_sha256,
            defines,
            npcs,
            npc_files,
            npcs_note,
        })
    }
}

fn read_text(vfs: &Vfs, name: &str) -> Option<String> {
    let key = normalize_resource_path(name).ok()?;
    vfs.read_text_with(name, hint_for(&key), false)
        .ok()
        .map(|d| d.text)
}

fn read_dialog(vfs: &Vfs) -> Result<(Vec<String>, String), Finding> {
    let missing = |m: String| Finding::new(ErrorClass::Input, "WorldDialog.txt", 0, m);
    let raw = vfs
        .read("WorldDialog.txt")
        .map_err(|e| missing(format!("cannot read WorldDialog.txt: {e}")))?;
    let text = read_text(vfs, "WorldDialog.txt")
        .ok_or_else(|| missing("cannot decode WorldDialog.txt".to_owned()))?;
    Ok((parse_dialog_text(&text).lines, sha256_hex(&raw)))
}

type NpcLoad = (
    Option<Vec<NpcBinding>>,
    Vec<(String, String)>,
    Option<String>,
);

fn load_npcs(vfs: &Vfs) -> NpcLoad {
    if !vfs.exists("Masquerade.prj") {
        return (
            None,
            Vec::new(),
            Some(
                "the Resource folder has no Masquerade.prj: character*.inc was not loaded"
                    .to_owned(),
            ),
        );
    }
    let opts = flyff_data::DataOptions::for_tests();
    match flyff_data::load::load_all(vfs, &opts) {
        Ok(out) => {
            let mut list: Vec<NpcBinding> = out
                .game
                .npcs
                .iter()
                .filter_map(|n| {
                    Some(NpcBinding {
                        key: n.key.clone(),
                        dialog_file: n.dialog_file.clone()?,
                        dialog_key: n.dialog_key.clone()?,
                    })
                })
                .collect();
            list.sort_by_key(|a| a.key.to_ascii_lowercase());
            let files = out
                .game
                .npcs
                .sources
                .iter()
                .filter_map(|name| Some((name.clone(), sha256_hex(&vfs.read(name).ok()?))))
                .collect();
            (Some(list), files, None)
        }
        Err(e) => (
            None,
            Vec::new(),
            Some(format!("character*.inc was not loaded: {e}")),
        ),
    }
}

/// Lower-case hex SHA-256.
pub fn sha256_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let digest = Sha256::digest(bytes);
    digest.iter().fold(String::with_capacity(64), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// Finds `name` in `dir` ignoring ASCII case.
pub fn find_ci(dir: &Path, name: &str) -> Option<PathBuf> {
    let exact = dir.join(name);
    if exact.exists() {
        return Some(exact);
    }
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .find(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(name))
        .map(|e| e.path())
}
