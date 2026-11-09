//! The `AI { #SCAN {} #BATTLE {} #MOVE {} }` block of `propMoverEx.inc` (`CProject::LoadPropMoverEx_AI`,
//! `_SCAN`, `_BATTLE`, `_MOVE`; src:_Common/ProjectLux.cpp:25-466; Plan/14 §6.5.2, ref/12 §9.2).
//!
//! The grammar is a small command language: an identifier selects a command (`Scan`, `Attack`, `Recovery`,
//! `RangeAttack`, `KeepRangeAttack`, `Summon`, `Evade`, `Helper`, `Berserk`, `Loot`), numbers after it are its
//! arguments, and a few option words (`low`/`Sam`/`Hi`, `u`/`m`/`a`, `all`/`sam`, `d`) refine the last command.
//! It is ported statement for statement. **A syntax error** makes the original return FALSE, which aborts
//! the whole load of `propMoverEx.inc`; here it is an Error `SyntaxError`, the rest of the `AI {}` block is skipped and
//! the values read so far stay (the block is still `present`). The shipped file only uses `Scan range N chao N`
//! (3 movers) and `Attack Berserk 50 1.5`.
//!
//! The values land in [`MoverAi`] and in the `attack_first_range` / `runaway_hp` fields of [`MoverEx`]
//! (`Scan range` and `Evade` write into the mover itself in the original).

use super::{MoverAi, MoverEx};
use crate::props::mover::MoverProp;
use crate::types::IdTable;
use flyff_script::{DiagCode, Script, Severity, TokenType, c_atoi};

/// `MAX_JOB`: the length of the per-job range-attack table.
const MAX_JOB: usize = 32;
/// `MAX_SUMMON`: the most monsters one summon call may create.
const MAX_SUMMON: i32 = 8;

/// The AI command that the following numbers belong to (`AICMD`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Cmd {
    None,
    Scan,
    Attack,
    Recovery,
    RangeAttack,
    KeepRangeAttack,
    Summon,
    Evade,
    Helper,
    Berserk,
    Loot,
}

/// A reader that counts the braces it passes, so that a syntax error can skip to the end of the `AI {}`.
struct Reader<'a, 'b> {
    s: &'a mut Script<'b>,
    depth: i32,
}

impl Reader<'_, '_> {
    fn next(&mut self) {
        self.s.get_token();
        if self.s.token().starts_with('{') {
            self.depth += 1;
        } else if self.s.token().starts_with('}') {
            self.depth -= 1;
        }
    }

    fn tok(&self) -> &str {
        self.s.token()
    }

    fn ty(&self) -> TokenType {
        self.s.token_type()
    }

    fn eq(&self, word: &str) -> bool {
        self.s.token().eq_ignore_ascii_case(word)
    }

    fn atoi(&self) -> i32 {
        c_atoi(self.s.token().as_bytes())
    }

    fn syntax(&mut self, mover: u32, what: &str) -> Result<(), ()> {
        let t = self.s.token().to_owned();
        self.s.report(
            Severity::Error,
            DiagCode::SyntaxError,
            format!("propMoverEx.inc: MoverID={mover} AI: {what} (at {t:?})"),
        );
        Err(())
    }

    fn error(&mut self, code: DiagCode, msg: String) {
        self.s.report(Severity::Error, code, msg);
    }
}

/// Reads the `AI {` ... `}` block whose `AI` token was just read, into `ex` (and `ex.attack_first_range` /
/// `ex.runaway_hp`). `movers` is only used to check that a summoned mover exists. Returns after the block's
/// closing brace (also after an error, which is reported).
pub(super) fn parse_ai(
    s: &mut Script<'_>,
    mover: u32,
    ex: &mut MoverEx,
    movers: &IdTable<MoverProp>,
) {
    let mut r = Reader { s, depth: 0 };
    if run_ai(&mut r, mover, ex, movers).is_err() {
        // skip to the closing brace of the AI block (depth 0), or the end of the file
        while r.depth > 0 && !r.s.finished() {
            r.next();
        }
    }
}

fn run_ai(
    r: &mut Reader<'_, '_>,
    mover: u32,
    ex: &mut MoverEx,
    movers: &IdTable<MoverProp>,
) -> Result<(), ()> {
    r.next(); // {
    if !r.tok().starts_with('{') {
        return r.syntax(mover, "AI is not followed by {");
    }
    loop {
        r.next();
        if r.tok().starts_with('}') || r.s.finished() {
            return Ok(());
        }
        if !r.tok().starts_with('#') {
            return r.syntax(mover, "a #SCAN, #BATTLE or #MOVE section is expected");
        }
        if r.eq("#SCAN") {
            scan(r, mover, ex)?;
        } else if r.eq("#BATTLE") {
            battle(r, mover, &mut ex.ai, &mut ex.runaway_hp, movers)?;
        } else if r.eq("#MOVE") {
            mv(r, mover, &mut ex.ai)?;
        } else {
            return r.syntax(mover, "unknown section name");
        }
    }
}

fn open_section(r: &mut Reader<'_, '_>, mover: u32, name: &str) -> Result<(), ()> {
    r.next(); // {
    if r.tok().starts_with('{') {
        Ok(())
    } else {
        r.syntax(mover, &format!("{name} is not followed by {{"))
    }
}

/// `#SCAN { Scan job N | range N | quest N | item N | chao N }`.
fn scan(r: &mut Reader<'_, '_>, mover: u32, ex: &mut MoverEx) -> Result<(), ()> {
    open_section(r, mover, "#SCAN")?;
    let mut cmd = Cmd::None;
    loop {
        r.next();
        if r.tok().starts_with('}') || r.s.finished() {
            return Ok(());
        }
        if r.ty() != TokenType::Identifier {
            continue;
        }
        if cmd == Cmd::Scan {
            if r.eq("job") {
                ex.ai.scan_job = r.s.get_number_i32();
            } else if r.eq("range") {
                ex.attack_first_range = r.s.get_number_i32();
            } else if r.eq("quest") {
                ex.ai.scan_quest_id = r.s.get_number();
            } else if r.eq("item") {
                ex.ai.scan_item_idx = r.s.get_number();
            } else if r.eq("chao") {
                ex.ai.scan_chao = r.s.get_number_i32();
            } else {
                return r.syntax(mover, "unknown Scan option");
            }
        }
        if r.eq("scan") {
            if cmd != Cmd::None {
                return r.syntax(mover, "Scan after another command");
            }
            cmd = Cmd::Scan;
        }
    }
}

/// `#BATTLE { Attack [cunning low|Sam|Hi] [N] | Recovery N N N [u|m|a] | RangeAttack N | KeepRangeAttack N |
/// Summon PROB NUM ID | Evade N | Helper [all|sam] N N | Berserk HP MUL | Randomtarget }`.
fn battle(
    r: &mut Reader<'_, '_>,
    mover: u32,
    ai: &mut MoverAi,
    runaway_hp: &mut i16,
    movers: &IdTable<MoverProp>,
) -> Result<(), ()> {
    open_section(r, mover, "#BATTLE")?;
    let mut cmd = Cmd::None;
    loop {
        r.next();
        if r.tok().starts_with('}') || r.s.finished() {
            return Ok(());
        }
        match r.ty() {
            TokenType::Identifier => {
                if r.eq("Attack") {
                    cmd = Cmd::Attack;
                    ai.melee_attack = 1;
                } else if r.eq("cunning") {
                    if cmd == Cmd::None {
                        return r.syntax(mover, "cunning without a command");
                    }
                    if cmd == Cmd::Attack {
                        r.next();
                        ai.lv_cond = if r.eq("low") {
                            1
                        } else if r.eq("Sam") {
                            2
                        } else if r.eq("Hi") {
                            3
                        } else {
                            return r.syntax(mover, "cunning is followed by low, Sam or Hi");
                        };
                    }
                } else if r.eq("Recovery") {
                    cmd = Cmd::Recovery;
                    ai.recv_cond = 1;
                    ai.recv_cond_me = 0;
                    ai.recv_cond_how = 100;
                    ai.recv_cond_mp = 0;
                } else if r.eq("u") || r.eq("m") || r.eq("a") {
                    if cmd == Cmd::None {
                        return r.syntax(mover, "u/m/a without a command");
                    }
                    if cmd == Cmd::Recovery {
                        ai.recv_cond_who = if r.eq("u") {
                            1
                        } else if r.eq("m") {
                            2
                        } else {
                            3
                        };
                        ai.recv_cond = 2;
                    }
                } else if r.eq("RangeAttack") {
                    cmd = Cmd::RangeAttack;
                } else if r.eq("KeepRangeAttack") {
                    cmd = Cmd::KeepRangeAttack;
                } else if r.eq("Summon") {
                    cmd = Cmd::Summon;
                } else if r.eq("Evade") {
                    cmd = Cmd::Evade;
                } else if r.eq("Helper") {
                    cmd = Cmd::Helper;
                    ai.unit_help_ms = 0;
                    ai.help_range_mul = 2;
                    ai.help_who = 1;
                } else if r.eq("all") || r.eq("sam") {
                    if cmd == Cmd::None {
                        return r.syntax(mover, "all/sam without a command");
                    }
                    if cmd == Cmd::Helper {
                        ai.help_who = if r.eq("all") { 1 } else { 2 };
                    }
                } else if r.eq("Berserk") {
                    cmd = Cmd::Berserk;
                } else if r.eq("Randomtarget") {
                    // accepted, no effect
                } else {
                    return r.syntax(mover, "unknown #BATTLE word");
                }
            }
            TokenType::Number => battle_number(r, mover, cmd, ai, runaway_hp, movers)?,
            _ => {}
        }
    }
}

fn battle_number(
    r: &mut Reader<'_, '_>,
    mover: u32,
    cmd: Cmd,
    ai: &mut MoverAi,
    runaway_hp: &mut i16,
    movers: &IdTable<MoverProp>,
) -> Result<(), ()> {
    let n = r.atoi();
    match cmd {
        Cmd::None => return r.syntax(mover, "a number without a command"),
        Cmd::Attack => ai.hp_cond = n,
        Cmd::Recovery => {
            if ai.recv_cond_me == 0 {
                ai.recv_cond_me = n;
            } else if ai.recv_cond_how == 100 {
                ai.recv_cond_how = n;
            } else if ai.recv_cond_mp == 0 {
                ai.recv_cond_mp = n;
            }
        }
        Cmd::RangeAttack | Cmd::KeepRangeAttack => {
            // `nJob` is always MAX_JOB: the range goes to every job; the keep-distance bit is 0x80
            let mut range = n;
            if cmd == Cmd::KeepRangeAttack {
                range |= 0x80;
            }
            ai.range_attack = [range as u8; MAX_JOB];
        }
        Cmd::Summon => {
            ai.summ_prob = n;
            ai.summ_num = r.s.get_number_i32();
            if ai.summ_num > MAX_SUMMON {
                r.error(
                    DiagCode::ValueOutOfRange,
                    format!("propMoverEx.inc: MoverID={mover} Summon of {} monsters (max {MAX_SUMMON}); clamped", ai.summ_num),
                );
                ai.summ_num = MAX_SUMMON; // __NEW_SUMMON_RULE
            }
            r.next();
            if r.ty() != TokenType::Number {
                r.error(
                    DiagCode::SyntaxError,
                    format!(
                        "propMoverEx.inc: MoverID={mover} Summon: the monster id is not a number"
                    ),
                );
            }
            ai.summ_id = r.atoi();
            if u32::try_from(ai.summ_id).map_or(true, |id| !movers.contains(id)) {
                r.error(
                    DiagCode::RefUnresolved,
                    format!(
                        "propMoverEx.inc: MoverID={mover} Summon: mover {} has no property",
                        ai.summ_id
                    ),
                );
            }
        }
        Cmd::Evade => *runaway_hp = n as i16,
        Cmd::Helper => {
            if ai.unit_help_ms == 0 {
                ai.unit_help_ms = n as u32;
            } else if ai.help_range_mul == 2 {
                ai.help_range_mul = n;
            }
        }
        Cmd::Berserk => {
            ai.berserk_hp = n;
            ai.berserk_dmg_mul = r.s.get_float();
            if ai.berserk_dmg_mul <= 0.0 || ai.berserk_dmg_mul >= 20.0 {
                r.error(
                    DiagCode::ValueOutOfRange,
                    format!("propMoverEx.inc: MoverID={mover} Berserk damage multiplier {} is implausible", ai.berserk_dmg_mul),
                );
            }
        }
        Cmd::Scan | Cmd::Loot => {}
    }
    Ok(())
}

/// `#MOVE { Loot [d] PROB }` (the loot behaviour is switched off in this build: `loot` stays 0).
fn mv(r: &mut Reader<'_, '_>, mover: u32, ai: &mut MoverAi) -> Result<(), ()> {
    open_section(r, mover, "#MOVE")?;
    let mut cmd = Cmd::None;
    loop {
        r.next();
        if r.tok().starts_with('}') || r.s.finished() {
            return Ok(());
        }
        match r.ty() {
            TokenType::Identifier => {
                if r.eq("Loot") {
                    cmd = Cmd::Loot;
                    ai.loot = 0;
                } else if r.eq("d") {
                    ai.loot = 0;
                } else {
                    return r.syntax(mover, "unknown #MOVE word");
                }
            }
            TokenType::Number => {
                if cmd == Cmd::None {
                    return r.syntax(mover, "a number without a command");
                }
                if cmd == Cmd::Loot {
                    ai.loot_prob = r.atoi();
                }
            }
            _ => {}
        }
    }
}
