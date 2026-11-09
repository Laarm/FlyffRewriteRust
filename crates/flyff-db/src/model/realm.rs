//! Records of the lord, tax and secret-room group (migration `0006_realm.sql`, Plan/13 §11, 22 §9-§11).
//!
//! Pure serde data, no SQL. The elections, deposits, tenders and refunds are money, so the ops that write these
//! records enforce transition guards: pure functions in `crate::repo::realm` that the ops and the tests share (the
//! legal edges are listed in the crate's `NOTES.md`, section "Content command API").

use flyff_common::{CharacterId, GuildId, UnixTime, WorldId};
use serde::{Deserialize, Serialize};

/// Longest lord pledge in bytes (`tblLordCandidates.Pledge`, 22 §9.1).
pub const LORD_PLEDGE_MAX_BYTES: usize = 255;

/// `lord_elections.state` (`CLordElection::State`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ElectionState {
    /// 0, `eReady`: prepared, the candidacy starts at `begin_at`.
    Ready,
    /// 1, `eCandidacy`: candidates register and raise their deposits.
    Candidacy,
    /// 2, `eVote`: the electors vote for the top candidates.
    Vote,
    /// 3, `eExpired`: ended; terminal.
    Expired,
}

impl ElectionState {
    /// The stored code.
    pub const fn code(self) -> u8 {
        match self {
            ElectionState::Ready => 0,
            ElectionState::Candidacy => 1,
            ElectionState::Vote => 2,
            ElectionState::Expired => 3,
        }
    }

    /// The state of a stored code.
    pub const fn from_code(code: u8) -> Option<ElectionState> {
        match code {
            0 => Some(ElectionState::Ready),
            1 => Some(ElectionState::Candidacy),
            2 => Some(ElectionState::Vote),
            3 => Some(ElectionState::Expired),
            _ => None,
        }
    }
}

/// One row of `lord_elections` (`tblLordElection` + `tblLord`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LordElectionRecord {
    /// `idElection`, allocated from `meta.next_lord_election_id`.
    pub id: u32,
    /// Ready, candidacy, vote or expired; only forward, expired is terminal.
    pub state: ElectionState,
    /// Start of the candidacy; `UnixTime(0)` = not scheduled.
    pub begin_at: UnixTime,
    /// The vote requirement (computed by the lord service).
    pub requirement: i32,
    /// The winner, only in state 3 (`None` = nobody was elected that term).
    pub lord_id: Option<CharacterId>,
    /// When the vote ended, only in state 3.
    pub ended_at: Option<UnixTime>,
}

impl LordElectionRecord {
    /// A prepared election (`PrepareNext`): state 0, no requirement, no winner.
    pub fn ready(id: u32, begin_at: UnixTime) -> Self {
        Self {
            id,
            state: ElectionState::Ready,
            begin_at,
            requirement: 0,
            lord_id: None,
            ended_at: None,
        }
    }
}

/// One row of `lord_candidates` (`tblLordCandidates`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LordCandidateRecord {
    /// The election (no row of a purged character is ever deleted: history).
    pub election_id: u32,
    /// The candidate.
    pub character_id: CharacterId,
    /// The deposit in penya; only grows.
    pub deposit: i64,
    /// The votes; only grow.
    pub votes: i32,
    /// The pledge, at most [`LORD_PLEDGE_MAX_BYTES`] bytes.
    pub pledge: String,
    /// `IsLeftOut 'D'`: cut at the vote start (rank over 10); the deposit was refunded; terminal.
    pub left_out: bool,
    /// `tCreate`: the tie-break of equal votes; never rewritten.
    pub created_at: UnixTime,
}

impl LordCandidateRecord {
    /// A new candidate with `deposit` penya, no votes and no pledge.
    pub fn new(
        election_id: u32,
        character_id: CharacterId,
        deposit: i64,
        created_at: UnixTime,
    ) -> Self {
        Self {
            election_id,
            character_id,
            deposit,
            votes: 0,
            pledge: String::new(),
            left_out: false,
            created_at,
        }
    }
}

/// One row of `lord_votes` (`tblLordElector`): one vote per elector and election.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LordVoteRecord {
    /// The election.
    pub election_id: u32,
    /// The elector.
    pub elector_id: CharacterId,
    /// When he voted.
    pub voted_at: UnixTime,
}

/// One row of `lord_events` (`tblLordEvent`): a running global EXP / drop boost, one per creator.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct LordEventRecord {
    /// The creator (primary key).
    pub character_id: CharacterId,
    /// Minutes left (starts at 60); only goes down, a row reaching 0 is deleted by the lord service.
    pub ticks_left: i32,
    /// The EXP factor; never rewritten.
    pub exp_factor: f32,
    /// The drop factor; never rewritten.
    pub item_factor: f32,
    /// When the event was created; never rewritten.
    pub created_at: UnixTime,
}

/// `tax_owners.continent` (`CONT_*` of `Continent.h`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum TaxContinent {
    /// 15, `CONT_EAST` (Flaris, Saint Morning, Ricis): owned by a guild.
    East,
    /// 240, `CONT_WEST` (Darkon): owned by a guild.
    West,
    /// 255, `CONT_ALL`: the lord's share, owned by a character.
    All,
}

impl TaxContinent {
    /// The stored code.
    pub const fn code(self) -> u8 {
        match self {
            TaxContinent::East => 15,
            TaxContinent::West => 240,
            TaxContinent::All => 255,
        }
    }

    /// The continent of a stored code.
    pub const fn from_code(code: u8) -> Option<TaxContinent> {
        match code {
            15 => Some(TaxContinent::East),
            240 => Some(TaxContinent::West),
            255 => Some(TaxContinent::All),
            _ => None,
        }
    }

    /// `true` for the two sides of the secret room (East and West).
    pub const fn is_side(self) -> bool {
        matches!(self, TaxContinent::East | TaxContinent::West)
    }
}

/// `tax_details.kind` (`TAX_SALES`, `TAX_PURCHASE`, `TAX_ADMISSION`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum TaxKind {
    /// 0: tax on sales to NPC shops.
    Sales,
    /// 1: tax on purchases from NPC shops.
    Purchase,
    /// 2: admission (the Heaven Tower); not for the West.
    Admission,
}

impl TaxKind {
    /// The stored code.
    pub const fn code(self) -> u8 {
        match self {
            TaxKind::Sales => 0,
            TaxKind::Purchase => 1,
            TaxKind::Admission => 2,
        }
    }

    /// The kind of a stored code.
    pub const fn from_code(code: u8) -> Option<TaxKind> {
        match code {
            0 => Some(TaxKind::Sales),
            1 => Some(TaxKind::Purchase),
            2 => Some(TaxKind::Admission),
            _ => None,
        }
    }
}

/// One row of `tax_owners` (`TAX_INFO_TBL`): who owns a continent in a round and who will next.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaxOwnerRecord {
    /// The round (`meta.tax_round`).
    pub round: u32,
    /// East, West or All.
    pub continent: TaxContinent,
    /// The owner: a guild id (East, West) or a character id (All); `None` = no owner.
    pub owner_id: Option<u32>,
    /// The owner from the next weekly change on (the secret-room winner, or the lord for All).
    pub next_owner_id: Option<u32>,
    /// `bSetTaxRate`: the next owner has chosen the rates.
    pub rate_set: bool,
    /// When this round started.
    pub changed_at: UnixTime,
}

/// One row of `tax_details` (`TAX_DETAIL_TBL`): rates and the counters of one kind on one continent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaxDetailRecord {
    /// The round.
    pub round: u32,
    /// East, West or All.
    pub continent: TaxContinent,
    /// Sales, purchase or admission.
    pub kind: TaxKind,
    /// The rate in percent (0..=100).
    pub rate: i32,
    /// The rate from the next weekly change on (0..=100).
    pub next_rate: i32,
    /// How many taxed transactions (`int`).
    pub tx_count: i32,
    /// The collected penya below one Perin (`int`).
    pub gold: i32,
    /// The collected Perins (`int`).
    pub perin: i32,
}

/// `secret_room_tenders.state`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SecretRoomTenderState {
    /// `T`: tendered, in the tender list.
    Tendered,
    /// `C`: cancelled by the master (80 % refunded). A cancelled guild may tender again (`C -> T`).
    Cancelled,
    /// `F`: failed the ranking at the open (98 % refunded by mail). It may tender again (`F -> T`).
    Failed,
    /// `W`: won the war.
    Won,
    /// `L`: lost the war.
    Lost,
}

impl SecretRoomTenderState {
    /// The stored letter.
    pub const fn code(self) -> char {
        match self {
            SecretRoomTenderState::Tendered => 'T',
            SecretRoomTenderState::Cancelled => 'C',
            SecretRoomTenderState::Failed => 'F',
            SecretRoomTenderState::Won => 'W',
            SecretRoomTenderState::Lost => 'L',
        }
    }

    /// The stored letter as the text bound to the `state` column.
    pub const fn as_str(self) -> &'static str {
        match self {
            SecretRoomTenderState::Tendered => "T",
            SecretRoomTenderState::Cancelled => "C",
            SecretRoomTenderState::Failed => "F",
            SecretRoomTenderState::Won => "W",
            SecretRoomTenderState::Lost => "L",
        }
    }

    /// The state of a stored letter.
    pub const fn from_code(code: char) -> Option<SecretRoomTenderState> {
        match code {
            'T' => Some(SecretRoomTenderState::Tendered),
            'C' => Some(SecretRoomTenderState::Cancelled),
            'F' => Some(SecretRoomTenderState::Failed),
            'W' => Some(SecretRoomTenderState::Won),
            'L' => Some(SecretRoomTenderState::Lost),
            _ => None,
        }
    }

    /// The state of a stored text.
    pub fn parse(text: &str) -> Option<SecretRoomTenderState> {
        let mut chars = text.chars();
        let first = chars.next()?;
        if chars.next().is_some() {
            return None;
        }
        SecretRoomTenderState::from_code(first)
    }
}

/// One row of `secret_room_tenders` (`SECRET_ROOM_TBL`): one row per round, side and guild.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretRoomTenderRecord {
    /// The round (`meta.secret_room_round`).
    pub round: u32,
    /// The side: East or West (`All` is refused).
    pub continent: TaxContinent,
    /// The tendering guild (no foreign key: history survives a disband).
    pub guild_id: GuildId,
    /// The tender in penya (a raise stores the new total).
    pub penya: i64,
    /// `T`, `C`, `F`, `W` or `L`.
    pub state: SecretRoomTenderState,
    /// The room world of the war; `WorldId(0)` = none.
    pub world_id: WorldId,
    /// The war state (`SrWarState` code of the secret-room service).
    pub war_state: i32,
    /// The kill counter.
    pub kills: i32,
    /// When the guild tendered (the order of equal tenders).
    pub tendered_at: UnixTime,
}

impl SecretRoomTenderRecord {
    /// A new tender (state `T`) of `penya` on `continent`.
    pub fn tender(
        round: u32,
        continent: TaxContinent,
        guild_id: GuildId,
        penya: i64,
        tendered_at: UnixTime,
    ) -> Self {
        Self {
            round,
            continent,
            guild_id,
            penya,
            state: SecretRoomTenderState::Tendered,
            world_id: WorldId(0),
            war_state: 0,
            kills: 0,
            tendered_at,
        }
    }
}

/// The stored line-up of a guild (`secret_room_lineups`): the members it sends into the war. The order is not
/// stored; the members load in character-id order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretRoomLineup {
    /// The side (East or West); its `code()` is the `continent` of `DbOp::ReplaceSecretRoomLineup`.
    pub continent: TaxContinent,
    /// The guild.
    pub guild_id: GuildId,
    /// The members.
    pub members: Vec<CharacterId>,
}

/// What the lord service loads at boot (`StartupState.lord`, 22 §9).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LordLoad {
    /// The newest election row by id, any state (`None` on a fresh database: the service creates the first one).
    pub election: Option<LordElectionRecord>,
    /// The candidates of that election, left-out ones included, by `created_at` then character id.
    pub candidates: Vec<LordCandidateRecord>,
    /// The electors of that election who already voted, by `voted_at` then character id.
    pub voters: Vec<CharacterId>,
    /// The current lord: `lord_id` of the newest election in state 3; `None` when there is none or it had no
    /// winner (legacy `uspRestoreLord`).
    pub lord: Option<CharacterId>,
    /// The running lord events, by `created_at` then creator (the order of the f32 sum of 22 §9.5).
    pub events: Vec<LordEventRecord>,
    /// `(skill_id, cooldown_min)` of every stored skill, by skill id.
    pub skill_cooldowns: Vec<(u32, i32)>,
}

/// What the tax service loads at boot (`StartupState.tax`, 22 §10): the rows of the current round only.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaxLoad {
    /// `meta.tax_round` (0 on a fresh database).
    pub round: u32,
    /// The owner rows of that round, by continent code.
    pub owners: Vec<TaxOwnerRecord>,
    /// The detail rows of that round, by continent code then kind.
    pub details: Vec<TaxDetailRecord>,
}

/// What the secret-room service loads at boot (`StartupState.secret_room`, 22 §11): the tenders still in state
/// `T` of the current round with their line-ups. Cancelled, failed, won and lost rows and older rounds are
/// history and never loaded (a war in progress does not survive a restart, KEEP).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretRoomLoad {
    /// `meta.secret_room_round` (0 on a fresh database).
    pub round: u32,
    /// The `T` tenders: by continent code, `penya` descending, `tendered_at`, guild id.
    pub tenders: Vec<SecretRoomTenderRecord>,
    /// The stored line-ups of those tenders, by continent code then guild id.
    pub lineups: Vec<SecretRoomLineup>,
}
