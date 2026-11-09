//! Reading the C++ sources (Plan/20 §5.3 steps 1-3): text normalisation, comment stripping, the function
//! bodies of `NpcScript.cpp`, the `g_tables` registrations of `NpcScriptHelper.cpp` and the `g_nKeys` table.

use crate::error::{ErrorClass, Finding};
use regex::Regex;

/// The dialog key lines (`DIALOG_KEY_LINES`, src:WORLDDIALOG/NpcScript.cpp:296-343 `g_nKeys`, Plan/20 §5.5).
pub const DIALOG_KEY_LINES: [u32; 46] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25,
    26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 865, 866,
];

/// Decodes a source file: UTF-8 (lossy), without BOM, `\r\n` and lone `\r` turned into `\n`.
pub fn normalise(raw: &[u8]) -> String {
    let raw = raw.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(raw);
    let text = String::from_utf8_lossy(raw);
    if text.contains('\r') {
        text.replace("\r\n", "\n").replace('\r', "\n")
    } else {
        text.into_owned()
    }
}

/// Replaces `//` and `/* */` comments by nothing (newlines are kept, so line numbers stay valid). String and
/// character literals are skipped, so a comment marker inside one is left alone.
pub fn strip_comments(text: &str) -> String {
    #[derive(Clone, Copy, PartialEq)]
    enum S {
        Code,
        Line,
        Block,
        Str,
        Chr,
    }
    let mut out = String::with_capacity(text.len());
    let mut st = S::Code;
    let mut it = text.chars().peekable();
    while let Some(c) = it.next() {
        match st {
            S::Code => match c {
                '/' if it.peek() == Some(&'/') => {
                    it.next();
                    st = S::Line;
                }
                '/' if it.peek() == Some(&'*') => {
                    it.next();
                    st = S::Block;
                    out.push(' ');
                }
                '"' => {
                    out.push(c);
                    st = S::Str;
                }
                '\'' => {
                    out.push(c);
                    st = S::Chr;
                }
                _ => out.push(c),
            },
            S::Line => {
                if c == '\n' {
                    out.push('\n');
                    st = S::Code;
                }
            }
            S::Block => {
                if c == '\n' {
                    out.push('\n');
                } else if c == '*' && it.peek() == Some(&'/') {
                    it.next();
                    st = S::Code;
                }
            }
            S::Str | S::Chr => {
                out.push(c);
                let close = if st == S::Str { '"' } else { '\'' };
                if c == '\\' {
                    if let Some(n) = it.next() {
                        out.push(n);
                    }
                } else if c == close || c == '\n' {
                    st = S::Code;
                }
            }
        }
    }
    out
}

/// Maps a byte offset of a text to its 1-based line.
pub struct LineIndex {
    starts: Vec<usize>,
}

impl LineIndex {
    /// Indexes `text`.
    pub fn new(text: &str) -> LineIndex {
        let mut starts = vec![0];
        starts.extend(text.match_indices('\n').map(|(i, _)| i + 1));
        LineIndex { starts }
    }

    /// The 1-based line of `offset`.
    pub fn line(&self, offset: usize) -> u32 {
        let idx = self.starts.partition_point(|s| *s <= offset);
        u32::try_from(idx).unwrap_or(u32::MAX)
    }
}

/// One `void CNpcScript::<script>_<key>()` function.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CppFunction {
    /// The full member name (`mafl_Babario_0`).
    pub name: String,
    /// The part before the last `_<digits>`, in its original case.
    pub script: String,
    /// The digits after the last underscore.
    pub key_text: String,
    /// The key as a number.
    pub key: u32,
    /// The text between the braces (comments removed, newlines kept).
    pub body: String,
    /// The line of the first character of the body.
    pub body_line: u32,
    /// The line of the function head.
    pub head_line: u32,
}

/// Splits `name_<digits>` into the script part and the digits.
pub fn split_suffix(name: &str) -> Option<(&str, &str)> {
    let (head, tail) = name.rsplit_once('_')?;
    (!head.is_empty() && !tail.is_empty() && tail.bytes().all(|b| b.is_ascii_digit()))
        .then_some((head, tail))
}

/// The result of reading `NpcScript.cpp`.
#[derive(Debug, Default)]
pub struct Functions {
    /// The functions in file order.
    pub list: Vec<CppFunction>,
    /// `void CNpcScript::name()` methods without a numeric suffix (ignored).
    pub other_methods: Vec<String>,
    /// Problems (duplicates, unbalanced braces).
    pub findings: Vec<Finding>,
}

/// Extracts the NPC functions from comment-stripped `NpcScript.cpp` text (Plan/20 §5.3 step 2).
pub fn extract_functions(stripped: &str) -> Functions {
    let mut out = Functions::default();
    let Ok(head) = Regex::new(r"\bvoid\s+CNpcScript::([A-Za-z_][A-Za-z0-9_]*)\s*\(\s*\)\s*\{")
    else {
        return out;
    };
    let lines = LineIndex::new(stripped);
    let bytes = stripped.as_bytes();
    let mut resume = 0usize;
    for caps in head.captures_iter(stripped) {
        let (Some(whole), Some(name)) = (caps.get(0), caps.get(1)) else {
            continue;
        };
        if whole.start() < resume {
            continue;
        }
        let open = whole.end(); // just behind `{`
        let mut depth = 1usize;
        let mut end = None;
        for (k, b) in bytes.iter().enumerate().skip(open) {
            match b {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(k);
                        break;
                    }
                }
                _ => {}
            }
        }
        let name = name.as_str();
        let Some(end) = end else {
            out.findings.push(Finding::new(
                ErrorClass::Syntax,
                name,
                lines.line(whole.start()),
                "unbalanced braces: the function body is not closed",
            ));
            break;
        };
        resume = end + 1;
        let Some((script, digits)) = split_suffix(name) else {
            out.other_methods.push(name.to_owned());
            continue;
        };
        let Ok(key) = digits.parse::<u32>() else {
            out.findings.push(Finding::new(
                ErrorClass::Syntax,
                name,
                lines.line(whole.start()),
                "the key suffix does not fit a number",
            ));
            continue;
        };
        if out.list.iter().any(|f| f.name == name) {
            out.findings.push(Finding::new(
                ErrorClass::DuplicateFunction,
                name,
                lines.line(whole.start()),
                "the function is defined twice",
            ));
            continue;
        }
        out.list.push(CppFunction {
            name: name.to_owned(),
            script: script.to_owned(),
            key_text: digits.to_owned(),
            key,
            body: stripped.get(open..end).unwrap_or("").to_owned(),
            body_line: lines.line(open),
            head_line: lines.line(whole.start()),
        });
    }
    out
}

/// One `{"reg_name", &CNpcScript::member}` entry of `g_tables`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Registration {
    /// The dispatch name.
    pub reg_name: String,
    /// The member function.
    pub member: String,
    /// The line in `NpcScriptHelper.cpp`.
    pub line: u32,
}

/// Extracts the `g_tables` initializer of `NpcScriptHelper.cpp` (Plan/20 §5.3 step 3).
pub fn extract_registrations(helper_text: &str) -> (Vec<Registration>, Vec<Finding>) {
    let stripped = strip_comments(helper_text);
    let mut findings = Vec::new();
    let mut regs = Vec::new();
    let (Ok(start), Ok(entry)) = (
        Regex::new(r"\bTABLES\s+g_tables\s*\[\s*\]\s*=\s*\{"),
        Regex::new(r#"\{\s*"([A-Za-z0-9_]+)"\s*,\s*&\s*CNpcScript::([A-Za-z0-9_]+)\s*\}"#),
    ) else {
        return (regs, findings);
    };
    let Some(m) = start.find(&stripped) else {
        findings.push(Finding::new(
            ErrorClass::Registration,
            "NpcScriptHelper.cpp",
            0,
            "the `TABLES g_tables[] = {` initializer was not found",
        ));
        return (regs, findings);
    };
    let lines = LineIndex::new(&stripped);
    let open = m.end();
    let mut depth = 1usize;
    let mut close = None;
    for (k, b) in stripped.as_bytes().iter().enumerate().skip(open) {
        match b {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    close = Some(k);
                    break;
                }
            }
            _ => {}
        }
    }
    let Some(close) = close else {
        findings.push(Finding::new(
            ErrorClass::Registration,
            "NpcScriptHelper.cpp",
            lines.line(open),
            "the g_tables initializer is not closed",
        ));
        return (regs, findings);
    };
    let region_end = close + 1;
    let region = stripped.get(open..close).unwrap_or("");
    let mut cursor = 0usize;
    for caps in entry.captures_iter(region) {
        let (Some(whole), Some(reg), Some(member)) = (caps.get(0), caps.get(1), caps.get(2)) else {
            continue;
        };
        let gap = region.get(cursor..whole.start()).unwrap_or("");
        if gap.chars().any(|c| !c.is_whitespace() && c != ',') {
            findings.push(Finding::new(
                ErrorClass::Registration,
                "NpcScriptHelper.cpp",
                lines.line(open + whole.start()),
                format!(
                    "unrecognised text between g_tables entries: {:?}",
                    gap.trim()
                ),
            ));
        }
        cursor = whole.end();
        regs.push(Registration {
            reg_name: reg.as_str().to_owned(),
            member: member.as_str().to_owned(),
            line: lines.line(open + whole.start()),
        });
    }
    let tail = region.get(cursor..).unwrap_or("");
    if tail.chars().any(|c| !c.is_whitespace() && c != ',') {
        findings.push(Finding::new(
            ErrorClass::Registration,
            "NpcScriptHelper.cpp",
            lines.line(region_end),
            format!(
                "unrecognised text at the end of g_tables: {:?}",
                tail.trim()
            ),
        ));
    }
    (regs, findings)
}

/// Reads `int g_nKeys[] = { ... };` of `NpcScript.cpp`; `None` when the table is not there.
pub fn extract_key_table(npc_script_text: &str) -> Option<Vec<u32>> {
    let stripped = strip_comments(npc_script_text);
    let re = Regex::new(r"\bint\s+g_nKeys\s*\[\s*\]\s*=\s*\{([^}]*)\}").ok()?;
    let caps = re.captures(&stripped)?;
    let inner = caps.get(1)?.as_str();
    inner
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.parse::<u32>().ok())
        .collect()
}
