//! `event::hooks`: the event-side hook objects of doc 22 (Plan/22 §2.2): the world hooks, the logout object, the
//! join policy and the layer drive-out that the events work packages extend, and the notes the world phase leaves
//! for them (the world phase has no `Services`).
//!
//! | object | installed by | content |
//! |---|---|---|
//! | [`EventWorldHooks`] | `wiring.rs` around the `StatsHooks` chain, as the `inner` of `MovementHooks` | answers `is_instance_world`; turns the portal trigger (`instance_enter`) and `replace_leaving` into [`WorldNote`]s; forwards everything else |
//! | [`EventLogout`] | `wiring.rs`, the OUTERMOST `with_logout` link | answers `is_instance_world` and `LogoutStep::InstanceGoOut`; forwards every other step and predicate |
//! | [`EventJoinPolicy`] | `wiring.rs` `with_policy` | answers `is_instance_world` and `instance_go_out`; the other predicates keep the trait defaults |
//! | [`EventDriveOut`] | `Game::new` `set_drive_out` | `LayerKind::Instance` -> the marking position; every other kind -> [`MadrigalDriveOut`] |
//! | [`WorldNotes`] | `Handlers.event_world` (the game keeps the other end in `Services.event.instances`) | the channel of [`WorldNote`]s |
//! | [`drain_world_notes`] | `Game::phase_worlds`, right after the siege `drain_deaths` | the one place that drains the notes of every variant |
//!
//! The siblings (guild-house, housing, secret-room, rainbow-race) add their code at their own
//! `// ---- <task>` banners below, and nowhere else.

use super::instance::{self, InstanceWorldSet};
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::world::Worlds;
use crate::world::join::{JoinPolicy, SavedLocation};
use crate::world::layer::{LayerDriveOut, LayerKind, MadrigalDriveOut, OnDieEntry};
use crate::world::logout::{LogoutHooks, LogoutStep};
use crate::world::object::{CtrlKey, MoverKey, ObjectStore, RespawnOrigin};
use crate::world::phases::{FrameCtx, WorldHooks};
use crate::world::visibility::Phase;
use flyff_common::defs::world::WI_WORLD_MINIROOM;
use flyff_common::math::Vec3;
use flyff_common::{CharacterId, LayerId, ObjId, SimFrame, WorldId};
use flyff_data::types::MenuId;
use flyff_data::worlds::RegionDef;

// ---- the notes -------------------------------------------------------------------------------------------------

/// What the world phase noticed for the event services; handled by [`drain_world_notes`] after the phase.
#[derive(Clone, Debug, PartialEq)]
pub enum WorldNote {
    /// A player walked into an instance-dungeon portal (`EnteranceDungeon`). `from` is the world and layer he
    /// stood in when the portal fired, `world` / `pos` the region's `tele_world` / `tele_pos`.
    InstancePortal {
        /// The player.
        player: ObjId,
        /// Where he stood.
        from: (WorldId, LayerId),
        /// The dungeon world.
        world: WorldId,
        /// The portal's target position.
        pos: Vec3,
    },
    /// A player changed world or layer in `_replace` (`WorldHooks::replace_leaving`).
    LayerChanged {
        /// The player.
        player: ObjId,
        /// The world and layer he left.
        from: (WorldId, LayerId),
        /// The world and layer he entered.
        to: (WorldId, LayerId),
    },
    // ---- housing (doc 22 §13, task housing) ----
    /// A player came into a mini room (world 209) by a replace (`WorldHooks::world_change_entered`): the papering of the
    /// room is sent to him after the drain (`World.cpp:1656-1670`).
    RoomEntered {
        /// The player.
        player: ObjId,
        /// The room layer he entered.
        layer: LayerId,
    },
    // ---- guild-house, housing, secret-room, rainbow-race: your variants at your own banner ----
}

/// The channel of [`WorldNote`]s: a cloneable flume pair (game state travels on channels, D2). `Default` is a fresh
/// pair that no hook holds the sender of.
#[derive(Clone, Debug)]
pub struct WorldNotes {
    tx: flume::Sender<WorldNote>,
    rx: flume::Receiver<WorldNote>,
}

impl Default for WorldNotes {
    fn default() -> Self {
        let (tx, rx) = flume::unbounded();
        WorldNotes { tx, rx }
    }
}

impl WorldNotes {
    /// Leaves a note.
    pub fn post(&self, note: WorldNote) {
        // the receiver lives as long as this value: the send cannot fail
        let _ = self.tx.send(note);
    }

    /// Takes the notes posted so far, oldest first.
    pub fn take(&self) -> Vec<WorldNote> {
        self.rx.try_iter().collect()
    }
}

/// The one place that drains the notes of every variant (`Game::phase_worlds`, after the world phase and the
/// siege deaths).
///
/// * `InstancePortal` -> [`instance::on_portal`], unless the player is gone, already has a queued replace or no longer
///   stands where the portal fired (a duplicate trigger of a player who is already on his way in counts once);
/// * `LayerChanged` -> [`instance::on_replace`].
pub fn drain_world_notes(svc: &mut Services, ctx: &mut GameCtx<'_>) {
    for note in svc.event.instances.notes.take() {
        match note {
            WorldNote::InstancePortal {
                player,
                from,
                world,
                pos,
            } => {
                let stands = ctx
                    .worlds
                    .player(player)
                    .is_some_and(|m| (m.core.world, m.core.layer) == from);
                let queued = ctx
                    .worlds
                    .layer(from.0, from.1)
                    .is_some_and(|l| l.queues.replace.iter().any(|e| e.id == player));
                if stands && !queued {
                    instance::on_portal(svc, ctx, player, world, pos);
                }
            }
            WorldNote::LayerChanged { player, from, to } => {
                instance::on_replace(svc, ctx, player, from, to);
                // ---- guild-house (doc 22 §12, task guild-house): the room of a house lets go when its last player left ----
                super::guild_house::on_layer_changed(svc, ctx, player, from, to);
            }
            // ---- housing (doc 22 §13, task housing) ----
            WorldNote::RoomEntered { player, layer } => {
                super::housing::on_room_entered(svc, ctx, player, layer);
            }
        }
    }
}

// ---- the world hooks -------------------------------------------------------------------------------------------

/// The doc-22 world hooks (`MovementHooks`' inner): `is_instance_world` from the dungeon set, `instance_enter` and
/// `replace_leaving` as notes, every other hook forwarded unchanged to the wrapped hooks (a dropped forward would
/// silently kill `StatsHooks::tail`).
pub struct EventWorldHooks {
    set: InstanceWorldSet,
    notes: WorldNotes,
    inner: Option<Box<dyn WorldHooks>>,
}

impl EventWorldHooks {
    /// The hooks for the dungeon worlds `set`, noting into `notes`.
    pub fn new(set: InstanceWorldSet, notes: WorldNotes) -> EventWorldHooks {
        EventWorldHooks {
            set,
            notes,
            inner: None,
        }
    }

    /// Chains `inner` behind: it sees every hook but the two this object answers.
    pub fn with_inner(mut self, inner: Box<dyn WorldHooks>) -> EventWorldHooks {
        self.inner = Some(inner);
        self
    }
}

impl std::fmt::Debug for EventWorldHooks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventWorldHooks").finish_non_exhaustive()
    }
}

#[deny(clippy::missing_trait_methods)]
impl WorldHooks for EventWorldHooks {
    fn on_phase(&mut self, world: WorldId, layer: LayerId, phase: Phase) {
        if let Some(i) = self.inner.as_mut() {
            i.on_phase(world, layer, phase);
        }
    }

    fn init_ai(&mut self, store: &mut ObjectStore, id: ObjId) {
        if let Some(i) = self.inner.as_mut() {
            i.init_ai(store, id);
        }
    }

    fn on_npc_added(&mut self, id: ObjId, world: WorldId, layer: LayerId, menus: &[MenuId]) {
        if let Some(i) = self.inner.as_mut() {
            i.on_npc_added(id, world, layer, menus);
        }
    }

    fn ai_begin(&mut self, ctx: &mut FrameCtx<'_>, awake: &[MoverKey]) {
        if let Some(i) = self.inner.as_mut() {
            i.ai_begin(ctx, awake);
        }
    }

    fn ai(&mut self, ctx: &mut FrameCtx<'_>, mover: MoverKey) {
        if let Some(i) = self.inner.as_mut() {
            i.ai(ctx, mover);
        }
    }

    fn snapshot(&mut self, ctx: &mut FrameCtx<'_>, frame: SimFrame, awake: &[MoverKey]) {
        if let Some(i) = self.inner.as_mut() {
            i.snapshot(ctx, frame, awake);
        }
    }

    fn step(&mut self, ctx: &mut FrameCtx<'_>, frame: SimFrame, mover: MoverKey) {
        if let Some(i) = self.inner.as_mut() {
            i.step(ctx, frame, mover);
        }
    }

    fn barrier(&mut self, ctx: &mut FrameCtx<'_>, frame: SimFrame, awake: &[MoverKey]) {
        if let Some(i) = self.inner.as_mut() {
            i.barrier(ctx, frame, awake);
        }
    }

    fn tail(&mut self, ctx: &mut FrameCtx<'_>, mover: MoverKey) {
        if let Some(i) = self.inner.as_mut() {
            i.tail(ctx, mover);
        }
    }

    fn process_ctrl(&mut self, ctx: &mut FrameCtx<'_>, ctrl: CtrlKey) {
        if let Some(i) = self.inner.as_mut() {
            i.process_ctrl(ctx, ctrl);
        }
    }

    fn layer_process(&mut self, ctx: &mut FrameCtx<'_>) {
        if let Some(i) = self.inner.as_mut() {
            i.layer_process(ctx);
        }
    }

    fn on_die(
        &mut self,
        worlds: &mut Worlds,
        world: WorldId,
        layer: LayerId,
        entries: Vec<OnDieEntry>,
    ) {
        if let Some(i) = self.inner.as_mut() {
            i.on_die(worlds, world, layer, entries);
        }
    }

    fn replace(&mut self, worlds: &mut Worlds, world: WorldId, layer: LayerId) {
        if let Some(i) = self.inner.as_mut() {
            i.replace(worlds, world, layer);
        }
    }

    fn replace_leaving(
        &mut self,
        store: &mut ObjectStore,
        player: ObjId,
        from: (WorldId, LayerId),
        to: (WorldId, LayerId),
    ) {
        // `Mover.cpp:2343-2350`: the leave of a dungeon is booked by the drain after the phase
        self.notes
            .post(WorldNote::LayerChanged { player, from, to });
        if let Some(i) = self.inner.as_mut() {
            i.replace_leaving(store, player, from, to);
        }
    }

    fn world_change_entered(&mut self, ctx: &mut FrameCtx<'_>, player: MoverKey) {
        // ---- housing (doc 22 §13, task housing): a player who came into a mini room gets its papering after the phase ----
        if ctx.world.0 == WI_WORLD_MINIROOM
            && let Some(id) = ctx.store.movers.get(player).map(|m| m.core.id)
        {
            self.notes.post(WorldNote::RoomEntered {
                player: id,
                layer: ctx.layer,
            });
        }
        if let Some(i) = self.inner.as_mut() {
            i.world_change_entered(ctx, player);
        }
    }

    fn region_class_changed(&mut self, store: &mut ObjectStore, player: ObjId, old: u32, new: u32) {
        if let Some(i) = self.inner.as_mut() {
            i.region_class_changed(store, player, old, new);
        }
    }

    fn is_instance_world(&self, world: WorldId) -> bool {
        self.set.contains(world)
    }

    fn instance_enter(&mut self, store: &mut ObjectStore, player: ObjId, region: &RegionDef) {
        // `EnteranceDungeon`: the services take it from the drain after the phase
        let Some(core) = store.core(player) else {
            return;
        };
        self.notes.post(WorldNote::InstancePortal {
            player,
            from: (core.world, core.layer),
            world: WorldId(region.tele_world),
            pos: Vec3::new(region.tele_pos[0], region.tele_pos[1], region.tele_pos[2]),
        });
    }

    fn on_npc_removed(&mut self, id: ObjId) {
        if let Some(i) = self.inner.as_mut() {
            i.on_npc_removed(id);
        }
    }

    fn on_destroyed(
        &mut self,
        world: WorldId,
        layer: LayerId,
        origin: RespawnOrigin,
        aggressive: bool,
    ) {
        if let Some(i) = self.inner.as_mut() {
            i.on_destroyed(world, layer, origin, aggressive);
        }
    }

    fn layer_gc(&mut self, worlds: &mut Worlds, world: WorldId) {
        if let Some(i) = self.inner.as_mut() {
            i.layer_gc(worlds, world);
        }
    }
}

// ---- the logout ------------------------------------------------------------------------------------------------

/// THE doc-22 logout object, the OUTERMOST `with_logout` link: `is_instance_world` and
/// [`LogoutStep::InstanceGoOut`] are answered here, every other step and predicate goes to the wrapped hooks.
/// Housing (`DestroyHousing`), secret-room (`secret_room_position`), rainbow-race (`RainbowDropOut`) and guild-house
/// (`GuildHouseGoOut`) add their arms at their banners.
pub struct EventLogout {
    set: InstanceWorldSet,
    inner: Option<Box<dyn LogoutHooks>>,
}

impl EventLogout {
    /// The logout object over `inner`.
    pub fn with_inner(set: InstanceWorldSet, inner: Box<dyn LogoutHooks>) -> EventLogout {
        EventLogout {
            set,
            inner: Some(inner),
        }
    }

    /// The logout object without anything behind it.
    pub fn new(set: InstanceWorldSet) -> EventLogout {
        EventLogout { set, inner: None }
    }
}

impl std::fmt::Debug for EventLogout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventLogout").finish_non_exhaustive()
    }
}

#[deny(clippy::missing_trait_methods)]
impl LogoutHooks for EventLogout {
    fn step(
        &mut self,
        step: LogoutStep,
        svc: &mut Services,
        ctx: &mut GameCtx<'_>,
        player: PlayerRef,
    ) {
        match step {
            LogoutStep::InstanceGoOut => instance::on_logout(svc, ctx, player),
            // ---- housing (doc 22 §13, task housing): DestroyHousing ----
            LogoutStep::DestroyHousing => super::housing::on_logout(svc, ctx, player),
            // ---- rainbow race (doc 22 §14, task rainbow-race): `SetDropOut` in Progress; the rest still sees the step ----
            LogoutStep::RainbowDropOut => {
                crate::event::rainbow::on_logout(svc, ctx, player);
                if let Some(i) = self.inner.as_mut() {
                    i.step(step, svc, ctx, player);
                }
            }
            // ---- guild-house (doc 22 §12, task guild-house): GuildHouseGoOut (step 6, after the snapshot) ----
            LogoutStep::GuildHouseGoOut => {
                super::guild_house::on_logout(svc, ctx, player);
                if let Some(i) = self.inner.as_mut() {
                    i.step(step, svc, ctx, player);
                }
            }
            _ => {
                if let Some(i) = self.inner.as_mut() {
                    i.step(step, svc, ctx, player);
                }
            }
        }
    }

    fn secret_room_position(&self, ctx: &GameCtx<'_>, player: PlayerRef) -> Option<Vec3> {
        // ---- secret-room (task secret-room): a logout inside a room saves at the revival point of the room's side ----
        super::secret_room::rules::logout_position(ctx, player).or_else(|| {
            self.inner
                .as_ref()
                .and_then(|i| i.secret_room_position(ctx, player))
        })
    }

    fn is_instance_world(&self, world: WorldId) -> bool {
        self.set.contains(world)
    }

    fn is_chaotic(&self, ctx: &GameCtx<'_>, player: PlayerRef) -> bool {
        match self.inner.as_ref() {
            Some(i) => i.is_chaotic(ctx, player),
            None => ctx
                .player(player)
                .and_then(|m| m.player())
                .is_some_and(|p| p.pk.propensity > 0),
        }
    }
}

// ---- the join policy -------------------------------------------------------------------------------------------

/// The doc-22 placement predicates: `is_instance_world` and `instance_go_out`; the other methods keep the trait
/// defaults (guild-house and secret-room fill them at their banners).
pub struct EventJoinPolicy {
    set: InstanceWorldSet,
    // ---- guild-house (doc 22 §12.2, task guild-house) ----
    /// The admission of the character being placed: `(character, saved world, saved layer, admitted)`, computed by
    /// [`JoinPolicy::prepare`] (which has the services) and answered by `guild_house_login_ok` (which has none).
    gh_login: Option<(CharacterId, WorldId, LayerId, bool)>,
}

impl EventJoinPolicy {
    /// The policy for the dungeon worlds `set`.
    pub fn new(set: InstanceWorldSet) -> EventJoinPolicy {
        EventJoinPolicy {
            set,
            gh_login: None,
        }
    }
}

impl std::fmt::Debug for EventJoinPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventJoinPolicy").finish_non_exhaustive()
    }
}

impl JoinPolicy for EventJoinPolicy {
    fn is_instance_world(&self, world: WorldId) -> bool {
        self.set.contains(world)
    }

    fn instance_go_out(&mut self, svc: &mut Services, ctx: &mut GameCtx<'_>, p: PlayerRef) {
        instance::go_out(svc, ctx, p);
    }

    // ---- guild-house (doc 22 §12.2, task guild-house): `prepare` and `guild_house_login_ok` ----
    fn prepare(
        &mut self,
        svc: &mut Services,
        ctx: &mut GameCtx<'_>,
        c: CharacterId,
        saved: SavedLocation,
    ) {
        let ok = super::guild_house::login_admission(svc, ctx, c, saved);
        self.gh_login = Some((c, saved.world, saved.layer, ok));
    }

    fn guild_house_login_ok(&self, world: WorldId, layer: LayerId, c: CharacterId) -> bool {
        if !super::guild_house::is_guild_house_world(world) {
            return true;
        }
        // a guild-house world without a matching admission (no `prepare` ran, another character or position) is refused
        self.gh_login
            .is_some_and(|(cc, w, l, ok)| ok && cc == c && w == world && l == layer)
    }

    // ---- secret-room (`secret_room_participant`): at its banner ----
}

// ---- the layer drive-out ---------------------------------------------------------------------------------------

/// The doc-22 [`LayerDriveOut`]: a dungeon layer sends its occupants to their marking position; every other kind
/// (including a GM `/cl` layer) goes the Madrigal way (a guild house `revalidate`s; housing replaces its arm).
#[derive(Clone, Copy, Debug, Default)]
pub struct EventDriveOut;

impl LayerDriveOut for EventDriveOut {
    fn drive_out(
        &mut self,
        worlds: &mut Worlds,
        world: WorldId,
        kind: LayerKind,
        layer: LayerId,
        player: ObjId,
    ) {
        match kind {
            LayerKind::Instance => instance::go_out_worlds(worlds, player),
            // ---- housing (LayerKind::MiniRoom, doc 22 §13, task housing): the go-out of a room ----
            LayerKind::MiniRoom => super::housing::drive_out(worlds, player),
            _ => MadrigalDriveOut.drive_out(worlds, world, kind, layer, player),
        }
    }
}
