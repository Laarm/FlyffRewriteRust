//! `DPSProp.txt` (`CProject::LoadDPSProp`, Plan/14 §6.14, L68; src:_Common/ProjectCmn.cpp:926-961): the monsters
//! that get a damage meter besides the bosses, `MONSTER '{' id* '}'`. The file is empty in this data set.
//!
//! The parser lives with `MonsterBuffs.inc` in [`super::monster_buffs`] (both are L67/L68 and share their test
//! module); `GameData::dps_monsters` holds the ids.

#[cfg(feature = "load")]
pub(crate) use super::monster_buffs::parse_dps_monsters;
