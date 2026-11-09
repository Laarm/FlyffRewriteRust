//! The `Masquerade.prj` driver (Plan/14 §5.2, ref/17 §6.1; src:_Common/Project.cpp:463-575).
//!
//! The project file is read with a **raw** `Scanner` (no define or string substitution, whitespace mode,
//! `//` and `/* */` comments honoured). The loop is: read a token; compare it **case-sensitively** with the
//! key table; on a match read one more token (the argument; for a quoted name this is the text without
//! the quotes) and record the entry. Every other token, including `project`, `define`, `string`,
//! `localDef`, `textClient`, `textServer`, `beginItem`, `Event_LevelUp_ItemList` and their arguments, is
//! skipped one token at a time. Entries are returned in **file order**, which is the dispatch order.

use flyff_script::{Scanner, ScannerOptions};
use std::sync::Arc;

/// The keys of the table of 14 §5.2.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PrjKey {
    /// `model`: `mdlObj.inc` / `mdlDyna.inc` (L10, L11).
    Model,
    /// `world`: `World.inc` (L12).
    World,
    /// `terrain`: client only, deferred to P7.
    Terrain,
    /// `billboard`: the argument is read, nothing is loaded.
    Billboard,
    /// `propMover` (L13).
    PropMover,
    /// `propItem` (L14).
    PropItem,
    /// `propSkill` (L15).
    PropSkill,
    /// `MotionProp` (the original's spelling; this prj says `propMotion`, which is not a key).
    MotionProp,
    /// `propTroupeSkill` (L16).
    PropTroupeSkill,
    /// `propKarma`: inactive at v15 (`__VER < 8`), dropped.
    PropKarma,
    /// `propJob` (L17).
    PropJob,
    /// `propCtrl` (L18).
    PropCtrl,
    /// `propSkillAdd` (L19).
    PropSkillAdd,
    /// `character` (L20, three times in the real prj).
    Character,
    /// `propQuest` (L21, five times).
    PropQuest,
    /// `expTable` (L22).
    ExpTable,
    /// `help`: client only, deferred to P7.
    Help,
}

impl PrjKey {
    /// The key table (spelling is case-sensitive).
    pub const ALL: [(&'static str, PrjKey); 17] = [
        ("model", PrjKey::Model),
        ("world", PrjKey::World),
        ("terrain", PrjKey::Terrain),
        ("billboard", PrjKey::Billboard),
        ("propMover", PrjKey::PropMover),
        ("propItem", PrjKey::PropItem),
        ("propSkill", PrjKey::PropSkill),
        ("MotionProp", PrjKey::MotionProp),
        ("propTroupeSkill", PrjKey::PropTroupeSkill),
        ("propKarma", PrjKey::PropKarma),
        ("propJob", PrjKey::PropJob),
        ("propCtrl", PrjKey::PropCtrl),
        ("propSkillAdd", PrjKey::PropSkillAdd),
        ("character", PrjKey::Character),
        ("propQuest", PrjKey::PropQuest),
        ("expTable", PrjKey::ExpTable),
        ("help", PrjKey::Help),
    ];

    /// The key of a token, if it is one.
    pub fn from_token(token: &str) -> Option<PrjKey> {
        Self::ALL.iter().find(|(n, _)| *n == token).map(|(_, k)| *k)
    }

    /// The spelling of the key.
    pub fn name(self) -> &'static str {
        Self::ALL
            .iter()
            .find(|(_, k)| *k == self)
            .map_or("?", |(n, _)| n)
    }
}

/// One recognised line of the project file.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PrjEntry {
    /// The key.
    pub key: PrjKey,
    /// The argument token (quotes removed), `""` when the file ended after the key.
    pub arg: String,
    /// 1-based line of the key.
    pub line: u32,
}

/// Parses the project file text into its recognised entries, in file order.
pub fn parse_prj(text: &str, file: &str) -> Vec<PrjEntry> {
    let mut sc = Scanner::new(text, Arc::from(file), ScannerOptions::default());
    let mut out = Vec::new();
    loop {
        sc.get_token(false);
        if sc.finished() {
            break;
        }
        if let Some(key) = PrjKey::from_token(sc.token()) {
            let line = sc.line();
            sc.get_token(false);
            // At the end of the file the argument is empty (`finished`).
            out.push(PrjEntry {
                key,
                arg: sc.token().to_owned(),
                line,
            });
            if sc.finished() {
                break;
            }
        }
    }
    out
}
