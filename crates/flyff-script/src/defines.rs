//! The numeric `#define` table (Plan/14 §4.3, ref/17 §5.3; `LoadDefines` + `CScript::PreScan` +
//! `ExecDefine`, src:_Common/ProjectCmn.cpp:724-762, Script.cpp:301-420).
//!
//! 17 headers are scanned in a fixed order. The scan is deliberately dumb, like the original: it
//! understands only `#define NAME <number>` (and `<hex>`), ignores `#if`/`#ifdef`/`#else`/`#endif`
//! (every branch is read and the **first** definition in file order wins) and aborts the rest of a
//! file at the first `{`. Names defined as other names (`JOB_ALL`), negative values
//! (`HM_SATISFIED`) and expressions are absent from the table. Lookups are case-sensitive.

use crate::cnum::{c_atoi_checked, c_sscanf_x};
use crate::diag::{DiagCode, Diagnostics, Severity};
use crate::scanner::{Scanner, ScannerOptions, TokenType};
use crate::script::ScriptVariant;
use flyff_common::det::DetMap;
use std::sync::Arc;

/// The define headers in load order (`CProject::LoadDefines`; `ContinentDef.h` and
/// `defineMapComboBoxData.h` are DROPped because `__IMPROVE_MAP_SYSTEM` is inactive). Resolution of the
/// file names is case-insensitive (`resdata.h` is `ResData.h` on disk).
pub const DEFINE_HEADERS: [&str; 17] = [
    "define.h",
    "defineNeuz.h",
    "defineQuest.h",
    "defineJob.h",
    "defineItem.h",
    "defineWorld.h",
    "defineItemkind.h",
    "lang.h",
    "defineObj.h",
    "defineAttribute.h",
    "defineSkill.h",
    "defineText.h",
    "defineSound.h",
    "resdata.h",
    "WndStyle.h",
    "definelordskill.h",
    "defineHonor.h",
];

#[derive(Clone, Debug)]
struct DefineEntry {
    value: i32,
    /// Index into `Defines::files`.
    file: u16,
    line: u32,
}

/// The define table: first definition wins, iteration is in file order.
#[derive(Clone, Default, Debug)]
pub struct Defines {
    by_name: DetMap<String, DefineEntry>,
    files: Vec<Arc<str>>,
}

impl Defines {
    /// An empty table.
    pub fn new() -> Self {
        Self::default()
    }

    /// The value of `name` (case-sensitive).
    pub fn get(&self, name: &str) -> Option<i32> {
        self.by_name.get(name).map(|e| e.value)
    }

    /// The value of `name` as the unsigned bit pattern (`DWORD` fields).
    pub fn value_u32(&self, name: &str) -> Option<u32> {
        self.get(name).map(|v| v as u32)
    }

    /// `true` when `name` is defined.
    pub fn contains(&self, name: &str) -> bool {
        self.by_name.contains_key(name)
    }

    /// The first name (in file order) that starts with `prefix` and has `value`. Debug/export helper.
    pub fn name_of(&self, prefix: &str, value: i32) -> Option<&str> {
        self.by_name
            .iter()
            .find(|(name, e)| e.value == value && name.starts_with(prefix))
            .map(|(name, _)| name.as_str())
    }

    /// All `(name, value)` pairs in file order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, i32)> {
        self.by_name.iter().map(|(n, e)| (n.as_str(), e.value))
    }

    /// Number of distinct names.
    pub fn len(&self) -> usize {
        self.by_name.len()
    }

    /// `true` when no name is defined.
    pub fn is_empty(&self) -> bool {
        self.by_name.is_empty()
    }

    /// File and 1-based line of the definition of `name`.
    pub fn origin(&self, name: &str) -> Option<(&str, u32)> {
        let e = self.by_name.get(name)?;
        let file = self.files.get(usize::from(e.file))?;
        Some((file.as_ref(), e.line))
    }

    /// Inserts a definition (`map::insert`: the **first** definition wins). Returns `true` when the
    /// name was new. `file`/`line` only document the origin.
    pub fn insert(&mut self, name: &str, value: i32, file: &str, line: u32) -> bool {
        if self.by_name.contains_key(name) {
            return false;
        }
        let idx = self.file_index(file);
        self.by_name.insert(
            name.to_owned(),
            DefineEntry {
                value,
                file: idx,
                line,
            },
        );
        true
    }

    fn file_index(&mut self, file: &str) -> u16 {
        let pos = match self.files.iter().rposition(|f| f.as_ref() == file) {
            Some(p) => p,
            None => {
                self.files.push(Arc::from(file));
                self.files.len() - 1
            }
        };
        u16::try_from(pos).unwrap_or(u16::MAX)
    }

    /// `CScript::PreScan` of one header: reads `text` and adds its definitions. Returns the number of
    /// names added. Diagnostics: `DefineDuplicate` (Info), `DefineIgnored` (Info/Warning),
    /// `NumberOverflow`, string errors of the scanner.
    pub fn scan_header(&mut self, text: &str, file: &str, diags: &mut Diagnostics) -> usize {
        let file_arc: Arc<str> = Arc::from(file);
        let mut sc = Scanner::new(text, file_arc.clone(), ScannerOptions::default());
        let before = self.by_name.len();
        self.pre_scan(&mut sc, file, &file_arc, diags);
        diags.extend(sc.take_diagnostics());
        self.by_name.len() - before
    }

    fn pre_scan(
        &mut self,
        sc: &mut Scanner<'_>,
        file: &str,
        file_arc: &Arc<str>,
        diags: &mut Diagnostics,
    ) {
        // A first token of exactly "#" would switch the original to "compiler mode"; it never
        // happens in a header, and in that mode nothing is defined.
        let m = sc.mark();
        sc.get_token(false);
        if sc.token() == "#" {
            return;
        }
        sc.reset(m);

        let mut brace = false;
        loop {
            if brace {
                // KEEP-QUIRK: a `{` aborts the rest of the file.
                return;
            }
            let ty = sc.get_token(false);
            if ty == TokenType::Temp && ScriptVariant::Server.is_keyword(sc.token()) {
                self.exec_define(sc, file, file_arc, diags);
            } else if ty == TokenType::Temp {
                // IDENTIFIER: peek the next token; `(` means a "function": skip to the closing `)`.
                let m = sc.mark();
                sc.get_token(false);
                if sc.token().starts_with('(') {
                    sc.skip_past_close_paren();
                } else {
                    sc.reset(m);
                }
            } else if sc.token().starts_with('{') {
                brace = true;
            }
            if sc.finished() {
                return;
            }
        }
    }

    /// `CScript::ExecDefine`.
    fn exec_define(
        &mut self,
        sc: &mut Scanner<'_>,
        file: &str,
        file_arc: &Arc<str>,
        diags: &mut Diagnostics,
    ) {
        sc.get_token(false);
        let name = sc.token().to_owned();
        let line = sc.line();
        let value_mark = sc.mark();
        let vty = sc.get_token(false);
        let value = match vty {
            TokenType::Number => {
                let (v, overflow) = c_atoi_checked(sc.token().as_bytes());
                if overflow {
                    diags.report(
                        Severity::Warning,
                        DiagCode::NumberOverflow,
                        Some(file_arc),
                        line,
                        format!("#define {name} {}: value saturates", sc.token()),
                    );
                }
                v
            }
            TokenType::Hex => match c_sscanf_x(sc.token().as_bytes()) {
                Some(v) => v as i32,
                None => {
                    diags.report(
                        Severity::Warning,
                        DiagCode::DefineIgnored,
                        Some(file_arc),
                        line,
                        format!("#define {name} 0x{}: no hex digits, ignored", sc.token()),
                    );
                    return;
                }
            },
            TokenType::Temp => {
                // `#define JOB_ALL MAX_JOB`, or a valueless include guard followed by the next
                // `#define`: give the token back and ignore the define.
                sc.reset(value_mark);
                diags.report(
                    Severity::Info,
                    DiagCode::DefineIgnored,
                    Some(file_arc),
                    line,
                    format!("#define {name}: no numeric value, ignored"),
                );
                return;
            }
            _ => {
                diags.report(
                    Severity::Info,
                    DiagCode::DefineIgnored,
                    Some(file_arc),
                    line,
                    format!("#define {name} {}: not a plain number, ignored", sc.token()),
                );
                return;
            }
        };
        if !self.insert(&name, value, file, line) {
            diags.report(
                Severity::Info,
                DiagCode::DefineDuplicate,
                Some(file_arc),
                line,
                format!("define {name} is defined again; the first definition wins"),
            );
        }
    }
}
