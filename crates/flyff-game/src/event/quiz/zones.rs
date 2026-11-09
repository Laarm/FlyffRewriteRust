//! The geometry of the quiz world (Plan/22 §15.2; `Quiz.cpp:681-720` `InitQuizData` / `GetZoneType`,
//! `:226-293` `TeleportToQuizEvent` / `GetTeleportPos`).
//!
//! World `WI_WORLD_QUIZ` 211, layer 0. Every rect is `[left, right) x [top, bottom)` on `(x as i32, z as i32)`
//! (`CRect::PtInRect` over the truncated position). The jitter of a teleport is the KEEP-QUIRK `xrandom(4) - 2` for x
//! and again for z (asymmetric: -2..=1).

use flyff_common::defs::world::{WI_WORLD_MADRIGAL, WI_WORLD_QUIZ};
use flyff_common::math::Vec3;
use flyff_common::rng::GameRng;
use flyff_common::{LayerId, WorldId};
use flyff_db::QuizType;

/// The quiz world.
pub const WORLD: WorldId = WorldId(WI_WORLD_QUIZ);
/// Every quiz teleport lands in the default layer.
pub const LAYER: LayerId = LayerId(0);
/// `ZONE_QUIZ`: a random answer position of the type (only while the quiz zone is open).
pub const ZONE_QUIZ: i32 = 100;
/// `ZONE_1`: answer 1 (OX: O).
pub const ZONE_1: i32 = 101;
/// `ZONE_4`: answer 4 (4C only).
pub const ZONE_4: i32 = 104;
/// `ZONE_WATCHING`: the watch zone.
pub const ZONE_WATCHING: i32 = 105;
/// `ZONE_EXIT`: back to Madrigal.
pub const ZONE_EXIT: i32 = 106;

/// Where `GoOut` sends everybody: Madrigal (Flaris), (6971.984, 99.8, 3336.884), no jitter.
pub const EXIT_POS: Vec3 = Vec3::new(6971.984, 99.8, 3336.884);
/// The world of the exit (`WI_WORLD_MADRIGAL`).
pub const EXIT_WORLD: WorldId = WorldId(WI_WORLD_MADRIGAL);

/// `CRect`: `[left, right) x [top, bottom)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    /// Inclusive left edge (x).
    pub left: i32,
    /// Inclusive top edge (z).
    pub top: i32,
    /// Exclusive right edge (x).
    pub right: i32,
    /// Exclusive bottom edge (z).
    pub bottom: i32,
}

impl Rect {
    /// `CRect(left, top, right, bottom)`.
    pub const fn new(left: i32, top: i32, right: i32, bottom: i32) -> Rect {
        Rect {
            left,
            top,
            right,
            bottom,
        }
    }

    /// `PtInRect(POINT{(int)x, (int)z})`: the position is truncated toward zero first; right and bottom exclude.
    pub fn contains(&self, pos: Vec3) -> bool {
        let (x, z) = (pos.x as i32, pos.z as i32);
        x >= self.left && x < self.right && z >= self.top && z < self.bottom
    }
}

/// Which part of the quiz world a position belongs to (`GetZoneType`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZoneKind {
    /// Inside the quiz rect.
    Quiz,
    /// Anywhere else.
    Watching,
}

/// The geometry of one quiz type (`QUIZDATA`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Geometry {
    /// `QuizEventRect`.
    pub quiz_rect: Rect,
    /// `vWatchingZonePos`.
    pub watching_pos: Vec3,
    /// `aExamplePos`: the answer positions (OX: two, 4C: four).
    pub answer_pos: &'static [Vec3],
    /// `aExampleRect`: the answer rects, parallel to `answer_pos`.
    pub answer_rect: &'static [Rect],
}

static OX_POS: [Vec3; 2] = [
    Vec3::new(783.5, 100.0, 752.0),
    Vec3::new(816.5, 100.0, 752.0),
];
static OX_RECT: [Rect; 2] = [Rect::new(768, 736, 799, 768), Rect::new(801, 736, 832, 768)];
static FC_POS: [Vec3; 4] = [
    Vec3::new(1744.0, 100.0, 1872.0),
    Vec3::new(1766.0, 100.0, 1872.0),
    Vec3::new(1744.0, 100.0, 1840.0),
    Vec3::new(1766.0, 100.0, 1840.0),
];
static FC_RECT: [Rect; 4] = [
    Rect::new(1728, 1857, 1759, 1888),
    Rect::new(1761, 1857, 1792, 1888),
    Rect::new(1728, 1824, 1759, 1855),
    Rect::new(1761, 1824, 1792, 1855),
];

impl Geometry {
    /// `InitQuizData(type)`.
    pub const fn of(quiz_type: QuizType) -> Geometry {
        match quiz_type {
            QuizType::Ox => Geometry {
                quiz_rect: Rect::new(768, 736, 832, 768),
                watching_pos: Vec3::new(798.747, 100.0, 705.266),
                answer_pos: &OX_POS,
                answer_rect: &OX_RECT,
            },
            QuizType::FourChoice => Geometry {
                quiz_rect: Rect::new(1728, 1792, 1824, 1888),
                watching_pos: Vec3::new(1759.507, 100.0, 1790.158),
                answer_pos: &FC_POS,
                answer_rect: &FC_RECT,
            },
        }
    }

    /// `GetZoneType`: [`ZoneKind::Quiz`] inside the quiz rect, else [`ZoneKind::Watching`].
    pub fn zone_of(&self, pos: Vec3) -> ZoneKind {
        if self.quiz_rect.contains(pos) {
            ZoneKind::Quiz
        } else {
            ZoneKind::Watching
        }
    }

    /// `PtInRect` of the rect of answer `answer` (1-based); `false` for an answer the type does not have.
    pub fn in_answer_rect(&self, pos: Vec3, answer: u8) -> bool {
        usize::from(answer)
            .checked_sub(1)
            .and_then(|i| self.answer_rect.get(i))
            .is_some_and(|r| r.contains(pos))
    }

    /// The answer position of `answer` (1-based), for the `XI_QUIZCORRECTANSWER` effect.
    pub fn answer_position(&self, answer: u8) -> Option<Vec3> {
        usize::from(answer)
            .checked_sub(1)
            .and_then(|i| self.answer_pos.get(i))
            .copied()
    }

    /// `GetTeleportPos` plus the jitter of `TeleportToQuizEvent`: `ZONE_QUIZ` draws the answer
    /// (`random_below(2)` for OX, `random_below(4)` for 4C), `101..=104` is answer `n` (OX has only 1 and 2: the
    /// legacy zero vector means "refused"), `105` the watch zone, anything else none (no draw happens then). The
    /// jitter follows: `x += random_below(4) - 2`, then `z += random_below(4) - 2` (draw order: answer, x, z).
    pub fn teleport_pos(&self, zone: i32, rng: &mut dyn GameRng) -> Option<Vec3> {
        let base = match zone {
            ZONE_QUIZ => {
                let n = rng.random_below(self.answer_pos.len() as u32) as usize;
                *self.answer_pos.get(n)?
            }
            ZONE_1..=ZONE_4 => *self.answer_pos.get((zone - ZONE_1) as usize)?,
            ZONE_WATCHING => self.watching_pos,
            _ => return None,
        };
        Some(jitter(base, rng))
    }
}

/// `vPos += (xrandom(4) - 2, 0, xrandom(4) - 2)`: x first, then z.
pub fn jitter(base: Vec3, rng: &mut dyn GameRng) -> Vec3 {
    let dx = rng.random_below(4) as i32 - 2;
    let dz = rng.random_below(4) as i32 - 2;
    Vec3::new(base.x + dx as f32, base.y, base.z + dz as f32)
}

/// `dwRemainTime / MIN(1) + 1` of `CQuiz::NoticeMessage`: the minutes shown by a pre-start or watch-zone notice,
/// `None` when nothing remains (`remain_ms <= 0`: the legacy returned without a notice).
pub fn notice_minutes(remain_ms: i64) -> Option<i32> {
    if remain_ms <= 0 {
        return None;
    }
    i32::try_from(remain_ms / 60_000 + 1).ok()
}
