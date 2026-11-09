//! The mini-game scripts: `MiniGame_Alphabet.txt(.txt)`, `MiniGame_KawiBawiBo.txt`, `MiniGame_FiveSystem.txt`,
//! `MiniGame_Reassemble.txt` (`CMiniGame::Load_*`, Plan/14 §6.14; ref/15 §17; src:_Common/MiniGame.cpp:645-850).
//! All four are read with a raw `CScanner`; item names are resolved with `GetDefineNum` (an unknown name is an
//! Error and the row is skipped; the original aborts the load).
//!
//! ```text
//! Alphabet:   PENYA N     WORD word PUBLIC ...question-key '{' (ITEM COUNT PROB)* '}'
//!             (the question text is looked up by atoi( last 4 chars of the key ) in MiniGame_Alphabet.txt.txt:
//!              `KEY rest-of-line` per question; an unknown id is an Error)
//! KawiBawiBo: SENDITEM WINS DRAW LOST WIN ITEM COUNT         (probabilities n / 1,000,000)
//! FiveSystem: MINPENYA n  MAXPENYA n  MULTIPLE n  PERCENT COUNT MIN MAX      (COUNT 0..5)
//! Reassemble: CARD '{' ITEM* '}'  (at most 10, the original tests `size() <= 9` before pushing)
//!             ITEM '{' '{' (ITEM COUNT PROB)* '}'
//! ```
//!
//! This data set ships 50 alphabet words with empty question texts, 11 `SENDITEM` lines (the rest are in a
//! `/* */` block), six `PERCENT` rows, nine cards and one reward.

use serde::{Deserialize, Serialize};

/// A paid item with its count and probability (`__ITEMGAME`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MiniGameItem {
    /// The item.
    pub item: u32,
    /// The count.
    pub count: i32,
    /// The probability out of 1,000,000.
    pub per: u32,
}

/// One alphabet word (`__ALPHABET`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlphabetWord {
    /// The answer, upper-cased.
    pub word: String,
    /// How many letters are shown at the start (`nPublic`).
    pub public: i32,
    /// The question text (empty when the question file has none for the id).
    pub question: String,
    /// The reward items.
    pub items: Vec<MiniGameItem>,
}

/// One `SENDITEM` line (`__KAWIBAWIBO`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct KawiBawiBoRow {
    /// The win streak the row is for (the map key).
    pub wins: i32,
    /// The draw probability (n / 1,000,000).
    pub draw: u32,
    /// The loss probability.
    pub lost: u32,
    /// The win probability.
    pub win: u32,
    /// The reward item.
    pub item: u32,
    /// The reward count.
    pub count: i32,
}

/// One `PERCENT` row (`__FIVESYSTEM`): the chance (n / 1,000,000) of a pot with `count` top bids.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FiveSystemRow {
    /// The number of top bids (0..=5).
    pub count: i32,
    /// `dwMin`: the chance of the top bid.
    pub min: u32,
    /// `dwMax`: the chance of every other bid.
    pub max: u32,
}

/// The mini-game tables.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MiniGameTables {
    /// Alphabet: the penya paid per try.
    pub alphabet_pay_penya: i32,
    /// Alphabet: the words in file order.
    pub alphabet: Vec<AlphabetWord>,
    /// KawiBawiBo: the rows ascending by win streak (`map::insert`: the first line of a streak wins).
    pub kawibawibo: Vec<KawiBawiBoRow>,
    /// KawiBawiBo: the highest win streak (`nKawiBawiBo_MaxWin`).
    pub kawibawibo_max_win: i32,
    /// FiveSystem: `MINPENYA`.
    pub five_min_penya: i32,
    /// FiveSystem: `MAXPENYA`.
    pub five_max_penya: i32,
    /// FiveSystem: `MULTIPLE`.
    pub five_multiple: i32,
    /// FiveSystem: the `PERCENT` rows (the first row of a count wins).
    pub five_percent: Vec<FiveSystemRow>,
    /// Reassemble: the card items (at most 10).
    pub reassemble_cards: Vec<u32>,
    /// Reassemble: the rewards.
    pub reassemble_items: Vec<MiniGameItem>,
}

impl MiniGameTables {
    /// `FindKawibawibo( winCount )`: the row for the streak, capped at the highest configured streak; `None`
    /// for a negative streak or a streak without a row (the original returns a zeroed row).
    pub fn find_kawibawibo(&self, wins: i32) -> Option<&KawiBawiBoRow> {
        if wins < 0 {
            return None;
        }
        let w = wins.min(self.kawibawibo_max_win);
        self.kawibawibo.iter().find(|r| r.wins == w)
    }
}

#[cfg(feature = "load")]
pub(crate) use parse::{
    parse_alphabet, parse_five_system, parse_kawibawibo, parse_questions, parse_reassemble,
};

#[cfg(feature = "load")]
mod parse {
    use super::{AlphabetWord, FiveSystemRow, KawiBawiBoRow, MiniGameItem, MiniGameTables};
    use flyff_script::{
        DiagCode, Diagnostics, Scanner, ScannerOptions, ScriptCtx, Severity, c_atoi,
    };
    use std::collections::BTreeMap;
    use std::sync::Arc;

    fn open(sc: &Scanner<'_>) -> bool {
        !sc.token().starts_with('}') && !sc.finished()
    }

    /// The text id of a key: `atoi( Token.Right( 4 ) )` (the last four characters, or all of a shorter token).
    pub(crate) fn id_of(key: &str) -> i32 {
        let n = key.chars().count();
        let tail: String = key.chars().skip(n.saturating_sub(4)).collect();
        c_atoi(tail.as_bytes())
    }

    fn report(
        diags: &mut Diagnostics,
        sc: &Scanner<'_>,
        severity: Severity,
        code: DiagCode,
        msg: String,
    ) {
        diags.report(severity, code, Some(sc.file()), sc.line(), msg);
    }

    /// `MiniGame_Alphabet.txt.txt`: `KEY rest-of-line` lines; the first line of an id wins (`map::insert`).
    pub(crate) fn parse_questions(
        text: &str,
        file: &Arc<str>,
        diags: &mut Diagnostics,
    ) -> BTreeMap<i32, String> {
        let mut out = BTreeMap::new();
        let mut sc = Scanner::new(text, Arc::clone(file), ScannerOptions::default());
        sc.get_token(false);
        while !sc.finished() {
            let id = id_of(sc.token());
            let question = sc.get_last_full().to_owned();
            out.entry(id).or_insert(question);
            sc.get_token(false);
        }
        diags.extend(sc.take_diagnostics());
        out
    }

    fn item_row(
        sc: &mut Scanner<'_>,
        sctx: &ScriptCtx,
        diags: &mut Diagnostics,
        what: &str,
    ) -> Option<MiniGameItem> {
        let name = sc.token().to_owned();
        let item = sctx.defines.get(&name).map(|v| v as u32);
        let count = sc.get_number_i32(false);
        let per = sc.get_number(false);
        match item {
            Some(item) if item != u32::MAX => Some(MiniGameItem { item, count, per }),
            _ => {
                report(
                    diags,
                    sc,
                    Severity::Error,
                    DiagCode::UnknownIdentifier,
                    format!("{what}: {name} is not a define; the row is skipped"),
                );
                None
            }
        }
    }

    /// `Load_Alphabet` (the second half) over `MiniGame_Alphabet.txt`, merged into `out`.
    pub(crate) fn parse_alphabet(
        text: &str,
        file: &Arc<str>,
        sctx: &ScriptCtx,
        questions: &BTreeMap<i32, String>,
        diags: &mut Diagnostics,
        out: &mut MiniGameTables,
    ) {
        let mut sc = Scanner::new(text, Arc::clone(file), ScannerOptions::default());
        sc.get_token(false);
        while !sc.finished() {
            match sc.token().to_owned().as_str() {
                "PENYA" => out.alphabet_pay_penya = sc.get_number_i32(false),
                "WORD" => {
                    sc.get_token(false);
                    let mut w = AlphabetWord {
                        word: sc.token().to_uppercase(),
                        public: 0,
                        ..AlphabetWord::default()
                    };
                    w.public = sc.get_number_i32(false);
                    let key = sc.get_last_full().to_owned();
                    let id = id_of(&key);
                    match questions.get(&id) {
                        Some(q) => w.question = q.clone(),
                        None => report(
                            diags,
                            &sc,
                            Severity::Error,
                            DiagCode::RefUnresolved,
                            format!(
                                "MiniGame_Alphabet.txt: question id {id} ({key:?}) of {:?} does not exist",
                                w.word
                            ),
                        ),
                    }
                    sc.get_token(false); // {
                    sc.get_token(false);
                    while open(&sc) {
                        if let Some(it) = item_row(&mut sc, sctx, diags, "MiniGame_Alphabet.txt") {
                            w.items.push(it);
                        }
                        sc.get_token(false);
                    }
                    out.alphabet.push(w);
                }
                _ => {}
            }
            sc.get_token(false);
        }
        diags.extend(sc.take_diagnostics());
    }

    /// `Load_KawiBawiBo` over `MiniGame_KawiBawiBo.txt`.
    pub(crate) fn parse_kawibawibo(
        text: &str,
        file: &Arc<str>,
        sctx: &ScriptCtx,
        diags: &mut Diagnostics,
        out: &mut MiniGameTables,
    ) {
        let mut sc = Scanner::new(text, Arc::clone(file), ScannerOptions::default());
        let mut max_win = 0;
        sc.get_token(false);
        while !sc.finished() {
            if sc.token() == "SENDITEM" {
                let wins = sc.get_number_i32(false);
                let draw = sc.get_number(false);
                let lost = sc.get_number(false);
                let win = sc.get_number(false);
                sc.get_token(false);
                let name = sc.token().to_owned();
                let item = sctx.defines.get(&name).map(|v| v as u32);
                let count = sc.get_number_i32(false);
                match item {
                    Some(item) if item != u32::MAX => {
                        if !out.kawibawibo.iter().any(|r| r.wins == wins) {
                            out.kawibawibo.push(KawiBawiBoRow {
                                wins,
                                draw,
                                lost,
                                win,
                                item,
                                count,
                            });
                        }
                        max_win = max_win.max(wins);
                    }
                    _ => report(
                        diags,
                        &sc,
                        Severity::Error,
                        DiagCode::UnknownIdentifier,
                        format!(
                            "MiniGame_KawiBawiBo.txt: {name} is not a define; the line is skipped"
                        ),
                    ),
                }
            }
            sc.get_token(false);
        }
        out.kawibawibo.sort_by_key(|r| r.wins);
        out.kawibawibo_max_win = max_win;
        diags.extend(sc.take_diagnostics());
    }

    /// `Load_FiveSystem` over `MiniGame_FiveSystem.txt`.
    pub(crate) fn parse_five_system(
        text: &str,
        file: &Arc<str>,
        diags: &mut Diagnostics,
        out: &mut MiniGameTables,
    ) {
        let mut sc = Scanner::new(text, Arc::clone(file), ScannerOptions::default());
        sc.get_token(false);
        while !sc.finished() {
            match sc.token().to_owned().as_str() {
                "MINPENYA" => out.five_min_penya = sc.get_number_i32(false),
                "MAXPENYA" => out.five_max_penya = sc.get_number_i32(false),
                "MULTIPLE" => out.five_multiple = sc.get_number_i32(false),
                "PERCENT" => {
                    let row = FiveSystemRow {
                        count: sc.get_number_i32(false),
                        min: sc.get_number(false),
                        max: sc.get_number(false),
                    };
                    if !out.five_percent.iter().any(|r| r.count == row.count) {
                        out.five_percent.push(row);
                    }
                }
                _ => {}
            }
            sc.get_token(false);
        }
        diags.extend(sc.take_diagnostics());
    }

    /// `Load_Reassemble` over `MiniGame_Reassemble.txt`.
    pub(crate) fn parse_reassemble(
        text: &str,
        file: &Arc<str>,
        sctx: &ScriptCtx,
        diags: &mut Diagnostics,
        out: &mut MiniGameTables,
    ) {
        let mut sc = Scanner::new(text, Arc::clone(file), ScannerOptions::default());
        sc.get_token(false);
        while !sc.finished() {
            match sc.token().to_owned().as_str() {
                "CARD" => {
                    sc.get_token(false); // {
                    sc.get_token(false);
                    while open(&sc) {
                        let name = sc.token().to_owned();
                        match sctx.defines.get(&name).map(|v| v as u32) {
                            Some(card) if card != u32::MAX => {
                                // KEEP-QUIRK: `size() <= 9` before the push allows a tenth card
                                if out.reassemble_cards.len() <= 9 {
                                    out.reassemble_cards.push(card);
                                }
                            }
                            _ => report(
                                diags,
                                &sc,
                                Severity::Error,
                                DiagCode::UnknownIdentifier,
                                format!(
                                    "MiniGame_Reassemble.txt: CARD {name} is not a define; skipped"
                                ),
                            ),
                        }
                        sc.get_token(false);
                    }
                }
                "ITEM" => {
                    sc.get_token(false); // {
                    sc.get_token(false); // {
                    while open(&sc) {
                        if let Some(it) = item_row(&mut sc, sctx, diags, "MiniGame_Reassemble.txt")
                        {
                            out.reassemble_items.push(it);
                        }
                        sc.get_token(false);
                    }
                }
                _ => {}
            }
            sc.get_token(false);
        }
        diags.extend(sc.take_diagnostics());
    }
}
