//! The seven race games and the shape check of their packet (Plan/22 §14.4, §14.6, VR-064, PL-105; the originals
//! `WORLDSERVER/MiniGame{Gawibawibo,Diceplay,Arithmetic,Stopwatch,Typing,Pairgame,Ladder}.cpp`).
//!
//! Everything here is pure: a game takes a [`GameRng`] and a [`GameRequest`] and answers a [`GameStep`] (the replies
//! and whether the game is complete), so the goldens run on a seeded generator and the draw-order tests on a
//! `ScriptedRng`. The manager ([`super::handlers`]) owns the racer state, the NPC checks and the completion rules.
//!
//! Draw-order rules (normative, they decide what a seed plays): `pick_next` draws `random_below(7)`; a dice game
//! draws `random_range(2, 13)` when it is created; the rest is in the documentation of each game.

use super::arithmetic;
use flyff_common::defs::neuz::MMI_LORD_RAINBOW_KAWIBAWIBO;
use flyff_common::rng::GameRng;
use flyff_proto::pb;
use smallvec::SmallVec;
use std::fmt::Debug;

/// How many games the race has (`RMG_MAX`).
pub const GAME_COUNT: u32 = 7;
/// The `completed` mask of a racer who played all seven (`IsAllCompleted`).
pub const ALL_COMPLETED: u16 = 0x7F;
/// The longest text a client may send (`ReadString(.., 256)`).
pub const MAX_TEXT_BYTES: usize = 256;
/// The most texts a client packet may carry for the games (the typing game reads three).
pub const MAX_TEXTS: usize = 3;

/// `RMG_*`: the id of a race game (also the bit position of its `now_game` bit).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RaceGameId {
    /// 0: rock-paper-scissors (`RMG_GAWIBAWIBO`).
    Gawibawibo = 0,
    /// 1: dice (`RMG_DICEPLAY`).
    Diceplay = 1,
    /// 2: arithmetic (`RMG_ARITHMATIC`).
    Arithmetic = 2,
    /// 3: stopwatch (`RMG_STOPWATCH`).
    Stopwatch = 3,
    /// 4: typing (`RMG_TYPING`).
    Typing = 4,
    /// 5: card pairs (`RMG_PAIRGAME`).
    Pairgame = 5,
    /// 6: ladder (`RMG_LADDER`).
    Ladder = 6,
}

impl RaceGameId {
    /// All seven, in id order.
    pub const ALL: [RaceGameId; 7] = [
        RaceGameId::Gawibawibo,
        RaceGameId::Diceplay,
        RaceGameId::Arithmetic,
        RaceGameId::Stopwatch,
        RaceGameId::Typing,
        RaceGameId::Pairgame,
        RaceGameId::Ladder,
    ];

    /// The game with id `g` (0-6).
    pub fn from_index(g: u32) -> Option<RaceGameId> {
        RaceGameId::ALL.get(usize::try_from(g).ok()?).copied()
    }

    /// The id (0-6).
    pub fn index(self) -> u32 {
        self as u32
    }

    /// The `now_game` bit (`1 << g`).
    pub fn bit(self) -> u16 {
        1 << (self as u16)
    }

    /// The game whose `now_game` bit is `bit` (exactly one of the seven bits), else `None`.
    pub fn from_bit(bit: u32) -> Option<RaceGameId> {
        if bit.count_ones() != 1 {
            return None;
        }
        RaceGameId::from_index(bit.trailing_zeros())
    }

    /// The menu of the coloured NPC that opens this game (`MMI_LORD_RAINBOW_KAWIBAWIBO + g`, 113-119).
    pub fn menu(self) -> u32 {
        MMI_LORD_RAINBOW_KAWIBAWIBO + self.index()
    }
}

/// `MP_*`: the state of a mini-game packet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MpState {
    /// 0 `MP_NONE`.
    None = 0,
    /// 1 `MP_OPENWND`: the client opened the game window.
    OpenWnd = 1,
    /// 2 `MP_TRUE`.
    True = 2,
    /// 3 `MP_FALSE`.
    False = 3,
    /// 4 `MP_FINISH`.
    Finish = 4,
    /// 5 `MP_FAIL`.
    Fail = 5,
}

impl MpState {
    /// The state of a wire code (0-5).
    pub fn from_code(code: i32) -> Option<MpState> {
        Some(match code {
            0 => MpState::None,
            1 => MpState::OpenWnd,
            2 => MpState::True,
            3 => MpState::False,
            4 => MpState::Finish,
            5 => MpState::Fail,
            _ => return None,
        })
    }

    /// The wire code.
    pub fn code(self) -> i32 {
        self as i32
    }
}

/// One validated client packet (`__MINIGAME_PACKET` / `__MINIGAME_EXT_PACKET`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameRequest {
    /// The `now_game` bit the client believes it plays.
    pub now_game: u16,
    /// The state the client sent.
    pub state: MpState,
    /// `nParam1`.
    pub p1: i32,
    /// `nParam2`.
    pub p2: i32,
    /// The texts of an ext packet (`Some` only with `ext`; at most [`MAX_TEXTS`], each at most [`MAX_TEXT_BYTES`]).
    pub data: Option<Vec<String>>,
}

impl GameRequest {
    /// A request without texts (tests and the GM pass).
    pub fn plain(now_game: u16, state: MpState, p1: i32, p2: i32) -> GameRequest {
        GameRequest {
            now_game,
            state,
            p1,
            p2,
            data: None,
        }
    }

    /// An ext request.
    pub fn ext(now_game: u16, state: MpState, p1: i32, data: Vec<String>) -> GameRequest {
        GameRequest {
            now_game,
            state,
            p1,
            p2: 0,
            data: Some(data),
        }
    }
}

/// One answer of a game: the plain packet (`RainbowRaceMiniGameState`) or the ext packet
/// (`RainbowRaceMiniGameExtState`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reply {
    /// `RainbowRaceMiniGameState`.
    Plain(pb::MiniGamePacket),
    /// `RainbowRaceMiniGameExtState`.
    Ext(pb::MiniGameExtPacket),
}

impl Reply {
    fn plain(now_game: u16, state: MpState, p1: i32, p2: i32) -> Reply {
        Reply::Plain(pb::MiniGamePacket {
            now_game: u32::from(now_game),
            state: state.code(),
            param1: p1,
            param2: p2,
        })
    }

    fn ext(now_game: u16, state: MpState, p1: i32, data: Vec<String>) -> Reply {
        Reply::Ext(pb::MiniGameExtPacket {
            packet: Some(pb::MiniGamePacket {
                now_game: u32::from(now_game),
                state: state.code(),
                param1: p1,
                param2: 0,
            }),
            data,
        })
    }

    /// A one-line form for the golden transcripts: `plain OpenWnd 5 0` / `ext Finish 5 []`.
    pub fn line(&self) -> String {
        let name = |code: i32| {
            MpState::from_code(code).map_or_else(|| format!("#{code}"), |s| format!("{s:?}"))
        };
        match self {
            Reply::Plain(p) => format!(
                "plain g{} {} {} {}",
                p.now_game,
                name(p.state),
                p.param1,
                p.param2
            ),
            Reply::Ext(e) => {
                let p = e.packet.unwrap_or_default();
                format!(
                    "ext g{} {} {} {:?}",
                    p.now_game,
                    name(p.state),
                    p.param1,
                    e.data
                )
            }
        }
    }
}

/// What a game answers: the replies in order and whether the game is now complete.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GameStep {
    /// The packets for the racer, in order (the dice game sends two after a miss).
    pub replies: SmallVec<[Reply; 2]>,
    /// The game is complete: the manager marks it, picks the next one and queues the prize.
    pub completed: bool,
}

impl GameStep {
    fn one(reply: Reply) -> GameStep {
        let mut replies = SmallVec::new();
        replies.push(reply);
        GameStep {
            replies,
            completed: false,
        }
    }

    fn finish(reply: Reply) -> GameStep {
        GameStep {
            completed: true,
            ..GameStep::one(reply)
        }
    }
}

/// One racer's instance of a game (`CMiniGameBase::Excute`). `Debug + Send` so the racer table can sit in the
/// event services.
pub trait RaceGame: Debug + Send {
    /// Which game this is.
    fn id(&self) -> RaceGameId;

    /// `true` when `play` may be called with `req`. A refused request changes nothing and draws nothing (VR-064): the
    /// game has no question yet, a typing play lacks its three texts, a pair play has a position outside the cards.
    fn accepts(&self, req: &GameRequest) -> bool;

    /// Plays one request (`req.now_game` is the game's own bit, the manager checked it).
    fn play(&mut self, rng: &mut dyn GameRng, req: &GameRequest) -> GameStep;
}

/// `true` for the request that opens the game window (`MP_OPENWND`).
fn is_open(req: &GameRequest) -> bool {
    req.state == MpState::OpenWnd
}

/// The new instance of game `id` (`CRainbowRace::SetMiniGame`). The dice game draws its first target here.
pub fn new_game(id: RaceGameId, rng: &mut dyn GameRng) -> Box<dyn RaceGame> {
    match id {
        RaceGameId::Gawibawibo => Box::new(Gawibawibo { streak: 0 }),
        RaceGameId::Diceplay => Box::new(Diceplay {
            target: dice_target(rng),
        }),
        RaceGameId::Arithmetic => Box::new(Arithmetic {
            count: 0,
            result: None,
        }),
        RaceGameId::Stopwatch => Box::new(Stopwatch { target: None }),
        RaceGameId::Typing => Box::new(Typing {
            strings: Vec::new(),
        }),
        RaceGameId::Pairgame => Box::new(Pairgame {
            stream: Vec::new(),
            pairs: 0,
        }),
        RaceGameId::Ladder => Box::new(Ladder),
    }
}

/// `CRainbowRace::SetNextMiniGame` (22 §14.4): `None` when all seven are done; else `g = random_below(7)` and, while
/// bit `g` is done, `g - 1` (0 wraps to 6).
pub fn pick_next(completed: u16, rng: &mut dyn GameRng) -> Option<RaceGameId> {
    if completed & ALL_COMPLETED == ALL_COMPLETED {
        return None;
    }
    let mut g = rng.random_below(GAME_COUNT);
    // at most seven steps: a bit is free (the mask is not full)
    for _ in 0..GAME_COUNT {
        if completed & (1 << g) == 0 {
            break;
        }
        g = if g == 0 { GAME_COUNT - 1 } else { g - 1 };
    }
    RaceGameId::from_index(g)
}

/// VR-064 step 1 (silent, nothing else runs): the shape of a `RainbowRaceMiniGamePacket`. `None` for a missing
/// packet, a `now_game` that is not exactly one of the seven bits, a state outside 0-5, and - with `ext` - more than
/// three texts or a text over 256 bytes. Without `ext` the texts are ignored (the legacy read none).
pub fn validate_shape(m: &pb::RainbowRaceMiniGamePacket) -> Option<GameRequest> {
    let p = m.packet.as_ref()?;
    let game = RaceGameId::from_bit(p.now_game)?;
    let state = MpState::from_code(p.state)?;
    let data = if m.ext {
        if m.data.len() > MAX_TEXTS || m.data.iter().any(|s| s.len() > MAX_TEXT_BYTES) {
            return None;
        }
        Some(m.data.clone())
    } else {
        None
    };
    Some(GameRequest {
        now_game: game.bit(),
        state,
        p1: p.param1,
        p2: p.param2,
        data,
    })
}

// ---- 0: rock-paper-scissors -------------------------------------------------------------------------------------------

/// `CMiniGameGawibawibo`: three wins in a row.
#[derive(Debug)]
struct Gawibawibo {
    streak: i32,
}

impl RaceGame for Gawibawibo {
    fn id(&self) -> RaceGameId {
        RaceGameId::Gawibawibo
    }

    fn accepts(&self, _req: &GameRequest) -> bool {
        true
    }

    fn play(&mut self, rng: &mut dyn GameRng, req: &GameRequest) -> GameStep {
        let g = req.now_game;
        if is_open(req) {
            self.streak = 0;
            return GameStep::one(Reply::plain(g, MpState::OpenWnd, 0, 0));
        }
        let r = rng.random_below(100);
        let mut state = if r < 33 {
            self.streak += 1;
            MpState::True
        } else if r < 67 {
            MpState::False
        } else {
            self.streak = 0;
            MpState::Fail
        };
        let done = self.streak == 3;
        if done {
            state = MpState::Finish;
        }
        let reply = Reply::plain(g, state, self.streak, 0);
        if done {
            GameStep::finish(reply)
        } else {
            GameStep::one(reply)
        }
    }
}

// ---- 1: dice ------------------------------------------------------------------------------------------------------------------

fn dice_target(rng: &mut dyn GameRng) -> i32 {
    i32::try_from(rng.random_range(2, 13)).unwrap_or(2)
}

/// `CMiniGameDiceplay`: roll two dice until their sum is the target.
#[derive(Debug)]
struct Diceplay {
    target: i32,
}

impl RaceGame for Diceplay {
    fn id(&self) -> RaceGameId {
        RaceGameId::Diceplay
    }

    fn accepts(&self, _req: &GameRequest) -> bool {
        true
    }

    fn play(&mut self, rng: &mut dyn GameRng, req: &GameRequest) -> GameStep {
        let g = req.now_game;
        if is_open(req) {
            return GameStep::one(Reply::plain(g, MpState::OpenWnd, self.target, 0));
        }
        let a = i32::try_from(rng.random_below(6)).unwrap_or(0) + 1;
        let b = i32::try_from(rng.random_below(6)).unwrap_or(0) + 1;
        if a + b == self.target {
            return GameStep::finish(Reply::plain(g, MpState::Finish, a, b));
        }
        let mut step = GameStep::one(Reply::plain(g, MpState::Fail, a, b));
        self.target = dice_target(rng);
        // the second packet tells the new target (`p2 = -1`)
        step.replies
            .push(Reply::plain(g, MpState::Fail, self.target, -1));
        step
    }
}

// ---- 2: arithmetic ------------------------------------------------------------------------------------------------------

/// `CMiniGameArithmetic`: five right answers in a row.
#[derive(Debug)]
struct Arithmetic {
    count: i32,
    /// The answer of the current question; `None` before the first question.
    result: Option<i32>,
}

impl Arithmetic {
    fn question(&mut self, rng: &mut dyn GameRng) -> String {
        let (text, result) = arithmetic::make_question(rng);
        self.result = Some(result);
        text
    }
}

impl RaceGame for Arithmetic {
    fn id(&self) -> RaceGameId {
        RaceGameId::Arithmetic
    }

    fn accepts(&self, req: &GameRequest) -> bool {
        // OpenWnd and a client time-out (`MP_FAIL`) ask for a question; an answer needs one
        is_open(req) || req.state == MpState::Fail || self.result.is_some()
    }

    fn play(&mut self, rng: &mut dyn GameRng, req: &GameRequest) -> GameStep {
        let g = req.now_game;
        if is_open(req) || req.state == MpState::Fail {
            if is_open(req) {
                self.count = 0;
            }
            let q = self.question(rng);
            return GameStep::one(Reply::ext(g, req.state, self.count, vec![q]));
        }
        if self.result == Some(req.p1) {
            self.count += 1;
            if self.count == 5 {
                return GameStep::finish(Reply::ext(g, MpState::Finish, self.count, Vec::new()));
            }
            let q = self.question(rng);
            GameStep::one(Reply::ext(g, MpState::True, self.count, vec![q]))
        } else {
            let q = self.question(rng);
            self.count = 0;
            GameStep::one(Reply::ext(g, MpState::False, self.count, vec![q]))
        }
    }
}

// ---- 3: stopwatch -------------------------------------------------------------------------------------------------------

/// `CMiniGameStopwatch`: stop the clock within 30 ms of the target.
#[derive(Debug)]
struct Stopwatch {
    /// The target in ms; `None` before the window was opened.
    target: Option<i32>,
}

fn stopwatch_target(rng: &mut dyn GameRng) -> i32 {
    i32::try_from(rng.random_range(300, 1000)).unwrap_or(300) * 10
}

impl RaceGame for Stopwatch {
    fn id(&self) -> RaceGameId {
        RaceGameId::Stopwatch
    }

    fn accepts(&self, req: &GameRequest) -> bool {
        is_open(req) || self.target.is_some()
    }

    fn play(&mut self, rng: &mut dyn GameRng, req: &GameRequest) -> GameStep {
        let g = req.now_game;
        if is_open(req) {
            let t = stopwatch_target(rng);
            self.target = Some(t);
            return GameStep::one(Reply::plain(g, MpState::OpenWnd, t, 0));
        }
        let Some(target) = self.target else {
            // unreachable after `accepts`; answer like a miss without a draw
            return GameStep::one(Reply::plain(g, MpState::Fail, 0, 0));
        };
        if (i64::from(target) - i64::from(req.p1)).abs() <= 30 {
            return GameStep::finish(Reply::plain(g, MpState::Finish, 0, 0));
        }
        let t = stopwatch_target(rng);
        self.target = Some(t);
        GameStep::one(Reply::plain(g, MpState::Fail, t, 0))
    }
}

// ---- 4: typing -----------------------------------------------------------------------------------------------------------------

/// `CMiniGameTyping`: type three strings of 20 characters.
#[derive(Debug)]
struct Typing {
    /// The three strings; empty before the window was opened.
    strings: Vec<String>,
}

impl Typing {
    fn make_question(&mut self, rng: &mut dyn GameRng) {
        self.strings.clear();
        for _ in 0..3 {
            let mut s = String::with_capacity(20);
            for _ in 0..20 {
                let c = match rng.random_below(3) {
                    0 => rng.random_range(48, 58),
                    1 => rng.random_range(97, 110),
                    _ => rng.random_range(110, 123),
                };
                // all three ranges are ASCII
                s.push(char::from(u8::try_from(c).unwrap_or(b'0')));
            }
            self.strings.push(s);
        }
    }
}

impl RaceGame for Typing {
    fn id(&self) -> RaceGameId {
        RaceGameId::Typing
    }

    fn accepts(&self, req: &GameRequest) -> bool {
        if is_open(req) {
            return true;
        }
        // the legacy read `vecszData[i]` of a packet without them
        !self.strings.is_empty()
            && req
                .data
                .as_ref()
                .is_some_and(|d| d.len() == self.strings.len())
    }

    fn play(&mut self, rng: &mut dyn GameRng, req: &GameRequest) -> GameStep {
        let g = req.now_game;
        if is_open(req) {
            self.make_question(rng);
            return GameStep::one(Reply::ext(g, MpState::OpenWnd, 0, self.strings.clone()));
        }
        if req.data.as_deref() == Some(self.strings.as_slice()) {
            GameStep::finish(Reply::ext(g, MpState::Finish, 0, Vec::new()))
        } else {
            GameStep::one(Reply::ext(g, MpState::Fail, 0, Vec::new()))
        }
    }
}

// ---- 5: card pairs ---------------------------------------------------------------------------------------------------------

/// The mark of a matched card.
const MATCHED: u8 = b'x';

/// `CMiniGamePairgame`: nine pairs among 18 cards.
#[derive(Debug)]
struct Pairgame {
    /// The cards `1..=9` twice each, a matched one is [`MATCHED`]; empty before the window was opened.
    stream: Vec<u8>,
    pairs: i32,
}

impl Pairgame {
    /// `MakeGame`: repeatedly picks a kind among the kinds that still have cards.
    fn deal(&mut self, rng: &mut dyn GameRng) {
        // (card, cards left)
        let mut kinds: Vec<(u8, u8)> = (1..=9).map(|c| (c, 2)).collect();
        self.stream.clear();
        while !kinds.is_empty() {
            let k = rng.random_below(u32::try_from(kinds.len()).unwrap_or(1)) as usize;
            let Some(kind) = kinds.get_mut(k) else { break };
            self.stream.push(kind.0);
            kind.1 -= 1;
            if kind.1 == 0 {
                kinds.remove(k);
            }
        }
    }

    fn stream_text(&self) -> String {
        self.stream.iter().map(|b| char::from(*b)).collect()
    }

    fn card(&self, pos: i32) -> Option<u8> {
        usize::try_from(pos)
            .ok()
            .and_then(|i| self.stream.get(i))
            .copied()
    }
}

impl RaceGame for Pairgame {
    fn id(&self) -> RaceGameId {
        RaceGameId::Pairgame
    }

    fn accepts(&self, req: &GameRequest) -> bool {
        // a position outside the cards is rejected (22 §14.6; the legacy answered `MP_FALSE`)
        is_open(req) || (self.card(req.p1).is_some() && self.card(req.p2).is_some())
    }

    fn play(&mut self, rng: &mut dyn GameRng, req: &GameRequest) -> GameStep {
        let g = req.now_game;
        if is_open(req) {
            self.pairs = 0;
            self.deal(rng);
            return GameStep::one(Reply::ext(g, MpState::OpenWnd, 0, vec![self.stream_text()]));
        }
        let (Some(a), Some(b)) = (self.card(req.p1), self.card(req.p2)) else {
            return GameStep::one(Reply::plain(g, MpState::False, 0, 0));
        };
        let mut state = MpState::False;
        let mut done = false;
        if req.p1 != req.p2 && a != MATCHED && b != MATCHED && a == b {
            self.pairs += 1;
            for pos in [req.p1, req.p2] {
                if let Some(slot) = usize::try_from(pos)
                    .ok()
                    .and_then(|i| self.stream.get_mut(i))
                {
                    *slot = MATCHED;
                }
            }
            state = MpState::True;
            if self.pairs == 9 {
                state = MpState::Finish;
                done = true;
            }
        }
        let reply = Reply::plain(g, state, 0, 0);
        if done {
            GameStep::finish(reply)
        } else {
            GameStep::one(reply)
        }
    }
}

// ---- 6: ladder ------------------------------------------------------------------------------------------------------------------

/// `CMiniGameLadder`: a 1 in 10 chance per try.
#[derive(Debug)]
struct Ladder;

impl RaceGame for Ladder {
    fn id(&self) -> RaceGameId {
        RaceGameId::Ladder
    }

    fn accepts(&self, _req: &GameRequest) -> bool {
        true
    }

    fn play(&mut self, rng: &mut dyn GameRng, req: &GameRequest) -> GameStep {
        let g = req.now_game;
        if is_open(req) {
            return GameStep::one(Reply::plain(g, MpState::OpenWnd, 0, 0));
        }
        if rng.random_below(10) == 0 {
            GameStep::finish(Reply::plain(g, MpState::Finish, 0, 0))
        } else {
            GameStep::one(Reply::plain(g, MpState::Fail, 0, 0))
        }
    }
}
