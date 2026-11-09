//! Records of the couple and campus group (migration `0004_relations.sql`, Plan/13 §9, 21 §11-§12).
//!
//! Pure data. The ops that write them are in [`crate::repo::relations`] (`DbOp::UpsertCouple`, ...). The game's
//! `CoupleService` / `CampusService` are the only writers of these tables.

use flyff_common::{CharacterId, UnixTime};
use serde::{Deserialize, Serialize};

/// One couple (`couples` plus its two `couple_members` rows).
///
/// `members` is `[first, second]` with the **higher character id first** (legacy `uspRestoreCouple`, ref/14 §11):
/// the member with the higher id sends the experience tick.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoupleRecord {
    /// `couples.id`, allocated from `meta.next_couple_id`.
    pub id: u32,
    /// Couple minutes (`nExperience`); level 21 is reached at 648.
    pub experience: i32,
    /// When the couple formed; never rewritten.
    pub created_at: UnixTime,
    /// `[first (higher id), second (lower id)]`.
    pub members: [CharacterId; 2],
}

impl CoupleRecord {
    /// A new couple with 0 experience; the higher id of `a` and `b` becomes "first".
    pub fn new(id: u32, a: CharacterId, b: CharacterId, created_at: UnixTime) -> Self {
        Self {
            id,
            experience: 0,
            created_at,
            members: if a >= b { [a, b] } else { [b, a] },
        }
    }

    /// The partner of `who` (`None` when `who` is not a member).
    pub fn partner_of(&self, who: CharacterId) -> Option<CharacterId> {
        let [first, second] = self.members;
        if who == first {
            Some(second)
        } else if who == second {
            Some(first)
        } else {
            None
        }
    }
}

/// One row of `couple_proposals`: the proposer may propose again after `cooldown_until` (with
/// `features.no_couple_cd` the cooldown is 0 s and no row exists).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoupleProposalRecord {
    /// The proposer (primary key).
    pub proposer_id: CharacterId,
    /// End of the cooldown.
    pub cooldown_until: UnixTime,
}

/// The role in a campus (`campus_members.role`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CampusRole {
    /// `CAMPUS_MASTER` = 1: at most one per campus.
    Master,
    /// `CAMPUS_PUPIL` = 2.
    Pupil,
}

impl CampusRole {
    /// The stored code.
    pub const fn code(self) -> u8 {
        match self {
            CampusRole::Master => 1,
            CampusRole::Pupil => 2,
        }
    }

    /// The role of a stored code (`1` or `2`).
    pub const fn from_code(code: u8) -> Option<CampusRole> {
        match code {
            1 => Some(CampusRole::Master),
            2 => Some(CampusRole::Pupil),
            _ => None,
        }
    }
}

/// One row of `campus_members`: a character belongs to at most one campus; a campus has at most one master.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CampusMemberRecord {
    /// The member (primary key).
    pub character_id: CharacterId,
    /// The campus.
    pub campus_id: u32,
    /// Master or pupil.
    pub role: CampusRole,
    /// When the member joined; never rewritten.
    pub joined_at: UnixTime,
}

/// One campus with its members (`StartupState.campuses`): the master first, then the pupils by join time.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CampusLoad {
    /// `campuses.id`, allocated from `meta.next_campus_id`.
    pub id: u32,
    /// When the campus was created.
    pub created_at: UnixTime,
    /// The members.
    pub members: Vec<CampusMemberRecord>,
}

impl CampusLoad {
    /// The master (`None` for a campus that lost it; the `CampusService` dissolves such a campus).
    pub fn master(&self) -> Option<&CampusMemberRecord> {
        self.members.iter().find(|m| m.role == CampusRole::Master)
    }
}
