//! Records of the guild-house, housing, rainbow-race and quiz group (migration `0007_houses_events.sql`, Plan/13
//! §12, 22 §12-§15).
//!
//! Pure serde data, no SQL. The furniture rows are items (an item consumed from an inventory becomes a row in the same
//! batch), the rainbow entries carry the application fee and the prizes, so the ops that write these records enforce
//! guards: pure functions in `crate::repo::houses` that the ops and the tests share (the legal edges are listed in the
//! crate's `NOTES.md`, section "Content command API").

use flyff_common::{CharacterId, GuildId, UnixTime, WorldId};
use serde::{Deserialize, Serialize};

/// Longest quiz question and longest single choice in bytes (`tblQuiz`, 22 §15.1).
pub const QUIZ_TEXT_MAX_BYTES: usize = 1024;

/// One row of `guild_houses` (`tblGuildHouse`, the small house only).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuildHouseRecord {
    /// The owning guild (primary key; the house is deleted with its guild).
    pub guild_id: GuildId,
    /// `WI_GUILDHOUSE_SMALL`, 212.
    pub world_id: WorldId,
    /// The end of the paid upkeep; `UnixTime(0)` = expired.
    pub upkeep_until: UnixTime,
    /// When the house was bought; never rewritten (a buy over an existing house is a `Conflict`).
    pub created_at: UnixTime,
}

/// One row of `guild_house_furniture` (`tblGuildHouse_Furniture`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct GuildHouseFurnitureRecord {
    /// The sequence number, allocated from `meta.next_guild_furniture_seq`; never 0.
    pub seq: u32,
    /// The owning guild; never rewritten.
    pub guild_id: GuildId,
    /// The item id of the furniture; never rewritten.
    pub item_id: u32,
    /// `true` while the piece stands in the house.
    pub installed: bool,
    /// The position in the house.
    pub pos: [f32; 3],
    /// The rotation.
    pub angle: f32,
    /// The end of the keep time; `UnixTime(0)` = the countdown has not started (it starts at the first setup).
    pub keep_until: UnixTime,
    /// When the piece was listed; never rewritten.
    pub listed_at: UnixTime,
    /// When the piece was last installed.
    pub installed_at: Option<UnixTime>,
}

impl GuildHouseFurnitureRecord {
    /// A freshly listed piece: not installed, no keep time, at the origin.
    pub fn listed(seq: u32, guild_id: GuildId, item_id: u32, listed_at: UnixTime) -> Self {
        Self {
            seq,
            guild_id,
            item_id,
            installed: false,
            pos: [0.0; 3],
            angle: 0.0,
            keep_until: UnixTime(0),
            listed_at,
            installed_at: None,
        }
    }
}

/// One row of `housing_furniture` (`tblHousing`): a piece of a character's mini room; an item id appears at most
/// once per owner.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct HousingFurnitureRecord {
    /// The owner.
    pub character_id: CharacterId,
    /// The item id (with the owner the primary key).
    pub item_id: u32,
    /// The end of the keep time; never rewritten (the server's keep time wins, 22 §13.2).
    pub keep_until: UnixTime,
    /// `true` while the piece stands in the room.
    pub installed: bool,
    /// The position in the room.
    pub pos: [f32; 3],
    /// The rotation.
    pub angle: f32,
    /// When the piece was listed; never rewritten.
    pub listed_at: UnixTime,
}

impl HousingFurnitureRecord {
    /// A freshly listed piece: not installed, at the origin.
    pub fn listed(
        character_id: CharacterId,
        item_id: u32,
        keep_until: UnixTime,
        listed_at: UnixTime,
    ) -> Self {
        Self {
            character_id,
            item_id,
            keep_until,
            installed: false,
            pos: [0.0; 3],
            angle: 0.0,
            listed_at,
        }
    }
}

/// `rainbow_race_entries.state`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RainbowEntryState {
    /// `N`: applied.
    Applied,
    /// `R`: finished, the rank is reserved.
    Finished,
    /// `W`: winner / final result of a finisher; terminal.
    Winner,
    /// `L`: lost (did not finish); terminal.
    Lost,
    /// `F`: failed / no-show (offline at the start, left during the race); terminal.
    Failed,
}

impl RainbowEntryState {
    /// The stored letter.
    pub const fn code(self) -> char {
        match self {
            RainbowEntryState::Applied => 'N',
            RainbowEntryState::Finished => 'R',
            RainbowEntryState::Winner => 'W',
            RainbowEntryState::Lost => 'L',
            RainbowEntryState::Failed => 'F',
        }
    }

    /// The stored letter as the text bound to the `state` column.
    pub const fn as_str(self) -> &'static str {
        match self {
            RainbowEntryState::Applied => "N",
            RainbowEntryState::Finished => "R",
            RainbowEntryState::Winner => "W",
            RainbowEntryState::Lost => "L",
            RainbowEntryState::Failed => "F",
        }
    }

    /// The state of a stored letter.
    pub const fn from_code(code: char) -> Option<RainbowEntryState> {
        match code {
            'N' => Some(RainbowEntryState::Applied),
            'R' => Some(RainbowEntryState::Finished),
            'W' => Some(RainbowEntryState::Winner),
            'L' => Some(RainbowEntryState::Lost),
            'F' => Some(RainbowEntryState::Failed),
            _ => None,
        }
    }

    /// The state of a stored text.
    pub fn parse(text: &str) -> Option<RainbowEntryState> {
        let mut chars = text.chars();
        let first = chars.next()?;
        if chars.next().is_some() {
            return None;
        }
        RainbowEntryState::from_code(first)
    }
}

/// One row of `rainbow_race_entries` (`RAINBOWRACE_TBL`): one row per round and character.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RainbowRaceEntryRecord {
    /// The round (`meta.rainbow_race_round`).
    pub round: u32,
    /// The racer.
    pub character_id: CharacterId,
    /// `N`, `R`, `W`, `L` or `F`.
    pub state: RainbowEntryState,
    /// The finishing rank (1-based) in `R` and `W`, else 0.
    pub ranking: i32,
    /// When the character applied (the order of the entries).
    pub applied_at: UnixTime,
    /// When the character finished.
    pub finished_at: Option<UnixTime>,
}

impl RainbowRaceEntryRecord {
    /// A new application (state `N`, no rank).
    pub fn apply(round: u32, character_id: CharacterId, applied_at: UnixTime) -> Self {
        Self {
            round,
            character_id,
            state: RainbowEntryState::Applied,
            ranking: 0,
            applied_at,
            finished_at: None,
        }
    }
}

/// `quiz_questions.quiz_type` (`TYPE_OX`, `TYPE_4C`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum QuizType {
    /// 1: true or false (answer 1 = O, 2 = X).
    Ox,
    /// 2: four choices (answer 1..=4).
    FourChoice,
}

impl QuizType {
    /// The stored code.
    pub const fn code(self) -> u8 {
        match self {
            QuizType::Ox => 1,
            QuizType::FourChoice => 2,
        }
    }

    /// The type of a stored code.
    pub const fn from_code(code: u8) -> Option<QuizType> {
        match code {
            1 => Some(QuizType::Ox),
            2 => Some(QuizType::FourChoice),
            _ => None,
        }
    }
}

/// One row of `quiz_questions` (`tblQuiz` + `tblQuizAnswer`): a question the operator imported. The shipped database
/// has none (PD-12).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuizQuestionRecord {
    /// The row id; `0` = a new question (the import looks the natural key `(quiz_type, question)` up).
    pub id: i64,
    /// OX or four choices.
    pub quiz_type: QuizType,
    /// The question text (1..=1024 bytes).
    pub question: String,
    /// The four choices of a four-choice question, none for OX.
    pub choices: Vec<String>,
    /// OX: 1 = O, 2 = X; four choices: 1..=4.
    pub answer: u8,
    /// The prize item (stored, unused by the original, 22 §15.1).
    pub prize_item_id: u32,
    /// The prize count (stored, unused).
    pub prize_count: u32,
    /// `m_chState 'T'`: only enabled questions are asked.
    pub enabled: bool,
    /// When the question was last asked (`MarkQuizQuestionsUsed`); the import keeps it.
    pub used_at: Option<UnixTime>,
}

impl QuizQuestionRecord {
    /// A new enabled OX question (`answer` 1 = O, 2 = X).
    pub fn ox(question: impl Into<String>, answer: u8) -> Self {
        Self {
            id: 0,
            quiz_type: QuizType::Ox,
            question: question.into(),
            choices: Vec::new(),
            answer,
            prize_item_id: 0,
            prize_count: 0,
            enabled: true,
            used_at: None,
        }
    }

    /// A new enabled four-choice question (`answer` 1..=4).
    pub fn four_choice(question: impl Into<String>, choices: [&str; 4], answer: u8) -> Self {
        Self {
            id: 0,
            quiz_type: QuizType::FourChoice,
            question: question.into(),
            choices: choices.iter().map(|c| (*c).to_owned()).collect(),
            answer,
            prize_item_id: 0,
            prize_count: 0,
            enabled: true,
            used_at: None,
        }
    }

    /// The limits of 22 §15.1: a question of 1..=1024 bytes; four-choice: exactly four non-empty choices of at most
    /// 1024 bytes each and an answer in 1..=4; OX: no choices and an answer of 1 or 2. The error text is meant for the
    /// operator (the TOML parser of `flyff quiz import` prefixes the line number). A negative `id` is refused too.
    pub fn validate(&self) -> Result<(), String> {
        if self.id < 0 {
            return Err(format!("the id {} is negative", self.id));
        }
        if self.question.is_empty() || self.question.len() > QUIZ_TEXT_MAX_BYTES {
            return Err(format!(
                "the question must be 1..={QUIZ_TEXT_MAX_BYTES} bytes, not {}",
                self.question.len()
            ));
        }
        match self.quiz_type {
            QuizType::Ox => {
                if !self.choices.is_empty() {
                    return Err("an OX question has no choices".to_owned());
                }
                if !(1..=2).contains(&self.answer) {
                    return Err(format!(
                        "the answer of an OX question is 1 (O) or 2 (X), not {}",
                        self.answer
                    ));
                }
            }
            QuizType::FourChoice => {
                if self.choices.len() != 4 {
                    return Err(format!(
                        "a four-choice question has exactly 4 choices, not {}",
                        self.choices.len()
                    ));
                }
                for (i, choice) in self.choices.iter().enumerate() {
                    if choice.is_empty() || choice.len() > QUIZ_TEXT_MAX_BYTES {
                        return Err(format!(
                            "choice {} must be 1..={QUIZ_TEXT_MAX_BYTES} bytes, not {}",
                            i + 1,
                            choice.len()
                        ));
                    }
                }
                if !(1..=4).contains(&self.answer) {
                    return Err(format!(
                        "the answer of a four-choice question is 1..=4, not {}",
                        self.answer
                    ));
                }
            }
        }
        Ok(())
    }
}

/// A guild house with its furniture (`StartupState.guild_houses`, 22 §12).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GuildHouseLoad {
    /// The house row.
    pub house: GuildHouseRecord,
    /// Its furniture in `seq` order (the list index of 22 §12).
    pub furniture: Vec<GuildHouseFurnitureRecord>,
}

/// What `CharacterLoad.housing` carries (22 §13): the rows of the character's own mini room and who may visit it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct HousingLoad {
    /// The raw furniture rows by `listed_at`, then item id. Expired rows are included: the game removes them at
    /// load (13 §17.4 step 7).
    pub items: Vec<HousingFurnitureRecord>,
    /// The characters allowed to enter, by character id.
    pub visitors: Vec<CharacterId>,
}

/// What the rainbow-race service loads at boot (`StartupState.rainbow_race`, 22 §14).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RainbowRaceLoad {
    /// `meta.rainbow_race_round` (0 on a fresh database).
    pub round: u32,
    /// The entries of that round, all states, by `applied_at` then character id.
    pub entries: Vec<RainbowRaceEntryRecord>,
    /// The previous ranking: the first (at most) five winners (`W`, ranking above 0) by ranking of the newest EARLIER
    /// round that has any entry row. A last race without a finisher shows an empty ranking (KEEP, legacy `S3` reads
    /// round `n - 1`); a round number raised without any row (no applicant) is passed over.
    pub prev_ranking: Vec<CharacterId>,
}
