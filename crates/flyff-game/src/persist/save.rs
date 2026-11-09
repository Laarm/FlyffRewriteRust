//! The save policy of the game thread (Plan/13 §17.1, WP-DB-07): generations, the 90 s staggered periodic
//! snapshots, urgent snapshots, the world-change trigger and the helper every trigger goes through.
//!
//! * [`SaveGenCounter`]: the **global** monotonic generation (starts at 1 each process run, 0 = nothing); every
//!   `CharacterSnapshot` carries a fresh one (13 §16.5).
//! * [`SaveScheduler`]: per online character a periodic deadline (`[save] interval_s`, reset by **any** snapshot of
//!   the character, so saves are staggered by join time), at most `max_periodic_per_tick` periodic snapshots per
//!   tick (overdue ones roll over), urgent snapshots at most one per `urgent_min_interval_ms`, and the world-change
//!   snapshot (`[save] on_world_change`). It only decides *when*; [`save_player`] builds and sends the snapshot.
//! * [`save_player`]: gen, snapshot ([`super::snapshot::build`]), `PersistHandle::save`, scheduler bookkeeping. A
//!   player-scoped panic taints the session (`SessionEntry.tainted`): nothing is ever built for it again (13 §20).
//!
//! The pass runs in the tick phase "timers" (3.8, [`crate::tick::Hook::SaveScheduler`]), after the worlds and the
//! handlers: periodic, urgent and world-change snapshots are never taken in the middle of a handler (13 §17.2).

use super::snapshot::{self, Location, SnapshotCtx};
use crate::ctx::{GameCtx, Services};
use flyff_common::{GameMs, ObjId, SaveGen};
use flyff_db::SaveReason;
use flyff_db::config::SaveConfig;
use std::collections::{BTreeMap, BTreeSet};

/// The global save generation counter (13 §16.5): `next()` returns 1, 2, 3, ...
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SaveGenCounter {
    last: u64,
}

impl SaveGenCounter {
    /// A counter that has handed out nothing.
    pub const fn new() -> Self {
        SaveGenCounter { last: 0 }
    }

    /// A fresh generation (never 0, strictly increasing).
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> SaveGen {
        self.last = self.last.saturating_add(1);
        SaveGen(self.last)
    }

    /// The last generation handed out (`SaveGen::ZERO` before the first).
    pub const fn last(&self) -> SaveGen {
        SaveGen(self.last)
    }
}

#[derive(Clone, Copy, Debug)]
struct SaveState {
    /// When the next periodic snapshot is due.
    periodic_due: GameMs,
    /// The tick time of the last snapshot (urgent rate limit).
    last_snapshot: Option<GameMs>,
    /// The generation of the last snapshot sent for the character (reported by `Left` of a tainted leave).
    last_gen: SaveGen,
    /// A handler asked for an urgent snapshot (`mark_urgent`).
    urgent: bool,
    /// A world change asked for a snapshot.
    world_change: bool,
}

/// Decides when the snapshots of the online characters are taken (13 §17.1).
#[derive(Clone, Debug)]
pub struct SaveScheduler {
    cfg: SaveConfig,
    gens: SaveGenCounter,
    players: BTreeMap<ObjId, SaveState>,
    /// `(deadline, player)` of every tracked character, ascending.
    periodic: BTreeSet<(GameMs, ObjId)>,
    /// Snapshots taken, by reason (tests, `/metrics`).
    pub taken: SaveCounters,
}

/// Counters of the snapshots the scheduler emitted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SaveCounters {
    /// `Periodic`.
    pub periodic: u64,
    /// `Urgent`.
    pub urgent: u64,
    /// `WorldChange`.
    pub world_change: u64,
    /// `Logout`, `ToCharSelect` and `Shutdown`.
    pub logout: u64,
    /// `Shutdown` (also counted in `logout`): the final snapshots of the shutdown countdown.
    pub shutdown: u64,
    /// Snapshots refused because the session is tainted.
    pub tainted_skipped: u64,
}

impl Default for SaveScheduler {
    fn default() -> Self {
        SaveScheduler::new(SaveConfig::default())
    }
}

impl SaveScheduler {
    /// A scheduler with the `[save]` section `cfg`.
    pub fn new(cfg: SaveConfig) -> SaveScheduler {
        SaveScheduler {
            cfg,
            gens: SaveGenCounter::new(),
            players: BTreeMap::new(),
            periodic: BTreeSet::new(),
            taken: SaveCounters::default(),
        }
    }

    /// The `[save]` section in force.
    pub fn config(&self) -> &SaveConfig {
        &self.cfg
    }

    fn interval_ms(&self) -> u64 {
        u64::from(self.cfg.interval_s).saturating_mul(1000)
    }

    /// A fresh generation (13 §16.5).
    pub fn next_gen(&mut self) -> SaveGen {
        self.gens.next()
    }

    /// The last generation handed out.
    pub fn last_issued(&self) -> SaveGen {
        self.gens.last()
    }

    /// The character entered the world at `now`: its first periodic snapshot is due `interval_s` later.
    pub fn track(&mut self, player: ObjId, now: GameMs) {
        self.untrack(player);
        let due = GameMs(now.0.saturating_add(self.interval_ms()));
        self.players.insert(
            player,
            SaveState {
                periodic_due: due,
                last_snapshot: None,
                last_gen: SaveGen::ZERO,
                urgent: false,
                world_change: false,
            },
        );
        self.periodic.insert((due, player));
    }

    /// The character left the world.
    pub fn untrack(&mut self, player: ObjId) {
        if let Some(s) = self.players.remove(&player) {
            self.periodic.remove(&(s.periodic_due, player));
        }
    }

    /// `true` while the character is tracked (in the world).
    pub fn is_tracked(&self, player: ObjId) -> bool {
        self.players.contains_key(&player)
    }

    /// Number of tracked characters.
    pub fn len(&self) -> usize {
        self.players.len()
    }

    /// `true` when nobody is tracked.
    pub fn is_empty(&self) -> bool {
        self.players.is_empty()
    }

    /// The generation of the last snapshot sent for the character (`SaveGen::ZERO` when none).
    pub fn last_gen(&self, player: ObjId) -> SaveGen {
        self.players
            .get(&player)
            .map_or(SaveGen::ZERO, |s| s.last_gen)
    }

    /// `scheduler.mark_urgent(character)` (13 §17.1): one snapshot at the end of the tick, at most one per
    /// `urgent_min_interval_ms`. An untracked character is ignored.
    pub fn mark_urgent(&mut self, player: ObjId) {
        if let Some(s) = self.players.get_mut(&player) {
            s.urgent = true;
        }
    }

    /// The world id of the character changed (13 §17.1 "World change"): one `WorldChange` snapshot, unless
    /// `[save] on_world_change` is off.
    pub fn request_world_change(&mut self, player: ObjId) {
        if !self.cfg.on_world_change {
            return;
        }
        if let Some(s) = self.players.get_mut(&player) {
            s.world_change = true;
        }
    }

    /// A snapshot of `player` with generation `gen` was sent at `now`: the periodic timer restarts (**any**
    /// snapshot resets it), the urgent and world-change requests are served.
    pub fn record(&mut self, player: ObjId, gen_sent: SaveGen, now: GameMs, reason: SaveReason) {
        let interval = self.interval_ms();
        if let Some(s) = self.players.get_mut(&player) {
            self.periodic.remove(&(s.periodic_due, player));
            s.periodic_due = GameMs(now.0.saturating_add(interval));
            self.periodic.insert((s.periodic_due, player));
            s.last_snapshot = Some(now);
            s.last_gen = gen_sent;
            s.urgent = false;
            s.world_change = false;
        }
        match reason {
            SaveReason::Periodic => self.taken.periodic += 1,
            SaveReason::Urgent => self.taken.urgent += 1,
            SaveReason::WorldChange => self.taken.world_change += 1,
            SaveReason::Logout | SaveReason::ToCharSelect => self.taken.logout += 1,
            SaveReason::Shutdown => {
                self.taken.logout += 1;
                self.taken.shutdown += 1;
            }
            SaveReason::Atomic => {}
        }
    }

    /// The snapshots the pass of this tick takes: world-change requests, then urgent ones (rate limited), then
    /// the periodic ones that are due, in `(deadline, ObjId)` order, at most `max_periodic_per_tick` of them
    /// (overdue ones roll to the next tick). One snapshot per character per pass.
    pub fn take_due(&self, now: GameMs) -> Vec<(ObjId, SaveReason)> {
        let mut out: Vec<(ObjId, SaveReason)> = Vec::new();
        let mut chosen: BTreeSet<ObjId> = BTreeSet::new();
        for (&p, s) in &self.players {
            if s.world_change {
                out.push((p, SaveReason::WorldChange));
                chosen.insert(p);
            }
        }
        let min = u64::from(self.cfg.urgent_min_interval_ms);
        for (&p, s) in &self.players {
            if s.urgent
                && !chosen.contains(&p)
                && s.last_snapshot
                    .is_none_or(|t| now.0.saturating_sub(t.0) >= min)
            {
                out.push((p, SaveReason::Urgent));
                chosen.insert(p);
            }
        }
        let cap = self.cfg.max_periodic_per_tick.max(1) as usize;
        let mut periodic = 0;
        for &(due, p) in &self.periodic {
            if due > now || periodic >= cap {
                break;
            }
            if chosen.insert(p) {
                out.push((p, SaveReason::Periodic));
                periodic += 1;
            }
        }
        out
    }
}

/// Builds and sends one snapshot of `player` (13 §17.2): `false` when there is nothing to save (not a live
/// player, no session entry, or the session is tainted: **never** a snapshot after a panic, 13 §20).
///
/// `location` replaces the stored position (the logout location of `destroy_player`). On success the generation
/// is returned and the scheduler's bookkeeping is updated.
pub fn save_player(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    player: ObjId,
    reason: SaveReason,
    location: Option<Location>,
) -> Option<SaveGen> {
    let entry = ctx.sessions.get(player)?;
    if entry.tainted {
        svc.saves.taken.tainted_skipped += 1;
        return None;
    }
    let mover = ctx.worlds.player(player)?;
    let gen_now = svc.saves.next_gen();
    let snap = snapshot::build(
        mover,
        &SnapshotCtx {
            entry,
            generation: gen_now,
            reason,
            now: ctx.clock.game_ms,
            unix_ms: ctx.clock.unix_ms,
            max_pack: u16::try_from(ctx.cfg.rules.gameplay.max_pack_item).unwrap_or(u16::MAX),
            location,
        },
    )?;
    ctx.persist.save(snap);
    svc.saves.record(player, gen_now, ctx.clock.game_ms, reason);
    Some(gen_now)
}

/// The pass of tick phase 3.8: the world-change requests queued by `_replace`, then every snapshot the
/// scheduler says is due.
pub fn run_pass(svc: &mut Services, ctx: &mut GameCtx<'_>) {
    for player in ctx.worlds.take_save_requests() {
        svc.saves.request_world_change(player);
    }
    let now = ctx.clock.game_ms;
    for (player, reason) in svc.saves.take_due(now) {
        if save_player(svc, ctx, player, reason, None).is_none() {
            // not savable (tainted or gone): keep it from being re-selected every tick
            svc.saves.untrack(player);
        }
    }
}
