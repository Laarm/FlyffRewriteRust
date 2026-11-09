//! The ScriptLib helper table (Plan/20 §6.1, §6.3-§6.8): the [`Helper`] enum, the [`HelperSig`] of each of the
//! 100 names, and the legality rules the `d:Name(...)` calls of an NPC script are checked against.
//!
//! The union of 100 names is 82 helpers that exist both in the DLL class `CNpcScript` and in the `Functions`
//! table of `FunctionsInfo.h`, 9 DLL-only aliases and no-ops, and 9 table-only stubs (`dll == false`) that the
//! transpiled scripts never call but hand-written scripts may (D20-06). The spelling of every name is the C++
//! spelling (`GetGValue_` included).
//!
//! The same table, as the transpiler resolves calls against it, is dumped to
//! `tools/npcscript-transpile/tests/golden/helpers.txt`; `tests/helper_table.rs` compares the two.

/// What an argument means to the host (Plan/20 §6.1).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ArgKind {
    /// An integer (ids, counts, define constants): a Lua integer or an integral float.
    Int,
    /// A float (`Replace` coordinates): any Lua number.
    Float,
    /// A `WorldDialog.txt` line id: a non-negative integer. The host resolves the text.
    TextId,
}

/// Whether the rewrite implements the helper fully, as a constant-returning stub, or as a no-op.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum HelperStatus {
    /// Implemented with the semantics of Plan/20 §6.
    Impl,
    /// Returns a constant and logs once (`MonHuntStart*`, `IsPlayerSkillPoint`, the table-only names).
    Stub,
    /// Does nothing (the original did nothing, too).
    NoOp,
}

/// What a helper needs from the dialog context.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum CtxReq {
    /// Works with or without a player (`NpcId`, `Speak`, `Random`, `Trace`, ...).
    None,
    /// Needs the player: in the `#auto` context (no player) the host returns 0 and does nothing.
    Player,
}

/// One helper: `d:<name>(...)` in the emitted Lua.
#[derive(Debug, PartialEq, Eq)]
pub struct HelperSig {
    /// The enum value (the index into [`HELPERS`]).
    pub helper: Helper,
    /// The Lua method name (identical to the C++ member).
    pub name: &'static str,
    /// The argument kinds of the longest legal call (variadic helpers: of the fixed part).
    pub params: &'static [ArgKind],
    /// The legal argument counts (the C++ overloads); the first is the minimum.
    pub arities: &'static [u8],
    /// `true` when any count of at least `arities[0]` is legal (`SayName`).
    pub variadic: bool,
    /// `true` when the call yields a value usable in an expression.
    pub returns_value: bool,
    /// Implemented, stub or no-op.
    pub status: HelperStatus,
    /// Player required?
    pub ctx: CtxReq,
    /// `true` for the 91 methods of `CNpcScript`; `false` for the 9 table-only names.
    pub dll: bool,
}

impl HelperSig {
    /// The smallest legal argument count.
    pub fn min_args(&self) -> u8 {
        self.arities.first().copied().unwrap_or(0)
    }

    /// `true` when a call with `n` arguments is legal.
    pub fn accepts(&self, n: usize) -> bool {
        let Ok(n) = u8::try_from(n) else {
            return false;
        };
        if self.variadic {
            n >= self.min_args()
        } else {
            self.arities.contains(&n)
        }
    }

    /// The kind of argument `i` (`Int` past the fixed part of a variadic helper).
    pub fn kind_of(&self, i: usize) -> ArgKind {
        self.params.get(i).copied().unwrap_or(ArgKind::Int)
    }

    /// The legal counts as text for error messages (`"1, 2 or 3"`, `"at least 3"`).
    pub fn arities_text(&self) -> String {
        if self.variadic {
            return format!("at least {}", self.min_args());
        }
        let list: Vec<String> = self.arities.iter().map(u8::to_string).collect();
        match list.as_slice() {
            [] => "none".to_owned(),
            [one] => one.clone(),
            [head @ .., last] => format!("{} or {last}", head.join(", ")),
        }
    }
}

macro_rules! helper_table {
    ($(
        $id:ident [$($k:ident),*] $arities:expr, $variadic:expr, $ret:expr, $status:ident, $ctx:ident, $dll:expr;
    )+) => {
        /// A ScriptLib helper (Plan/20 §6.1). The discriminant is the index into [`HELPERS`].
        #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        #[repr(u8)]
        #[allow(non_camel_case_types)]
        pub enum Helper {
            $(
                #[doc = concat!("`d:", stringify!($id), "(...)`")]
                $id,
            )+
        }

        impl Helper {
            /// All helpers in table order.
            pub const ALL: [Helper; HELPER_COUNT] = [$(Helper::$id),+];
        }

        /// Number of helpers (Plan/20 D20-06).
        pub const HELPER_COUNT: usize = [$(stringify!($id)),+].len();

        /// The 100 helpers in the order of Plan/20 §6.3-§6.8.
        pub static HELPERS: [HelperSig; HELPER_COUNT] = [
            $(
                HelperSig {
                    helper: Helper::$id,
                    name: stringify!($id),
                    params: &[$(ArgKind::$k),*],
                    arities: $arities,
                    variadic: $variadic,
                    returns_value: $ret,
                    status: HelperStatus::$status,
                    ctx: CtxReq::$ctx,
                    dll: $dll,
                },
            )+
        ];
    };
}

// name            params          arities   variadic value status ctx    dll
helper_table! {
    // ---- 6.3 context and environment
    GetVal [] &[0], false, true, Impl, None, true;
    GetQuestId [] &[0], false, true, Impl, None, true;
    GetSrcId [] &[0], false, true, Impl, None, true;
    PcId [] &[0], false, true, Impl, None, true;
    Src [] &[0], false, true, Impl, None, true;
    PC [] &[0], false, true, Impl, None, true;
    GetDstId [] &[0], false, true, Impl, None, true;
    NpcId [] &[0], false, true, Impl, None, true;
    Dst [] &[0], false, true, Impl, None, true;
    NPC [] &[0], false, true, Impl, None, true;
    GetLang [] &[0], false, true, Impl, None, true;
    GetParam1 [] &[0], false, true, NoOp, None, true;
    GetParam2 [] &[0], false, true, NoOp, None, true;
    GetParam3 [] &[0], false, true, NoOp, None, true;
    GetParam4 [] &[0], false, true, NoOp, None, true;
    Random [Int] &[1], false, true, Impl, None, true;
    GetDay [] &[0], false, true, Impl, None, true;
    GetHour [] &[0], false, true, Impl, None, true;
    GetMin [] &[0], false, true, Impl, None, true;
    GetLocalEventState [Int] &[1], false, true, Impl, None, true;
    IsWormonServer [] &[0], false, true, Impl, None, true;
    Trace [TextId] &[1], false, false, Impl, None, true;
    Run [TextId, Int] &[2], false, false, Impl, None, true;
    // ---- 6.4 dialog output
    Say [TextId] &[1], false, false, Impl, Player, true;
    AddSay [TextId] &[1], false, false, Impl, Player, true;
    Speak [Int, TextId] &[2], false, false, Impl, None, true;
    AddKey [TextId, TextId, Int] &[1, 2, 3], false, false, Impl, Player, true;
    AddCondKey [TextId, TextId] &[2], false, true, Impl, Player, true;
    AddAnswer [TextId, TextId, Int] &[1, 3], false, false, Impl, Player, true;
    AddQuestKey [Int, TextId] &[2], false, false, Impl, Player, true;
    RemoveKey [TextId] &[1], false, false, Impl, Player, true;
    RemoveAllKey [] &[0], false, false, Impl, Player, true;
    SayQuest [Int, Int] &[2], false, false, Impl, Player, true;
    Exit [] &[0], false, false, Impl, Player, true;
    PrintSystemMessage [Int] &[1], false, false, Impl, Player, true;
    PlaySound [TextId] &[1], false, false, Impl, Player, true;
    QuerySetPlayerName [] &[0], false, true, Impl, Player, true;
    // ---- 6.5 quest helpers
    LaunchQuest [] &[0], false, false, Impl, Player, true;
    GetQuestState [Int] &[1], false, true, Impl, Player, true;
    IsSetQuest [Int] &[1], false, true, Impl, Player, true;
    SetQuest [Int] &[1], false, false, Impl, Player, true;
    SetQuestState [Int, Int] &[2], false, false, Impl, Player, true;
    BeginQuest [Int] &[1], false, false, Impl, Player, true;
    EndQuest [Int] &[1], false, false, Impl, Player, true;
    RemoveQuest [Int] &[1], false, false, Impl, Player, true;
    IsGuildQuest [Int] &[1], false, true, Impl, Player, true;
    GetGuildQuestState [Int] &[1], false, true, Impl, Player, true;
    MonHuntStart [Int, Int, Int, Int] &[4], false, true, Stub, Player, true;
    MonHuntStartParty [Int, Int, Int, Int] &[4], false, true, Stub, Player, true;
    DropQuestItem [Int, Int] &[2], false, true, Impl, Player, true;
    ExpQuestEffect [Int, Int] &[2], false, false, Impl, Player, true;
    // ---- 6.6 player queries and mutations
    GetPlayerLvl [] &[0], false, true, Impl, Player, true;
    GetPlayerJob [] &[0], false, true, Impl, Player, true;
    GetPlayerSex [] &[0], false, true, Impl, Player, true;
    GetPlayerGold [] &[0], false, true, Impl, Player, true;
    GetPlayerExpPercent [] &[0], false, true, Impl, Player, true;
    GetEmptyInventoryNum [] &[0], false, true, Impl, Player, true;
    GetItemNum [Int] &[1], false, true, Impl, Player, true;
    CreateItem [Int, Int] &[2], false, false, Impl, Player, true;
    RemoveItem [Int, Int] &[2], false, true, Impl, Player, true;
    RemoveAllItem [Int] &[1], false, false, Impl, Player, true;
    EquipItem [Int] &[1], false, false, Impl, Player, true;
    AddGold [Int] &[1], false, false, Impl, Player, true;
    RemoveGold [Int] &[1], false, false, Impl, Player, true;
    AddExp [Int] &[1], false, false, Impl, Player, true;
    InitStat [] &[0], false, false, Impl, Player, true;
    InitStr [] &[0], false, false, Impl, Player, true;
    InitSta [] &[0], false, false, Impl, Player, true;
    InitDex [] &[0], false, false, Impl, Player, true;
    InitInt [] &[0], false, false, Impl, Player, true;
    ChangeJob [Int] &[1], false, false, Impl, Player, true;
    SetLevel [Int] &[1], false, true, Impl, Player, true;
    AddGPPoint [Int] &[1], false, true, Impl, Player, true;
    Replace [Int, Float, Float, Float] &[4], false, false, Impl, Player, true;
    ReplaceKey [Int, TextId] &[2], false, false, Impl, Player, true;
    // ---- 6.7 party and guild
    IsParty [] &[0], false, true, Impl, Player, true;
    IsPartyMaster [] &[0], false, true, Impl, Player, true;
    GetPartyNum [] &[0], false, true, Impl, Player, true;
    GetPartyLevel [] &[0], false, true, Impl, Player, true;
    IsPartyGuild [] &[0], false, true, Impl, Player, true;
    IsGuild [] &[0], false, true, Impl, Player, true;
    IsGuildMaster [] &[0], false, true, Impl, Player, true;
    CreateGuild [] &[0], false, true, Impl, Player, true;
    DestroyGuild [] &[0], false, true, Impl, Player, true;
    // ---- 6.8 no-ops and legacy stubs
    SetScriptTimer [Int] &[1], false, false, NoOp, None, true;
    SayName [Int, Int, TextId] &[3], true, false, NoOp, None, true;
    SpeakName [Int, Int, TextId] &[3], false, false, NoOp, None, true;
    EndSay [] &[0], false, false, NoOp, None, true;
    SetMark [] &[0], false, false, NoOp, None, true;
    GoMark [] &[0], false, false, NoOp, None, true;
    IsPlayerSkillPoint [] &[0], false, true, Stub, None, true;
    GetGValue_ [] &[0], false, true, Stub, None, false;
    SetGValue [] &[0], false, true, Stub, None, false;
    AddGValue [] &[0], false, true, Stub, None, false;
    CreateMover [] &[0], false, true, Stub, None, false;
    CreateChar [] &[0], false, true, Stub, None, false;
    RemoveMover [] &[0], false, true, Stub, None, false;
    SetTimer [] &[0], false, true, Stub, None, false;
    IsTimeOut [] &[0], false, true, Stub, None, false;
    RandomIndex [] &[0], false, true, Stub, None, false;
}

impl Helper {
    /// The table row of this helper.
    pub fn sig(self) -> &'static HelperSig {
        &HELPERS[self as usize]
    }

    /// The Lua method name.
    pub fn name(self) -> &'static str {
        self.sig().name
    }

    /// Finds a helper by its exact (case-sensitive) name.
    pub fn by_name(name: &str) -> Option<Helper> {
        HELPERS.iter().find(|h| h.name == name).map(|h| h.helper)
    }

    /// The helper at table index `i`.
    pub fn from_index(i: usize) -> Option<Helper> {
        Helper::ALL.get(i).copied()
    }
}

/// One line per helper in the format of `tools/npcscript-transpile/tests/golden/helpers.txt`.
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
