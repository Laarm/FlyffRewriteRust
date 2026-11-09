//! The help family of `Client/`: `help.inc`, `treeHelp.inc`, `faq.inc`, `tip.inc`, `instantHelp.inc`,
//! `tutorial.inc` and `Guide.inc` (Plan/14 §6.19; ref/17 §8.11). All are UTF-16 files read with `CScript`
//! (`IDS_` keys resolve to their text).
//!
//! | file | grammar | result |
//! |---|---|---|
//! | `help.inc` | `KEY '{' STR* '}'` (`CProject::LoadHelp`, src:_Common/Project.cpp:1104-1127) | `topics`: key to `"   " + join(STR, "\n\n   ")` |
//! | `treeHelp.inc` | nested `KEY ('{' ... '}')?` (`CWndTreeCtrl::InterpriteScript`, src:_Interface/WndControl.cpp:957-995) | `tree` |
//! | `faq.inc` | `Q '{' A '}'` (`CWndHelpFAQ::LoadFAQ`, src:_Interface/WndHelp.cpp:325-345) | `faq`: question, `"    " + A` |
//! | `tip.inc` | `STR*` (`CWndHelpTip::OnInitialUpdate`, src:_Interface/WndHelp.cpp:514-527) | `tips`: `"    " + STR` |
//! | `instantHelp.inc` | `STR*` (src:_Interface/WndCommunication.cpp:321-331) | `instant`: the strings |
//! | `tutorial.inc` | `'{' TITLE CONTENT '}'*` (`CWndTutorial::LoadTutorial`, src:_Interface/WndTutorial.cpp:103-135) | `tutorial` |
//! | `Guide.inc` | `EVENT '{' BEGINNER n SHOWLEVEL n FLAG n KEY n STR [SEQUENCE n] [LEVEL n] [CONDITION n] [INPUTCHAR n] [INPUTSTR STR] '}'` (`CWndGuideSystem::LoadGuide`, src:_Interface/WndGuideSystem.cpp:681-756) | `guide` (the server reads only the maximum `LEVEL`) |
//!
//! The strings are what the original client held after loading, prefixes included. Two loop quirks of the
//! original are fixed (no effect on the data set): `tutorial.inc` ended with a phantom entry made of
//! empty strings, and a block that runs into the end of the file is dropped instead of looping.

use serde::{Deserialize, Serialize};

/// A node of the help tree (`treeHelp.inc`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct HelpTreeNode {
    /// The keyword shown in the tree (also the key of a help topic).
    pub key: String,
    /// The sub-nodes (depth at most 3 in the data).
    pub children: Vec<HelpTreeNode>,
}

/// A titled text: an FAQ entry, an instant-help line, a tutorial page.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct HelpEntry {
    /// The question or title.
    pub key: String,
    /// The answer or content.
    pub text: String,
}

/// One step of the guide system (`GUIDE_STRUCT`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct GuideStep {
    /// The first token: a `GUIDE_EVENT_*` or `APP_*` value (`m_nEventMsg`).
    pub event: u32,
    /// `BEGINNER`.
    pub beginner: bool,
    /// `SHOWLEVEL`.
    pub show_level: i32,
    /// `FLAG`.
    pub flag: bool,
    /// `KEY`.
    pub key: i32,
    /// The guide text (IDS key resolved).
    pub text: String,
    /// `SEQUENCE` (0 = an event guide).
    pub sequence: i32,
    /// `LEVEL` (the tutorial level the step belongs to).
    pub level: i32,
    /// `CONDITION` (`NO_CONDITION` 0 ... `OPEN_WINDOW` 7).
    pub condition: i32,
    /// `INPUTCHAR`.
    pub input_char: u32,
    /// `INPUTSTR` (the string and any further tokens up to the closing brace, concatenated).
    pub input_str: String,
}

/// Everything of the help family (`ClientData::help`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HelpTexts {
    /// `m_mapHelp`: key to `"   " + join(STR, "\n\n   ")`, in file order (a later duplicate replaces the text).
    pub topics: indexmap::IndexMap<String, String>,
    /// The help tree.
    pub tree: Vec<HelpTreeNode>,
    /// The FAQ.
    pub faq: Vec<HelpEntry>,
    /// The login tips (with the `"    "` prefix).
    pub tips: Vec<String>,
    /// The instant-help lines, as `key = text` with an empty key.
    pub instant: Vec<HelpEntry>,
    /// The tutorial pages.
    pub tutorial: Vec<HelpEntry>,
    /// The full guide.
    pub guide: Vec<GuideStep>,
}

#[cfg(feature = "load")]
pub(crate) use parse::{
    parse_faq, parse_guide, parse_help_topics, parse_instant_help, parse_tips, parse_tree_help,
    parse_tutorial,
};

#[cfg(feature = "load")]
mod parse {
    use super::{GuideStep, HelpEntry, HelpTreeNode};
    use flyff_script::{DiagCode, Diagnostics, ScannerOptions, Script, ScriptCtx, Severity};
    use indexmap::IndexMap;
    use std::sync::Arc;

    fn script<'a>(
        text: &'a str,
        file: &Arc<str>,
        sctx: &'a ScriptCtx,
        diags: &'a mut Diagnostics,
    ) -> Script<'a> {
        Script::new(
            text,
            Arc::clone(file),
            ScannerOptions::default(),
            sctx,
            diags,
        )
    }

    /// `CProject::LoadHelp`.
    pub(crate) fn parse_help_topics(
        text: &str,
        file: &Arc<str>,
        sctx: &ScriptCtx,
        diags: &mut Diagnostics,
    ) -> IndexMap<String, String> {
        let mut out = IndexMap::new();
        let mut sc = script(text, file, sctx, diags);
        sc.get_token();
        while !sc.finished() {
            let key = sc.token().to_owned();
            sc.get_token(); // {
            sc.get_token();
            let mut s = String::from("   ");
            while !sc.token().starts_with('}') && !sc.finished() {
                s.push_str(sc.token());
                s.push_str("\n\n   ");
                sc.get_token();
            }
            if sc.finished() && !sc.token().starts_with('}') {
                sc.report(
                    Severity::Warning,
                    DiagCode::ValueOutOfRange,
                    format!("help topic `{key}` runs into the end of the file; dropped"),
                );
                break;
            }
            out.insert(key, s);
            sc.get_token();
        }
        out
    }

    /// The deepest nesting of `treeHelp.inc` accepted (the data has 3 levels).
    const MAX_TREE_DEPTH: usize = 32;

    /// `CWndTreeCtrl::InterpriteScript`: the current token is always the pending keyword.
    fn interpret(sc: &mut Script<'_>, out: &mut Vec<HelpTreeNode>, depth: usize) {
        sc.get_token(); // keyword
        while !sc.token().starts_with('}') && !sc.finished() {
            let mut node = HelpTreeNode {
                key: sc.token().to_owned(),
                children: Vec::new(),
            };
            sc.get_token(); // the token after the keyword
            if sc.token().starts_with('{') {
                if depth >= MAX_TREE_DEPTH {
                    sc.report(
                        Severity::Error,
                        DiagCode::CapacityExceeded,
                        "treeHelp: nesting too deep; the rest is ignored".to_owned(),
                    );
                    out.push(node);
                    return;
                }
                interpret(sc, &mut node.children, depth + 1);
            }
            out.push(node);
        }
        if sc.finished() {
            return;
        }
        sc.get_token(); // the token after the closing brace is the parent's next keyword
    }

    /// `CWndTreeCtrl::LoadTreeScript`.
    pub(crate) fn parse_tree_help(
        text: &str,
        file: &Arc<str>,
        sctx: &ScriptCtx,
        diags: &mut Diagnostics,
    ) -> Vec<HelpTreeNode> {
        let mut out = Vec::new();
        let mut sc = script(text, file, sctx, diags);
        loop {
            interpret(&mut sc, &mut out, 0);
            sc.get_token();
            if sc.finished() {
                break;
            }
        }
        out
    }

    /// `CWndHelpFAQ::LoadFAQ`: `Q { A }` pairs; the answer gets four leading spaces.
    pub(crate) fn parse_faq(
        text: &str,
        file: &Arc<str>,
        sctx: &ScriptCtx,
        diags: &mut Diagnostics,
    ) -> Vec<HelpEntry> {
        let mut out = Vec::new();
        let mut sc = script(text, file, sctx, diags);
        sc.get_token();
        while !sc.finished() {
            let key = sc.token().to_owned();
            sc.get_token(); // {
            sc.get_token();
            if sc.finished() {
                break;
            }
            out.push(HelpEntry {
                key,
                text: format!("    {}", sc.token()),
            });
            sc.get_token(); // }
            sc.get_token();
        }
        out
    }

    /// `CWndHelpTip`: every token is a tip, with four leading spaces.
    pub(crate) fn parse_tips(
        text: &str,
        file: &Arc<str>,
        sctx: &ScriptCtx,
        diags: &mut Diagnostics,
    ) -> Vec<String> {
        let mut out = Vec::new();
        let mut sc = script(text, file, sctx, diags);
        sc.get_token();
        while !sc.finished() {
            out.push(format!("    {}", sc.token()));
            sc.get_token();
        }
        out
    }

    /// The instant-help lines: every token is one line.
    pub(crate) fn parse_instant_help(
        text: &str,
        file: &Arc<str>,
        sctx: &ScriptCtx,
        diags: &mut Diagnostics,
    ) -> Vec<HelpEntry> {
        let mut out = Vec::new();
        let mut sc = script(text, file, sctx, diags);
        sc.get_token();
        while !sc.finished() {
            out.push(HelpEntry {
                key: String::new(),
                text: sc.token().to_owned(),
            });
            sc.get_token();
        }
        out
    }

    /// `CWndTutorial::LoadTutorial`: `{ TITLE CONTENT }` pages. **FIX:** the original stored one phantom page
    /// of empty strings after the last block; it is not kept.
    pub(crate) fn parse_tutorial(
        text: &str,
        file: &Arc<str>,
        sctx: &ScriptCtx,
        diags: &mut Diagnostics,
    ) -> Vec<HelpEntry> {
        let mut out = Vec::new();
        let mut sc = script(text, file, sctx, diags);
        loop {
            sc.get_token(); // {
            if sc.finished() {
                break;
            }
            sc.get_token();
            let key = sc.token().to_owned();
            sc.get_token();
            if sc.finished() {
                break;
            }
            let content = sc.token().to_owned();
            out.push(HelpEntry { key, text: content });
            sc.get_token(); // }
        }
        out
    }

    /// `CWndGuideSystem::LoadGuide`.
    pub(crate) fn parse_guide(
        text: &str,
        file: &Arc<str>,
        sctx: &ScriptCtx,
        diags: &mut Diagnostics,
    ) -> Vec<GuideStep> {
        let mut out = Vec::new();
        let mut sc = script(text, file, sctx, diags);
        while !sc.finished() {
            let event = sc.get_number();
            if sc.finished() {
                break;
            }
            sc.get_token(); // {
            if !sc.token().starts_with('{') {
                sc.get_token();
                continue;
            }
            let mut g = GuideStep {
                event,
                ..GuideStep::default()
            };
            sc.get_token(); // BEGINNER
            g.beginner = sc.get_number() != 0;
            sc.get_token(); // SHOWLEVEL
            g.show_level = sc.get_number_i32();
            sc.get_token(); // FLAG
            g.flag = sc.get_number() != 0;
            sc.get_token(); // KEY
            g.key = sc.get_number_i32();
            sc.get_token();
            g.text = sc.token().to_owned();
            sc.get_token();
            let is = |sc: &Script<'_>, w: &str| sc.token().eq_ignore_ascii_case(w);
            if is(&sc, "SEQUENCE") {
                g.sequence = sc.get_number_i32();
                sc.get_token();
            }
            if is(&sc, "LEVEL") {
                g.level = sc.get_number_i32();
                sc.get_token();
            }
            if is(&sc, "CONDITION") {
                g.condition = sc.get_number_i32();
                sc.get_token();
            }
            if is(&sc, "INPUTCHAR") {
                g.input_char = sc.get_number();
                sc.get_token();
            }
            if is(&sc, "INPUTSTR") {
                sc.get_token();
                g.input_str = sc.token().to_owned();
                sc.get_token(); // }
                while !sc.token().starts_with('}') && !sc.finished() {
                    g.input_str.push_str(sc.token());
                    sc.get_token();
                }
            }
            out.push(g);
        }
        out
    }
}
