//! Records of the guild group (migration `0003_guilds.sql`, Plan/13 §8, §16.3, §16.9; rules in Plan/21 §5, §6).
//!
//! Pure data (serde, no SQL). The ops that write them are in [`crate::repo::guilds`] (`DbOp::UpsertGuild`, ...), the
//! atomic batches in [`crate::batches_social`]. Every id is a `flyff_common` newtype; times are Unix seconds.
//!
//! The game thread is the only writer of guild state, so every record is a **whole row**: an upsert rewrites all
//! columns except the immutable ones (`created_at`, `joined_at`, a war's declarer, acceptor, start and member counts).

use super::container::RepairNote;
use super::docs::GuildBankDoc;
use flyff_common::{CharacterId, GuildId, GuildWarId, UnixTime};
use serde::{Deserialize, Serialize};

/// Ranks of a guild (`GUD_MASTER` .. `GUD_ROOKIE`): `rank` is `0..=4`.
pub const GUILD_RANKS: usize = 5;
/// `GUD_MASTER`.
pub const RANK_MASTER: u8 = 0;
/// `GUD_ROOKIE`: the rank of a new member and of a demoted master.
pub const RANK_ROOKIE: u8 = 4;
/// The highest guild level (Plan/21 §5.2).
pub const GUILD_MAX_LEVEL: i32 = 50;
/// The longest guild name in characters (UD-21; the column and 21 §5.4 say 16).
pub const GUILD_NAME_MAX_CHARS: usize = 16;
/// The longest guild notice in bytes (`m_szNotice` is 127 + NUL).
pub const GUILD_NOTICE_MAX_BYTES: usize = 127;
/// The largest logo number (1..=27 are the cloaks, 999 is the blank cloak).
pub const GUILD_LOGO_MAX: u32 = 999;
/// The longest member nickname in bytes (21 §5.5).
pub const GUILD_ALIAS_MAX_BYTES: usize = 12;
/// The permission bits of the founding master (`m_adwPower[GUD_MASTER] = 0xFF`, all other ranks 0).
pub const DEFAULT_RANK_POWERS: [u32; GUILD_RANKS] = [0xFF, 0, 0, 0, 0];
/// How many log rows per kind the in-memory tail keeps (`TOP 100`, 13 §8).
pub const GUILD_LOG_TAIL: usize = 100;
/// The last `WR_*` code of a war result (`WR_DRAW`).
pub const WAR_RESULT_MAX: u8 = 9;

/// One row of `guilds` (`GUILD_TBL`, Plan/13 §8, 21 §5.1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuildRecord {
    /// `guilds.id`, allocated from `meta.next_guild_id`.
    pub id: GuildId,
    /// The name, `""` until the guild is named. Unique case-insensitively among named guilds (PL-058).
    pub name: String,
    /// Level `1..=50`.
    pub level: i32,
    /// `m_nGoldGuild`: the guild's penya pool.
    pub gold: u32,
    /// `m_dwContributionPxp`.
    pub pxp: u32,
    /// Logo number (0 none, 1..=27, 999 blank cloak).
    pub logo: u32,
    /// The notice, at most [`GUILD_NOTICE_MAX_BYTES`] bytes.
    pub notice: String,
    /// `m_adwPower[rank]`: permission bits per rank.
    pub rank_powers: [u32; GUILD_RANKS],
    /// `m_adwPenya[rank]`: the daily salary per rank.
    pub rank_salaries: [u32; GUILD_RANKS],
    /// `m_nWin`.
    pub war_wins: i32,
    /// `m_nLose`.
    pub war_losses: i32,
    /// `m_nSurrender` (stored and loaded, never changed by the war code: KEEP).
    pub war_surrenders: i32,
    /// `m_nWinPoint`.
    pub war_points: i32,
    /// Creation time; never rewritten.
    pub created_at: UnixTime,
}

impl GuildRecord {
    /// A new guild at level 1 with the founding powers `[0xFF, 0, 0, 0, 0]` and no salaries. An empty `name` makes
    /// an unnamed guild.
    pub fn new(id: GuildId, name: impl Into<String>, created_at: UnixTime) -> Self {
        Self {
            id,
            name: name.into(),
            level: 1,
            gold: 0,
            pxp: 0,
            logo: 0,
            notice: String::new(),
            rank_powers: DEFAULT_RANK_POWERS,
            rank_salaries: [0; GUILD_RANKS],
            war_wins: 0,
            war_losses: 0,
            war_surrenders: 0,
            war_points: 0,
            created_at,
        }
    }
}

/// One row of `guild_members` (`GUILD_MEMBER_TBL`). A character is in at most one guild; a guild has at most one
/// member with rank 0 (the master).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuildMemberRecord {
    /// The member (primary key).
    pub character_id: CharacterId,
    /// The guild.
    pub guild_id: GuildId,
    /// `0..=4` (`GUD_MASTER` .. `GUD_ROOKIE`).
    pub rank: u8,
    /// Sub-grade within the rank, `0..=2`.
    pub class: u8,
    /// Nickname, at most [`GUILD_ALIAS_MAX_BYTES`] bytes (`""` = none).
    pub alias: String,
    /// `m_nGiveGold`: penya contributed; a penya withdrawal subtracts, so it **may go negative** (KEEP).
    pub contributed_gold: i64,
    /// `m_dwGivePxpCount`.
    pub contributed_pxp: u32,
    /// `m_nSurrender`: how often the member surrendered in a war.
    pub surrenders: i32,
    /// The war the member takes part in, `0` = none (or surrendered).
    pub war_id: u32,
    /// When the member joined; never rewritten.
    pub joined_at: UnixTime,
}

impl GuildMemberRecord {
    /// A new member of `guild` with `rank`, class 0, no nickname and no contributions.
    pub fn new(
        character_id: CharacterId,
        guild_id: GuildId,
        rank: u8,
        joined_at: UnixTime,
    ) -> Self {
        Self {
            character_id,
            guild_id,
            rank,
            class: 0,
            alias: String::new(),
            contributed_gold: 0,
            contributed_pxp: 0,
            surrenders: 0,
            war_id: 0,
            joined_at,
        }
    }
}

/// The kind of a `guild_bank_log` row (13 §8): the single letter stored in the `kind` column.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum GuildBankLogKind {
    /// `A`: an item was put into the bank.
    ItemPut,
    /// `D`: an item was taken out.
    ItemTaken,
    /// `I`: penya was contributed.
    PenyaIn,
    /// `O`: penya was taken out.
    PenyaOut,
    /// `C`: a guild cloak was created.
    Cloak,
    /// `P`: the daily salary was paid.
    Salary,
    /// `R`: a GM removed an item or penya.
    GmRemoval,
    /// `L`: a contribution that raised the guild level.
    ContributionLevelUp,
}

impl GuildBankLogKind {
    /// Every kind, in the order of the `CHECK` constraint.
    pub const ALL: [GuildBankLogKind; 8] = [
        GuildBankLogKind::ItemPut,
        GuildBankLogKind::ItemTaken,
        GuildBankLogKind::PenyaIn,
        GuildBankLogKind::PenyaOut,
        GuildBankLogKind::Cloak,
        GuildBankLogKind::Salary,
        GuildBankLogKind::GmRemoval,
        GuildBankLogKind::ContributionLevelUp,
    ];

    /// The stored letter.
    pub const fn code(self) -> char {
        match self {
            GuildBankLogKind::ItemPut => 'A',
            GuildBankLogKind::ItemTaken => 'D',
            GuildBankLogKind::PenyaIn => 'I',
            GuildBankLogKind::PenyaOut => 'O',
            GuildBankLogKind::Cloak => 'C',
            GuildBankLogKind::Salary => 'P',
            GuildBankLogKind::GmRemoval => 'R',
            GuildBankLogKind::ContributionLevelUp => 'L',
        }
    }

    /// The stored letter as the text bound to the `kind` column.
    pub const fn as_str(self) -> &'static str {
        match self {
            GuildBankLogKind::ItemPut => "A",
            GuildBankLogKind::ItemTaken => "D",
            GuildBankLogKind::PenyaIn => "I",
            GuildBankLogKind::PenyaOut => "O",
            GuildBankLogKind::Cloak => "C",
            GuildBankLogKind::Salary => "P",
            GuildBankLogKind::GmRemoval => "R",
            GuildBankLogKind::ContributionLevelUp => "L",
        }
    }

    /// The kind of a stored letter.
    pub const fn from_code(code: char) -> Option<GuildBankLogKind> {
        Some(match code {
            'A' => GuildBankLogKind::ItemPut,
            'D' => GuildBankLogKind::ItemTaken,
            'I' => GuildBankLogKind::PenyaIn,
            'O' => GuildBankLogKind::PenyaOut,
            'C' => GuildBankLogKind::Cloak,
            'P' => GuildBankLogKind::Salary,
            'R' => GuildBankLogKind::GmRemoval,
            'L' => GuildBankLogKind::ContributionLevelUp,
            _ => return None,
        })
    }

    /// The kind of a stored text (`"A"`..`"L"`).
    pub fn parse(text: &str) -> Option<GuildBankLogKind> {
        let mut chars = text.chars();
        let first = chars.next()?;
        if chars.next().is_some() {
            return None;
        }
        GuildBankLogKind::from_code(first)
    }

    /// The kind of a `GUILDLOG_VIEW` type: 1 -> `A`, 2 -> `D`, 3 -> `I`, 4 -> `O`; anything else has no view.
    pub const fn view_kind(view: u8) -> Option<GuildBankLogKind> {
        match view {
            1 => Some(GuildBankLogKind::ItemPut),
            2 => Some(GuildBankLogKind::ItemTaken),
            3 => Some(GuildBankLogKind::PenyaIn),
            4 => Some(GuildBankLogKind::PenyaOut),
            _ => None,
        }
    }

    /// The `GUILDLOG_VIEW` type of this kind (`None` for the kinds the client cannot list).
    pub const fn view(self) -> Option<u8> {
        match self {
            GuildBankLogKind::ItemPut => Some(1),
            GuildBankLogKind::ItemTaken => Some(2),
            GuildBankLogKind::PenyaIn => Some(3),
            GuildBankLogKind::PenyaOut => Some(4),
            _ => None,
        }
    }
}

/// One row of `guild_bank_log`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuildBankLogRecord {
    /// `guild_bank_log.id`; `0` in a record about to be inserted (`InsertGuildBankLog` returns the new id).
    pub id: i64,
    /// The guild.
    pub guild_id: GuildId,
    /// The acting character, `0` = the system (salary, GM removal); no foreign key.
    pub character_id: CharacterId,
    /// What happened.
    pub kind: GuildBankLogKind,
    /// The item (kinds `A`, `D`, `C`, `R`), else 0.
    pub item_id: u32,
    /// `m_nAbilityOption` of the item.
    pub upgrade_level: i32,
    /// Item count, or the penya amount for `I`, `O`, `P`, `L`.
    pub count: i64,
    /// When.
    pub at: UnixTime,
}

impl GuildBankLogRecord {
    /// A new log row (`id` 0) without item or amount.
    pub fn new(
        guild_id: GuildId,
        character_id: CharacterId,
        kind: GuildBankLogKind,
        at: UnixTime,
    ) -> Self {
        Self {
            id: 0,
            guild_id,
            character_id,
            kind,
            item_id: 0,
            upgrade_level: 0,
            count: 0,
            at,
        }
    }

    /// The item and its count.
    #[must_use]
    pub fn with_item(mut self, item_id: u32, upgrade_level: i32, count: i64) -> Self {
        self.item_id = item_id;
        self.upgrade_level = upgrade_level;
        self.count = count;
        self
    }

    /// A penya amount.
    #[must_use]
    pub fn with_penya(mut self, penya: i64) -> Self {
        self.count = penya;
        self
    }
}

/// The newest [`GUILD_LOG_TAIL`] log rows of the four listable kinds, newest first (what `GuildLogViewReq` shows;
/// the `GuildService` keeps one per guild and appends every row it writes).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuildBankLogTail {
    /// Kind `A` (view 1), newest first.
    pub put: Vec<GuildBankLogRecord>,
    /// Kind `D` (view 2).
    pub taken: Vec<GuildBankLogRecord>,
    /// Kind `I` (view 3).
    pub penya_in: Vec<GuildBankLogRecord>,
    /// Kind `O` (view 4).
    pub penya_out: Vec<GuildBankLogRecord>,
}

impl GuildBankLogTail {
    fn list_mut(&mut self, kind: GuildBankLogKind) -> Option<&mut Vec<GuildBankLogRecord>> {
        match kind {
            GuildBankLogKind::ItemPut => Some(&mut self.put),
            GuildBankLogKind::ItemTaken => Some(&mut self.taken),
            GuildBankLogKind::PenyaIn => Some(&mut self.penya_in),
            GuildBankLogKind::PenyaOut => Some(&mut self.penya_out),
            _ => None,
        }
    }

    /// The rows of one kind, newest first (empty for the kinds without a view).
    pub fn of_kind(&self, kind: GuildBankLogKind) -> &[GuildBankLogRecord] {
        match kind {
            GuildBankLogKind::ItemPut => &self.put,
            GuildBankLogKind::ItemTaken => &self.taken,
            GuildBankLogKind::PenyaIn => &self.penya_in,
            GuildBankLogKind::PenyaOut => &self.penya_out,
            _ => &[],
        }
    }

    /// The rows of a `GuildLogViewReq` type (1..=4), newest first.
    pub fn view(&self, view: u8) -> &[GuildBankLogRecord] {
        GuildBankLogKind::view_kind(view).map_or(&[], |kind| self.of_kind(kind))
    }

    /// Adds a row at the front of its list and keeps [`GUILD_LOG_TAIL`] rows. Returns `false` (and stores nothing)
    /// for a kind without a view (`C`, `P`, `R`, `L`).
    pub fn push(&mut self, record: GuildBankLogRecord) -> bool {
        let Some(list) = self.list_mut(record.kind) else {
            return false;
        };
        list.insert(0, record);
        list.truncate(GUILD_LOG_TAIL);
        true
    }
}

/// One side of a war (`WAR_ENTRY`, 21 §6.1).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WarSideRecord {
    /// Members at the start (immutable once stored).
    pub members: i32,
    /// `nDead`.
    pub deaths: i32,
    /// `nSurrender`.
    pub surrenders: i32,
    /// `nAbsent`: seconds the master was offline.
    pub absent_s: i32,
}

/// One row of `guild_wars` (`GUILD_WAR_TBL`). `result` is the exact `WR_*` code `0..=9` (FIX: the legacy collapsed
/// AB and DD into GN); `None` = running. A war with a result is final.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuildWarRecord {
    /// `guild_wars.id`, allocated from `meta.next_guild_war_id`.
    pub id: GuildWarId,
    /// The declaring guild (immutable; no foreign key, history survives a disband).
    pub declarer: GuildId,
    /// The accepting guild (immutable).
    pub acceptor: GuildId,
    /// Start time (immutable); the 2 h timeout continues from it after a restart.
    pub started_at: UnixTime,
    /// End time.
    pub ended_at: Option<UnixTime>,
    /// `WR_*` `0..=9`, `None` while the war runs.
    pub result: Option<u8>,
    /// The declarer's counters.
    pub decl: WarSideRecord,
    /// The acceptor's counters.
    pub acpt: WarSideRecord,
    /// The declarer's win points after the war.
    pub decl_points_after: Option<i32>,
    /// The acceptor's win points after the war.
    pub acpt_points_after: Option<i32>,
}

impl GuildWarRecord {
    /// A war that starts now with the member counts of both guilds.
    pub fn start(
        id: GuildWarId,
        declarer: GuildId,
        acceptor: GuildId,
        decl_members: i32,
        acpt_members: i32,
        started_at: UnixTime,
    ) -> Self {
        Self {
            id,
            declarer,
            acceptor,
            started_at,
            ended_at: None,
            result: None,
            decl: WarSideRecord {
                members: decl_members,
                ..WarSideRecord::default()
            },
            acpt: WarSideRecord {
                members: acpt_members,
                ..WarSideRecord::default()
            },
            decl_points_after: None,
            acpt_points_after: None,
        }
    }

    /// `true` while no result is stored.
    pub fn is_running(&self) -> bool {
        self.result.is_none()
    }
}

/// One row of `guild_quests` (`GUILD_QUEST_TBL`; empty with the shipped data, F-253).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuildQuestRecord {
    /// The guild.
    pub guild_id: GuildId,
    /// The quest (FIX: the primary key is per guild **and** quest).
    pub quest_id: u16,
    /// `nState`.
    pub state: i32,
    /// When it started; never rewritten.
    pub started_at: UnixTime,
    /// When it ended.
    pub ended_at: Option<UnixTime>,
}

/// Everything the `GuildService` loads of one guild at boot (`StartupState.guilds`, Plan/13 §16.9).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GuildLoad {
    /// The guild row.
    pub guild: GuildRecord,
    /// The roster ordered by rank, join time and character id; the master (rank 0) comes first.
    pub members: Vec<GuildMemberRecord>,
    /// The 42-slot bank, repaired with [`GuildBankDoc::validate_and_repair`]; a guild without a `guild_banks` row
    /// gets an empty bank.
    pub bank: GuildBankDoc,
    /// What the repair of the bank had to do (an `Orphaned` item must not be dropped silently).
    pub repairs: Vec<RepairNote>,
    /// The newest 100 log rows of the kinds `A`, `D`, `I`, `O`.
    pub log_tail: GuildBankLogTail,
    /// The guild quests (empty with the shipped data).
    pub quests: Vec<GuildQuestRecord>,
}

impl GuildLoad {
    /// The master: the member with rank 0 (`None` for a guild whose master was purged; logged at load).
    pub fn master(&self) -> Option<&GuildMemberRecord> {
        self.members.iter().find(|m| m.rank == RANK_MASTER)
    }
}
