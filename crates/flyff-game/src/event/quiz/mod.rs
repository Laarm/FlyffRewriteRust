//! `event::quiz`: the quiz event (Plan/22 §15, WP-EVENT-19; `_Common/Quiz.cpp`, `Quiz.h`).
//!
//! A GM starts the quiz ([`gm::open`]); the questions come from `quiz_questions` (shipped empty, PD-12: the operator
//! imports them with `flyff quiz import`, [`import`]). Players enter at the `MaFl_Ete` NPC in Flaris ([`on_entrance`]),
//! stand on an answer in world 211 ([`on_teleport`] moves them; they walk the last metres themselves), and after each
//! question the ones outside the right answer rect go to the watch zone. The survivors of the last question get the
//! prize by mail (`prize`). Memory only (E9): a restart ends the run, players saved in 211 log in at Madrigal.
//!
//! | module | content |
//! |---|---|
//! | [`mod@import`] | the TOML import file: format, parser, validator (pure) |
//! | [`zones`] | the rects and positions of both types, the teleport draws, `notice_minutes` |
//! | [`gm`] | the control seam of gm-content-control: `open`, `state_next`, `close`, `is_running`, `state_line` |
//! | `flow` | the open continuation, the step machine and `close` |
//! | `prize` | the survivors' mails, `AtomicBatch::quiz_prize`, the used marks, audit `quiz.winner` |
//!
//! **Phases** ([`QuizPhase`]): `Close 0 -> Open 1 -> Question 4 -> CorrectAnswer 5 -> DropOut 6 -> (Question | CloseWait
//! 2) -> (WatchingZoneOpen 3 -> Question | Close 0 with the closing wait) -> close`. `running` (`m_bRun`) stays true
//! through the closing wait, so the entrance answers `NOT_OPEN` while the quiz still ends on its own timer. Every
//! duration is `ctx.clock.game_ms`, every comparison the legacy strict `now > next`, every random draw
//! `ctx.rng.event()`. The 1 s slot items 9 ([`process`]) and 16 ([`notice_second`]) of the scheduler drive it.

mod flow;
pub mod gm;
pub mod import;
mod prize;
pub mod zones;

pub use gm::QuizGmError;

use crate::chat;
use crate::chat::ingress::notice;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::gm::text::format_positional;
use crate::item::pets::eat::hooks::dismiss as dismiss_eat_pet;
use crate::item::pets::system::hooks::with_player;
use crate::item::pets::system::summon::{has_pet_buff, release, remove_bonus};
use crate::outbox::Audience;
use crate::world::ReplaceKind;
use flyff_common::defs::neuz::MMI_QUIZ_ENTRANCE;
use flyff_common::defs::text::{TID_GAME_QUIZ_DO_NOT_TELEPORT, TID_GAME_QUIZ_NOT_OPEN};
use flyff_common::math::Vec3;
use flyff_common::{GameMs, ObjId, WorldId};
use flyff_data::lua_config::{QUIZ_TYPE_4C, QUIZ_TYPE_OX};
use flyff_db::{QuizQuestionRecord, QuizType};
use flyff_proto::pb;
use serde_json::json;
use std::collections::VecDeque;
use zones::{Geometry, ZoneKind};

/// `REMAIN_QUIZ` (`Quiz.h`): with more questions than this left after a winner round the watch zone reopens
/// instead of the quiz closing.
pub const REMAIN_QUIZ: usize = 30;
/// The pre-start and watch-zone notices come once a minute (`QuizDBCtrl.cpp` `tick > cycle`, `cycle = tick + 60 s`).
const NOTICE_CYCLE_MS: u64 = 60_000;
/// The only channel (`QuizChannel` of the Lua is ignored, 22 §15.1).
const CHANNEL: i32 = 1;

/// `QE_*` (`Quiz.h`): the state of the quiz, as the client's `QuizState` / `QuizMessage` carry it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum QuizPhase {
    /// `QE_CLOSE 0`: closed, or (while `running`) the closing wait after the last winner.
    #[default]
    Close,
    /// `QE_OPEN 1`: the entrance time.
    Open,
    /// `QE_CLOSE_WAIT 2`: the winners are paid; the next run decides between the watch zone and the end.
    CloseWait,
    /// `QE_WATCHINGZONE_OPEN 3`: the watch zone is open for the next set of questions.
    WatchingZoneOpen,
    /// `QE_QUESTION 4`: the question is on screen.
    Question,
    /// `QE_CORRECT_ANSWER 5`: waiting for the answers.
    CorrectAnswer,
    /// `QE_DROP_OUT 6`: the answer is shown, the wrong players leave next.
    DropOut,
    /// `QE_WINNER 7`: the legacy database-only state; never entered here, kept for the code table.
    Winner,
}

impl QuizPhase {
    /// The legacy `QE_*` value (`QuizState.state`, `QuizMessage.state`).
    pub const fn code(self) -> i32 {
        match self {
            QuizPhase::Close => 0,
            QuizPhase::Open => 1,
            QuizPhase::CloseWait => 2,
            QuizPhase::WatchingZoneOpen => 3,
            QuizPhase::Question => 4,
            QuizPhase::CorrectAnswer => 5,
            QuizPhase::DropOut => 6,
            QuizPhase::Winner => 7,
        }
    }

    /// The name `gm::state_line` prints.
    pub const fn name(self) -> &'static str {
        match self {
            QuizPhase::Close => "Close",
            QuizPhase::Open => "Open",
            QuizPhase::CloseWait => "CloseWait",
            QuizPhase::WatchingZoneOpen => "WatchingZoneOpen",
            QuizPhase::Question => "Question",
            QuizPhase::CorrectAnswer => "CorrectAnswer",
            QuizPhase::DropOut => "DropOut",
            QuizPhase::Winner => "Winner",
        }
    }
}

/// The run settings copied from `GameData.lua.quiz` at each open (`AnswerTime` stays unused, as legacy).
#[derive(Clone, Debug)]
struct RunSettings {
    /// `AutoMode`: the 1 s slot drives the timers.
    auto: bool,
    /// `QuizType`.
    quiz_type: QuizType,
    /// `EntranceTime`.
    entrance_ms: u64,
    /// `WaitTime`.
    wait_ms: u64,
    /// `QuestionTime`.
    question_ms: u64,
    /// `WatchingZoneOpenTime`.
    watching_ms: u64,
    /// `CloseWaitTime`.
    close_wait_ms: u64,
    /// `PrizeItemId`.
    prize_item: u32,
    /// `PrizeItemNum`.
    prize_count: i32,
}

/// The quiz service (`Services.event.quiz`): memory only (E9).
#[derive(Debug)]
pub struct QuizService {
    /// The phase.
    phase: QuizPhase,
    /// `m_bRun`: true from a successful open to the close, the closing wait included.
    running: bool,
    /// The generation of an open whose question read is in flight.
    loading: Option<u64>,
    /// The generation counter of [`QuizService::loading`].
    load_gen: u64,
    /// The settings of the running quiz.
    settings: Option<RunSettings>,
    /// `m_dwNextTime`: the deadline of the current phase (0 = due).
    next: GameMs,
    /// The next pre-start notice (`None` = at the next run of item 16).
    notice_due: Option<GameMs>,
    /// The questions left, the front is the current one.
    questions: VecDeque<QuizQuestionRecord>,
    /// The ids asked since the last `QuizPrize` batch (marked used with it, or at the close).
    asked_unmarked: Vec<i64>,
    /// `m_nQuizCount`: the number shown in "question n has ended".
    quiz_count: u32,
    /// The host NPCs standing in Madrigal.
    npcs: Vec<ObjId>,
    /// The event id of the running (or last) quiz.
    event_id: u32,
    /// `meta.next_quiz_event_id`.
    next_event_id: u32,
    /// Who started the quiz.
    opened_by: Option<PlayerRef>,
    /// The line of the last refused open (PD-12 or a read error), until a successful open.
    last_refusal: Option<String>,
}

impl Default for QuizService {
    fn default() -> Self {
        QuizService::new(1)
    }
}

impl QuizService {
    /// A closed quiz; the event ids continue at `next_event_id` (`StartupState.counters.next_quiz_event_id`).
    pub fn new(next_event_id: u32) -> QuizService {
        QuizService {
            phase: QuizPhase::Close,
            running: false,
            loading: None,
            load_gen: 0,
            settings: None,
            next: GameMs(0),
            notice_due: None,
            questions: VecDeque::new(),
            asked_unmarked: Vec::new(),
            quiz_count: 0,
            npcs: Vec::new(),
            event_id: 0,
            next_event_id: next_event_id.max(1),
            opened_by: None,
            last_refusal: None,
        }
    }

    /// The phase.
    pub fn phase(&self) -> QuizPhase {
        self.phase
    }

    /// `true` from a successful open to the close (the closing wait included).
    pub fn is_running(&self) -> bool {
        self.running
    }

    /// The type of the running quiz.
    pub fn quiz_type(&self) -> Option<QuizType> {
        self.settings
            .as_ref()
            .filter(|_| self.running)
            .map(|s| s.quiz_type)
    }

    /// The milliseconds until the deadline of the current phase, `None` when the quiz is not running or the deadline
    /// is not in the future.
    pub fn remaining(&self, now: GameMs) -> Option<u64> {
        (self.running && self.next > now).then(|| self.next.0 - now.0)
    }

    /// The questions left (the current one included).
    pub fn questions_left(&self) -> usize {
        self.questions.len()
    }

    /// The event id of the running (or last) quiz; 0 before the first open.
    pub fn event_id(&self) -> u32 {
        self.event_id
    }

    /// The line of the last refused open, until a successful one.
    pub fn last_refusal(&self) -> Option<&str> {
        self.last_refusal.as_deref()
    }

    /// The admin view of a later phase (22 §20): state, type, phase, the time left and the deadline in game ms, the
    /// event id and the counts.
    pub fn to_json(&self, now: GameMs) -> serde_json::Value {
        json!({
            "running": self.running,
            "loading": self.loading.is_some(),
            "state": self.phase.code(),
            "phase": self.phase.name(),
            "type": self.quiz_type().map(QuizType::code),
            "remaining_ms": self.remaining(now),
            "next_ms": self.next.0,
            "event_id": self.event_id,
            "questions": self.questions.len(),
            "question_no": self.quiz_count,
            "refused": self.last_refusal,
        })
    }

    /// The geometry of the running quiz's type, else of the configured type (reachable only by a GM who stands in
    /// world 211 without a quiz; the legacy zero geometry before the first open is not kept).
    fn geometry(&self, ctx: &GameCtx<'_>) -> Option<Geometry> {
        self.quiz_type()
            .or_else(|| configured_type(ctx))
            .map(Geometry::of)
    }
}

/// `QuizType` of `QuizEvent.lua`; `None` without the script or with a type that is neither OX nor 4C.
pub(crate) fn configured_type(ctx: &GameCtx<'_>) -> Option<QuizType> {
    let t = ctx.data.lua.quiz.as_ref()?.quiz_type;
    match t {
        QUIZ_TYPE_OX => Some(QuizType::Ox),
        QUIZ_TYPE_4C => Some(QuizType::FourChoice),
        _ => None,
    }
}

/// `true` for the quiz world (`WI_WORLD_QUIZ`): the chat radius, the item rules and the login rule key on it.
pub fn is_quiz_world(world: WorldId) -> bool {
    world == zones::WORLD
}

// ---- the 1 s slot items -------------------------------------------------------------------------------------------

/// 1 s slot item 9 `event::quiz::process` (22 §2.3, §15.3): the state timers, only with `AutoMode` and a running quiz
/// (`ThreadMng.cpp:452-456`). Without `AutoMode` only [`gm::state_next`] advances.
pub fn process(s: &mut Services, ctx: &mut GameCtx<'_>) {
    let q = &s.event.quiz;
    if q.running && q.settings.as_ref().is_some_and(|c| c.auto) {
        flow::step(s, ctx, false);
    }
}

/// 1 s slot item 16 `event::quiz::notice_second` (22 §2.3, §15.3): the minute notices of the entrance time
/// (`PRE_NOTICE1/2`) and of the open watch zone (`CUR_NOTICE1/2`) to everybody (`QuizDBCtrl.cpp:122-146` + `CQuiz::
/// NoticeMessage`): the first at the next run, then one per 60 s; the cycle advances even when nothing is sent.
pub fn notice_second(s: &mut Services, ctx: &mut GameCtx<'_>) {
    let now = ctx.clock.game_ms;
    let q = &mut s.event.quiz;
    if !q.running || !matches!(q.phase, QuizPhase::Open | QuizPhase::WatchingZoneOpen) {
        return;
    }
    if q.notice_due.is_some_and(|due| now <= due) {
        return;
    }
    q.notice_due = Some(GameMs(now.0 + NOTICE_CYCLE_MS));
    let remain = i64::try_from(q.next.0).unwrap_or(i64::MAX) - i64::try_from(now.0).unwrap_or(0);
    let Some(minutes) = zones::notice_minutes(remain) else {
        return;
    };
    let (first, second) = if q.phase == QuizPhase::Open {
        (
            flyff_common::defs::text::TID_GAME_QUIZ_PRE_NOTICE1,
            flyff_common::defs::text::TID_GAME_QUIZ_PRE_NOTICE2,
        )
    } else {
        (
            flyff_common::defs::text::TID_GAME_QUIZ_CUR_NOTICE1,
            flyff_common::defs::text::TID_GAME_QUIZ_CUR_NOTICE2,
        )
    };
    notice_all(ctx, first, false, CHANNEL, minutes);
    notice_all(ctx, second, false, 0, 0);
}

/// `SendQuizSystemMessage`: one `QuizNotice` to everybody online.
fn notice_all(ctx: &mut GameCtx<'_>, tid: u32, all: bool, channel: i32, time: i32) {
    ctx.broadcast(
        None,
        ObjId::NULL,
        Audience::AllOnline,
        pb::QuizNotice {
            text_id: i32::try_from(tid).unwrap_or(i32::MAX),
            all,
            channel,
            time,
        },
    );
}

// ---- the world of the quiz ----------------------------------------------------------------------------------------

/// The player movers of `(211, layer 0)` in the layer's insertion order (`IsInQuizEventPlayer`; game masters count,
/// the legacy exclusion is commented out).
fn players(ctx: &GameCtx<'_>) -> Vec<ObjId> {
    ctx.worlds
        .layer(zones::WORLD, zones::LAYER)
        .map(|l| {
            l.members
                .iter()
                .copied()
                .filter(|o| ctx.worlds.player(*o).is_some())
                .collect()
        })
        .unwrap_or_default()
}

/// The position of a player.
fn pos_of(ctx: &GameCtx<'_>, obj: ObjId) -> Option<Vec3> {
    ctx.worlds.player(obj).map(|m| m.core.pos)
}

/// `AddQuizEventMessage`: `tid` rendered with `args`, as a `QuizMessage` carrying `state` (0 = no state change) to
/// everybody in the quiz layer. An empty text is not sent (`User.cpp:8698-8732`).
fn say(ctx: &mut GameCtx<'_>, tid: u32, args: &[String], state: i32) {
    let message = format_positional(ctx.data.texts.get(tid), args);
    if message.is_empty() {
        return;
    }
    to_layer(ctx, pb::QuizMessage { message, state });
}

/// One message to everybody in the quiz layer.
fn to_layer(ctx: &mut GameCtx<'_>, msg: impl Into<pb::server_msg::Msg>) {
    ctx.broadcast(
        None,
        ObjId::NULL,
        Audience::Layer(zones::WORLD, zones::LAYER),
        msg,
    );
}

/// `REPLACE( WI_WORLD_QUIZ | WI_WORLD_MADRIGAL, pos, REPLACE_NORMAL, nDefaultLayer )`. A refused replace (a flying
/// non-GM, a missing world or layer, a position outside the world) is silent, as legacy.
fn go(ctx: &mut GameCtx<'_>, who: ObjId, world: WorldId, pos: Vec3) {
    if let Err(e) = ctx
        .worlds
        .replace(who, world, pos, zones::LAYER, ReplaceKind::Normal)
    {
        tracing::debug!(player = who.0, world = world.0, error = %e, "quiz: a teleport was refused");
    }
}

/// `GoOut`: back to Madrigal (6971.984, 99.8, 3336.884), no jitter.
fn go_out(ctx: &mut GameCtx<'_>, who: ObjId) {
    go(ctx, who, zones::EXIT_WORLD, zones::EXIT_POS);
}

/// `TeleportToQuizEvent` after its checks: the position of `zone` with the jitter of the event stream.
fn teleport_zone(s: &Services, ctx: &mut GameCtx<'_>, who: ObjId, zone: i32) -> bool {
    let Some(geo) = s.event.quiz.geometry(ctx) else {
        return false;
    };
    let Some(pos) = geo.teleport_pos(zone, ctx.rng.event()) else {
        return false;
    };
    go(ctx, who, zones::WORLD, pos);
    true
}

// ---- the client messages --------------------------------------------------------------------------------------------

/// `QuizEntrance` (`CQuiz::EntranceQuizEvent`, 22 §15.3 step 2). In order: no NPC with menu 137 in range ⇒ ignored
/// (VR-012, PL-107; legacy checked nothing); flying ⇒ ignored; the quiz closed (or in its closing wait) ⇒
/// `TID_GAME_QUIZ_NOT_OPEN`. Else `QuizState{type, state}`, the system pet released, the eat pet dismissed and a
/// stale `BUFF_PET` removed, **and the teleport still happens** (FIX PL-104; legacy skipped it behind an `else if`):
/// the quiz zone while the entrance or the watch zone is open, the watch zone otherwise.
pub fn on_entrance(s: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    let Some(m) = ctx.worlds.player(who.0) else {
        return;
    };
    let (world, layer, pos) = (m.core.world, m.core.layer, m.core.pos);
    let flying = m.sim.act.flags.fly();
    if ctx
        .worlds
        .close_npc(MMI_QUIZ_ENTRANCE, world, layer, pos)
        .is_none()
    {
        tracing::debug!(
            player = who.0.0,
            "quiz: QuizEntrance without the host NPC in range"
        );
        return;
    }
    if flying {
        return;
    }
    let (phase, quiz_type) = (s.event.quiz.phase, s.event.quiz.quiz_type());
    if phase == QuizPhase::Close {
        notice::defined(ctx, who, TID_GAME_QUIZ_NOT_OPEN, Vec::new());
        return;
    }
    if let Some(t) = quiz_type {
        chat::send_to(
            ctx,
            who,
            pb::QuizState {
                r#type: i32::from(t.code()),
                state: phase.code(),
            },
        );
    }
    remove_pets(s, ctx, who);
    let zone = if matches!(phase, QuizPhase::Open | QuizPhase::WatchingZoneOpen) {
        zones::ZONE_QUIZ
    } else {
        zones::ZONE_WATCHING
    };
    teleport_zone(s, ctx, who.0, zone);
}

/// The pets of `PL-104`: the summoned system pet is released, the eat pet dismissed, a `BUFF_PET` entry that is still
/// there (a stale slot without a summoned pet) removed.
fn remove_pets(s: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    with_player(s, ctx, who, |m, cx, _, fx| release(m, cx, fx));
    dismiss_eat_pet(s, ctx, who);
    with_player(s, ctx, who, |m, cx, _, fx| {
        if has_pet_buff(m) {
            remove_bonus(m, cx, fx);
        }
    });
}

/// `QuizTeleport{zone}` (`CQuiz::TeleportToQuizEvent`, 22 §15.3 step 8). Outside world 211 ⇒ ignored silently
/// (VR-062; legacy had no check). Then the legacy order, each refusal `TID_GAME_QUIZ_DO_NOT_TELEPORT`: `101..=104`
/// during DropOut or from outside the quiz rect; `100` outside Open and WatchingZoneOpen; `106` ⇒ the Madrigal exit
/// (any phase, no jitter); a zone without a position (an unknown value, OX 103/104) refused; else the position of the
/// zone with the jitter. No NPC check: the menus 138/139 and the zone buttons send this from anywhere in 211.
pub fn on_teleport(s: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, zone: i32) {
    let Some(m) = ctx.worlds.player(who.0) else {
        return;
    };
    if !is_quiz_world(m.core.world) {
        tracing::debug!(
            player = who.0.0,
            zone,
            "quiz: QuizTeleport outside world 211 (VR-062)"
        );
        return;
    }
    let pos = m.core.pos;
    let phase = s.event.quiz.phase;
    let refuse = |ctx: &mut GameCtx<'_>| {
        notice::defined(ctx, who, TID_GAME_QUIZ_DO_NOT_TELEPORT, Vec::new());
    };
    if (zones::ZONE_1..=zones::ZONE_4).contains(&zone) {
        let outside = s
            .event
            .quiz
            .geometry(ctx)
            .is_none_or(|g| g.zone_of(pos) == ZoneKind::Watching);
        if phase == QuizPhase::DropOut || outside {
            refuse(ctx);
            return;
        }
    }
    if zone == zones::ZONE_QUIZ && !matches!(phase, QuizPhase::Open | QuizPhase::WatchingZoneOpen) {
        refuse(ctx);
        return;
    }
    if zone == zones::ZONE_EXIT {
        go_out(ctx, who.0);
        return;
    }
    if !teleport_zone(s, ctx, who.0, zone) {
        refuse(ctx);
    }
}
