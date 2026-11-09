//! The ScriptLib helper table the transpiler resolves calls against (Plan/20 §5.3 step 8, §6.1).
//!
//! The 100 names of §6.3-6.8 with their legal argument counts, the kind of each argument and whether the
//! helper yields a value. 91 of them are methods of the original `CNpcScript` class (`dll == true`, the
//! only ones the C++ scripts can call); the other 9 exist only in the `Functions` table of
//! `FunctionsInfo.h` and are bound by the run time for hand-written scripts (`dll == false`).
//!
//! This is the transpiler's own copy of the table: `flyff_lua::HELPERS` (WP-NPC-05) is the run-time side. The
//! names, spellings and argument counts must agree; `tests/helper_table.rs` compares this table with the
//! original headers (source-gated) and the emitted Lua is checked against it (public CI).

/// What an argument means to the host (Plan/20 §6.1).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ArgKind {
    /// An integer (ids, counts, define constants).
    Int,
    /// A float (`Replace` coordinates).
    Float,
    /// A `WorldDialog.txt` line id; the host resolves the text.
    TextId,
}

/// One helper: `d:<name>(...)` in the emitted Lua.
#[derive(Debug, PartialEq, Eq)]
pub struct HelperSig {
    /// The Lua method name (identical to the C++ member).
    pub name: &'static str,
    /// The argument kinds, in order, of the longest legal call (variadic helpers: of the fixed part).
    pub params: &'static [ArgKind],
    /// The legal argument counts (the C++ overloads).
    pub arities: &'static [u8],
    /// `true` when any count of at least `arities[0]` is legal.
    pub variadic: bool,
    /// `true` when the call yields a value usable in an expression.
    pub returns_value: bool,
    /// `true` for the 91 methods of `CNpcScript`; `false` for the 9 table-only names.
    pub dll: bool,
}

impl HelperSig {
    /// `true` when a call with `n` arguments is legal.
    pub fn accepts(&self, n: usize) -> bool {
        let Ok(n) = u8::try_from(n) else {
            return false;
        };
        if self.variadic {
            self.arities.first().is_some_and(|min| n >= *min)
        } else {
            self.arities.contains(&n)
        }
    }

    /// The kind of argument `i` (`Int` past the fixed part of a variadic helper).
    pub fn kind_of(&self, i: usize) -> ArgKind {
        self.params.get(i).copied().unwrap_or(ArgKind::Int)
    }

    /// The indices of the `TextId` arguments of a call with `n` arguments.
    pub fn text_args(&self, n: usize) -> impl Iterator<Item = usize> + '_ {
        (0..n).filter(|i| self.kind_of(*i) == ArgKind::TextId)
    }
}

use ArgKind::{Float as F, Int as I, TextId as T};

const fn h(
    name: &'static str,
    params: &'static [ArgKind],
    arities: &'static [u8],
    returns_value: bool,
) -> HelperSig {
    HelperSig {
        name,
        params,
        arities,
        variadic: false,
        returns_value,
        dll: true,
    }
}

const fn table_only(name: &'static str) -> HelperSig {
    HelperSig {
        name,
        params: &[],
        arities: &[0],
        variadic: false,
        returns_value: true,
        dll: false,
    }
}

/// The helper table in the order of Plan/20 §6.3-6.8.
pub static HELPERS: [HelperSig; 100] = [
    // 6.3 context and environment
    h("GetVal", &[], &[0], true),
    h("GetQuestId", &[], &[0], true),
    h("GetSrcId", &[], &[0], true),
    h("PcId", &[], &[0], true),
    h("Src", &[], &[0], true),
    h("PC", &[], &[0], true),
    h("GetDstId", &[], &[0], true),
    h("NpcId", &[], &[0], true),
    h("Dst", &[], &[0], true),
    h("NPC", &[], &[0], true),
    h("GetLang", &[], &[0], true),
    h("GetParam1", &[], &[0], true),
    h("GetParam2", &[], &[0], true),
    h("GetParam3", &[], &[0], true),
    h("GetParam4", &[], &[0], true),
    h("Random", &[I], &[1], true),
    h("GetDay", &[], &[0], true),
    h("GetHour", &[], &[0], true),
    h("GetMin", &[], &[0], true),
    h("GetLocalEventState", &[I], &[1], true),
    h("IsWormonServer", &[], &[0], true),
    h("Trace", &[T], &[1], false),
    h("Run", &[T, I], &[2], false),
    // 6.4 dialog output
    h("Say", &[T], &[1], false),
    h("AddSay", &[T], &[1], false),
    h("Speak", &[I, T], &[2], false),
    h("AddKey", &[T, T, I], &[1, 2, 3], false),
    h("AddCondKey", &[T, T], &[2], true),
    h("AddAnswer", &[T, T, I], &[1, 3], false),
    h("AddQuestKey", &[I, T], &[2], false),
    h("RemoveKey", &[T], &[1], false),
    h("RemoveAllKey", &[], &[0], false),
    h("SayQuest", &[I, I], &[2], false),
    h("Exit", &[], &[0], false),
    h("PrintSystemMessage", &[I], &[1], false),
    h("PlaySound", &[T], &[1], false),
    h("QuerySetPlayerName", &[], &[0], true),
    // 6.5 quest helpers
    h("LaunchQuest", &[], &[0], false),
    h("GetQuestState", &[I], &[1], true),
    h("IsSetQuest", &[I], &[1], true),
    h("SetQuest", &[I], &[1], false),
    h("SetQuestState", &[I, I], &[2], false),
    h("BeginQuest", &[I], &[1], false),
    h("EndQuest", &[I], &[1], false),
    h("RemoveQuest", &[I], &[1], false),
    h("IsGuildQuest", &[I], &[1], true),
    h("GetGuildQuestState", &[I], &[1], true),
    h("MonHuntStart", &[I, I, I, I], &[4], true),
    h("MonHuntStartParty", &[I, I, I, I], &[4], true),
    h("DropQuestItem", &[I, I], &[2], true),
    h("ExpQuestEffect", &[I, I], &[2], false),
    // 6.6 player queries and mutations
    h("GetPlayerLvl", &[], &[0], true),
    h("GetPlayerJob", &[], &[0], true),
    h("GetPlayerSex", &[], &[0], true),
    h("GetPlayerGold", &[], &[0], true),
    h("GetPlayerExpPercent", &[], &[0], true),
    h("GetEmptyInventoryNum", &[], &[0], true),
    h("GetItemNum", &[I], &[1], true),
    h("CreateItem", &[I, I], &[2], false),
    h("RemoveItem", &[I, I], &[2], true),
    h("RemoveAllItem", &[I], &[1], false),
    h("EquipItem", &[I], &[1], false),
    h("AddGold", &[I], &[1], false),
    h("RemoveGold", &[I], &[1], false),
    h("AddExp", &[I], &[1], false),
    h("InitStat", &[], &[0], false),
    h("InitStr", &[], &[0], false),
    h("InitSta", &[], &[0], false),
    h("InitDex", &[], &[0], false),
    h("InitInt", &[], &[0], false),
    h("ChangeJob", &[I], &[1], false),
    h("SetLevel", &[I], &[1], true),
    h("AddGPPoint", &[I], &[1], true),
    h("Replace", &[I, F, F, F], &[4], false),
    h("ReplaceKey", &[I, T], &[2], false),
    // 6.7 party and guild
    h("IsParty", &[], &[0], true),
    h("IsPartyMaster", &[], &[0], true),
    h("GetPartyNum", &[], &[0], true),
    h("GetPartyLevel", &[], &[0], true),
    h("IsPartyGuild", &[], &[0], true),
    h("IsGuild", &[], &[0], true),
    h("IsGuildMaster", &[], &[0], true),
    h("CreateGuild", &[], &[0], true),
    h("DestroyGuild", &[], &[0], true),
    // 6.8 no-ops and legacy stubs
    h("SetScriptTimer", &[I], &[1], false),
    HelperSig {
        name: "SayName",
        params: &[I, I, T],
        arities: &[3],
        variadic: true,
        returns_value: false,
        dll: true,
    },
    h("SpeakName", &[I, I, T], &[3], false),
    h("EndSay", &[], &[0], false),
    h("SetMark", &[], &[0], false),
    h("GoMark", &[], &[0], false),
    h("IsPlayerSkillPoint", &[], &[0], true),
    table_only("GetGValue_"),
    table_only("SetGValue"),
    table_only("AddGValue"),
    table_only("CreateMover"),
    table_only("CreateChar"),
    table_only("RemoveMover"),
    table_only("SetTimer"),
    table_only("IsTimeOut"),
    table_only("RandomIndex"),
];

/// One line per helper: `index name(kinds) arities returns dll`, the machine-readable form of the table
/// (`tests/golden/helpers.txt`, the reference for the run-time binding of WP-NPC-05).
pub fn dump_table() -> String {
    let mut out = String::from("# idx name(params) arities returns_value dll\n");
    for (i, h) in HELPERS.iter().enumerate() {
        let kinds: Vec<&str> = h
            .params
            .iter()
            .map(|k| match k {
                ArgKind::Int => "Int",
                ArgKind::Float => "Float",
                ArgKind::TextId => "TextId",
            })
            .collect();
        let arities: Vec<String> = h.arities.iter().map(u8::to_string).collect();
        out.push_str(&format!(
            "{:03} {}({}) {}{} {} {}\n",
            i + 1,
            h.name,
            kinds.join(", "),
            arities.join("/"),
            if h.variadic { "+" } else { "" },
            if h.returns_value { "value" } else { "void" },
            if h.dll { "dll" } else { "table-only" },
        ));
    }
    out
}

/// Finds a helper by its exact (case-sensitive) name.
pub fn lookup(name: &str) -> Option<&'static HelperSig> {
    HELPERS.iter().find(|h| h.name == name)
}

/// The index of a helper in [`HELPERS`].
pub fn index_of(name: &str) -> Option<usize> {
    HELPERS.iter().position(|h| h.name == name)
}
