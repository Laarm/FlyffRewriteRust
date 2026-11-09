//! NPC definitions: `character.inc`, `character-etc.inc`, `character-school.inc` (`LoadCharacter`, Plan/14
//! §6.8; src:_Common/Project.cpp:2978-3323; ref/16 §4.2, ref/13 §12.1).
//!
//! The types are always compiled (wasm-safe); the token-driven parser is behind feature `load`. The
//! vendor stock of each NPC (Plan/14 §6.8.2) is computed by [`crate::vendor`] after all three files are
//! loaded.
//!
//! # Grammar (`LoadCharacter`)
//!
//! ```text
//! file  := { KEY '{' body '}' }*      // KEY is read with substitution; braces are only counted
//! body  := statement*                 // unknown tokens ('(', ')', ';', setting, Setting, ...) are skipped
//! ```
//!
//! After `KEY` exactly one token is consumed as "the `{`" whatever it is (so the `NPC_Door` style without
//! `{` works); the body then ends when the brace count (starting at 1) returns to 0.
//!
//! # Quirks kept on purpose
//!
//! * `SetName`, `SetImage`, `m_szChar` and `AddVendorSlot` read their text with `GetLangScript` (one token,
//!   then two skipped tokens, empty becomes one space). `m_szChar = X ;` therefore also swallows the token
//!   after the `;`.
//! * `AddVenderSlot` (the old spelling) reads `( n , value )` with no skipped-token tail and no `" "`
//!   default.
//! * `SetLang ( L )` (no sub-language) swallows one extra token after the `)`.
//! * `SetEquip` resets the list; at most [`MAX_NPC_EQUIP`] items are kept.
//! * `AddShopItem ( slot , item , cost )` **mutates the global item row** (`cost`, or `refer_value1` when the
//!   NPC's vendor type is not 0 at that point of the file), recorded in `ShopItemEntry::price_override`.
//!   The original's `if (cost && cost < 1) cost = 1` is a no-op on a `DWORD` and is not reproduced.
//! * A repeated NPC key replaces the earlier definition (`CMapStringToPtr::SetAt`), in the position of the
//!   first one.

use crate::types::{ItemId, MenuId, MoverId, MusicId, SkillId};
use flyff_common::defs::neuz;
use flyff_common::det::DetMap;
use serde::{Deserialize, Serialize};

/// `MAX_VENDOR_INVENTORY_TAB`: shop tabs per NPC.
pub const MAX_VENDOR_INVENTORY_TAB: usize = 4;
/// `MAX_VENDOR_INVENTORY`: slots per shop tab.
pub const MAX_VENDOR_INVENTORY: usize = 100;
/// `MAX_HUMAN_PARTS`: the longest `SetEquip` list.
pub const MAX_NPC_EQUIP: usize = neuz::MAX_HUMAN_PARTS as usize;
/// `MAX_MOVER_MENU`: menu ids are below this.
pub const MAX_MOVER_MENU: u32 = neuz::MAX_MOVER_MENU;

/// All NPC definitions (`m_mapCharacter`), keyed by the lower-cased key, in file order.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct NpcTable {
    /// Lower-case key to definition, file order; a repeated key keeps the position of the first and the
    /// content of the last definition (Warning `PropIdDuplicate`).
    pub by_key: DetMap<String, NpcDef>,
    /// The character files in load order (`NpcDef::source` indexes this list): normally `character.inc`,
    /// `character-etc.inc`, `character-school.inc`.
    pub sources: Vec<String>,
}

impl NpcTable {
    /// The NPC with `key` (case-insensitive, like `CProject::GetCharacter`).
    pub fn get(&self, key: &str) -> Option<&NpcDef> {
        self.by_key.get(&key.to_ascii_lowercase())
    }

    /// Number of definitions.
    pub fn len(&self) -> usize {
        self.by_key.len()
    }

    /// `true` when no NPC is defined.
    pub fn is_empty(&self) -> bool {
        self.by_key.is_empty()
    }

    /// The NPCs in file order.
    pub fn iter(&self) -> impl Iterator<Item = &NpcDef> + '_ {
        self.by_key.values()
    }
}

/// One NPC (`CHARACTER`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NpcDef {
    /// The key as written (`m_szKey`), e.g. `MaFl_Mikyel`.
    pub key: String,
    /// 0 `character.inc`, 1 `-etc`, 2 `-school` (index into [`NpcTable::sources`]).
    pub source: u8,
    /// `SetName(IDS)` / `m_szName = STR` (display text, `" "` when the IDS text is empty).
    pub name: String,
    /// `SetImage(IDS)` / `m_szChar`: the portrait file in `Char/`, e.g. `char_Agent.tga`.
    pub portrait: String,
    /// `m_szDialog = "MaFl_Mikyel.txt"` (`None` when absent or empty).
    pub dialog_file: Option<String>,
    /// The NPC script key (doc 20): `dialog_file` minus its last 4 characters, lower-cased
    /// (`CNpcProperty::LoadDialog`); `None` when there is no dialog or the name is 4 characters or less.
    pub dialog_key: Option<String>,
    /// `m_szDlgQuest` (unused by the game).
    pub dlg_quest: String,
    /// `m_nStructure = SRT_*` (-1 when absent, as `CHARACTER::Clear`).
    pub structure: i32,
    /// `SetFigure(mover, hairMesh, hairColor, headMesh)`.
    pub figure: Option<NpcFigure>,
    /// `SetMusic(BGM_*)` (0 = none).
    pub music: MusicId,
    /// `SetEquip(...)`, at most [`MAX_NPC_EQUIP`] entries; a new `SetEquip` resets the list.
    pub equip: Vec<ItemId>,
    /// The enabled `MMI_*` menu entries (`AddMenu`, and `AddMenuLang(ENG, 0, ...)`), ascending, unique.
    pub menus: Vec<MenuId>,
    /// The shop definition and its precomputed stock.
    pub vendor: VendorDef,
    /// `SetBuffSkill(skill, lv, minLv, maxLv, timeMs)` in file order.
    pub buff_skills: Vec<NpcBuffSkill>,
    /// `AddTeleport(x, z)` in file order.
    pub teleports: Vec<[f32; 2]>,
    /// `SetLang(lang [, sub])`: the spawn language filter (`__CHIPI_DYO`).
    pub languages: Vec<(u16, u16)>,
    /// `SetOutput(FALSE)` gives `false` (default `true`).
    pub output: bool,
}

impl NpcDef {
    /// A fresh definition with the `CHARACTER::Clear` defaults.
    pub fn new(key: impl Into<String>, source: u8) -> Self {
        Self {
            key: key.into(),
            source,
            name: String::new(),
            portrait: String::new(),
            dialog_file: None,
            dialog_key: None,
            dlg_quest: String::new(),
            structure: -1,
            figure: None,
            music: 0,
            equip: Vec::new(),
            menus: Vec::new(),
            vendor: VendorDef::default(),
            buff_skills: Vec::new(),
            teleports: Vec::new(),
            languages: Vec::new(),
            output: true,
        }
    }

    /// `true` when the menu `mmi` is enabled (`m_abMoverMenu[mmi]`).
    pub fn has_menu(&self, mmi: MenuId) -> bool {
        self.menus.binary_search(&mmi).is_ok()
    }

    /// `CMover::IsVendorNPC`: the *definition* has shop content: a rule in any tab, chip items for a chip
    /// vendor (`vendor_type == 1`), or an `AddShopItem`. (The plan's wording "any tab has a non-empty
    /// stock" differs only for an NPC whose rules all come out empty.)
    pub fn is_vendor(&self) -> bool {
        self.vendor.tabs.iter().any(|t| {
            !t.rules.is_empty()
                || !t.shop_items.is_empty()
                || (self.vendor.vendor_type == 1 && !t.chip_items.is_empty())
        })
    }

    /// `true` when any tab ended up with at least one stock entry.
    pub fn has_stock(&self) -> bool {
        self.vendor.tabs.iter().any(|t| !t.stock.is_empty())
    }

    /// Sets [`Self::dialog_key`] from [`Self::dialog_file`] (`CNpcProperty::LoadDialog`).
    pub fn derive_dialog_key(&mut self) {
        self.dialog_key = self.dialog_file.as_deref().and_then(dialog_key_of);
    }
}

/// `CNpcProperty::LoadDialog`: the file name minus its last 4 bytes, lower-cased; `None` for names of 4
/// bytes or less (and for a cut that would split a multi-byte character).
pub fn dialog_key_of(file: &str) -> Option<String> {
    let n = file.len().checked_sub(4).filter(|n| *n > 0)?;
    file.get(..n).map(str::to_ascii_lowercase)
}

/// `SetFigure(mover, hairMesh, hairColor, headMesh)`: the look of a human NPC.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct NpcFigure {
    /// `MI_MALE` / `MI_FEMALE` (`m_dwMoverIdx`).
    pub mover: MoverId,
    /// `m_dwHairMesh`.
    pub hair_mesh: u32,
    /// `m_dwHairColor` (ARGB).
    pub hair_color: u32,
    /// `m_dwHeadMesh`.
    pub head_mesh: u32,
}

/// `SetBuffSkill(skill, level, minPlayerLv, maxPlayerLv, timeMs)` (`NPC_BUFF_SKILL`, `__NPC_BUFF`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct NpcBuffSkill {
    /// `dwSkillID`.
    pub skill: SkillId,
    /// `dwSkillLV`.
    pub level: u32,
    /// `nMinPlayerLV`.
    pub min_player_lv: i32,
    /// `nMaxPlayerLV`.
    pub max_player_lv: i32,
    /// `dwSkillTime` in ms.
    pub time_ms: u32,
}

/// The shop of an NPC: its type and four tabs.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct VendorDef {
    /// `SetVenderType(n)`: 0 penya shop, 1 red-chip shop (any other value behaves like 0).
    pub vendor_type: i32,
    /// The four tabs (`MAX_VENDOR_INVENTORY_TAB`).
    pub tabs: [VendorTab; MAX_VENDOR_INVENTORY_TAB],
}

/// One shop tab.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct VendorTab {
    /// `AddVendorSlot` / `AddVenderSlot` / `AddVendorSlotLang(ENG, 0, ...)`: the tab caption.
    pub title: String,
    /// `AddVendorItem` / `AddVenderItem` / `AddVendorItemLang(ENG, 0, ...)` rules in file order.
    pub rules: Vec<VendorRule>,
    /// `AddVendorItem2` / `AddVenderItem2`: the explicit items of a chip shop.
    pub chip_items: Vec<ItemId>,
    /// `AddShopItem` entries in file order (repack).
    pub shop_items: Vec<ShopItemEntry>,
    /// The precomputed stock (Plan/14 §6.8.2, F7), at most [`MAX_VENDOR_INVENTORY`] entries.
    pub stock: Vec<VendorStockEntry>,
}

/// `AddVendorItem(slot, ik3, job, rareMin, rareMax, total)` (`VENDOR_ITEM`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VendorRule {
    /// `m_nItemkind3`.
    pub ik3: i32,
    /// `m_nItemJob` (-1 = any).
    pub job: i32,
    /// `m_nUniqueMin`: the lowest rarity.
    pub uniq_min: i32,
    /// `m_nUniqueMax`: the highest rarity.
    pub uniq_max: i32,
    /// `m_nTotalNum` (unused by the game).
    pub total: i32,
}

/// `AddShopItem(slot, item [, price])`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShopItemEntry {
    /// The item.
    pub item: ItemId,
    /// The price that was written into the global item row (F6), if one was given.
    pub price_override: Option<u32>,
}

/// One shop slot: a full stack of an item (`CItemElem` with `m_nItemNum = (short)dwPackMax`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VendorStockEntry {
    /// The item.
    pub item: ItemId,
    /// `(short)dwPackMax`.
    pub count: i16,
    /// `m_nHitPoint` = `dwEndurance` (raw).
    pub hit_point: u32,
}

#[cfg(feature = "load")]
pub(crate) use parse::parse_character_file;

#[cfg(feature = "load")]
mod parse {
    use super::{
        MAX_MOVER_MENU, MAX_NPC_EQUIP, MAX_VENDOR_INVENTORY_TAB, NpcBuffSkill, NpcDef, NpcFigure,
        ShopItemEntry, VendorRule,
    };
    use crate::items_ext::except::{SERVER_LANG, SERVER_SUBLANG};
    use crate::props::item::ItemProp;
    use crate::types::IdTable;
    use flyff_script::{DiagCode, Diagnostics, ScannerOptions, Script, ScriptCtx, Severity};
    use std::sync::Arc;

    /// The statements `LoadCharacter` recognises (the first token decides).
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    enum Stmt {
        Open,
        Close,
        RandomItem,
        SetEquip,
        MName,
        SetName,
        SetFigure,
        SetMusic,
        MStructure,
        MChar,
        MDialog,
        MDlgQuest,
        SetImage,
        AddMenuLang,
        AddMenu,
        AddVenderSlot,
        AddVendorSlot,
        AddVendorSlotLang,
        AddVendorItemLang,
        AddVendorItem,
        AddVendorItem2,
        SetVenderType,
        SetBuffSkill,
        SetLang,
        SetOutput,
        AddTeleport,
        AddShopItem,
    }

    fn stmt_of(token: &str, add_shop_item: bool) -> Option<Stmt> {
        Some(match token {
            "{" => Stmt::Open,
            "}" => Stmt::Close,
            "randomItem" => Stmt::RandomItem,
            "SetEquip" => Stmt::SetEquip,
            "m_szName" => Stmt::MName,
            "SetName" => Stmt::SetName,
            "SetFigure" => Stmt::SetFigure,
            "SetMusic" => Stmt::SetMusic,
            "m_nStructure" => Stmt::MStructure,
            "m_szChar" => Stmt::MChar,
            "m_szDialog" => Stmt::MDialog,
            "m_szDlgQuest" => Stmt::MDlgQuest,
            "SetImage" => Stmt::SetImage,
            "AddMenuLang" => Stmt::AddMenuLang,
            "AddMenu" => Stmt::AddMenu,
            "AddVenderSlot" => Stmt::AddVenderSlot,
            "AddVendorSlot" => Stmt::AddVendorSlot,
            "AddVendorSlotLang" => Stmt::AddVendorSlotLang,
            "AddVendorItemLang" => Stmt::AddVendorItemLang,
            "AddVenderItem" | "AddVendorItem" => Stmt::AddVendorItem,
            "AddVenderItem2" | "AddVendorItem2" => Stmt::AddVendorItem2,
            "SetVenderType" => Stmt::SetVenderType,
            "SetBuffSkill" => Stmt::SetBuffSkill,
            "SetLang" => Stmt::SetLang,
            "SetOutput" => Stmt::SetOutput,
            "AddTeleport" => Stmt::AddTeleport,
            "AddShopItem" if add_shop_item => Stmt::AddShopItem,
            _ => return None,
        })
    }

    /// The tab index of a slot number, or an Error (`IndexOutOfRange`) for a slot the original would
    /// have written past its arrays with.
    fn tab_index(sc: &mut Script<'_>, key: &str, stmt: &str, slot: i32) -> Option<usize> {
        match usize::try_from(slot) {
            Ok(i) if i < MAX_VENDOR_INVENTORY_TAB => Some(i),
            _ => {
                let msg = format!(
                    "{key}: {stmt} slot {slot} is not below {MAX_VENDOR_INVENTORY_TAB}; ignored"
                );
                sc.report(Severity::Error, DiagCode::IndexOutOfRange, msg);
                None
            }
        }
    }

    /// `m_abMoverMenu[mmi] = TRUE` for `0 <= mmi < MAX_MOVER_MENU`; anything else is an Error and ignored.
    fn enable_menu(sc: &mut Script<'_>, npc: &mut NpcDef, mmi: i32) {
        match u32::try_from(mmi) {
            Ok(id) if id < MAX_MOVER_MENU => {
                if let Err(pos) = npc.menus.binary_search(&id) {
                    npc.menus.insert(pos, id);
                }
            }
            _ => {
                let msg = format!(
                    "{}: menu id {mmi} is not below MAX_MOVER_MENU ({MAX_MOVER_MENU}); ignored",
                    npc.key
                );
                sc.report(Severity::Error, DiagCode::IndexOutOfRange, msg);
            }
        }
    }

    fn lang_is_server(lang: i32, sub: i32) -> bool {
        lang == SERVER_LANG && sub == SERVER_SUBLANG
    }

    /// `SetEquip '(' ITEM {',' ITEM}* ')'`: the loop tests the first character of the *current* token, so
    /// the `(` enters it. A truncated file ends it (the original would spin).
    fn read_equip(sc: &mut Script<'_>, npc: &mut NpcDef) {
        npc.equip.clear();
        sc.get_token(); // (
        while !sc.token().starts_with(')') && !sc.finished() {
            let item = sc.get_number();
            if npc.equip.len() < MAX_NPC_EQUIP {
                npc.equip.push(item);
            }
            sc.get_token(); // , or )
        }
    }

    /// `InterpretRandomItem`: `{` then tokens up to the first one that starts with `}`. The braces are not
    /// counted by the caller.
    fn skip_random_item(sc: &mut Script<'_>) {
        sc.get_token(); // {
        sc.get_token();
        while !sc.token().starts_with('}') && !sc.finished() {
            sc.get_token();
        }
    }

    /// `AddVendorItem(Lang)` payload after the (optional) language prefix: `slot , ik3 , job , min , max ,
    /// total )`. Returns the slot and the rule.
    fn read_vendor_rule(sc: &mut Script<'_>) -> (i32, VendorRule) {
        let slot = sc.get_number_i32();
        sc.get_token();
        let ik3 = sc.get_number_i32();
        sc.get_token();
        let job = sc.get_number_i32();
        sc.get_token();
        let uniq_min = sc.get_number_i32();
        sc.get_token();
        let uniq_max = sc.get_number_i32();
        sc.get_token();
        let total = sc.get_number_i32();
        sc.get_token(); // )
        (
            slot,
            VendorRule {
                ik3,
                job,
                uniq_min,
                uniq_max,
                total,
            },
        )
    }

    fn add_rule(sc: &mut Script<'_>, npc: &mut NpcDef, stmt: &str, slot: i32, rule: VendorRule) {
        if let Some(i) = tab_index(sc, &npc.key, stmt, slot)
            && let Some(tab) = npc.vendor.tabs.get_mut(i)
        {
            tab.rules.push(rule);
        }
    }

    fn set_title(sc: &mut Script<'_>, npc: &mut NpcDef, stmt: &str, slot: i32, title: String) {
        if let Some(i) = tab_index(sc, &npc.key, stmt, slot)
            && let Some(tab) = npc.vendor.tabs.get_mut(i)
        {
            tab.title = title;
        }
    }

    /// Executes one statement whose keyword has just been read.
    #[allow(clippy::too_many_lines)] // one arm per statement of the original `if` chain
    fn run_statement(
        sc: &mut Script<'_>,
        stmt: Stmt,
        npc: &mut NpcDef,
        items: &mut IdTable<ItemProp>,
    ) {
        match stmt {
            Stmt::Open | Stmt::Close => {} // handled by the caller
            Stmt::RandomItem => skip_random_item(sc),
            Stmt::SetEquip => read_equip(sc, npc),
            Stmt::MName => {
                sc.get_token(); // =
                sc.get_token();
                npc.name = sc.token().to_owned();
            }
            Stmt::SetName => {
                sc.get_token(); // (
                npc.name = sc.lang_script();
            }
            Stmt::SetFigure => {
                sc.get_token(); // (
                let mover = sc.get_number();
                sc.get_token(); // ,
                let hair_mesh = sc.get_number();
                sc.get_token(); // ,
                let hair_color = sc.get_number();
                sc.get_token(); // ,
                let head_mesh = sc.get_number();
                npc.figure = Some(NpcFigure {
                    mover,
                    hair_mesh,
                    hair_color,
                    head_mesh,
                });
            }
            Stmt::SetMusic => {
                sc.get_token(); // (
                npc.music = sc.get_number();
            }
            Stmt::MStructure => {
                sc.get_token(); // =
                npc.structure = sc.get_number_i32();
            }
            Stmt::MChar => {
                sc.get_token(); // =
                npc.portrait = sc.lang_script();
            }
            Stmt::SetImage => {
                sc.get_token(); // (
                npc.portrait = sc.lang_script();
            }
            Stmt::MDialog => {
                sc.get_token(); // =
                sc.get_token();
                let s = sc.token().to_owned();
                npc.dialog_file = (!s.is_empty()).then_some(s);
            }
            Stmt::MDlgQuest => {
                sc.get_token(); // =
                sc.get_token();
                npc.dlg_quest = sc.token().to_owned();
            }
            Stmt::AddMenuLang => {
                sc.get_token(); // (
                let lang = sc.get_number_i32();
                sc.get_token(); // ,
                let sub = sc.get_number_i32();
                sc.get_token(); // ,
                let mmi = sc.get_number_i32();
                sc.get_token(); // )
                if lang_is_server(lang, sub) {
                    enable_menu(sc, npc, mmi);
                }
            }
            Stmt::AddMenu => {
                sc.get_token(); // (
                let mmi = sc.get_number_i32();
                sc.get_token(); // )
                enable_menu(sc, npc, mmi);
            }
            Stmt::AddVenderSlot => {
                sc.get_token(); // (
                let slot = sc.get_number_i32();
                sc.get_token(); // ,
                sc.get_token();
                let title = sc.token().to_owned();
                sc.get_token(); // )
                set_title(sc, npc, "AddVenderSlot", slot, title);
            }
            Stmt::AddVendorSlot => {
                sc.get_token(); // (
                let slot = sc.get_number_i32();
                sc.get_token(); // ,
                let title = sc.lang_script();
                set_title(sc, npc, "AddVendorSlot", slot, title);
            }
            Stmt::AddVendorSlotLang => {
                sc.get_token(); // (
                let lang = sc.get_number_i32();
                sc.get_token(); // ,
                let sub = sc.get_number_i32();
                sc.get_token(); // ,
                let slot = sc.get_number_i32();
                sc.get_token(); // ,
                let title = sc.lang_script();
                if lang_is_server(lang, sub) {
                    set_title(sc, npc, "AddVendorSlotLang", slot, title);
                }
            }
            Stmt::AddVendorItemLang => {
                sc.get_token(); // (
                let lang = sc.get_number_i32();
                sc.get_token(); // ,
                let sub = sc.get_number_i32();
                sc.get_token(); // ,
                let (slot, rule) = read_vendor_rule(sc);
                if lang_is_server(lang, sub) {
                    add_rule(sc, npc, "AddVendorItemLang", slot, rule);
                }
            }
            Stmt::AddVendorItem => {
                sc.get_token(); // (
                let (slot, rule) = read_vendor_rule(sc);
                add_rule(sc, npc, "AddVendorItem", slot, rule);
            }
            Stmt::AddVendorItem2 => {
                sc.get_token(); // (
                let slot = sc.get_number_i32();
                sc.get_token(); // ,
                let id = sc.get_number();
                sc.get_token(); // )
                if let Some(i) = tab_index(sc, &npc.key, "AddVendorItem2", slot)
                    && let Some(tab) = npc.vendor.tabs.get_mut(i)
                {
                    tab.chip_items.push(id);
                }
            }
            Stmt::SetVenderType => {
                sc.get_token(); // (
                npc.vendor.vendor_type = sc.get_number_i32();
                sc.get_token(); // )
            }
            Stmt::SetBuffSkill => {
                sc.get_token(); // (
                let skill = sc.get_number();
                sc.get_token(); // ,
                let level = sc.get_number();
                sc.get_token(); // ,
                let min_player_lv = sc.get_number_i32();
                sc.get_token(); // ,
                let max_player_lv = sc.get_number_i32();
                sc.get_token(); // ,
                let time_ms = sc.get_number();
                sc.get_token(); // )
                npc.buff_skills.push(NpcBuffSkill {
                    skill,
                    level,
                    min_player_lv,
                    max_player_lv,
                    time_ms,
                });
            }
            Stmt::SetLang => {
                sc.get_token(); // (
                let lang = sc.get_number();
                let mut sub = 0;
                sc.get_token(); // , or )
                if sc.is(",") {
                    sub = sc.get_number();
                } else {
                    sc.get_token(); // the original swallows one more token here
                }
                npc.languages.push((lang as u16, sub as u16));
            }
            Stmt::SetOutput => {
                sc.get_token(); // (
                sc.get_token();
                if sc.token().eq_ignore_ascii_case("FALSE") {
                    npc.output = false;
                }
                sc.get_token(); // )
            }
            Stmt::AddTeleport => {
                sc.get_token(); // (
                let x = sc.get_float();
                sc.get_token(); // ,
                let z = sc.get_float();
                sc.get_token(); // )
                npc.teleports.push([x, z]);
            }
            Stmt::AddShopItem => {
                sc.get_token(); // (
                let slot = sc.get_number_i32();
                sc.get_token(); // ,
                let id = sc.get_number();
                sc.get_token(); // , or )
                let mut price_override = None;
                let slot_ok = tab_index(sc, &npc.key, "AddShopItem", slot);
                if sc.is(",") {
                    let cost = sc.get_number();
                    if slot_ok.is_some() {
                        // F6: the price is written into the global item row. (`if (cost && cost < 1)
                        // cost = 1` is a no-op on a DWORD.)
                        if let Some(p) = items.get_mut(id) {
                            if npc.vendor.vendor_type == 0 {
                                p.cost = cost;
                            } else {
                                p.refer_value1 = cost;
                            }
                        }
                        price_override = Some(cost);
                    }
                }
                if let Some(i) = slot_ok
                    && let Some(tab) = npc.vendor.tabs.get_mut(i)
                {
                    tab.shop_items.push(ShopItemEntry {
                        item: id,
                        price_override,
                    });
                }
            }
        }
    }

    /// `CProject::LoadCharacter` over decoded `text`: the NPCs of one file in file order.
    ///
    /// `items` is the loaded item table: `AddShopItem` with a price mutates it (F6). `add_shop_item`
    /// switches the statement on (`__ADDSHOPITEM`); when off the keyword is just an unknown token.
    pub(crate) fn parse_character_file(
        text: &str,
        file: &Arc<str>,
        sctx: &ScriptCtx,
        source: u8,
        add_shop_item: bool,
        items: &mut IdTable<ItemProp>,
        diags: &mut Diagnostics,
    ) -> Vec<NpcDef> {
        let mut out = Vec::new();
        let mut sc = Script::new(
            text,
            Arc::clone(file),
            ScannerOptions::default(),
            sctx,
            diags,
        );
        sc.get_token(); // the key
        while !sc.finished() {
            let mut npc = NpcDef::new(sc.token(), source);
            sc.get_token(); // '{' (whatever the token is)
            let mut block = 1u32;
            while block != 0 && !sc.finished() {
                sc.get_token();
                match stmt_of(sc.token(), add_shop_item) {
                    Some(Stmt::Open) => block += 1,
                    Some(Stmt::Close) => block -= 1,
                    Some(stmt) => run_statement(&mut sc, stmt, &mut npc, items),
                    None => {}
                }
            }
            npc.derive_dialog_key();
            out.push(npc);
            sc.get_token(); // the next key
        }
        out
    }
}
