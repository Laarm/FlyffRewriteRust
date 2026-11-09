//! The pure part of the equipment rules (Plan/16 §13.1, §13.3; ref/10 §14; src:_Common/MoverEquip.cpp:459-1046,
//! 1478-1702): who may wear an item, which part it goes to and which parts it displaces.
//!
//! The server's `flyff_game::stats::equip` adds what needs world state (flight, pets, terrain) and the
//! transaction around the container; the browser uses [`check_equip`] for the tooltip's red requirement
//! lines.

use crate::consts::job::{JOB_BLADE, JOB_BLADE_HERO, JOB_BLADE_MASTER, JTYPE_MASTER};
use crate::consts::{MAX_HUMAN_PARTS, NULL_ID};
use crate::data::RulesData;
use crate::item::random_option::RandomOptWord;
use crate::jobs::{is_interiority_job, job_type};
use flyff_common::defs::attribute::{HD_ONE, HD_TWO};
use flyff_common::defs::item::II_WEA_KNU_ISHOFIST;
use flyff_common::defs::itemkind::{IK3_ARROW, IK3_BOW, IK3_SHIELD};
use flyff_common::defs::neuz::{
    PARTS_EARRING1, PARTS_LWEAPON, PARTS_RING1, PARTS_RWEAPON, PARTS_SHIELD,
};
use flyff_common::defs::text::{TID_GAME_REQLEVEL, TID_GAME_WRONGJOB, TID_GAME_WRONGSEX};
use flyff_data::props::item::ItemProp;
use smallvec::SmallVec;

/// Why an equip request was refused: the text for the player (`None` = silent) and its arguments, already
/// formatted for `DefinedText` (item names are quoted, `"\"<name>\""`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EquipDenied {
    /// `TID_*` of the `DefinedText`; `None` = no message.
    pub tid: Option<u32>,
    /// The `DefinedText` arguments.
    pub args: Vec<String>,
}

impl EquipDenied {
    /// A refusal without a message.
    pub fn silent() -> EquipDenied {
        EquipDenied {
            tid: None,
            args: Vec::new(),
        }
    }

    /// A refusal with a text and arguments.
    pub fn with(tid: u32, args: Vec<String>) -> EquipDenied {
        EquipDenied {
            tid: Some(tid),
            args,
        }
    }

    /// A refusal with a text without arguments.
    pub fn text(tid: u32) -> EquipDenied {
        EquipDenied::with(tid, Vec::new())
    }
}

/// The wearer's facts the requirement checks read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Wearer {
    /// `SEX_MALE` 0 / `SEX_FEMALE` 1.
    pub sex: u32,
    /// The job id.
    pub job: i32,
    /// The level.
    pub level: i32,
}

/// `CItemElem::GetLimitLevel` (src:_Common/Item.cpp:700-733): `NULL_ID` for an item without a level limit,
/// else `max(0, limit_level1 + level_down)` where the level-down (−5 / −10) comes from bits 62 / 63 of the
/// random-option word.
pub fn limit_level(prop: &ItemProp, opt: RandomOptWord) -> u32 {
    if prop.limit_level1 == NULL_ID {
        return NULL_ID;
    }
    let v = i64::from(prop.limit_level1) + i64::from(opt.level_down().levels());
    u32::try_from(v.max(0)).unwrap_or(NULL_ID)
}

/// `CItemElem::IsLimitLevel(mover)`: a master or hero wearing an item of a lower tier (or of its own tier
/// when the item has no job) is not level-limited by the item; everybody else needs `level >=
/// limit_level`.
pub fn is_limit_level(d: &RulesData, w: &Wearer, prop: &ItemProp, opt: RandomOptWord) -> bool {
    let own = job_type(d, w.job);
    // `GetJobType(NULL_ID)` is the mover's own tier
    let item = if prop.item_job == NULL_ID {
        own
    } else {
        job_type(d, i32::try_from(prop.item_job).unwrap_or(-1))
    };
    if own != u32::MAX && own >= JTYPE_MASTER && own > item {
        return false;
    }
    let limit = limit_level(prop, opt);
    i64::from(w.level) < i64::from(limit)
}

/// Steps 2-5 of `IsEquipAble` (Plan/16 §13.1): sex, job, level (skipped with `ignore_level`) and the bow rule
/// for arrows. `rweapon_is_bow` tells whether the right hand holds a bow. `name` is the item's display name
/// for the quoted message arguments.
pub fn check_equip(
    d: &RulesData,
    w: &Wearer,
    prop: &ItemProp,
    opt: RandomOptWord,
    name: &str,
    rweapon_is_bow: bool,
    ignore_level: bool,
) -> Result<(), EquipDenied> {
    if prop.item_sex != NULL_ID && prop.item_sex != w.sex {
        return Err(EquipDenied::with(TID_GAME_WRONGSEX, vec![quoted(name)]));
    }
    if prop.item_job != NULL_ID
        && !is_interiority_job(d, w.job, i32::try_from(prop.item_job).unwrap_or(-1))
    {
        return Err(EquipDenied::with(TID_GAME_WRONGJOB, vec![quoted(name)]));
    }
    if !ignore_level && prop.limit_level1 != NULL_ID && is_limit_level(d, w, prop, opt) {
        return Err(EquipDenied::with(
            TID_GAME_REQLEVEL,
            vec![quoted(&limit_level(prop, opt).to_string())],
        ));
    }
    if prop.item_kind3 == IK3_ARROW && !rweapon_is_bow {
        return Err(EquipDenied::silent());
    }
    Ok(())
}

fn quoted(s: &str) -> String {
    format!("\"{s}\"")
}

/// The props of the 31 equipment slots (`None` = empty), the input of the slot planner.
#[derive(Clone, Copy, Debug)]
pub struct Worn<'a> {
    /// `parts[p]` = the prop of the item in part `p`.
    pub parts: [Option<&'a ItemProp>; MAX_HUMAN_PARTS],
}

impl<'a> Worn<'a> {
    /// Nothing worn.
    pub fn empty() -> Worn<'a> {
        Worn {
            parts: [None; MAX_HUMAN_PARTS],
        }
    }

    /// The prop in `part`.
    pub fn get(&self, part: u32) -> Option<&'a ItemProp> {
        self.parts
            .get(usize::try_from(part).ok()?)
            .copied()
            .flatten()
    }

    fn has(&self, part: u32) -> bool {
        self.get(part).is_some()
    }
}

/// `true` for the blade jobs that may dual-wield (`JOB_BLADE`, its master and hero).
pub fn is_dual_wield_job(job: i32) -> bool {
    job == JOB_BLADE as i32 || job == JOB_BLADE_MASTER as i32 || job == JOB_BLADE_HERO as i32
}

/// The part an item goes to (Plan/16 §13.3 steps 1, 2 and 6): the prop's part; a one-handed weapon of a blade
/// whose right hand already holds a one-handed weapon (and no shield) goes to the left hand; a ring or
/// earring goes to the second slot when the first is taken and the second is free. `None` for an item that
/// is not equipment.
pub fn equip_target(job: i32, prop: &ItemProp, worn: &Worn<'_>) -> Option<u32> {
    if prop.parts == NULL_ID || usize::try_from(prop.parts).ok()? >= MAX_HUMAN_PARTS {
        return None;
    }
    let mut target = prop.parts;
    // 2. dual wield
    if prop.handed == HD_ONE && prop.parts == PARTS_RWEAPON {
        let right_is_one_handed = worn.get(PARTS_RWEAPON).is_some_and(|r| r.handed == HD_ONE);
        if is_dual_wield_job(job)
            && prop.id != II_WEA_KNU_ISHOFIST
            && right_is_one_handed
            && !worn.has(PARTS_SHIELD)
        {
            target = PARTS_LWEAPON;
        }
    }
    // 6. rings and earrings
    if (target == PARTS_RING1 || target == PARTS_EARRING1)
        && worn.has(target)
        && !worn.has(target + 1)
    {
        target = prop.partsub;
    }
    Some(target)
}

/// The parts an equip displaces, in the legacy order and without repeats (Plan/16 §13.3 steps 3-5 and 7):
/// a shield displaces a two-handed right weapon, else a left-hand weapon; a two-handed weapon displaces the
/// shield, the right weapon and (FIX PL-077) the left weapon; `exclusive` displaces the item in that part;
/// finally whatever sits in `target` is replaced.
pub fn displaced_parts(prop: &ItemProp, target: u32, worn: &Worn<'_>) -> SmallVec<[u32; 4]> {
    let mut out: SmallVec<[u32; 4]> = SmallVec::new();
    let add = |p: u32, out: &mut SmallVec<[u32; 4]>| {
        if !out.contains(&p) {
            out.push(p);
        }
    };
    // 3. shield
    if prop.item_kind3 == IK3_SHIELD {
        if worn.get(PARTS_RWEAPON).is_some_and(|r| r.handed == HD_TWO) {
            add(PARTS_RWEAPON, &mut out);
        } else if worn
            .get(PARTS_LWEAPON)
            .is_some_and(|l| l.parts == PARTS_RWEAPON)
        {
            add(PARTS_LWEAPON, &mut out);
        }
    }
    // 4. two-handed
    if prop.handed == HD_TWO {
        for part in [PARTS_SHIELD, PARTS_RWEAPON, PARTS_LWEAPON] {
            if worn.has(part) {
                add(part, &mut out);
            }
        }
    }
    // 5. exclusive
    if prop.exclusive != NULL_ID && worn.has(prop.exclusive) {
        add(prop.exclusive, &mut out);
    }
    // 7. replace
    if worn.has(target) {
        add(target, &mut out);
    }
    out
}

/// Which part an unequip of `part` swaps: unequipping the right weapon while the left hand holds a weapon
/// moves the left weapon to the right hand.
pub fn swaps_left_to_right(part: u32, worn: &Worn<'_>) -> bool {
    part == PARTS_RWEAPON && worn.has(PARTS_LWEAPON)
}

/// `true` when the right hand holds a bow (the arrow rule).
pub fn rweapon_is_bow(worn: &Worn<'_>) -> bool {
    worn.get(PARTS_RWEAPON)
        .is_some_and(|r| r.item_kind3 == IK3_BOW)
}
