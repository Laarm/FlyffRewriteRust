//! The 64-bit packed random-option word of an item (Plan/19 §2.2, ref/13 §5.3;
//! src:_Common/Item.h:155-199, randomoption.cpp:91-207, Item.cpp:690-719).
//!
//! The same word is the persisted form (`ItemRecord.random_option`) and the wire form
//! (`ItemElem.random_opt_item_id`), so no second representation exists. It is bit-packed (bits 61, 62 and 63
//! are used) and therefore never goes through a JavaScript number.
//!
//! | bits | meaning |
//! |---|---|
//! | 0-7 | the stat-bonus id (`RandomOptItem` of `propItemEtc.inc`); 0 = none |
//! | 8-25, 26-43, 44-61 | lines 0, 1, 2: bits 0-8 `abs(adj)`, bit 9 sign, bits 10-16 DST (mask `0x7F`) |
//! | 61 | [`RandomOptWord::AWAKE_SAFE_FLAG`] (overlaps the unused top DST bit of line 2) |
//! | 62, 63 | level-down −5, −10 |

/// One decoded line: a DST and its signed value (`GetParam`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RandomOptLine {
    /// The DST id, `1..=127`.
    pub dst: u8,
    /// The value, `-511..=511`.
    pub adj: i16,
}

/// The level reduction carried by bits 62 and 63 (`GetLevelDown`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LevelDown {
    /// No reduction.
    None = 0,
    /// Bit 62: the item needs 5 levels less.
    Minus5 = -5,
    /// Bit 63: the item needs 10 levels less.
    Minus10 = -10,
}

impl LevelDown {
    /// The signed number of levels (0, −5 or −10).
    pub fn levels(self) -> i32 {
        self as i32
    }
}

/// The packed random-option word (`m_iRandomOptItemId`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct RandomOptWord(pub u64);

impl RandomOptWord {
    /// `MAX_RANDOM_OPTION`: the number of line slots.
    pub const MAX_LINES: usize = 3;
    /// Mask of one 18-bit line slot (before shifting).
    const LINE_MASK: u64 = 0x3FFFF;
    /// `InitializeRandomOption` keeps the stat id and both level-down bits.
    pub const KEEP_MASK: u64 = 0xC000_0000_0000_00FF;
    /// `_AWAKE_SAFE_FLAG`: a protected awakening is pending.
    pub const AWAKE_SAFE_FLAG: u64 = 0x2000_0000_0000_0000;
    /// Level-down −5.
    pub const LEVEL_DOWN_5: u64 = 0x4000_0000_0000_0000;
    /// Level-down −10.
    pub const LEVEL_DOWN_10: u64 = 0x8000_0000_0000_0000;

    /// The word from its wire/persisted form (the signed 64-bit field reinterpreted).
    pub fn from_i64(v: i64) -> RandomOptWord {
        RandomOptWord(v as u64)
    }

    /// The wire form (`sint64`).
    pub fn to_i64(self) -> i64 {
        self.0 as i64
    }

    /// `GetRandomOpt`: the stat-bonus id (the low byte).
    pub fn stat_id(self) -> u8 {
        (self.0 & 0xFF) as u8
    }

    /// `SetRandomOpt` (`UI_RANDOMOPTITEMID`): replaces the low byte; awakening lines and level-down stay.
    pub fn set_stat_id(&mut self, id: u8) {
        self.0 = (self.0 & !0xFF) | u64::from(id);
    }

    /// `GetParam(i)`: line `i` (`0..3`); `None` when the slot is out of range or its DST is 0.
    pub fn line(self, i: usize) -> Option<RandomOptLine> {
        if i >= Self::MAX_LINES {
            return None;
        }
        let v = (self.0 >> (8 + 18 * i)) & Self::LINE_MASK;
        let mut adj = (v & 0x1FF) as i16;
        if v & 0x200 != 0 {
            adj = -adj;
        }
        let dst = ((v >> 10) & 0x7F) as u8;
        (dst > 0).then_some(RandomOptLine { dst, adj })
    }

    /// `GetViewRandomOptionSize`: consecutive non-empty 18-bit slots from line 0, ignoring the safe flag.
    /// A slot counts when **any** of its bits is set (the legacy mask test, not "DST > 0"; the two agree for
    /// every word the game writes).
    pub fn view_line_count(self) -> usize {
        let mut n = 0;
        for i in 0..Self::MAX_LINES {
            if (self.0 >> (8 + 18 * i)) & Self::LINE_MASK != 0 {
                n += 1;
            } else {
                return n;
            }
        }
        n
    }

    /// `GetRandomOptionSize`: [`RandomOptWord::view_line_count`], but 0 while the protected-awakening flag
    /// is set.
    pub fn line_count(self) -> usize {
        if self.safe_flag() {
            0
        } else {
            self.view_line_count()
        }
    }

    /// The decoded lines of [`RandomOptWord::line_count`] (the lines that take effect).
    pub fn lines(self) -> impl Iterator<Item = RandomOptLine> {
        (0..self.line_count()).filter_map(move |i| self.line(i))
    }

    /// `SetParam`: appends a line at the next free slot. `false` when all three slots are used, the DST or
    /// value does not fit the layout (`dst` 1..=127, `|adj|` ≤ 511) or the protected-awakening flag is set.
    pub fn push_line(&mut self, l: RandomOptLine) -> bool {
        if l.dst == 0 || l.dst > 0x7F || l.adj.unsigned_abs() > 0x1FF || self.safe_flag() {
            return false;
        }
        let n = self.view_line_count();
        if n >= Self::MAX_LINES {
            return false;
        }
        let mut v = u64::from(l.adj.unsigned_abs()) | (u64::from(l.dst) << 10);
        if l.adj < 0 {
            v |= 0x200;
        }
        self.0 |= v << (8 + 18 * n);
        true
    }

    /// `InitializeRandomOption`: clears the lines and the safe flag, keeps the stat id and the level-down.
    pub fn initialize(&mut self) {
        self.0 &= Self::KEEP_MASK;
    }

    /// `IsCheckedSafeFlag`.
    pub fn safe_flag(self) -> bool {
        self.0 & Self::AWAKE_SAFE_FLAG == Self::AWAKE_SAFE_FLAG
    }

    /// `SetSafeFlag` / `ResetSafeFlag`.
    pub fn set_safe_flag(&mut self, on: bool) {
        if on {
            self.0 |= Self::AWAKE_SAFE_FLAG;
        } else {
            self.0 &= !Self::AWAKE_SAFE_FLAG;
        }
    }

    /// `GetLevelDown`: bit 63 → −10, else bit 62 → −5.
    pub fn level_down(self) -> LevelDown {
        if self.0 & Self::LEVEL_DOWN_10 != 0 {
            LevelDown::Minus10
        } else if self.0 & Self::LEVEL_DOWN_5 != 0 {
            LevelDown::Minus5
        } else {
            LevelDown::None
        }
    }

    /// Sets the level-down bits (exactly one of them, or none).
    pub fn set_level_down(&mut self, l: LevelDown) {
        self.0 &= !(Self::LEVEL_DOWN_5 | Self::LEVEL_DOWN_10);
        match l {
            LevelDown::None => {}
            LevelDown::Minus5 => self.0 |= Self::LEVEL_DOWN_5,
            LevelDown::Minus10 => self.0 |= Self::LEVEL_DOWN_10,
        }
    }
}

/// The three kinds of random-option tables an item can use (`CRandomOptionProperty::GetRandomOptionKind`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RandomOptKind {
    /// Gear awakening.
    Awakening,
    /// Fashion blessing.
    Blessing,
    /// A system pet of level C or better.
    SystemPet,
    /// An eat pet.
    EatPet,
}

/// `GetRandomOptionKind(item)` from the item's `parts`, `IK3` and (for eggs) pet level. With
/// `features.only_armor_awakes` (the repack default, the legacy `__ONLY_ARMOR_AWAKES` build flag) only
/// suit, gauntlet, boots and helmet awaken and nothing else has a kind. `parts` is the prop's `dwParts`
/// (`NULL_ID` for items that are not equipment).
pub fn random_option_kind(
    parts: u32,
    ik3: u32,
    egg_level_c_or_better: bool,
    only_armor_awakes: bool,
) -> Option<RandomOptKind> {
    use flyff_common::defs::itemkind::{IK3_EGG, IK3_PET};
    use flyff_common::defs::neuz::*;
    match parts {
        PARTS_UPPER_BODY | PARTS_HAND | PARTS_FOOT | PARTS_CAP => {
            return Some(RandomOptKind::Awakening);
        }
        _ => {}
    }
    if only_armor_awakes {
        return None;
    }
    match parts {
        PARTS_RWEAPON | PARTS_SHIELD => Some(RandomOptKind::Awakening),
        PARTS_CLOTH | PARTS_CLOAK | PARTS_HAT | PARTS_GLOVE | PARTS_BOOTS => {
            Some(RandomOptKind::Blessing)
        }
        _ if ik3 == IK3_EGG && egg_level_c_or_better => Some(RandomOptKind::SystemPet),
        _ if ik3 == IK3_PET => Some(RandomOptKind::EatPet),
        _ => None,
    }
}
