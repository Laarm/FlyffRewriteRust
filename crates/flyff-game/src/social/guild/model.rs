//! The guild model (Plan/21 §5.1-§5.2; WP-SOCIAL-09): ranks, power bits, members, the `Guild` record the game keeps
//! in memory and the only builders of its database records.
//!
//! **One truth.** [`Guild`] is the state; `PlayerSocial.guild` / `.war` of the online members are mirrors kept by
//! `GuildService::sync_member`. The database records are built here and nowhere else ([`Guild::to_record`],
//! [`GuildMemberState::to_record`]) so that the guild, war, bank and salary code of different tasks write the same rows.
//!
//! **Level table.** `GameData.guild.levels` row `i` is level `i + 1` (already divided by `gameplay.guild_rate`): the
//! member limit of level `L` is row `L - 1`, the cost of the NEXT level is row `L` (the off-by-one trap of 21 §5.2).

use flyff_common::{CharacterId, GuildId, GuildWarId, UnixTime};
use flyff_data::GameData;
use flyff_db::model::{GUILD_ALIAS_MAX_BYTES, GUILD_MAX_LEVEL, GUILD_NOTICE_MAX_BYTES};
use flyff_db::{GuildBankLogTail, GuildLoad, GuildMemberRecord, GuildQuestRecord, GuildRecord};
use flyff_rules::social::guild_rank_cap;
use std::collections::BTreeMap;

/// The member limit used when the game data has no level table at all (tests, a broken `expTable.inc`): level 1 of
/// the shipped table.
pub const DEFAULT_MAX_MEMBERS: usize = 30;

/// The ranks of a guild (`GUD_MASTER` .. `GUD_ROOKIE`); the number is the wire value (`member_level`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GuildRank {
    /// 0, at most one.
    Master = 0,
    /// 1, at most 5.
    Kingpin = 1,
    /// 2, at most 10.
    Captain = 2,
    /// 3, at most 20.
    Supporter = 3,
    /// 4, as many as the guild level allows; the rank of a new member and of a demoted master.
    Rookie = 4,
}

impl GuildRank {
    /// Every rank, highest first.
    pub const ALL: [GuildRank; 5] = [
        GuildRank::Master,
        GuildRank::Kingpin,
        GuildRank::Captain,
        GuildRank::Supporter,
        GuildRank::Rookie,
    ];

    /// The rank of a wire / database value; `None` outside `0..=4`.
    pub const fn from_u8(v: u8) -> Option<GuildRank> {
        Some(match v {
            0 => GuildRank::Master,
            1 => GuildRank::Kingpin,
            2 => GuildRank::Captain,
            3 => GuildRank::Supporter,
            4 => GuildRank::Rookie,
            _ => return None,
        })
    }

    /// The rank of a signed wire value (`GuildMemberLevelReq.rank`); `None` outside `0..=4`.
    pub fn from_i32(v: i32) -> Option<GuildRank> {
        u8::try_from(v).ok().and_then(GuildRank::from_u8)
    }

    /// The wire / database value.
    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    /// The rank name of the `admin.guild` audit record (`web/src/admin/gen/dto.ts GuildRank`): `master`, `kingpin`,
    /// `captain`, `supporter`, `member`.
    pub const fn audit_name(self) -> &'static str {
        match self {
            GuildRank::Master => "master",
            GuildRank::Kingpin => "kingpin",
            GuildRank::Captain => "captain",
            GuildRank::Supporter => "supporter",
            GuildRank::Rookie => "member",
        }
    }

    /// The rank of an [`audit_name`](GuildRank::audit_name).
    pub fn from_audit_name(name: &str) -> Option<GuildRank> {
        GuildRank::ALL.into_iter().find(|r| r.audit_name() == name)
    }
}

/// The permission bits of a rank (`PF_*`, 21 §2.1). A plain `u32` newtype: the workspace has no `bitflags`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GuildPower(pub u32);

impl GuildPower {
    /// `PF_MEMBERLEVEL`: change ranks.
    pub const MEMBERLEVEL: GuildPower = GuildPower(0x01);
    /// `PF_LEVEL`: change classes.
    pub const LEVEL: GuildPower = GuildPower(0x02);
    /// `PF_INVITATION`: invite.
    pub const INVITATION: GuildPower = GuildPower(0x04);
    /// `PF_PENYA`: take penya out of the bank.
    pub const PENYA: GuildPower = GuildPower(0x08);
    /// `PF_ITEM`: take items out of the bank.
    pub const ITEM: GuildPower = GuildPower(0x10);
    /// `PF_GUILDHOUSE_FURNITURE`.
    pub const GH_FURNITURE: GuildPower = GuildPower(0x20);
    /// `PF_GUILDHOUSE_UPKEEP`.
    pub const GH_UPKEEP: GuildPower = GuildPower(0x40);

    /// The raw bits.
    pub const fn bits(self) -> u32 {
        self.0
    }

    /// `true` when `self` and `other` share a bit (`powers[rank] & p != 0`).
    pub const fn intersects(self, other: GuildPower) -> bool {
        self.0 & other.0 != 0
    }
}

/// One member (`CGuildMember`, 21 §5.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GuildMemberState {
    /// The character.
    pub id: CharacterId,
    /// The rank.
    pub rank: GuildRank,
    /// The class within the rank, `0..=2`.
    pub class: u8,
    /// The nickname (`""` = none), at most 12 bytes.
    pub alias: String,
    /// `m_nGiveGold`: penya contributed; a penya withdrawal subtracts, so it may go negative (KEEP). The database
    /// column is an `i64`; the wire and the original are `int`.
    pub give_gold: i32,
    /// `m_dwGivePxpCount`.
    pub give_pxp: u32,
    /// How often the member surrendered in a war (guild-war's counter).
    pub surrenders: i32,
    /// The war the member fights in (`None` = none or surrendered); the mirror is `PlayerSocial.war`.
    pub war_id: Option<GuildWarId>,
    /// When the member joined.
    pub joined_at: UnixTime,
}

impl GuildMemberState {
    /// A new member without contributions, nickname or war.
    pub fn new(id: CharacterId, rank: GuildRank, joined_at: UnixTime) -> GuildMemberState {
        GuildMemberState {
            id,
            rank,
            class: 0,
            alias: String::new(),
            give_gold: 0,
            give_pxp: 0,
            surrenders: 0,
            war_id: None,
            joined_at,
        }
    }

    /// The member of a stored row. Tolerant: an unknown rank becomes Rookie, a class above 2 is cut to 2, a
    /// contribution outside the `int` range is clamped (the database column is an `i64`).
    pub fn from_record(r: &GuildMemberRecord) -> GuildMemberState {
        GuildMemberState {
            id: r.character_id,
            rank: GuildRank::from_u8(r.rank).unwrap_or(GuildRank::Rookie),
            class: r.class.min(2),
            alias: cut_bytes(&r.alias, GUILD_ALIAS_MAX_BYTES),
            give_gold: i32::try_from(r.contributed_gold).unwrap_or(if r.contributed_gold < 0 {
                i32::MIN
            } else {
                i32::MAX
            }),
            give_pxp: r.contributed_pxp,
            surrenders: r.surrenders,
            war_id: (r.war_id != 0).then_some(GuildWarId(r.war_id)),
            joined_at: r.joined_at,
        }
    }

    /// The database row of this member in `guild` (the only builder; `UpsertGuildMember`).
    pub fn to_record(&self, guild: GuildId) -> GuildMemberRecord {
        GuildMemberRecord {
            character_id: self.id,
            guild_id: guild,
            rank: self.rank.as_u8(),
            class: self.class,
            alias: self.alias.clone(),
            contributed_gold: i64::from(self.give_gold),
            contributed_pxp: self.give_pxp,
            surrenders: self.surrenders,
            war_id: self.war_id.map_or(0, |w| w.0),
            joined_at: self.joined_at,
        }
    }
}

/// The 42-slot guild bank (guild-bank-chat's runtime container, [`super::bank`]).
pub use super::bank::GuildBank;

/// One guild (`CGuild`, 21 §5.1).
#[derive(Clone, Debug)]
pub struct Guild {
    /// The id (`meta.next_guild_id`).
    pub id: GuildId,
    /// The name, `""` until the master names the guild.
    pub name: String,
    /// Level `1..=50`.
    pub level: i32,
    /// `m_nGoldGuild`: the penya pool.
    pub gold: u32,
    /// `m_dwContributionPxp`.
    pub pxp: u32,
    /// The logo (0 none, 1..=27, 999 blank cloak).
    pub logo: u32,
    /// The notice, at most 127 bytes.
    pub notice: String,
    /// `m_adwPower[rank]`: the permission bits per rank, stored raw.
    pub powers: [u32; 5],
    /// `m_adwPenya[rank]`: the daily salary per rank.
    pub salaries: [u32; 5],
    /// War wins.
    pub war_wins: i32,
    /// War losses.
    pub war_losses: i32,
    /// `m_nSurrender`: stored and shown, never changed by the war code (KEEP).
    pub war_surrenders: i32,
    /// `m_nWinPoint`.
    pub war_points: i32,
    /// The running war (guild-war's, `None` = none).
    pub war: Option<GuildWarId>,
    /// The enemy guild of the running war.
    pub enemy: Option<GuildId>,
    /// The members by character id (the order of every message).
    pub members: BTreeMap<CharacterId, GuildMemberState>,
    /// The bank (guild-bank-chat).
    pub bank: GuildBank,
    /// The newest 100 log rows per listable kind (guild-bank-chat's views).
    pub log_tail: GuildBankLogTail,
    /// The live guild quests, sorted by quest id (the database load order; `quest::guild`, WP-NPC-16).
    pub quests: Vec<GuildQuestRecord>,
    /// Creation time.
    pub created_at: UnixTime,
}

impl Guild {
    /// A new guild: level 1, no gold or PXP, the founding powers `[0xFF, 0, 0, 0, 0]`, no salaries, no members.
    pub fn new(id: GuildId, name: impl Into<String>, created_at: UnixTime) -> Guild {
        let r = GuildRecord::new(id, name, created_at);
        Guild {
            id,
            name: r.name,
            level: r.level,
            gold: r.gold,
            pxp: r.pxp,
            logo: r.logo,
            notice: r.notice,
            powers: r.rank_powers,
            salaries: r.rank_salaries,
            war_wins: 0,
            war_losses: 0,
            war_surrenders: 0,
            war_points: 0,
            war: None,
            enemy: None,
            members: BTreeMap::new(),
            bank: GuildBank::empty(),
            log_tail: GuildBankLogTail::default(),
            quests: Vec::new(),
            created_at,
        }
    }

    /// The guild of a loaded row set. Tolerant: the level is clamped to `1..=50`, the notice cut to 127 bytes,
    /// members with an unknown rank become Rookies. The caller drops the members the directory does not know.
    pub fn from_load(l: &GuildLoad) -> Guild {
        let r = &l.guild;
        Guild {
            id: r.id,
            name: r.name.clone(),
            level: r.level.clamp(1, GUILD_MAX_LEVEL),
            gold: r.gold,
            pxp: r.pxp,
            logo: r.logo,
            notice: cut_bytes(&r.notice, GUILD_NOTICE_MAX_BYTES),
            powers: r.rank_powers,
            salaries: r.rank_salaries,
            war_wins: r.war_wins,
            war_losses: r.war_losses,
            war_surrenders: r.war_surrenders,
            war_points: r.war_points,
            war: None,
            enemy: None,
            members: l
                .members
                .iter()
                .map(|m| (m.character_id, GuildMemberState::from_record(m)))
                .collect(),
            bank: GuildBank::from_doc(l.bank.clone()),
            log_tail: l.log_tail.clone(),
            quests: {
                let mut quests = l.quests.clone();
                quests.sort_by_key(|q| q.quest_id);
                quests
            },
            created_at: r.created_at,
        }
    }

    /// The `guilds` row of this guild (the only builder; `UpsertGuild`). The bank is a separate document.
    pub fn to_record(&self) -> GuildRecord {
        GuildRecord {
            id: self.id,
            name: self.name.clone(),
            level: self.level,
            gold: self.gold,
            pxp: self.pxp,
            logo: self.logo,
            notice: self.notice.clone(),
            rank_powers: self.powers,
            rank_salaries: self.salaries,
            war_wins: self.war_wins,
            war_losses: self.war_losses,
            war_surrenders: self.war_surrenders,
            war_points: self.war_points,
            created_at: self.created_at,
        }
    }

    /// `true` once the master named the guild.
    pub fn is_named(&self) -> bool {
        !self.name.is_empty()
    }

    /// The master: the member of rank 0 (`None` for a guild whose master was purged; the guild is kept).
    pub fn master(&self) -> Option<CharacterId> {
        self.members
            .values()
            .find(|m| m.rank == GuildRank::Master)
            .map(|m| m.id)
    }

    /// The member `c`, if it belongs to this guild.
    pub fn member(&self, c: CharacterId) -> Option<&GuildMemberState> {
        self.members.get(&c)
    }

    /// How many members have `rank`.
    pub fn count_rank(&self, rank: GuildRank) -> usize {
        self.members.values().filter(|m| m.rank == rank).count()
    }

    /// `true` while a war runs (every roster and management action is refused then, 21 §5.5).
    pub fn at_war(&self) -> bool {
        self.war.is_some()
    }

    /// `CGuild::IsCmdCap` with the fix PL-092: the Master has every power even when `powers[0]` was zeroed; a
    /// non-member has none; every other rank needs a common bit of `powers[rank]`.
    pub fn has_power(&self, who: CharacterId, p: GuildPower) -> bool {
        match self.members.get(&who) {
            None => false,
            Some(m) if m.rank == GuildRank::Master => true,
            Some(m) => self
                .powers
                .get(usize::from(m.rank.as_u8()))
                .is_some_and(|bits| GuildPower(*bits).intersects(p)),
        }
    }

    /// The member limit of this guild (`GuildService::max_members` of the guild's own level).
    pub fn max_members(&self, data: &GameData) -> usize {
        max_members(data, self.level)
    }

    /// How many members `rank` may have in this guild (1 / 5 / 10 / 20 / the member limit).
    pub fn rank_cap(&self, data: &GameData, rank: GuildRank) -> usize {
        guild_rank_cap(rank.as_u8(), self.max_members(data))
    }

    /// `true` when no further member fits.
    pub fn is_full(&self, data: &GameData) -> bool {
        self.members.len() >= self.max_members(data)
    }
}

/// `max_members(level) = levels[level - 1].max_member` (21 §5.2); [`DEFAULT_MAX_MEMBERS`] without a table row.
pub fn max_members(data: &GameData, level: i32) -> usize {
    flyff_rules::social::guild_max_members(level, &data.guild.levels)
        .and_then(|n| usize::try_from(n).ok())
        .unwrap_or(DEFAULT_MAX_MEMBERS)
}

/// `s` cut to at most `max` bytes on a character boundary.
pub(crate) fn cut_bytes(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_owned();
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s.get(..end).unwrap_or_default().to_owned()
}
