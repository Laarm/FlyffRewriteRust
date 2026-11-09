//! The `IDS_` string tables (Plan/14 §4.4, ref/17 §5.4; `LoadStrings` + `CScript::LoadString`,
//! src:_Common/ProjectCmn.cpp:619-722, Script.cpp:103-123).
//!
//! A `*.txt.txt` file is a list of `IDS_KEY<TAB>rest of the line` entries. All tables share one flat,
//! case-sensitive key space and the **first** entry of a key wins. The tokenizer later replaces a key
//! by its text while data files are read (see [`crate::Script`]).

use crate::defines::Defines;
use crate::diag::{DiagCode, Diagnostics, Severity};
use crate::scanner::{Scanner, ScannerOptions, TokenType};
use crate::script::ScriptVariant;
use flyff_common::det::DetMap;
use std::sync::Arc;

/// The string tables in load order (`CProject::LoadStrings`, English build): 64 entries, 63 distinct
/// files (`World\WdVolcane\WdVolcane.txt.txt` is listed twice; it is empty). Names are literals of the
/// original; resolution is case-insensitive and a missing file is skipped (`WdEvent01` is missing).
/// `Client\GameGuard.txt.txt` is commented out in the original (DROP).
pub const STRING_TABLE_FILES: [&str; 64] = [
    "character.txt.txt",
    "character-etc.txt.txt",
    "character-school.txt.txt",
    "etc.txt.txt",
    "propCtrl.txt.txt",
    "propItem.txt.txt",
    "propItemEtc.txt.txt",
    "propKarma.txt.txt",
    "propMotion.txt.txt",
    "propMover.txt.txt",
    "propQuest.txt.txt",
    "propQuest-DungeonandPK.txt.txt",
    "propQuest-RequestBox2.txt.txt",
    "propQuest-RequestBox.txt.txt",
    "propQuest-Scenario.txt.txt",
    "propSkill.txt.txt",
    "propTroupeSkill.txt.txt",
    "resData.txt.txt",
    "textClient.txt.txt",
    "textEmotion.txt.txt",
    "world.txt.txt",
    "Client\\treeHelp.txt.txt",
    "Client\\tip.txt.txt",
    "Client\\PatchClient.txt.txt",
    "Client\\InstantHelp.txt.txt",
    "Client\\help.txt.txt",
    "Client\\Guide.txt.txt",
    "Client\\tutorial.txt.txt",
    "Client\\faq.txt.txt",
    "World\\WdVolcane\\WdVolcane.txt.txt",
    "World\\WdMadrigal\\wdMadrigal.txt.txt",
    "World\\WdKebaras\\WdKebaras.txt.txt",
    "World\\WdGuildWar\\WdGuildWar.txt.txt",
    "World\\WdEvent01\\WdEvent01.txt.txt",
    "World\\DuMuscle\\DuMuscle.txt.txt",
    "World\\DuKrr\\DuKrr.txt.txt",
    "World\\DuFlMas\\DuFlMas.txt.txt",
    "World\\DuDaDk\\DuDaDk.txt.txt",
    "World\\DuBear\\DuBear.txt.txt",
    "World\\DuSaTemple\\DuSaTemple.txt.txt",
    "World\\DuSaTempleBoss\\DuSaTempleBoss.txt.txt",
    "World\\WdVolcane\\WdVolcane.txt.txt",
    "World\\WdVolcaneRed\\WdVolcaneRed.txt.txt",
    "World\\WdVolcaneYellow\\WdVolcaneYellow.txt.txt",
    "World\\WdArena\\WdArena.txt.txt",
    "lordskill.txt.txt",
    "World\\WdHeaven01\\wdheaven01.txt.txt",
    "World\\WdHeaven02\\wdheaven02.txt.txt",
    "World\\WdHeaven03\\wdheaven03.txt.txt",
    "World\\WdHeaven04\\wdheaven04.txt.txt",
    "World\\WdHeaven05\\wdheaven05.txt.txt",
    "World\\WdHeaven06\\wdheaven06.txt.txt",
    "World\\WdHeaven06_1\\wdheaven06_1.txt.txt",
    "honorList.txt.txt",
    "World\\WdCisland\\WdCisland.txt.txt",
    "World\\DuOminous\\duominous.txt.txt",
    "World\\DuOminous_1\\duominous_1.txt.txt",
    "World\\WdGuildhousesmall\\WdGuildhousesmall.txt.txt",
    "World\\WdGuildhousemiddle\\WdGuildhousemiddle.txt.txt",
    "World\\WdGuildhouselarge\\WdGuildhouselarge.txt.txt",
    "World\\DuDreadfulCave\\DuDreadfulCave.txt.txt",
    "World\\DuRustia\\DuRustia.txt.txt",
    "World\\DuRustia_1\\DuRustia_1.txt.txt",
    "teleport16.txt.txt",
];

/// The merged string table: key -> text, in load order.
#[derive(Clone, Default, Debug)]
pub struct StringTable {
    by_key: DetMap<String, String>,
}

impl StringTable {
    /// An empty table.
    pub fn new() -> Self {
        Self::default()
    }

    /// The text of `key` (exact, case-sensitive).
    pub fn get(&self, key: &str) -> Option<&str> {
        self.by_key.get(key).map(String::as_str)
    }

    /// `true` when `key` exists.
    pub fn contains(&self, key: &str) -> bool {
        self.by_key.contains_key(key)
    }

    /// Number of keys.
    pub fn len(&self) -> usize {
        self.by_key.len()
    }

    /// `true` when the table has no key.
    pub fn is_empty(&self) -> bool {
        self.by_key.is_empty()
    }

    /// All `(key, text)` pairs in load order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.by_key.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    /// Inserts an entry; the **first** definition of a key wins. Returns `true` when the key was new.
    pub fn insert(&mut self, key: &str, value: &str) -> bool {
        if self.by_key.contains_key(key) {
            return false;
        }
        self.by_key.insert(key.to_owned(), value.to_owned());
        true
    }

    /// `CScript::LoadString` of one file: reads `text` and adds its entries. Returns the number of keys
    /// added. `defines` is the table that the recovery path of a bad key substitutes from, as the
    /// original used the global define map.
    ///
    /// Per entry: `key` = raw token; a key that does not start with `IDS` is Error `StringBadKey`, the
    /// next token is read **with** substitution and becomes the next key candidate; otherwise `value` =
    /// rest of the line (trimmed, possibly empty, may contain tabs and `//`) and a duplicate key is
    /// Error `StringDuplicate`.
    pub fn load_text(
        &mut self,
        text: &str,
        file: &str,
        defines: &Defines,
        diags: &mut Diagnostics,
    ) -> usize {
        let file_arc: Arc<str> = Arc::from(file);
        let mut sc = Scanner::new(text, file_arc.clone(), ScannerOptions::default());
        let before = self.by_key.len();
        sc.get_token(false);
        let mut cur = sc.token().to_owned();
        while !sc.finished() {
            if !cur.starts_with("IDS") {
                diags.report(
                    Severity::Error,
                    DiagCode::StringBadKey,
                    Some(&file_arc),
                    sc.line(),
                    format!("LoadString: {file}: `{cur}` is not an IDS key"),
                );
                // `GetToken()` of CScript: classification and substitution.
                sc.get_token(false);
                cur = self.substituted(&sc, defines);
                continue;
            }
            let line = sc.line();
            let key = std::mem::take(&mut cur);
            let value = sc.get_last_full().to_owned();
            if self.by_key.contains_key(&key) {
                diags.report(
                    Severity::Error,
                    DiagCode::StringDuplicate,
                    Some(&file_arc),
                    line,
                    format!("string error: {key}"),
                );
            } else {
                self.by_key.insert(key, value);
            }
            sc.get_token(false);
            cur = sc.token().to_owned();
        }
        diags.extend(sc.take_diagnostics());
        self.by_key.len() - before
    }

    /// The text of the scanner's current (raw) token after `CScript::GetToken` substitution (server
    /// variant): identifiers become their string-table text or define value.
    fn substituted(&self, sc: &Scanner<'_>, defines: &Defines) -> String {
        let tok = sc.token();
        let is_identifier =
            sc.token_type() == TokenType::Temp && !ScriptVariant::Server.is_keyword(tok);
        if is_identifier {
            if let Some(v) = self.by_key.get(tok) {
                return v.clone();
            }
            if let Some(n) = defines.get(tok) {
                return n.to_string();
            }
        }
        tok.to_owned()
    }
}
