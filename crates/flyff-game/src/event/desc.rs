//! `event::desc`: `EventLuaDesc` (Plan/22 §4.12, WP-EVENT-04) and the events' view of the per-player 1 s timer.
//!
//! The messages:
//!
//! * **start / stop** (`state` 1 / 0): [`announce`] sends the description of every changed event with a non-empty
//!   description to everybody who is in a world (legacy `CUserMng::AddEventLua`), after the P9 pass and after a
//!   reload of `Event.lua`,
//! * **join** (`state` 2): [`EventLuaDescStep`] writes one message per running event with a non-empty description,
//!   in ascending event id (legacy `AddEventLuaDesc`, `JoinStep::EventLuaDesc`),
//! * **resend** (`state` 2): the same set again, every 90 s per player. The event side has **no timer**: the
//!   countdown is doc 16's `regen.slow_countdown`, which calls [`SecondHooks::resend_lua_desc`] on the 1 s timer of
//!   the tick tail ([`EventSecondHooks`]) — and [`resend_lua_desc`] is the same thing for a caller that has the
//!   services (a GM command, tests).
//!
//! The 1 s timer runs in the world phase, where there are no `Services`: the hook reads the descriptions from a
//! [`DescBoard`], a shared snapshot that [`EventLuaService`](super::EventLuaService) republishes whenever the set of
//! running events changes. `flyff-server` builds one board, gives it to [`EventSecondHooks`] (installed in
//! `StatsHooks::with_second`) and to the game through `Handlers.event_desc`.

use super::lua_events::EventChange;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::outbox::{Audience, Outbox};
use crate::stats::tick::{SecondCx, SecondHooks};
use crate::world::join::{JoinContributor, JoinCtx, JoinStep};
use flyff_common::{CharacterId, ObjId};
use flyff_proto::pb;
use smallvec::smallvec;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// A one-slot channel from the game to the world-phase 1 s timer (no `Mutex`: game state travels on channels, D2):
/// the game [`publish`](DescBoard::publish)es the descriptions of the running events (non-empty ones, ascending event
/// id) whenever they may have changed, and [`EventSecondHooks`] reads the newest snapshot when its 90 s countdown
/// fires. Both sides hold a clone.
#[derive(Clone, Debug)]
pub struct DescBoard {
    tx: flume::Sender<Arc<Vec<String>>>,
    rx: flume::Receiver<Arc<Vec<String>>>,
    /// What the 1 s timer needs of the Event family (Event.lua extras, 22 §4.8-§4.9).
    facts: Arc<SecondFacts>,
    /// The keep-connect channel: the 1 s timer sends the character, [`super::lua_events::keep_connect_drain`]
    /// receives.
    keep_tx: flume::Sender<CharacterId>,
    keep_rx: flume::Receiver<CharacterId>,
}

impl Default for DescBoard {
    fn default() -> Self {
        let (tx, rx) = flume::unbounded();
        let (keep_tx, keep_rx) = flume::unbounded();
        DescBoard {
            tx,
            rx,
            facts: Arc::default(),
            keep_tx,
            keep_rx,
        }
    }
}

/// What the world-phase 1 s timer reads of the Event family (it has no `Services`): the family state and the two
/// intervals of the running events. Stored by every [`EventLuaService::refresh`](super::EventLuaService::refresh)
/// with `Relaxed` ordering; the timer reads it on the same thread (the world phase runs inside the tick).
#[derive(Debug, Default)]
pub struct SecondFacts {
    /// The `Event` family is `Ready` (the legacy `IsPossible()`).
    pub ready: AtomicBool,
    /// `GetCouponEvent()` in ms, 0 = off.
    pub coupon_ms: AtomicU32,
    /// `GetKeepConnectTime()` in ms, 0 = off.
    pub keep_connect_ms: AtomicU32,
}

impl SecondFacts {
    /// Stores the facts of a refresh.
    pub fn store(&self, ready: bool, coupon_ms: u32, keep_connect_ms: u32) {
        self.ready.store(ready, Ordering::Relaxed);
        self.coupon_ms.store(coupon_ms, Ordering::Relaxed);
        self.keep_connect_ms
            .store(keep_connect_ms, Ordering::Relaxed);
    }
}

impl DescBoard {
    /// Replaces the pending snapshot (at most one is ever pending).
    pub fn publish(&self, descriptions: Vec<String>) {
        while self.rx.try_recv().is_ok() {}
        let _ = self.tx.send(Arc::new(descriptions));
    }

    /// The newest snapshot published since the last call, if any.
    fn take_newest(&self) -> Option<Arc<Vec<String>>> {
        self.rx.try_iter().last()
    }

    /// The facts of the 1 s timer (see [`SecondFacts`]).
    pub fn facts(&self) -> &SecondFacts {
        &self.facts
    }

    /// The characters whose keep-connect interval ran out since the last call, in arrival order.
    pub fn take_keep_connect(&self) -> Vec<CharacterId> {
        self.keep_rx.try_iter().collect()
    }

    /// How many characters wait on the channel (tests).
    pub fn pending_keep_connect(&self) -> usize {
        self.keep_rx.len()
    }

    /// Queues a character for the keep-connect mail (what the 1 s timer does; tests).
    pub fn queue_keep_connect(&self, who: CharacterId) {
        let _ = self.keep_tx.send(who);
    }
}

fn desc_msg(state: i32, description: &str) -> pb::EventLuaDesc {
    pb::EventLuaDesc {
        state,
        description: description.to_owned(),
    }
}

/// The hooks of doc 16's 1 s timer that belong to the events: the coupon and keep-connect steps (legacy steps 2 and
/// 3, [`SecondHooks::on_player_second`], WP-EVENT-06) and the 90 s hook (`SecondHooks::resend_lua_desc`, legacy
/// step 5 `AddEventLuaDesc`): the running events' descriptions again, as `state` 2 to the player. The rest points
/// of the guild house (legacy step 6, WP-EVENT-15) are added at the marked line.
#[derive(Clone, Debug, Default)]
pub struct EventSecondHooks {
    board: DescBoard,
    latest: Arc<Vec<String>>,
}

impl EventSecondHooks {
    /// Hooks over `board` (the board the game publishes to: `Handlers.event_desc`).
    pub fn new(board: DescBoard) -> Self {
        Self {
            board,
            latest: Arc::default(),
        }
    }
}

impl SecondHooks for EventSecondHooks {
    /// Legacy steps 2, 3 and 6 of the 1 s timer (`User.cpp:507-512`, 22 §4.8, §4.9): the coupon and the keep-connect
    /// tick sit in ONE `if ready` block (a family that is not `Ready` touches neither tick: `SetCoupon` and
    /// `SetKeepConnectEvent` return on `!IsPossible()`); the later steps run whatever the family state, so there
    /// is no early return.
    fn on_player_second(&mut self, player: &mut crate::mover::Mover, cx: &mut SecondCx<'_>) {
        let facts = self.board.facts();
        if facts.ready.load(Ordering::Relaxed) {
            let coupon_ms = facts.coupon_ms.load(Ordering::Relaxed);
            let keep_ms = facts.keep_connect_ms.load(Ordering::Relaxed);
            coupon_step(player, cx, coupon_ms);
            keep_connect_step(&self.board, player, cx, keep_ms);
        }

        // legacy step 6: guild-house rest points (task guild-house)
        super::guild_house::rest::on_player_second(player, cx);
    }

    fn resend_lua_desc(&mut self, _player: &mut crate::mover::Mover, cx: &mut SecondCx<'_>) {
        if let Some(newest) = self.board.take_newest() {
            self.latest = newest;
        }
        for d in self.latest.iter() {
            cx.out.to_self(desc_msg(2, d));
        }
    }
}

/// `CEventLua::SetCoupon` (22 §4.8): with the event on, the strict `now > tick + interval` adds one coupon,
/// restarts the tick and tells the player (`EventCoupon{coupons}`); with the event off the tick follows the clock.
fn coupon_step(m: &mut crate::mover::Mover, cx: &mut SecondCx<'_>, coupon_ms: u32) {
    let Some(p) = m.player_mut() else {
        return;
    };
    if coupon_ms == 0 {
        p.misc.coupon_tick = cx.now;
        return;
    }
    if cx.now.0 > p.misc.coupon_tick.0.saturating_add(u64::from(coupon_ms)) {
        p.misc.coupon_count = p.misc.coupon_count.saturating_add(1);
        p.misc.coupon_tick = cx.now;
        cx.out.to_self(pb::EventCoupon {
            coupons: p.misc.coupon_count,
        });
    }
}

/// `CEventLua::SetKeepConnectEvent` (22 §4.9): the same comparison; when due the character is queued for the
/// mail of [`super::lua_events::keep_connect_drain`].
fn keep_connect_step(
    board: &DescBoard,
    m: &mut crate::mover::Mover,
    cx: &mut SecondCx<'_>,
    keep_ms: u32,
) {
    let Some(p) = m.player_mut() else {
        return;
    };
    if keep_ms == 0 {
        p.misc.keep_connect_tick = cx.now;
        return;
    }
    let due_after = p
        .misc
        .keep_connect_tick
        .0
        .saturating_add(u64::from(keep_ms));
    if cx.now.0 > due_after {
        p.misc.keep_connect_tick = cx.now;
        board.queue_keep_connect(p.ident.character_id);
    }
}

/// `event::resend_lua_desc` (22 §2.2): sends the running events' descriptions to `p` again (`state` 2). It keeps no
/// timer; doc 16's 90 s countdown reaches the same messages through [`EventSecondHooks`].
pub fn resend_lua_desc(s: &mut Services, ctx: &mut GameCtx<'_>, p: PlayerRef) {
    for d in s.event.lua_events.descriptions() {
        ctx.broadcast(
            None,
            ObjId(0),
            Audience::Players(smallvec![p.0]),
            desc_msg(2, &d),
        );
    }
}

/// Tells everybody about the events that started or stopped (`state` 1 / 0); changes without a description are
/// silent (22 §4.2 step 2).
pub fn announce(ctx: &mut GameCtx<'_>, changes: &[EventChange]) {
    for c in changes.iter().filter(|c| !c.desc.is_empty()) {
        ctx.broadcast(
            None,
            ObjId(0),
            Audience::AllOnline,
            desc_msg(i32::from(c.running), &c.desc),
        );
    }
}

/// `EventLuaDesc` x N of the join batch (11 §2.5, 15 §9.2): one message per running event with a description.
pub struct EventLuaDescStep;

impl JoinContributor for EventLuaDescStep {
    fn step(&self) -> JoinStep {
        JoinStep::EventLuaDesc
    }

    fn write(&self, cx: &JoinCtx<'_>, out: &mut Outbox) {
        for d in cx.svc.event.lua_events.descriptions() {
            out.to_session(cx.session, 0, desc_msg(2, &d));
        }
    }
}
