//! Constants of the mover rules (Plan/16 §2): levels, tiers, slots, DST ids, mode bits, SM timer ids and the
//! server timing constants.
//!
//! Everything that exists in the original `Resource/define*.h` headers is re-exported from
//! [`flyff_common::defs`] (generated, WP-DATA-19) so there is exactly one copy of each value; the constants of
//! the original *source* headers (`authorization.h`, `CmnHdr.h`, `ProjectCmn.h`), which are not part of
//! `Resource/`, are written here with their citation.

pub use flyff_common::defs;

/// The DST ids, `CHS_*` bits and the other `defineAttribute.h` constants (`DST_STR` ... `DST_ALL_DEC_RATE`).
pub mod dst {
    pub use flyff_common::defs::attribute::*;
}

/// The job ids and tier bounds of `defineJob.h`.
pub mod job {
    pub use flyff_common::defs::job::*;
}

/// The equipment parts of `defineNeuz.h` (`PARTS_*`, `MAX_HUMAN_PARTS`).
pub mod parts {
    pub use flyff_common::defs::neuz::*;
}

// ---- levels, tiers, slots (ref/10 §4.2, Plan/16 §2.1) -----------------------------------------------------

/// `MAX_JOB_LEVEL`: the vagrant cap and level of the first job change.
pub const MAX_JOB_LEVEL: i32 = defs::job::MAX_JOB_LEVEL as i32;
/// `MAX_EXP_LEVEL`: expert levels (expert cap = 15 + 45 = 60).
pub const MAX_EXP_LEVEL: i32 = defs::job::MAX_EXP_LEVEL as i32;
/// `MAX_LEVEL`: the pro and master cap.
pub const MAX_LEVEL: i32 = defs::job::MAX_LEVEL as i32;
/// `MAX_LEGEND_LEVEL`: the hero cap.
pub const MAX_LEGEND_LEVEL: i32 = defs::job::MAX_LEGEND_LEVEL as i32;
/// `MAX_MONSTER_LEVEL`.
pub const MAX_MONSTER_LEVEL: i32 = defs::job::MAX_MONSTER_LEVEL as i32;
/// `MAX_JOB`: size of the job table.
pub const MAX_JOB: usize = defs::job::MAX_JOB as usize;
/// `MAX_SKILL_JOB`: length of `job_skills`; blocks start at 0, 3, 23, 43, 44 (`Mover.h:105`).
pub const MAX_SKILL_JOB: usize = 45;
/// First slot of the vagrant, expert, pro, master and hero skill blocks.
pub const SKILL_BLOCK_START: [usize; 5] = [0, 3, 23, 43, 44];
/// `MAX_ADJPARAMARY`: size of the stored DST arrays.
pub const MAX_ADJPARAMARY: usize = defs::attribute::MAX_ADJPARAMARY as usize;
/// `MAX_EXPCHARACTER`: rows of `expCharacter` (`ProjectCmn.h:24`).
pub const MAX_EXPCHARACTER: usize = 200;
/// `MAX_HUMAN_PARTS`: equipment parts.
pub const MAX_HUMAN_PARTS: usize = defs::neuz::MAX_HUMAN_PARTS as usize;
/// `MAX_INVENTORY` with `features.inventory_168`: equipment slot index = `MAX_INVENTORY + part` (doc 19).
pub const MAX_INVENTORY: usize = 168;
/// `RESTATE_LOW`: the `ReStateOneLow` refund cap.
pub const RESTATE_LOW: i32 = 50;
/// `MIN_HR`: the hit-chance floor.
pub const MIN_HR: i32 = 20;
/// The hit-chance ceiling.
pub const MAX_HR: i32 = 96;
/// `NULL_ID`: the legacy "none" id (`0xFFFFFFFF`).
pub const NULL_ID: u32 = 0xFFFF_FFFF;

// ---- DST aggregate (Plan/16 §4.1) -------------------------------------------------------------------------

/// `NULL_CHGPARAM`: "no override" in `chg` (`0x7FFFFFFF`).
pub const NULL_CHGPARAM: i32 = 0x7FFF_FFFF;

// ---- mode bits (`m_dwMode`, `authorization.h:20-60`, Plan/16 §2.5) -----------------------------------------

/// `MATCHLESS_MODE`.
pub const MATCHLESS_MODE: u32 = 0x0000_0001;
/// `TRANSPARENT_MODE`.
pub const TRANSPARENT_MODE: u32 = 0x0000_0002;
/// `ONEKILL_MODE`.
pub const ONEKILL_MODE: u32 = 0x0000_0004;
/// `DONMOVE_MODE`.
pub const DONMOVE_MODE: u32 = 0x0000_0008;
/// `SAYTALK_MODE`.
pub const SAYTALK_MODE: u32 = 0x0000_0010;
/// `MATCHLESS2_MODE`.
pub const MATCHLESS2_MODE: u32 = 0x0000_0020;
/// `NO_ATTACK_MODE`.
pub const NO_ATTACK_MODE: u32 = 0x0000_0040;
/// `ITEM_MODE`.
pub const ITEM_MODE: u32 = 0x0000_0080;
/// `COMMUNITY_MODE`.
pub const COMMUNITY_MODE: u32 = 0x0000_0100;
/// `TALK_MODE`.
pub const TALK_MODE: u32 = 0x0000_0200;
/// `SHOUTTALK_MODE`.
pub const SHOUTTALK_MODE: u32 = 0x0000_0400;
/// `RECOVERCHAO_MODE`.
pub const RECOVERCHAO_MODE: u32 = 0x0000_0800;
/// `FREEPK_MODE`.
pub const FREEPK_MODE: u32 = 0x0000_1000;
/// `PVPCONFIRM_MODE`.
pub const PVPCONFIRM_MODE: u32 = 0x0000_2000;
/// `QUERYSETPLAYERNAME_MODE`.
pub const QUERYSETPLAYERNAME_MODE: u32 = 0x0000_4000;
/// `MAILBOX_MODE`.
pub const MAILBOX_MODE: u32 = 0x0000_8000;
/// `EVENT_OLDBOY_MODE`.
pub const EVENT_OLDBOY_MODE: u32 = 0x0001_0000;
/// `EQUIP_DENIAL_MODE`.
pub const EQUIP_DENIAL_MODE: u32 = 0x0002_0000;
/// `EXPUP_STOP_MODE` (`features.exp_stop`).
pub const EXPUP_STOP_MODE: u32 = 0x0004_0000;
/// `GCWAR_NOT_CLICK_MODE`.
pub const GCWAR_NOT_CLICK_MODE: u32 = 0x0008_0000;
/// `GCWAR_RENDER_SKIP_MODE`.
pub const GCWAR_RENDER_SKIP_MODE: u32 = 0x0010_0000;
/// `QUIZ_RENDER_SKIP_MODE`.
pub const QUIZ_RENDER_SKIP_MODE: u32 = 0x0020_0000;
/// `OPTION_DONT_RENDER_MASK_MODE`.
pub const OPTION_DONT_RENDER_MASK_MODE: u32 = 0x0100_0000;
/// `FRESH_MODE`: cleared in the persisted value (Plan/13 §17.3).
pub const FRESH_MODE: u32 = 0x1000_0000;
/// `NOTFRESH_MODE`.
pub const NOTFRESH_MODE: u32 = 0x2000_0000;
/// `NOTFRESH2_MODE`.
pub const NOTFRESH2_MODE: u32 = 0x4000_0000;
/// `OUTOF_PARTYQUESTRGN_MODE`.
pub const OUTOF_PARTYQUESTRGN_MODE: u32 = 0x8000_0000;

/// `STATE_BASEMOTION_MODE` (`m_dwStateMode`).
pub const STATE_BASEMOTION_MODE: u32 = 0x0000_0004;
/// `OBJTYPE_FLYING` (`m_dwTypeFlag`).
pub const OBJTYPE_FLYING: u32 = 0x0001_0000;

// ---- SM premium timers (`_Network/CmnHdr.h:499-560`, Plan/16 §2.6) -----------------------------------------

/// Number of SM timers (`SM_MAX`).
pub const SM_MAX: usize = 26;
/// `SM_BANK15` (absolute).
pub const SM_BANK15: usize = 0;
/// `SM_BANK30` (absolute).
pub const SM_BANK30: usize = 1;
/// `SM_PARTYSKILL15` (absolute).
pub const SM_PARTYSKILL15: usize = 2;
/// `SM_PARTYSKILL30` (absolute).
pub const SM_PARTYSKILL30: usize = 3;
/// `SM_SHOUT15` (absolute).
pub const SM_SHOUT15: usize = 4;
/// `SM_SHOUT30` (absolute).
pub const SM_SHOUT30: usize = 5;
/// `SM_MAINTAIN_MP` (counter).
pub const SM_MAINTAIN_MP: usize = 6;
/// `SM_MAINTAIN_FP` (counter).
pub const SM_MAINTAIN_FP: usize = 7;
/// `SM_MAX_HP` (counter).
pub const SM_MAX_HP: usize = 8;
/// `SM_MAX_HP50` (counter).
pub const SM_MAX_HP50: usize = 9;
/// `SM_ACTPOINT` (counter).
pub const SM_ACTPOINT: usize = 10;
/// `SM_STR_DOWN` (dead).
pub const SM_STR_DOWN: usize = 11;
/// `SM_STA_DOWN` (dead).
pub const SM_STA_DOWN: usize = 12;
/// `SM_DEX_DOWN` (dead).
pub const SM_DEX_DOWN: usize = 13;
/// `SM_INT_DOWN` (dead).
pub const SM_INT_DOWN: usize = 14;
/// `SM_ATTACK_UP` (counter).
pub const SM_ATTACK_UP: usize = 15;
/// `SM_RESIST_ATTACK_LEFT` (counter).
pub const SM_RESIST_ATTACK_LEFT: usize = 16;
/// `SM_RESIST_ATTACK_RIGHT` (counter).
pub const SM_RESIST_ATTACK_RIGHT: usize = 17;
/// `SM_RESIST_DEFENSE` (counter).
pub const SM_RESIST_DEFENSE: usize = 18;
/// `SM_ESCAPE` (absolute).
pub const SM_ESCAPE: usize = 19;
/// `SM_REVIVAL` (flag, never decremented).
pub const SM_REVIVAL: usize = 20;
/// `SM_VELOCIJUMP` (counter).
pub const SM_VELOCIJUMP: usize = 21;
/// `SM_PARTYSKILL1` (absolute).
pub const SM_PARTYSKILL1: usize = 22;
/// `SM_ATTACK_UP1` (counter).
pub const SM_ATTACK_UP1: usize = 23;
/// `SM_SHOUT001` (absolute).
pub const SM_SHOUT001: usize = 24;
/// `SM_BANK001` (absolute).
pub const SM_BANK001: usize = 25;

/// The SM types whose value is an absolute `time_t` (the others are counters or flags); Plan/11 §5.8 turns
/// them into `expire_at` unix seconds in every message.
pub const SM_ABSOLUTE: [usize; 10] = [
    SM_BANK15,
    SM_BANK30,
    SM_PARTYSKILL15,
    SM_PARTYSKILL30,
    SM_SHOUT15,
    SM_SHOUT30,
    SM_ESCAPE,
    SM_PARTYSKILL1,
    SM_SHOUT001,
    SM_BANK001,
];

/// `true` when SM timer `sm` holds an absolute `time_t`.
pub fn sm_is_absolute(sm: usize) -> bool {
    SM_ABSOLUTE.contains(&sm)
}

// ---- timing (Plan/16 §2.7; the server ticks at 15 Hz, one tick = 4 sim frames) ------------------------------

/// `NEXT_TICK_RECOVERY`: regeneration period while sitting (ms).
pub const NEXT_TICK_RECOVERY_MS: u32 = 2000;
/// `NEXT_TICK_RECOVERYSTAND`: regeneration period while standing (ms).
pub const NEXT_TICK_RECOVERYSTAND_MS: u32 = 3000;
/// The attack-mode window: `0 < atk_cnt < ATTACK_MODE_TICKS` (`SEC1 * 10` ticks).
pub const ATTACK_MODE_TICKS: i32 = 150;
/// `SEC_SAVEPLAYER`: passes of the per-player 1 s timer between two saves / 90 s hooks.
pub const SEC_SAVEPLAYER: u32 = 90;
/// Revival invulnerability in ticks (doc 17).
pub const REVIVAL_INVULNERABLE_TICKS: i32 = 75;
/// `DST_HEAL` interval in ticks with `features.heal_over_time` (`PROCESS_COUNT * 3.0f`).
pub const HEAL_TICKS: i16 = 45;
/// `DST_HEAL` interval in ticks without `features.heal_over_time` (`PROCESS_COUNT * 6.0f`).
pub const HEAL_TICKS_SLOW: i16 = 90;
/// The fuel report period while flying and accelerating (`mv.count & 15 == 0`).
pub const FUEL_REPORT_MASK: u32 = 15;
