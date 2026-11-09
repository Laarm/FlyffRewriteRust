//! The player extension of a mover (Plan/16 §3.3): `PlayerExt` and the sub-structs this document owns.
//!
//! Every field has exactly one location (16 §3.1). The structs of other owners embedded here are stand-ins in
//! their owners' module paths (`item`, `skill`, `quest`, `social`) until their work packages land.

use crate::input::Authority;
use crate::item::PlayerItems;
use crate::quest::QuestBook;
use crate::skill::SkillRuntime;
use crate::social::PlayerSocial;
use crate::world::PlayerView;
use flyff_common::math::Vec3;
use flyff_common::{AccountId, CharacterId, GameMs, ObjId, UnixTime, WorldId};
use flyff_db::model::docs::SkillSlot;
use flyff_db::{SettingsDoc, SkillsDoc, SmTimersDoc, TaskbarDoc};
use flyff_rules::consts::{MAX_SKILL_JOB, SM_MAX};

/// Character identity only (16 §3.3). Session data lives in the `SessionEntry` (10 §6.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayerIdent {
    /// `m_idPlayer`.
    pub character_id: CharacterId,
    /// Owning account.
    pub account_id: AccountId,
    /// Character slot `0..=2` (`m_nSlot`).
    pub slot: u8,
    /// `m_szName`.
    pub name: String,
    /// The effective authority resolved at join (23 §1; `m_dwAuthorization`).
    pub authority: Authority,
}

/// Hair, skin and the fashion-hide flags (16 §3.3).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Appearance {
    /// `m_bySex`: 0 male, 1 female.
    pub sex: u8,
    /// `m_dwSkinSet`.
    pub skin_set: u8,
    /// `m_dwHairMesh`.
    pub hair_mesh: u8,
    /// `m_dwHairColor` (ARGB).
    pub hair_color: u32,
    /// `m_dwHeadMesh`.
    pub head_mesh: u8,
    /// `m_abHideCoat[5]` (0 HAT, 1 CLOTH, 2 GLOVE, 3 BOOTS, 4 CLOAK).
    pub hide_coat: [bool; 5],
}

impl Appearance {
    /// `characters.hide_coat_mask`: bit `z` = `hide_coat[z]`.
    pub fn hide_coat_mask(&self) -> u8 {
        self.hide_coat
            .iter()
            .enumerate()
            .fold(0_u8, |m, (z, &h)| if h { m | (1 << z) } else { m })
    }

    /// The flags of a stored mask (bits above 4 are ignored).
    pub fn hide_coat_from_mask(mask: u8) -> [bool; 5] {
        std::array::from_fn(|z| mask & (1 << z) != 0)
    }
}

/// One slot of `m_aJobSkill` (`SKILL`, 8 bytes): defined once, in `flyff-rules` (the job and skill-point rules read
/// and write the same table in the browser and on the server).
pub use flyff_rules::jobs::JobSkill;

/// EXP, level bookkeeping, points and the job skills (16 §3.3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Progression {
    /// `m_nExp1`: the EXP inside the current level.
    pub exp: i64,
    /// `max(max_level, level)` as persisted.
    pub max_level: u16,
    /// `m_nDeathExp`.
    pub death_exp: i64,
    /// `m_nDeathLevel`.
    pub death_level: i32,
    /// `m_nRemainGP`: unspent stat points.
    pub stat_points: i32,
    /// `m_nSkillPoint`: unspent skill points.
    pub skill_points: i32,
    /// `m_nSkillLevel`: the skill points ever granted (the cap basis).
    pub skill_level_total: i32,
    /// `m_aJobSkill[45]`.
    pub job_skills: [JobSkill; MAX_SKILL_JOB],
    /// `m_nFxp`: the flight EXP counter.
    pub flight_exp: i32,
    /// `m_nTutorialState` (the legacy `m_nFlightLv` column re-use).
    pub tutorial_state: i32,
    /// `m_nFame`.
    pub fame: i32,
    /// `m_nAngelExp`.
    pub angel_exp: i64,
    /// `m_nAngelLevel`.
    pub angel_level: i32,
}

impl Default for Progression {
    fn default() -> Self {
        Progression {
            exp: 0,
            max_level: 1,
            death_exp: 0,
            death_level: 0,
            stat_points: 0,
            skill_points: 0,
            skill_level_total: 0,
            job_skills: [JobSkill::EMPTY; MAX_SKILL_JOB],
            flight_exp: 0,
            tutorial_state: 0,
            fame: 0,
            angel_exp: 0,
            angel_level: 0,
        }
    }
}

impl Progression {
    /// The skill slots of a `SkillsDoc` (`None` becomes the empty slot; surplus entries are ignored here, the
    /// document keeps them).
    pub fn job_skills_from_doc(doc: &SkillsDoc) -> [JobSkill; MAX_SKILL_JOB] {
        std::array::from_fn(|i| match doc.slots.get(i).copied().flatten() {
            Some(SkillSlot { skill_id, level }) => JobSkill { skill_id, level },
            None => JobSkill::EMPTY,
        })
    }

    /// The `skills` document of the slots (`NULL_ID` becomes `None`).
    pub fn skills_doc(&self) -> SkillsDoc {
        SkillsDoc {
            slots: self
                .job_skills
                .iter()
                .map(|s| {
                    (!s.is_empty()).then_some(SkillSlot {
                        skill_id: s.skill_id,
                        level: s.level,
                    })
                })
                .collect(),
            ..SkillsDoc::default()
        }
    }
}

/// The premium-item (SM) timers of a player (16 §3.3, §14.1).
///
/// Absolute kinds store the expiry as unix seconds, counter kinds the remaining seconds, `SM_REVIVAL` a flag.
/// The three resist values are bytes with `0xFF` = none, else the element id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SmState {
    /// `m_dwSMTime[SM_*]`.
    pub timers: [u32; SM_MAX],
    /// `m_nPlusMaxHitPoint` (grilled eel).
    pub plus_max_hp: i32,
    /// `m_nAttackResistLeft`.
    pub attack_resist_left: u8,
    /// `m_nAttackResistRight`.
    pub attack_resist_right: u8,
    /// `m_nDefenseResist`.
    pub defense_resist: u8,
    /// Changes of the timers since the services last looked (not persisted): the party-skill modes are the
    /// social services' business and the 1 s timer of the tick tail has no services (`sm::drain_events`).
    pub events: Vec<SmEvent>,
}

/// One change of an SM timer for the social services.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SmEvent {
    /// The `SM_*` slot.
    pub sm: u8,
    /// `true` when the timer was set, `false` when it ended.
    pub active: bool,
}

impl Default for SmState {
    fn default() -> Self {
        SmState {
            timers: [0; SM_MAX],
            plus_max_hp: 0,
            attack_resist_left: 0xFF,
            attack_resist_right: 0xFF,
            defense_resist: 0xFF,
            events: Vec::new(),
        }
    }
}

impl SmState {
    /// From the stored document (13 §6.10). An all-zero document is the "no row" case of the legacy schema:
    /// the resist bytes are then "none" (`0xFF`), as the legacy constructor set them. Out-of-range stored
    /// resist values saturate to a byte.
    pub fn from_doc(d: &SmTimersDoc) -> SmState {
        let all_zero = d.timers.iter().all(|&t| t == 0)
            && d.max_hp_bonus == 0
            && d.attack_resist_left == 0
            && d.attack_resist_right == 0
            && d.defense_resist == 0;
        if all_zero {
            return SmState::default();
        }
        let byte = |v: i32| u8::try_from(v.clamp(0, 255)).unwrap_or(0xFF);
        SmState {
            timers: d.timers,
            plus_max_hp: d.max_hp_bonus,
            attack_resist_left: byte(d.attack_resist_left),
            attack_resist_right: byte(d.attack_resist_right),
            defense_resist: byte(d.defense_resist),
            events: Vec::new(),
        }
    }

    /// The `sm_timers` document (the resist bytes are stored as `i32`, `0xFF` = none).
    pub fn to_doc(&self) -> SmTimersDoc {
        SmTimersDoc {
            timers: self.timers,
            max_hp_bonus: self.plus_max_hp,
            attack_resist_left: i32::from(self.attack_resist_left),
            attack_resist_right: i32::from(self.attack_resist_right),
            defense_resist: i32::from(self.defense_resist),
            ..SmTimersDoc::default()
        }
    }
}

/// Regeneration and per-second timers (16 §3.3, §6). All values are `GameMs`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RegenTimers {
    /// `m_dwTickRecovery`: the next sitting regeneration.
    pub next_sit_ms: GameMs,
    /// `m_dwTickRecoveryStand`: the next standing regeneration.
    pub next_stand_ms: GameMs,
    /// `m_dwTickSFS`: the next per-second tick (`+1000` grid).
    pub next_sfs_ms: GameMs,
    /// `m_dwTimeout4Save`: passes of the 1 s timer until the 90 s block.
    pub slow_countdown: u8,
}

impl RegenTimers {
    /// The timers of a player that enters the world at `now` (`CUser` constructor: `m_dwTickSFS = now + 1000`,
    /// `m_dwTimeout4Save = 90`; the regeneration ticks start one period ahead).
    pub fn at_join(now: GameMs) -> RegenTimers {
        RegenTimers {
            next_sit_ms: GameMs(now.0 + u64::from(flyff_rules::consts::NEXT_TICK_RECOVERY_MS)),
            next_stand_ms: GameMs(
                now.0 + u64::from(flyff_rules::consts::NEXT_TICK_RECOVERYSTAND_MS),
            ),
            next_sfs_ms: GameMs(now.0 + 1000),
            slow_countdown: flyff_rules::consts::SEC_SAVEPLAYER as u8,
        }
    }
}

/// Death-related flags: doc 17 writes, doc 16 reads (16 §3.3).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DeathFlags {
    /// `m_bLastPK`.
    pub last_pk: bool,
    /// `m_bGuildCombat`.
    pub guild_combat: bool,
    /// `m_bLastDuelParty`.
    pub last_duel_party: bool,
}

/// PK propensity fields (16 §16): stored here, rules in doc 17.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PkFields {
    /// `m_nPKValue`.
    pub value: i32,
    /// `m_dwPKPropensity`.
    pub propensity: u32,
    /// `m_dwPKExp`.
    pub pk_exp: u32,
}

/// Positions, counters and per-character settings without an owner doc of their own (16 §3.3 `misc`: no
/// session data).
#[derive(Clone, Debug, PartialEq)]
pub struct PlayerMisc {
    /// `return_world_id`: where the revival / return scroll leads.
    pub return_world: WorldId,
    /// `return_x/y/z`.
    pub return_pos: Vec3,
    /// The village chosen with `ReturnScrollReq` (`m_lpszVillage`): an index into
    /// `item::usage::blink::RETURN_VILLAGES`; runtime only, empty after a join.
    pub return_village: Option<u8>,
    /// `marking_world_id` (0 = no Lodelight mark).
    pub marking_world: WorldId,
    /// `marking_x/y/z`.
    pub marking_pos: Vec3,
    /// `mute_remaining_s` (the legacy `m_dwFlyTime` re-use).
    pub mute_remaining_s: u32,
    /// `messenger_state`.
    pub messenger_state: u32,
    /// Accumulated in-world seconds as loaded (doc 12 adds the time of the stay, 12 §12.6).
    pub play_time_s: i64,
    /// `attendance_bits`.
    pub attendance_bits: i64,
    /// `attendance_minutes`.
    pub attendance_minutes: u32,
    /// `attendance_elapsed`.
    pub attendance_elapsed: u32,
    /// `coupon_count`.
    pub coupon_count: i32,
    /// `rest_points`.
    pub rest_points: i32,
    /// `last_logout_at`.
    pub last_logout_at: UnixTime,
    /// The `settings` document (chat ignore list, privacy, client UI blob; 13 §6.12).
    pub settings: SettingsDoc,
    // ---- Event.lua extras (doc 22 §4.8-§4.9, task event-lua-extras): runtime only, never snapshotted ----
    /// `m_dwTickCoupon`: the game time of the last coupon (or of the join); the 1 s timer compares against it.
    pub coupon_tick: GameMs,
    /// `m_dwTickKeepConnect`: the game time of the last keep-connect mail (or of the join).
    pub keep_connect_tick: GameMs,
    // ---- rainbow race (doc 22 §14, task rainbow-race): runtime only, never snapshotted ----
    /// Mirror of `RainbowRaceService::is_entry` (Progress, a racer, not finished): the readers of the world phase
    /// without `Services` (blink wings, the escape wing, the tickets, Hero Return) see the teleport block through it.
    /// Never persisted.
    pub rainbow_entrant: bool,
    // ---- guild house rest points (doc 22 §12.7, task guild-house): runtime only, never snapshotted ----
    /// `m_tRestPointTick`: the game time at which the next rest-point increment is due inside the own guild house.
    pub rest_tick: GameMs,
    /// `m_nPrevRestPoint`: the value last sent as `GuildHouseRestPoint` (0 after a join and a re-attach).
    pub rest_sent: i32,
}

impl Default for PlayerMisc {
    fn default() -> Self {
        PlayerMisc {
            return_world: WorldId(1),
            return_pos: Vec3::ZERO,
            return_village: None,
            marking_world: WorldId(0),
            marking_pos: Vec3::ZERO,
            mute_remaining_s: 0,
            messenger_state: 0,
            play_time_s: 0,
            attendance_bits: 0,
            attendance_minutes: 0,
            attendance_elapsed: 0,
            coupon_count: 0,
            rest_points: 0,
            last_logout_at: UnixTime(0),
            settings: SettingsDoc::default(),
            coupon_tick: GameMs(0),
            keep_connect_tick: GameMs(0),
            rainbow_entrant: false,
            rest_tick: GameMs(0),
            rest_sent: 0,
        }
    }
}

/// The player extension of a [`crate::mover::Mover`] (16 §3.3): one field per owner doc.
#[derive(Clone, Debug)]
pub struct PlayerExt {
    // ---- owned by doc 16 ----
    /// Character identity.
    pub ident: PlayerIdent,
    /// Look.
    pub appearance: Appearance,
    /// EXP, points, job skills.
    pub progression: Progression,
    /// SM timers.
    pub sm: SmState,
    /// Regeneration and per-second timers.
    pub regen: RegenTimers,
    /// Death flags.
    pub death_flags: DeathFlags,
    /// PK fields.
    pub pk: PkFields,
    // ---- doc 15 types / doc 15 writes, stored here (15 §5.1, §11.2, §14.3) ----
    /// The non-player objects this player has spawned (`m_2npc`).
    pub view: PlayerView,
    /// `m_dwRegionAttr` (written by `update_region_attr`).
    pub region_attr: u32,
    /// `m_dwOldRegionAttr`.
    pub old_region_attr: u32,
    /// Exp-box / chest cool time: `(ctrl, until)` (15 §14.3).
    pub ctrl_ready: Option<(ObjId, GameMs)>,
    // ---- other owners (the type is the owner's) ----
    /// Inventory, banks, pockets, gold (19).
    pub items: PlayerItems,
    /// The trade partner slot (NPC shop window, player trade, stall) and the bank and stall flags (19 §7.2).
    pub econ: crate::econ::EconState,
    /// Skill cool-downs and action points (17).
    pub skills_rt: SkillRuntime,
    /// Quests (20).
    pub quests: QuestBook,
    /// The open NPC dialog and the script request stamp (20 §7.1).
    pub npc: crate::npc::PlayerNpc,
    /// Guild, party, honor (21).
    pub social: PlayerSocial,
    /// The taskbar document (42; 13 §6.8 kept as runtime form).
    pub taskbar: TaskbarDoc,
    /// Chat state: the anti-spam windows (23 §6.3). The mute countdown stays in `misc.mute_remaining_s`.
    pub chat: crate::chat::PlayerChat,
    /// Return and marking positions, mute, play time, attendance, rest points, settings.
    pub misc: PlayerMisc,
}
