//! `Exchange_Script.txt`: the exchange menus (`CExchange::Load_Script`, Plan/14 §6.13, ref/17 §8.7;
//! src:_Common/Exchange.cpp:34-205). Read with a raw `CScanner`; every name is resolved with `GetDefineNum`.
//!
//! ```text
//! MMI_X '{' DESCRIPTION '{' TID* '}'
//!           (SET TID '{' RESULTMSG '{' TID* '}'  CONDITION '{' (ITEM|PENYA COUNT)* '}'  REMOVE '{' (ITEM|PENYA COUNT)* '}'
//!                        CONDITION_POINT '{' (TYPE POINT)* '}'  REMOVE_POINT '{' (TYPE POINT)* '}'
//!                        PAY NUM '{' (ITEM COUNT PROB [FLAG])* '}' '}')* '}'
//! ```
//!
//! `PENYA` stands for `II_GOLD_SEED1`. At most 15 sets per menu are kept. The `PAY` probabilities are
//! normalised like the original (their sum is 1,000,000: the entry that crosses it is cut, a short sum is
//! added to the last entry); the original reports a bad sum only on the world server (here a Warning).

use serde::{Deserialize, Serialize};

/// The most sets of one menu (`nCount <= 15`).
pub const MAX_EXCHANGE_SETS: usize = 15;

/// One paid item of a set.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ExchangePay {
    /// The item.
    pub item: u32,
    /// The count.
    pub count: i32,
    /// The probability in 1/1,000,000.
    pub prob: i32,
    /// The optional flag number after the probability (0 when absent).
    pub flag: u8,
}

/// One exchange set of a menu.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ExchangeSet {
    /// The text id of the list entry.
    pub tid: u32,
    /// The result message text ids (success, failure).
    pub result_msgs: Vec<u32>,
    /// The items needed: `(item, count)`; `PENYA` is `II_GOLD_SEED1`.
    pub conditions: Vec<[u32; 2]>,
    /// The points needed: `(type, point)`.
    pub condition_points: Vec<[i32; 2]>,
    /// The items taken: `(item, count)`.
    pub removes: Vec<[u32; 2]>,
    /// The points taken: `(type, point)`.
    pub remove_points: Vec<[i32; 2]>,
    /// How many of the `pays` entries are paid out.
    pub pay_num: i32,
    /// The possible results.
    pub pays: Vec<ExchangePay>,
}

/// One exchange menu (`MMI_*`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ExchangeMenu {
    /// The `MMI_*` id.
    pub menu: u32,
    /// The description text ids.
    pub desc_tids: Vec<u32>,
    /// The sets (at most 15).
    pub sets: Vec<ExchangeSet>,
}

/// The exchange menus in file order.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExchangeTable {
    /// The menus; a repeated menu id replaces the earlier one in the original (`map::insert` keeps the first).
    pub menus: Vec<ExchangeMenu>,
}

#[cfg(feature = "load")]
pub(crate) use parse::parse_exchange;

#[cfg(feature = "load")]
mod parse {
    use super::{ExchangeMenu, ExchangePay, ExchangeSet, ExchangeTable, MAX_EXCHANGE_SETS};
    use flyff_common::defs::item::II_GOLD_SEED1;
    use flyff_script::{
        DiagCode, Diagnostics, Scanner, ScannerOptions, ScriptCtx, Severity, TokenType,
    };
    use std::sync::Arc;

    struct Rd<'a> {
        sc: Scanner<'a>,
        ctx: &'a ScriptCtx,
    }

    impl Rd<'_> {
        fn tok(&mut self) {
            self.sc.get_token(false);
        }
        fn cur(&self) -> &str {
            self.sc.token()
        }
        fn open(&self) -> bool {
            !self.cur().starts_with('}') && !self.sc.finished()
        }
        /// `CScript::GetDefineNum` of the current token (`0xFFFFFFFF` when unknown).
        fn define(&self) -> u32 {
            self.ctx
                .defines
                .get(self.cur())
                .map_or(u32::MAX, |v| v as u32)
        }
        fn num(&mut self) -> i32 {
            self.sc.get_number_i32(false)
        }
        /// The item of a `CONDITION`/`REMOVE` row: `PENYA` is `II_GOLD_SEED1`.
        fn item_or_penya(&self) -> u32 {
            if self.cur() == "PENYA" {
                II_GOLD_SEED1
            } else {
                self.define()
            }
        }
        /// `{ (ITEM COUNT)* }` after the keyword.
        fn item_list(&mut self) -> Vec<[u32; 2]> {
            let mut v = Vec::new();
            self.tok(); // {
            self.tok();
            while self.open() {
                let item = self.item_or_penya();
                let count = self.num();
                v.push([item, count as u32]);
                self.tok();
            }
            v
        }
        /// `{ (TYPE POINT)* }` after the keyword; a non-positive type is an Error and dropped.
        fn point_list(&mut self, diags: &mut Vec<String>) -> Vec<[i32; 2]> {
            let mut v = Vec::new();
            self.tok(); // {
            self.tok();
            while self.open() {
                let kind = self.define() as i32;
                let point = self.num();
                if kind > 0 {
                    v.push([kind, point]);
                } else {
                    diags.push("invalid point type".to_owned());
                }
                self.tok();
            }
            v
        }
    }

    /// `CExchange::Load_Script` over decoded `text`.
    pub(crate) fn parse_exchange(
        text: &str,
        file: &Arc<str>,
        sctx: &ScriptCtx,
        diags: &mut Diagnostics,
    ) -> ExchangeTable {
        let mut out = ExchangeTable::default();
        let mut problems: Vec<String> = Vec::new();
        let mut rd = Rd {
            sc: Scanner::new(text, Arc::clone(file), ScannerOptions::default()),
            ctx: sctx,
        };
        rd.tok();
        while !rd.sc.finished() {
            let menu_name = rd.cur().to_owned();
            let mut menu = ExchangeMenu {
                menu: rd.define(),
                ..ExchangeMenu::default()
            };
            rd.tok(); // {
            rd.tok();
            while rd.open() {
                match rd.cur().to_owned().as_str() {
                    "DESCRIPTION" => {
                        rd.tok(); // {
                        rd.tok();
                        while rd.open() {
                            menu.desc_tids.push(rd.define());
                            rd.tok();
                        }
                    }
                    "SET" => {
                        let mut set = ExchangeSet::default();
                        rd.tok();
                        set.tid = rd.define();
                        rd.tok(); // {
                        rd.tok();
                        while rd.open() {
                            match rd.cur().to_owned().as_str() {
                                "RESULTMSG" => {
                                    rd.tok(); // {
                                    rd.tok();
                                    while rd.open() {
                                        set.result_msgs.push(rd.define());
                                        rd.tok();
                                    }
                                }
                                "CONDITION" => set.conditions = rd.item_list(),
                                "REMOVE" => set.removes = rd.item_list(),
                                "CONDITION_POINT" => {
                                    set.condition_points = rd.point_list(&mut problems)
                                }
                                "REMOVE_POINT" => set.remove_points = rd.point_list(&mut problems),
                                "PAY" => {
                                    set.pay_num = rd.num();
                                    rd.tok(); // {
                                    rd.tok();
                                    let mut sum = 0i64;
                                    let mut room = true;
                                    while rd.open() {
                                        let mut pay = ExchangePay {
                                            item: rd.define(),
                                            ..ExchangePay::default()
                                        };
                                        pay.count = rd.num();
                                        pay.prob = rd.num();
                                        let m = rd.sc.mark();
                                        rd.tok();
                                        let numeric = rd.sc.token_type() == TokenType::Number;
                                        rd.sc.reset(m);
                                        if numeric {
                                            pay.flag = rd.num() as u8;
                                        }
                                        sum += i64::from(pay.prob);
                                        if sum > 1_000_000 {
                                            if room {
                                                pay.prob -= (sum - 1_000_000) as i32;
                                                set.pays.push(pay);
                                                room = false;
                                                problems.push(format!(
                                                    "{menu_name}: PAY probabilities exceed 1000000"
                                                ));
                                            }
                                        } else {
                                            set.pays.push(pay);
                                            if sum == 1_000_000 {
                                                room = false;
                                            }
                                        }
                                        rd.tok();
                                    }
                                    if sum < 1_000_000 {
                                        if let Some(last) = set.pays.last_mut() {
                                            last.prob += (1_000_000 - sum) as i32;
                                        }
                                        problems.push(format!("{menu_name}: PAY probabilities sum to {sum}, not 1000000"));
                                    }
                                }
                                _ => {}
                            }
                            rd.tok();
                        }
                        if menu.sets.len() < MAX_EXCHANGE_SETS {
                            menu.sets.push(set);
                        } else {
                            problems
                                .push(format!("{menu_name}: more than {MAX_EXCHANGE_SETS} sets"));
                        }
                    }
                    _ => {}
                }
                rd.tok();
            }
            if !out.menus.iter().any(|m| m.menu == menu.menu) {
                out.menus.push(menu);
            } else {
                problems.push(format!("{menu_name}: duplicate menu"));
            }
            rd.tok();
        }
        for p in problems {
            diags.report(
                Severity::Warning,
                DiagCode::ValueOutOfRange,
                Some(file),
                0,
                p,
            );
        }
        diags.extend(rd.sc.take_diagnostics());
        out
    }
}
