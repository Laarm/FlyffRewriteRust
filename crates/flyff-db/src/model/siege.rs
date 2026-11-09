//! Records of the siege and GC 1:1 group (migration `0005_siege.sql`, Plan/13 §10, 21 §7-§8).
//!
//! Bids and refunds are money, so the ops that write these records enforce transition guards: pure functions in
//! `crate::repo::siege` that the ops and the tests share (the legal edges are listed in the crate's `NOTES.md`).

use flyff_common::{CharacterId, GuildId, UnixTime};
use serde::{Deserialize, Serialize};

/// `guild_sieges.status` (`tblCombatInfo.Status`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum SiegeStatus {
    /// 10: open for bids / applying.
    Open,
    /// 20: started.
    Started,
    /// 30: ended.
    Ended,
}

impl SiegeStatus {
    /// The stored code.
    pub const fn code(self) -> u8 {
        match self {
            SiegeStatus::Open => 10,
            SiegeStatus::Started => 20,
            SiegeStatus::Ended => 30,
        }
    }

    /// The status of a stored code.
    pub const fn from_code(code: u8) -> Option<SiegeStatus> {
        match code {
            10 => Some(SiegeStatus::Open),
            20 => Some(SiegeStatus::Started),
            30 => Some(SiegeStatus::Ended),
            _ => None,
        }
    }

    /// `true` when a row in status `self` may be rewritten with status `next`: only forward
    /// (`10 -> 20 -> 30`, `10 -> 30`) or unchanged.
    pub fn can_become(self, next: SiegeStatus) -> bool {
        next >= self
    }
}

/// `guild_siege_bids.status` (`tblCombatJoinGuild.Status`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BidStatus {
    /// 10: applied.
    Applied,
    /// 11: cancelled (the 80 % refund was mailed).
    Cancelled,
    /// 12: applied again after a cancel.
    Reapplied,
    /// 20: fighting.
    Fighting,
    /// 30: graded, reward / refund pending.
    Graded,
    /// 31: paid (or nothing to pay): terminal.
    Paid,
    /// 32: expired unclaimed: terminal.
    Expired,
}

impl BidStatus {
    /// The stored code.
    pub const fn code(self) -> u8 {
        match self {
            BidStatus::Applied => 10,
            BidStatus::Cancelled => 11,
            BidStatus::Reapplied => 12,
            BidStatus::Fighting => 20,
            BidStatus::Graded => 30,
            BidStatus::Paid => 31,
            BidStatus::Expired => 32,
        }
    }

    /// The status of a stored code.
    pub const fn from_code(code: u8) -> Option<BidStatus> {
        match code {
            10 => Some(BidStatus::Applied),
            11 => Some(BidStatus::Cancelled),
            12 => Some(BidStatus::Reapplied),
            20 => Some(BidStatus::Fighting),
            30 => Some(BidStatus::Graded),
            31 => Some(BidStatus::Paid),
            32 => Some(BidStatus::Expired),
            _ => None,
        }
    }

    /// `true` for the statuses in which the bid counts for the next siege (10, 12 and, after a restart-interrupted
    /// siege, 20).
    pub const fn is_active(self) -> bool {
        matches!(
            self,
            BidStatus::Applied | BidStatus::Reapplied | BidStatus::Fighting
        )
    }

    /// `true` for 31 and 32: nothing may change any more.
    pub const fn is_terminal(self) -> bool {
        matches!(self, BidStatus::Paid | BidStatus::Expired)
    }
}

/// `guild_siege_players.status` (`tblCombatJoinPlayer.Status`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PlayerRewardStatus {
    /// 30: graded, reward pending (the default).
    Pending,
    /// 31: paid: terminal.
    Paid,
    /// 32: expired unclaimed: terminal.
    Expired,
}

impl PlayerRewardStatus {
    /// The stored code.
    pub const fn code(self) -> u8 {
        match self {
            PlayerRewardStatus::Pending => 30,
            PlayerRewardStatus::Paid => 31,
            PlayerRewardStatus::Expired => 32,
        }
    }

    /// The status of a stored code.
    pub const fn from_code(code: u8) -> Option<PlayerRewardStatus> {
        match code {
            30 => Some(PlayerRewardStatus::Pending),
            31 => Some(PlayerRewardStatus::Paid),
            32 => Some(PlayerRewardStatus::Expired),
            _ => None,
        }
    }
}

/// `gc1to1_tenders.state` (21 §8.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TenderState {
    /// `T`: tendered, in the tender list.
    Tendered,
    /// `F`: failed (not selected at open); a 98 % refund can be claimed.
    Failed,
    /// `C`: cancelled by the master (80 % refunded). A cancelled guild may tender again (`C -> T`).
    Cancelled,
    /// `E`: ended, the guild fought and its arena closed.
    Ended,
    /// `N`: the failed refund was not claimed before the next open (forfeited).
    NotClaimed,
    /// `G`: the failed refund was given.
    Given,
}

impl TenderState {
    /// The stored letter.
    pub const fn code(self) -> char {
        match self {
            TenderState::Tendered => 'T',
            TenderState::Failed => 'F',
            TenderState::Cancelled => 'C',
            TenderState::Ended => 'E',
            TenderState::NotClaimed => 'N',
            TenderState::Given => 'G',
        }
    }

    /// The stored letter as the text bound to the `state` column.
    pub const fn as_str(self) -> &'static str {
        match self {
            TenderState::Tendered => "T",
            TenderState::Failed => "F",
            TenderState::Cancelled => "C",
            TenderState::Ended => "E",
            TenderState::NotClaimed => "N",
            TenderState::Given => "G",
        }
    }

    /// The state of a stored letter.
    pub const fn from_code(code: char) -> Option<TenderState> {
        match code {
            'T' => Some(TenderState::Tendered),
            'F' => Some(TenderState::Failed),
            'C' => Some(TenderState::Cancelled),
            'E' => Some(TenderState::Ended),
            'N' => Some(TenderState::NotClaimed),
            'G' => Some(TenderState::Given),
            _ => None,
        }
    }

    /// The state of a stored text.
    pub fn parse(text: &str) -> Option<TenderState> {
        let mut chars = text.chars();
        let first = chars.next()?;
        if chars.next().is_some() {
            return None;
        }
        TenderState::from_code(first)
    }
}

/// One row of `guild_sieges` (`tblCombatInfo`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuildSiegeRecord {
    /// `CombatID`, allocated from `meta.next_siege_id`.
    pub id: u32,
    /// Open (10), started (20) or ended (30); only forward.
    pub status: SiegeStatus,
    /// When the row was created; never rewritten.
    pub opened_at: UnixTime,
    /// When the siege started.
    pub started_at: Option<UnixTime>,
    /// When it ended.
    pub ended_at: Option<UnixTime>,
    /// The winner (status 30).
    pub winner_guild_id: Option<GuildId>,
    /// The winner's win streak (`StraightWin`).
    pub winner_streak: i32,
    /// The best player.
    pub best_player_id: Option<CharacterId>,
}

impl GuildSiegeRecord {
    /// A new siege row in status 10 (open for bids).
    pub fn open(id: u32, opened_at: UnixTime) -> Self {
        Self {
            id,
            status: SiegeStatus::Open,
            opened_at,
            started_at: None,
            ended_at: None,
            winner_guild_id: None,
            winner_streak: 0,
            best_player_id: None,
        }
    }
}

/// One row of `guild_siege_bids` (`tblCombatJoinGuild`). Bids and refunds are money: see
/// [`crate::repo::siege`] for the transition guards.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuildSiegeBidRecord {
    /// The siege.
    pub siege_id: u32,
    /// The bidding guild (no foreign key: history).
    pub guild_id: GuildId,
    /// The status.
    pub status: BidStatus,
    /// `CombatFee`: the bid in penya.
    pub bid: i64,
    /// `ReturnCombatFee`: what is refunded.
    pub refund: i64,
    /// The claimable reward.
    pub reward: i64,
    /// Points.
    pub points: i32,
    /// `StraightWin`.
    pub straight_wins: i32,
    /// When the bid was made (the ranking order of equal bids).
    pub applied_at: UnixTime,
    /// When it was cancelled.
    pub cancelled_at: Option<UnixTime>,
    /// When the reward was claimed (or expired).
    pub rewarded_at: Option<UnixTime>,
}

impl GuildSiegeBidRecord {
    /// A new bid (status 10) of `bid` penya.
    pub fn apply(siege_id: u32, guild_id: GuildId, bid: i64, applied_at: UnixTime) -> Self {
        Self {
            siege_id,
            guild_id,
            status: BidStatus::Applied,
            bid,
            refund: 0,
            reward: 0,
            points: 0,
            straight_wins: 0,
            applied_at,
            cancelled_at: None,
            rewarded_at: None,
        }
    }
}

/// One row of `guild_siege_players` (`tblCombatJoinPlayer`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuildSiegePlayerRecord {
    /// The siege.
    pub siege_id: u32,
    /// The player's guild.
    pub guild_id: GuildId,
    /// The player.
    pub character_id: CharacterId,
    /// Pending (30), paid (31) or expired (32).
    pub status: PlayerRewardStatus,
    /// The player's points in this siege.
    pub points: i32,
    /// The claimable reward.
    pub reward: i64,
    /// When the reward was claimed (or expired).
    pub rewarded_at: Option<UnixTime>,
}

/// One row of `gc1to1_tenders` (`GUILD_COMBAT_1TO1_TENDER_TBL`): one row per round and guild.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Gc1to1TenderRecord {
    /// The round (`meta.gc1to1_round`).
    pub round: u32,
    /// The tendering guild.
    pub guild_id: GuildId,
    /// The tender in penya (a re-tender raises it).
    pub penya: i64,
    /// `T`, `F`, `C`, `E`, `N` or `G`.
    pub state: TenderState,
    /// When the guild tendered.
    pub tendered_at: UnixTime,
}

/// One row of the cross-siege player ranking (`uspRankGuildCombatPlayer`): the sum of a character's points.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SiegeRankRow {
    /// The player.
    pub character_id: CharacterId,
    /// `characters.job`.
    pub job: u8,
    /// `SUM(points)` over all sieges.
    pub points: i64,
}

/// What the `SiegeService` loads at boot (`StartupState.sieges`, 21 §7.2).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SiegeLoad {
    /// The newest siege row of any status (`None` on a fresh database: the service creates the first one).
    pub current: Option<GuildSiegeRecord>,
    /// The bids of `current`, all statuses, in `applied_at` order.
    pub bids: Vec<GuildSiegeBidRecord>,
    /// The line-up rows of `current`.
    pub players: Vec<GuildSiegePlayerRecord>,
    /// Bids in status 30 of older sieges (reward / refund claimable). When `current` itself is ended its status-30
    /// rows are listed here as well as in `bids`.
    pub unpaid_bids: Vec<GuildSiegeBidRecord>,
    /// Player rows in status 30 with a reward, same rule.
    pub unpaid_players: Vec<GuildSiegePlayerRecord>,
    /// The newest siege in status 30: the last winner, his streak and the best player.
    pub last_ended: Option<GuildSiegeRecord>,
    /// The sum of points per player over all sieges, highest first, then by character id.
    pub ranking: Vec<SiegeRankRow>,
}

/// What the `Gc1to1Service` loads at boot (`StartupState.gc1to1`; the round counter is `counters.gc1to1_round`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Gc1to1Load {
    /// Every tender in state `T` or `F`, by round, then penya descending, then `tendered_at`.
    pub tenders: Vec<Gc1to1TenderRecord>,
    /// `(round, guild, members in fight order)` of the tenders above.
    pub lineups: Vec<(u32, GuildId, Vec<CharacterId>)>,
}
