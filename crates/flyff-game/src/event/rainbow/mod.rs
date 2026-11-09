//! `event::rainbow`: the rainbow race and its seven games (Plan/22 §14, WP-EVENT-17/18; the original
//! `_Common/RainbowRace.cpp`, `WORLDSERVER/MiniGame*.cpp`).
//!
//! A weekly race: applicants (Saturday 12:00-20:00, the fee goes into the pot) must play the seven games at the
//! coloured NPCs in Madrigal and report back at the end NPC; the finishing order shares the pot by mail. It runs only
//! while flag 25 `events.rainbow_race` is on (OFF by default, PD-14) and the start NPC `MaFl_RainbowStart` exists only
//! when the flag was on at world load.
//!
//! | file | content |
//! |---|---|
//! | `mod.rs` | [`RainbowRaceService`], [`RrState`], [`Racer`], the manager ([`process`], [`on_open_time`], [`boot`]), the race (begin, finish, end), the teleport block ([`is_entry`](RainbowRaceService::is_entry), [`is_entry_obj`], [`teleport_block`]) and the logout drop-out ([`on_logout`]) |
//! | `handlers.rs` | the five client messages |
//! | `games.rs`, `arithmetic.rs` | the seven games, the shape check of their packet, the question generator |
//! | `prizes.rs` | the rank prizes, the mini-game prize queue, `cvttsd2si` |
//! | `gm.rs` | the pure GM calls (`/rropen`, `/rrnext`, `/rrpass`, `/content rr ..`) |
//! | `join.rs` | `RainbowRaceNowState` at the join of a racer |
//!
//! **Time.** All durations are game ms of the tick clock; "due" is `now > next_at`, strict as the legacy
//! `GetNextTime() < dwTick`. The wall clock decides only the application window (a predicate on the local stamp) and
//! the opening (the wall job `rainbow_race_open`, [`on_open_time`]). The script is never copied: every step reads
//! `Services.event.configs.rainbow`, so a `/lua rr` reload applies to the next transition, as the legacy read the Lua
//! globals at each one.
//!
//! **Memory only.** The race in progress is not persisted (E9): after a restart the state is Closed, the unfinished
//! applicants come back from the entry rows (`R` rows too, legacy `S2` reset them to `N`) and the previous ranking from
//! the newest earlier round.
//!
//! **Teleport block.** [`RainbowRaceService::is_entry`] is the authority; the service mirrors it into the runtime-only
//! `PlayerMisc.rainbow_entrant` of the online racer at every transition (begin, finish, clear; a logout drops the
//! mover), so the world-phase readers without `Services` see it: the blink and escape wings (`UseFacts::of`), the
//! tickets of the instances, Hero Return (`CombatHooks::rainbow_race_entry`), and - through [`is_entry_obj`] - the party
//! summon, the messenger summons and the couple ring.

pub mod arithmetic;
pub mod games;
pub mod gm;
pub mod handlers;
pub mod join;
pub mod prizes;

use self::games::{RaceGame, Reply};
use crate::chat::ingress::notice;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::outbox::Audience;
use crate::tick::LocalStamp;
use flyff_common::config::EventId;
use flyff_common::defs::obj::XI_SKILL_MER_ONE_SPECIALHIT02;
use flyff_common::defs::text::{
    TID_GAME_RAINBOWRACE_CAPTION_APPLICATION, TID_GAME_RAINBOWRACE_CAPTION_FINISH01,
    TID_GAME_RAINBOWRACE_CAPTION_FINISH02, TID_GAME_RAINBOWRACE_CAPTION_READY01,
    TID_GAME_RAINBOWRACE_CAPTION_READY02, TID_GAME_RAINBOWRACE_CAPTION_START,
    TID_GAME_RAINBOWRACE_CAPTION_WAIT, TID_GAME_RAINBOWRACE_NOTELEPORT,
};
use flyff_common::defs::world::WI_WORLD_MADRIGAL;
use flyff_common::math::Vec3;
use flyff_common::{CharacterId, GameMs, LayerId, ObjId, UnixTime, WorldId};
use flyff_db::audit::content as audit;
use flyff_db::{DbOp, PersistHandle, RainbowEntryState, RainbowRaceEntryRecord, RainbowRaceLoad};
use flyff_proto::pb;
use std::collections::BTreeMap;

/// How long a completed game waits before its prize is mailed (`SEC(15)`).
pub const MINI_GAME_PRIZE_DELAY_MS: u64 = 15_000;
/// The longest previous ranking the window shows (`GetPrevRanking`: `i < 5`).
pub const PREV_RANKING_MAX: usize = 5;

/// `CRainbowRaceMng::RR_*`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RrState {
    /// 0 `RR_CLOSED`.
    #[default]
    Closed = 0,
    /// 1 `RR_APPLICATION`: applications are open.
    Application = 1,
    /// 2 `RR_OPEN`: the race opened; the next second decides between Wait and Closed.
    Open = 2,
    /// 3 `RR_WAIT`: the wait before the race (`Wait_Time`).
    Wait = 3,
    /// 4 `RR_READY`: the preparation (`Ready_Time`).
    Ready = 4,
    /// 5 `RR_PROGRESS`: the race (`Progress_Time`).
    Progress = 5,
}

impl RrState {
    /// The legacy code (0-5).
    pub fn code(self) -> u8 {
        self as u8
    }

    /// The name in the GM line (`Closed`, `Application`, `Open`, `Wait`, `Ready`, `Progress`).
    pub fn name(self) -> &'static str {
        match self {
            RrState::Closed => "Closed",
            RrState::Application => "Application",
            RrState::Open => "Open",
            RrState::Wait => "Wait",
            RrState::Ready => "Ready",
            RrState::Progress => "Progress",
        }
    }
}

/// One applicant / racer (`CRainbowRace`).
#[derive(Debug)]
pub struct Racer {
    /// The bit of the game being played (`1 << g`), 0 = none (before the start, after the seventh game, finished).
    pub now_game: u16,
    /// The bits of the completed games (`0x7F` = all); 0 once the racer finished (`SetFinish`).
    pub completed: u16,
    /// The racer reported at the end NPC (`IsFinished`).
    pub finished: bool,
    /// The instance of the current game.
    pub game: Option<Box<dyn RaceGame>>,
    /// When the character applied (the entry row's `applied_at`).
    pub applied_at: UnixTime,
    /// When the racer finished.
    pub finished_at: Option<UnixTime>,
    /// The finishing rank (1-based), 0 while not finished.
    pub ranking: i32,
}

impl Racer {
    /// A fresh applicant (no game yet: it is picked when the race starts).
    pub fn new(applied_at: UnixTime) -> Racer {
        Racer {
            now_game: 0,
            completed: 0,
            finished: false,
            game: None,
            applied_at,
            finished_at: None,
            ranking: 0,
        }
    }
}

/// A read-only picture of the race for tests and a later admin view.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RainbowStatus {
    /// The manager state.
    pub state: RrState,
    /// `next_at - now` (saturating), 0 in Closed.
    pub remain_ms: u32,
    /// The current round (`meta.rainbow_race_round`).
    pub round: u32,
    /// How many applicants / racers there are.
    pub applicants: usize,
    /// Per racer in character order: `(character, now_game, completed, finished)`.
    pub racers: Vec<(CharacterId, u16, u16, bool)>,
    /// The finishing order of the running race.
    pub ranking: Vec<CharacterId>,
    /// The ranking of the previous race (at most five).
    pub prev_ranking: Vec<CharacterId>,
}

/// The rainbow race (`Services.event.rainbow`, Plan/22 §14).
#[derive(Debug, Default)]
pub struct RainbowRaceService {
    /// The manager state.
    pub state: RrState,
    /// When the next transition is due (`SetNextTime`), 0 = none.
    pub next_at: GameMs,
    /// The current round: the key of the entry rows and the counter `rainbow_race_round`.
    pub round: u32,
    /// The applicants and racers by character.
    pub racers: BTreeMap<CharacterId, Racer>,
    /// The finishers in order (`m_vecdwRankingId`).
    pub ranking: Vec<CharacterId>,
    /// The ranking of the previous race, at most five.
    pub prev_ranking: Vec<CharacterId>,
    /// `(racer, completed games, when)` of the mini-game prizes not yet mailed.
    pub prize_queue: Vec<(CharacterId, u32, GameMs)>,
    /// The race NPCs standing in Madrigal.
    pub npcs: Vec<ObjId>,
    /// A GM forced the Application state: the window predicate is ignored until `force_open` / `skip_wait`.
    pub gm_hold: bool,
    /// The inert-script warning was logged.
    pub inert_warned: bool,
    /// How many racers of this race were eliminated (offline at the start, left during the race).
    pub failed: u32,
}

impl RainbowRaceService {
    /// Moves the manager to `to` and logs the transition (the one place that assigns the state).
    pub(crate) fn go(&mut self, to: RrState) {
        if self.state != to {
            tracing::info!(
                from = self.state.name(),
                to = to.name(),
                round = self.round,
                applicants = self.racers.len(),
                "rainbow race: state change"
            );
        }
        self.state = to;
    }

    /// `CRainbowRaceMng::IsEntry`: the state is Progress and `c` races and has not finished.
    pub fn is_entry(&self, c: CharacterId) -> bool {
        self.state == RrState::Progress && self.racers.get(&c).is_some_and(|r| !r.finished)
    }

    /// `next_at - now`, 0 at or after it (FIX: the legacy DWORD underflowed after `/rrnext`).
    pub fn remain_ms(&self, now: GameMs) -> u32 {
        u32::try_from(self.next_at.0.saturating_sub(now.0)).unwrap_or(u32::MAX)
    }

    /// The `remain_ms` of the `RainbowRaceNowState` of a racer: 0 when finished or the race is Closed.
    fn racer_remain_ms(&self, finished: bool, now: GameMs) -> u32 {
        if finished || self.state == RrState::Closed {
            0
        } else {
            self.remain_ms(now)
        }
    }

    /// The picture of the race at game time `now`.
    pub fn status(&self, now: GameMs) -> RainbowStatus {
        RainbowStatus {
            state: self.state,
            remain_ms: if self.state == RrState::Closed {
                0
            } else {
                self.remain_ms(now)
            },
            round: self.round,
            applicants: self.racers.len(),
            racers: self
                .racers
                .iter()
                .map(|(c, r)| (*c, r.now_game, r.completed, r.finished))
                .collect(),
            ranking: self.ranking.clone(),
            prev_ranking: self.prev_ranking.clone(),
        }
    }

    /// The `RainbowRaceNowState` of racer `c` at game time `now`, `None` for a character that does not race.
    pub fn now_state_of(&self, c: CharacterId, now: GameMs) -> Option<pb::RainbowRaceNowState> {
        let r = self.racers.get(&c)?;
        Some(now_state(
            r.now_game,
            r.completed,
            self.racer_remain_ms(r.finished, now),
        ))
    }
}

/// `RainbowRaceNowState{race}`.
pub(crate) fn now_state(now_game: u16, completed: u16, remain_ms: u32) -> pb::RainbowRaceNowState {
    pb::RainbowRaceNowState {
        race: Some(pb::RainbowRace {
            now_game: u32::from(now_game),
            completed_mask: u32::from(completed),
            remain_ms,
        }),
    }
}

// ---- boot ---------------------------------------------------------------------------------------------------------

/// The entry row `c` has in `state` (`ranking` and `finished_at` as given); `applied_at` is the racer's.
fn entry_record(
    round: u32,
    c: CharacterId,
    applied_at: UnixTime,
    state: RainbowEntryState,
    ranking: i32,
    finished_at: Option<UnixTime>,
) -> RainbowRaceEntryRecord {
    RainbowRaceEntryRecord {
        round,
        character_id: c,
        state,
        ranking,
        applied_at,
        finished_at,
    }
}

/// `Game::new` (legacy `S1`, `S2`, `S3`): the round, the applicants (every `N` row of the round, and every `R` row,
/// which the legacy reset to `N` and so do we: a crash during the race loses the race, the applicants keep their paid
/// entry), the previous ranking. `W`, `L` and `F` rows are final and ignored. The entries are loaded whatever the flag
/// (a GM who turns the flag on later finds them). The state is Closed: the race in progress is memory only (E9).
pub fn boot(rr: &mut RainbowRaceService, load: &RainbowRaceLoad, persist: &PersistHandle) {
    *rr = RainbowRaceService {
        round: load.round,
        prev_ranking: load
            .prev_ranking
            .iter()
            .copied()
            .take(PREV_RANKING_MAX)
            .collect(),
        ..RainbowRaceService::default()
    };
    for e in &load.entries {
        match e.state {
            RainbowEntryState::Applied => {}
            RainbowEntryState::Finished => {
                persist.op(DbOp::UpsertRainbowRaceEntry(entry_record(
                    e.round,
                    e.character_id,
                    e.applied_at,
                    RainbowEntryState::Applied,
                    0,
                    None,
                )));
            }
            RainbowEntryState::Winner | RainbowEntryState::Lost | RainbowEntryState::Failed => {
                continue;
            }
        }
        rr.racers.insert(e.character_id, Racer::new(e.applied_at));
    }
}

// ---- small helpers --------------------------------------------------------------------------------------------------

fn text(ctx: &GameCtx<'_>, tid: u32) -> String {
    ctx.data.texts.get(tid).to_owned()
}

/// The character of a live player.
pub(crate) fn character_of(ctx: &GameCtx<'_>, who: PlayerRef) -> Option<CharacterId> {
    ctx.player(who)?.player().map(|p| p.ident.character_id)
}

/// The avatar of an online character that is not being removed.
fn avatar_of(svc: &Services, ctx: &GameCtx<'_>, c: CharacterId) -> Option<ObjId> {
    crate::social::siege::fight::live_avatar(&svc.presence, &*ctx.worlds, c)
}

/// Sets the mirror of [`RainbowRaceService::is_entry`] on an avatar (`PlayerMisc.rainbow_entrant`).
fn set_mirror(ctx: &mut GameCtx<'_>, obj: ObjId, on: bool) {
    if let Some(p) = ctx.worlds.player_mut(obj).and_then(|m| m.player_mut()) {
        p.misc.rainbow_entrant = on;
    }
}

fn caption(svc: &Services, ctx: &mut GameCtx<'_>, line: &str) {
    notice::caption(svc, ctx, Audience::AllOnline, false, WorldId(0), line);
}

fn caption_tid(svc: &Services, ctx: &mut GameCtx<'_>, tid: u32) {
    let line = text(ctx, tid);
    caption(svc, ctx, &line);
}

/// `RainbowRaceNowState` to one avatar.
pub(crate) fn send_now_state(
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    now_game: u16,
    completed: u16,
    remain_ms: u32,
) {
    crate::chat::send_to(ctx, who, now_state(now_game, completed, remain_ms));
}

/// The replies of a game to the racer, in order.
pub(crate) fn send_replies(
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    replies: impl IntoIterator<Item = Reply>,
) {
    for r in replies {
        match r {
            Reply::Plain(p) => {
                crate::chat::send_to(ctx, who, pb::RainbowRaceMiniGameState { packet: Some(p) });
            }
            Reply::Ext(e) => {
                crate::chat::send_to(
                    ctx,
                    who,
                    pb::RainbowRaceMiniGameExtState { packet: Some(e) },
                );
            }
        }
    }
}

/// `true` when the script has no race NPC: the manager does not start a race (22 §2.5 rule 2; the legacy exited the
/// server). A race already past Open runs to its end.
fn inert(s: &Services) -> bool {
    s.event.configs.rainbow.npcs.is_empty()
}

/// Logs the inert-script warning once per empty spell.
fn warn_inert(rr: &mut RainbowRaceService) {
    if !rr.inert_warned {
        rr.inert_warned = true;
        tracing::warn!(
            "RainbowRace.lua has no SetNPC row (missing or faulted script): the rainbow race does not start"
        );
    }
}

// ---- the manager ----------------------------------------------------------------------------------------------------

/// 1 s slot item 7 `event::rainbow::process` (22 §2.3, §14.2), gated on `events.rainbow_race`: one transition per
/// call (`CRainbowRaceMng::Process`). Durations come from the script at the moment of the transition.
pub fn process(s: &mut Services, ctx: &mut GameCtx<'_>) {
    let now = ctx.clock.game_ms;
    let (wait, ready, progress) = {
        let c = &s.event.configs.rainbow;
        (c.wait_time_ms, c.ready_time_ms, c.progress_time_ms)
    };
    let due = now > s.event.rainbow.next_at;
    match s.event.rainbow.state {
        RrState::Closed => {
            if inert(s) {
                warn_inert(&mut s.event.rainbow);
                return;
            }
            s.event.rainbow.inert_warned = false;
            if s.event
                .configs
                .schedule
                .in_rainbow_application(&ctx.clock.local)
            {
                caption_tid(s, ctx, TID_GAME_RAINBOWRACE_CAPTION_APPLICATION);
                s.event.rainbow.go(RrState::Application);
            }
        }
        RrState::Application => {
            // a GM-forced Application ignores the window until `force_open` / `skip_wait` clear the hold
            if s.event.rainbow.gm_hold {
                return;
            }
            if !s
                .event
                .configs
                .schedule
                .in_rainbow_application(&ctx.clock.local)
            {
                let rr = &mut s.event.rainbow;
                rr.next_at = GameMs(0);
                rr.go(RrState::Closed);
            }
        }
        RrState::Open => {
            if inert(s) {
                warn_inert(&mut s.event.rainbow);
                return;
            }
            if s.event.rainbow.racers.is_empty() {
                let rr = &mut s.event.rainbow;
                rr.next_at = GameMs(0);
                rr.go(RrState::Closed);
            } else {
                let rr = &mut s.event.rainbow;
                rr.next_at = GameMs(now.0.saturating_add(u64::from(wait)));
                rr.go(RrState::Wait);
                caption_tid(s, ctx, TID_GAME_RAINBOWRACE_CAPTION_WAIT);
            }
        }
        RrState::Wait => {
            if due {
                let rr = &mut s.event.rainbow;
                rr.next_at = GameMs(now.0.saturating_add(u64::from(ready)));
                rr.go(RrState::Ready);
                caption_tid(s, ctx, TID_GAME_RAINBOWRACE_CAPTION_READY01);
                caption_tid(s, ctx, TID_GAME_RAINBOWRACE_CAPTION_READY02);
            }
        }
        RrState::Ready => {
            if due {
                spawn_npcs(s, ctx);
                caption_tid(s, ctx, TID_GAME_RAINBOWRACE_CAPTION_START);
                let rr = &mut s.event.rainbow;
                rr.next_at = GameMs(now.0.saturating_add(u64::from(progress)));
                rr.go(RrState::Progress);
                begin_race(s, ctx);
            }
        }
        RrState::Progress => {
            if due {
                end_race(s, ctx);
            } else {
                prizes::deliver_mini_game_prizes(s, ctx, false);
            }
        }
    }
}

/// Wall job `rainbow_race_open` (22 §2.4, one `Weekly` per `SetTime` row, `Skip`): flag on, state Closed and not inside
/// an application window (`at`) -> Open; the next [`process`] decides between Wait and Closed. The boundary-crossing
/// and the skipped-second case are the scheduler's (PL-100). A script without race NPC never opens.
pub fn on_open_time(s: &mut Services, _ctx: &mut GameCtx<'_>, at: &LocalStamp) {
    if !s.event.flags.is_on(EventId::RainbowRace) || s.event.rainbow.state != RrState::Closed {
        return;
    }
    if inert(s) {
        warn_inert(&mut s.event.rainbow);
        return;
    }
    if s.event.configs.schedule.in_rainbow_application(at) {
        return;
    }
    s.event.rainbow.go(RrState::Open);
}

/// `SetNPC`: the race NPCs of `tNPC` stand in Madrigal, layer 0, in table order. A row without NPC definition or mover
/// prop is skipped with a warning (FIX: the legacy returned and skipped the rest).
fn spawn_npcs(s: &mut Services, ctx: &mut GameCtx<'_>) {
    let rows = s.event.configs.rainbow.npcs.clone();
    let world = WorldId(WI_WORLD_MADRIGAL);
    for row in rows {
        let (Some(def), Some(prop)) = (ctx.data.npc(&row.char_key), ctx.data.mover(row.mover))
        else {
            tracing::warn!(
                key = %row.char_key,
                name = %row.name,
                "rainbow race: the NPC has no definition or mover prop: not placed"
            );
            continue;
        };
        let (core, sim) =
            crate::mover::npc_parts(ctx.data, prop, Vec3::new(row.x, row.y, row.z), 0.0);
        let mover = crate::mover::Mover::new_npc(
            core,
            sim,
            prop,
            Some(def),
            ctx.data,
            ctx.rng.world(WorldId(1)),
        );
        match ctx.worlds.spawn_mover(world, LayerId(0), mover) {
            Ok(id) => s.event.rainbow.npcs.push(id),
            Err(e) => {
                tracing::warn!(error = %e, key = %row.char_key, "rainbow race: an NPC could not be placed")
            }
        }
    }
}

/// `RemoveNPC`: every race NPC that is still alive vanishes with the effect.
fn remove_npcs(s: &mut Services, ctx: &mut GameCtx<'_>) {
    for id in std::mem::take(&mut s.event.rainbow.npcs) {
        if ctx.worlds.core(id).is_some_and(|c| !c.deleted) {
            crate::social::cheer::sfx(ctx, id, XI_SKILL_MER_ONE_SPECIALHIT02);
            ctx.worlds.delete(id);
        }
    }
}

/// Gives `racer` the next game (`SetNextMiniGame`); none once all seven are done.
pub(crate) fn assign_next(racer: &mut Racer, rng: &mut dyn flyff_common::rng::GameRng) {
    match games::pick_next(racer.completed, rng) {
        Some(g) => {
            racer.now_game = g.bit();
            racer.game = Some(games::new_game(g, rng));
        }
        None => {
            racer.now_game = 0;
            racer.game = None;
        }
    }
}

/// `BeginRainbowRace`, in ascending character id: an applicant who is not online is eliminated (`F`); an online one
/// gets its first game (decision: picked here, not at the application) and its `RainbowRaceNowState`.
fn begin_race(s: &mut Services, ctx: &mut GameCtx<'_>) {
    let now = ctx.clock.game_ms;
    let round = s.event.rainbow.round;
    let ids: Vec<CharacterId> = s.event.rainbow.racers.keys().copied().collect();
    for c in ids {
        match avatar_of(s, ctx, c) {
            None => {
                if let Some(r) = s.event.rainbow.racers.remove(&c) {
                    s.event.rainbow.failed += 1;
                    ctx.persist.op(DbOp::UpsertRainbowRaceEntry(entry_record(
                        round,
                        c,
                        r.applied_at,
                        RainbowEntryState::Failed,
                        0,
                        None,
                    )));
                }
            }
            Some(obj) => {
                let rr = &mut s.event.rainbow;
                let remain = rr.remain_ms(now);
                let Some(r) = rr.racers.get_mut(&c) else {
                    continue;
                };
                assign_next(r, ctx.rng.event());
                let (g, done) = (r.now_game, r.completed);
                set_mirror(ctx, obj, true);
                send_now_state(ctx, PlayerRef(obj), g, done, remain);
            }
        }
    }
}

/// The end of the race (legacy order): the NPCs go, the queued mini-game prizes are mailed, the two finish captions,
/// the rank prizes and the database finalisation in ONE batch, the previous ranking, `Clear`.
fn end_race(s: &mut Services, ctx: &mut GameCtx<'_>) {
    remove_npcs(s, ctx);
    prizes::deliver_mini_game_prizes(s, ctx, true);
    caption_tid(s, ctx, TID_GAME_RAINBOWRACE_CAPTION_FINISH01);
    if let Some(winner) = s.event.rainbow.ranking.first().copied() {
        let name = s
            .presence
            .get(winner)
            .map(|e| e.name.clone())
            .unwrap_or_default();
        let template = text(ctx, TID_GAME_RAINBOWRACE_CAPTION_FINISH02);
        let line = crate::item::upgrade::random_option::format_text(
            &template,
            &[crate::item::upgrade::random_option::Arg::S(&name)],
        );
        caption(s, ctx, &line);
    }
    prizes::finish_round(s, ctx);
    let first_five: Vec<CharacterId> = s
        .event
        .rainbow
        .ranking
        .iter()
        .copied()
        .take(PREV_RANKING_MAX)
        .collect();
    s.event.rainbow.prev_ranking = first_five;
    clear(s, ctx);
}

/// `Clear`: every unfinished online racer gets the reset `RainbowRaceNowState{0, 0, 0}`, every online racer loses the
/// teleport mirror, the race state is dropped, the round counts up, the manager is Closed.
fn clear(s: &mut Services, ctx: &mut GameCtx<'_>) {
    let racers: Vec<(CharacterId, bool)> = s
        .event
        .rainbow
        .racers
        .iter()
        .map(|(c, r)| (*c, r.finished))
        .collect();
    for (c, finished) in racers {
        if let Some(obj) = avatar_of(s, ctx, c) {
            set_mirror(ctx, obj, false);
            if !finished {
                send_now_state(ctx, PlayerRef(obj), 0, 0, 0);
            }
        }
    }
    let rr = &mut s.event.rainbow;
    rr.racers.clear();
    rr.ranking.clear();
    rr.prize_queue.clear();
    rr.npcs.clear();
    rr.failed = 0;
    rr.round = rr.round.saturating_add(1);
    rr.next_at = GameMs(0);
    rr.gm_hold = false;
    rr.go(RrState::Closed);
}

// ---- a racer's progress ------------------------------------------------------------------------------------------------

/// A game of `c` is complete (it returned `completed`, or `/rrpass`): the bit is set, the next game is picked and
/// `RainbowRaceNowState` follows the game's own FINISH reply; the mini-game prize of the new count is queued.
pub(crate) fn complete_current(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    c: CharacterId,
) {
    let now = ctx.clock.game_ms;
    let rr = &mut svc.event.rainbow;
    let remain = rr.remain_ms(now);
    let Some(r) = rr.racers.get_mut(&c) else {
        return;
    };
    r.completed |= r.now_game;
    r.now_game = 0;
    r.game = None;
    assign_next(r, ctx.rng.event());
    let (g, done) = (r.now_game, r.completed);
    let count = done.count_ones();
    rr.prize_queue.push((c, count, now));
    send_now_state(ctx, who, g, done, remain);
}

/// `SetRanking` for a racer who passed every check: `SetFinish`, the state reset to `{0, 0, 0}`, the rank, the entry row
/// `R`, the audit line and the rank text in a message box.
pub(crate) fn finish_racer(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    c: CharacterId,
) {
    let now_unix = UnixTime(ctx.clock.unix_ms.0.div_euclid(1000));
    let rr = &mut svc.event.rainbow;
    let round = rr.round;
    let rank = i32::try_from(rr.ranking.len() + 1).unwrap_or(i32::MAX);
    let Some(r) = rr.racers.get_mut(&c) else {
        return;
    };
    r.finished = true;
    r.now_game = 0;
    r.completed = 0;
    r.game = None;
    r.ranking = rank;
    r.finished_at = Some(now_unix);
    let applied_at = r.applied_at;
    rr.ranking.push(c);
    set_mirror(ctx, who.0, false);
    send_now_state(ctx, who, 0, 0, 0);
    ctx.persist.op(DbOp::UpsertRainbowRaceEntry(entry_record(
        round,
        c,
        applied_at,
        RainbowEntryState::Finished,
        rank,
        Some(now_unix),
    )));
    let mut ev = audit::rainbow_finish(round, c, rank);
    if let Some(actor) = crate::audit::actor_of(ctx, who) {
        ev = ev.by(actor);
    }
    ctx.audit.emit(ev);
    let template = text(ctx, flyff_common::defs::text::TID_GAME_RAINBOWRACE_FINISH);
    let line = crate::item::upgrade::random_option::format_text(
        &template,
        &[crate::item::upgrade::random_option::Arg::D(i64::from(rank))],
    );
    notice::diag(ctx, who, &line);
}

// ---- the teleport block and the logout --------------------------------------------------------------------------------

/// `IsEntry` of an avatar (the party summon, the messenger summons and the couple ring ask this way).
pub fn is_entry_obj(svc: &Services, ctx: &GameCtx<'_>, obj: ObjId) -> bool {
    ctx.worlds
        .player(obj)
        .and_then(|m| m.player())
        .is_some_and(|p| svc.event.rainbow.is_entry(p.ident.character_id))
}

/// The rainbow part of the teleport block (22 §2.2): `Some(TID_GAME_RAINBOWRACE_NOTELEPORT)` while `who` is an entrant.
/// The text goes where the existing code already sends it; the callers decide.
pub fn teleport_block(svc: &Services, ctx: &GameCtx<'_>, who: PlayerRef) -> Option<u32> {
    is_entry_obj(svc, ctx, who.0).then_some(TID_GAME_RAINBOWRACE_NOTELEPORT)
}

/// `LogoutStep::RainbowDropOut` (step 4 of `destroy_player`, `User.cpp:4215-4219`; `SetDropOut`): in Progress an
/// unfinished racer is eliminated (`F`, removed from the table; the queued prizes and the ranking stay). Outside
/// Progress the application stays. The mover is dropped by the logout itself, so the mirror needs no reset.
pub fn on_logout(svc: &mut Services, ctx: &mut GameCtx<'_>, player: PlayerRef) {
    let Some(c) = character_of(ctx, player) else {
        return;
    };
    let rr = &mut svc.event.rainbow;
    if rr.state != RrState::Progress {
        return;
    }
    if rr.racers.get(&c).is_none_or(|r| r.finished) {
        return;
    }
    let round = rr.round;
    if let Some(r) = rr.racers.remove(&c) {
        rr.failed += 1;
        ctx.persist.op(DbOp::UpsertRainbowRaceEntry(entry_record(
            round,
            c,
            r.applied_at,
            RainbowEntryState::Failed,
            0,
            None,
        )));
    }
}
