//! The guild-war model (Plan/21 §6.1; WP-SOCIAL-15): the two sides, the ten results `WR_*` and the builders of the
//! wire message (`GuildWarInfo`) and of the database record (`GuildWarRecord`). Only RUNNING wars exist in memory;
//! an ended war is a database row.

use flyff_common::{GameMs, GuildId, GuildWarId, UnixTime};
use flyff_db::{GuildWarRecord, WarSideRecord};
use flyff_proto::pb;
use flyff_rules::social::WAR_DURATION_S;

/// `WF_WARTIME`: the flag of a running war on the wire (the ASCII value of `'0'`).
pub const FLAG_WARTIME: u32 = b'0' as u32;

/// The two sides of a war.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum WarRole {
    /// The guild that declared the war.
    Decl,
    /// The guild that accepted it.
    Acpt,
}

impl WarRole {
    /// Both roles, the declarer first (the order of every message).
    pub const BOTH: [WarRole; 2] = [WarRole::Decl, WarRole::Acpt];

    /// The other side.
    pub const fn other(self) -> WarRole {
        match self {
            WarRole::Decl => WarRole::Acpt,
            WarRole::Acpt => WarRole::Decl,
        }
    }

    /// The `bDecl` flag of the wire messages.
    pub const fn is_decl(self) -> bool {
        matches!(self, WarRole::Decl)
    }
}

/// How a war ended (`WR_*`, `guildwar.h:25-40`). The number is the wire value and the value stored in
/// `guild_wars.result` (FIX: the original stored the timeout results as `*_GN`). The winner is named first:
/// `DeclGn` = the declarer won because the acceptor's master died.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum WarResult {
    /// 0: the acceptor's master died, the declarer wins.
    DeclGn = 0,
    /// 1: the declarer's master died, the acceptor wins.
    AcptGn = 1,
    /// 2: the acceptor surrendered, the declarer wins.
    DeclSr = 2,
    /// 3: the declarer surrendered, the acceptor wins.
    AcptSr = 3,
    /// 4: timeout, the acceptor's master was absent longer, the declarer wins.
    DeclAb = 4,
    /// 5: timeout, the declarer's master was absent longer, the acceptor wins.
    AcptAb = 5,
    /// 6: timeout, the acceptor died more often, the declarer wins.
    DeclDd = 6,
    /// 7: timeout, the declarer died more often, the acceptor wins.
    AcptDd = 7,
    /// 8: both masters agreed to a truce; nothing changes.
    Truce = 8,
    /// 9: timeout with equal absence and deaths; nothing changes.
    Draw = 9,
}

impl WarResult {
    /// Every result in code order.
    pub const ALL: [WarResult; 10] = [
        WarResult::DeclGn,
        WarResult::AcptGn,
        WarResult::DeclSr,
        WarResult::AcptSr,
        WarResult::DeclAb,
        WarResult::AcptAb,
        WarResult::DeclDd,
        WarResult::AcptDd,
        WarResult::Truce,
        WarResult::Draw,
    ];

    /// The wire / database code `0..=9`.
    pub const fn code(self) -> u8 {
        self as u8
    }

    /// The result of a code; `None` above 9.
    pub fn from_code(code: u8) -> Option<WarResult> {
        WarResult::ALL.get(usize::from(code)).copied()
    }

    /// The side that won: `None` for a truce and a draw (no points change).
    pub const fn winner(self) -> Option<WarRole> {
        match self {
            WarResult::DeclGn | WarResult::DeclSr | WarResult::DeclAb | WarResult::DeclDd => {
                Some(WarRole::Decl)
            }
            WarResult::AcptGn | WarResult::AcptSr | WarResult::AcptAb | WarResult::AcptDd => {
                Some(WarRole::Acpt)
            }
            WarResult::Truce | WarResult::Draw => None,
        }
    }

    /// The result in which `winner` wins by the master's death (`WR_DECL_GN` / `WR_ACPT_GN`).
    pub const fn master_killed(winner: WarRole) -> WarResult {
        match winner {
            WarRole::Decl => WarResult::DeclGn,
            WarRole::Acpt => WarResult::AcptGn,
        }
    }

    /// The result in which `winner` wins because the other side surrendered.
    pub const fn surrender(winner: WarRole) -> WarResult {
        match winner {
            WarRole::Decl => WarResult::DeclSr,
            WarRole::Acpt => WarResult::AcptSr,
        }
    }
}

/// One side of a war (`WAR_ENTRY`, 21 §6.1). The numbers are `i32` like the wire message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WarSide {
    /// The guild.
    pub guild: GuildId,
    /// The member count at the start of the war (the base of the surrender ratio).
    pub size: i32,
    /// How many members surrendered.
    pub surrenders: i32,
    /// How many members (not the master: that ends the war) died.
    pub deaths: i32,
    /// The seconds the master was not online.
    pub absent_s: i32,
}

impl WarSide {
    /// A side at the start of a war.
    pub fn new(guild: GuildId, size: i32) -> WarSide {
        WarSide {
            guild,
            size,
            surrenders: 0,
            deaths: 0,
            absent_s: 0,
        }
    }

    /// The side of a stored record.
    pub fn from_record(guild: GuildId, r: &WarSideRecord) -> WarSide {
        WarSide {
            guild,
            size: r.members,
            surrenders: r.surrenders,
            deaths: r.deaths,
            absent_s: r.absent_s,
        }
    }

    /// The counters as stored.
    pub fn to_record(&self) -> WarSideRecord {
        WarSideRecord {
            members: self.size,
            deaths: self.deaths,
            surrenders: self.surrenders,
            absent_s: self.absent_s,
        }
    }

    /// `WAR_ENTRY` on the wire.
    pub fn wire(&self) -> pb::WarEntry {
        pb::WarEntry {
            id_guild: self.guild.0,
            size: self.size,
            surrender: self.surrenders,
            dead: self.deaths,
            absent: self.absent_s,
        }
    }
}

/// One running war (`CGuildWar`, 21 §6.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GuildWar {
    /// The id (`meta.next_guild_war_id`).
    pub id: GuildWarId,
    /// The declaring side.
    pub decl: WarSide,
    /// The accepting side.
    pub acpt: WarSide,
    /// When the war started: the 2 h clock runs from here, also across a restart.
    pub started_at: UnixTime,
    /// The game time of the last `UpsertGuildWar` of the counters (at most one per 10 s).
    pub last_persist: GameMs,
    /// A counter changed since the last write.
    pub dirty: bool,
}

impl GuildWar {
    /// A war that starts now.
    pub fn start(
        id: GuildWarId,
        decl: WarSide,
        acpt: WarSide,
        started_at: UnixTime,
        now: GameMs,
    ) -> GuildWar {
        GuildWar {
            id,
            decl,
            acpt,
            started_at,
            last_persist: now,
            dirty: false,
        }
    }

    /// The war of a stored running record (the guilds are checked by the caller).
    pub fn from_record(r: &GuildWarRecord) -> GuildWar {
        GuildWar {
            id: r.id,
            decl: WarSide::from_record(r.declarer, &r.decl),
            acpt: WarSide::from_record(r.acceptor, &r.acpt),
            started_at: r.started_at,
            last_persist: GameMs(0),
            dirty: false,
        }
    }

    /// The side `role`.
    pub fn side(&self, role: WarRole) -> &WarSide {
        match role {
            WarRole::Decl => &self.decl,
            WarRole::Acpt => &self.acpt,
        }
    }

    /// The side `role`, mutable.
    pub fn side_mut(&mut self, role: WarRole) -> &mut WarSide {
        match role {
            WarRole::Decl => &mut self.decl,
            WarRole::Acpt => &mut self.acpt,
        }
    }

    /// The role of `guild` in this war; `None` for a guild that does not fight in it.
    pub fn role_of(&self, guild: GuildId) -> Option<WarRole> {
        if self.decl.guild == guild {
            Some(WarRole::Decl)
        } else if self.acpt.guild == guild {
            Some(WarRole::Acpt)
        } else {
            None
        }
    }

    /// The guild of `role`.
    pub fn guild(&self, role: WarRole) -> GuildId {
        self.side(role).guild
    }

    /// The unix second from which the timeout rule applies (`started_at + 2 h`).
    pub fn ends_at(&self) -> i64 {
        self.started_at.0.saturating_add(WAR_DURATION_S)
    }

    /// `true` once `now_s` (wall-clock seconds) has reached [`GuildWar::ends_at`] (21 §6.3: `>=`, one second earlier
    /// than the original's strict `>`).
    pub fn timed_out(&self, now_s: i64) -> bool {
        now_s >= self.ends_at()
    }

    /// The result of the timeout rule (`OnWarTimeout`): the side whose master was absent longer loses; equal
    /// absence: the side with more deaths loses; equal: a draw.
    pub fn timeout_result(&self) -> WarResult {
        use std::cmp::Ordering;
        match self.decl.absent_s.cmp(&self.acpt.absent_s) {
            Ordering::Greater => WarResult::AcptAb,
            Ordering::Less => WarResult::DeclAb,
            Ordering::Equal => match self.decl.deaths.cmp(&self.acpt.deaths) {
                Ordering::Greater => WarResult::AcptDd,
                Ordering::Less => WarResult::DeclDd,
                Ordering::Equal => WarResult::Draw,
            },
        }
    }

    /// `CGuildWar::Serialize`: the war on the wire. Only running wars exist, so the flag is `'0'`.
    pub fn wire(&self) -> pb::GuildWarInfo {
        pb::GuildWarInfo {
            id_war: self.id.0,
            decl: Some(self.decl.wire()),
            acpt: Some(self.acpt.wire()),
            flag: FLAG_WARTIME,
            start_at: self.started_at.0,
        }
    }

    /// The running row (`result` and `ended_at` empty): the start and every counter write (the only builder).
    pub fn record(&self) -> GuildWarRecord {
        GuildWarRecord {
            id: self.id,
            declarer: self.decl.guild,
            acceptor: self.acpt.guild,
            started_at: self.started_at,
            ended_at: None,
            result: None,
            decl: self.decl.to_record(),
            acpt: self.acpt.to_record(),
            decl_points_after: None,
            acpt_points_after: None,
        }
    }

    /// The final row: the EXACT `WR_*` code (FIX), the end time, the final counters and the win points of both guilds
    /// after the war (`None` for a guild that no longer exists).
    pub fn ended_record(
        &self,
        result: WarResult,
        at: UnixTime,
        decl_points: Option<i32>,
        acpt_points: Option<i32>,
    ) -> GuildWarRecord {
        GuildWarRecord {
            ended_at: Some(at),
            result: Some(result.code()),
            decl_points_after: decl_points,
            acpt_points_after: acpt_points,
            ..self.record()
        }
    }
}
