//! The eat-pet AI `CAIPet` (Plan/18 §12, WP-AI-15; src:_AIInterface/AIPet.cpp; ref/12 §11.1).
//!
//! An eat pet (`IK3_PET` item, mover `prop.link_kind`, `AII_PET`) follows its owner and fetches ground items for
//! him. The AI owns the movement and the loot **scan**; everything that belongs to the item system (the loot
//! rights and the pet filter, the pickup on behalf of the owner, the dismissal of the pet) is the [`PetWorld`]
//! it calls, implemented by doc 19 (`item::pets::eat::EatPetWorld`, WP-ITEM-18). This module has no filter table
//! and no picker code (18 §12).
//!
//! ```text
//! pass (AI pass of the tick, frame 4n)
//!   queued Arrival(s)         loot_move && owner valid: the item within 5 m -> PetWorld::arrived (do_loot for the
//!                             owner, the multiple picker around the pet); Looted -> scan again, else the loot move ends
//!   NotOwnedPetInactivated    owner invalid -> the pet is deleted; owner dead / farther than 32 m -> PetWorld::inactivate
//!   !loot_move                Idle: owner farther than 1 m -> SetDestObj(owner), Trace
//!                             Trace: owner.IsRangeObj(pet, 0) -> Idle, STOP, ClearDest; no destination -> Idle
//!   count & 15 == 0           the scan (SubItemLoot) while not looting
//!   loot_move                 the item is gone -> stand still; the destination was reached -> the move ends
//! ```
//!
//! The pet is processed even when nobody sees it (the sleep rule exempts `AII_PET`), never collides with scenery
//! and moves at the speed of the mover it follows (the sim's `improved_pets` rule, `flyff_sim::kin::get_speed`).
//! The pass runs from [`MovementHooks`] (`ai` hook) with the world phase's
//! `FrameCtx`, because the scan reads the ground items and the arrival changes the owner's inventory; the generic
//! [`AiDriver`](super::AiDriver) never sees a pet.
//!
//! **Quirks kept on purpose** (ref/12 §11.1): `sub_item_loot` returns the *current* `loot_move`, so after a
//! successful pickup a scan that finds nothing new still answers `true` and the loot move ends one tick later
//! through the "item gone" branch; without `features.improved_pets` the nearest-item distance never shrinks, so the
//! **last** candidate of the window wins (KQ-36 / Q22).

use super::{AiBrain, AiMsg, AiSlot};
use crate::item::LootResult;
use crate::movement::{IntentAudience, MovementHooks, PendingIntent, intents};
use crate::mover::{Mover, monster_parts};
use crate::world::object::MoverKey;
use crate::world::phases::FrameCtx;
use crate::world::{LinkSet, Worlds};
use flyff_common::math::Vec3;
use flyff_common::rng::GameRng;
use flyff_common::{LayerId, ObjId, SimFrame, WorldId};
use flyff_data::GameData;
use flyff_data::types::MoverId;
use flyff_rules::consts::defs::neuz::AII_PET;
use flyff_sim::ActMsg;
use flyff_sim::bounds::{is_range_obj, radius};
use flyff_sim::types::{OBJMSG_STOP, OBJSTA_DMG_FLY_ALL};

/// `IsValidArea(pet, 32)`: the owner and his pet must stay within this many metres (strictly).
pub const OWNER_AREA: f32 = 32.0;
/// The loot scan only considers items closer than this (`15 * 15` squared).
pub const SCAN_RADIUS: f32 = 15.0;
/// An item counts as reached when the pet is closer than this (`5 * 5` squared).
pub const ARRIVAL_RADIUS: f32 = 5.0;
/// Idle pets walk to the owner when farther than this (`1.0 * 1.0` squared).
pub const FOLLOW_DISTANCE: f32 = 1.0;
/// `(GetCount() & 15) == 0`: the scan runs every 16th tick (about 1.07 s).
pub const SCAN_MASK: u32 = 15;
/// The scan's start distance (`fMinDist = 9999999.0f`, squared metres).
const SCAN_START_DIST_SQ: f32 = 9_999_999.0;
/// An `Arrival` queue longer than this drops new arrivals (a pet that is not processed would otherwise collect
/// them without limit).
const QUEUE_CAP: usize = 8;

/// `PETSTATE_IDLE` / `PETSTATE_TRACE`: whether the pet is running after its owner.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PetState {
    /// `PETSTATE_IDLE` (0).
    #[default]
    Idle = 0,
    /// `PETSTATE_TRACE` (1): the destination object is the owner.
    Trace = 1,
}

/// The brain of an eat pet (`CAIPet`, 18 §12). Lives in [`MonsterExt::ai`](crate::mover::MonsterExt).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PetAi {
    /// `m_idOwner` (`None` until `SetOwner`; an ownerless pet is not processed).
    pub owner: Option<ObjId>,
    /// `m_nState`.
    pub state: PetState,
    /// `m_bLootMove`.
    pub loot_move: bool,
    /// `m_idLootItem`.
    pub loot_item: Option<ObjId>,
    /// `m_dwMoverSfxId`: the effect of a vis pet (`XI_BUFFPET_GRADE1..3`), 0 = none.
    pub sfx: u32,
    /// The dismissal was requested (the pet is deleted by the hook): nothing runs any more, so a hook that
    /// leaves the pet standing is asked exactly once.
    pub gone: bool,
}

impl PetAi {
    /// A brain owned by `owner`.
    pub fn owned_by(owner: ObjId) -> PetAi {
        PetAi {
            owner: Some(owner),
            ..PetAi::default()
        }
    }
}

/// Why the AI asks for its own dismissal (`NotOwnedPetInactivated`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dismiss {
    /// `pOwner->IsDie()`.
    OwnerDead,
    /// `!pOwner->IsValidArea(pEatPet, 32)`: another world or layer, or farther than 32 m.
    OutOfArea,
}

/// What the eat-pet AI needs from the item system (19 §6.8, WP-ITEM-18; 18 §1.4 layering). The real one is
/// `item::pets::eat::EatPetWorld`; [`StubPets`] records the calls.
pub trait PetWorld: Send {
    /// `item::loot::pet_can_loot(owner, item)`: loot rights for a pet and the owner's pet filter.
    fn can_loot(&mut self, ctx: &FrameCtx<'_>, owner: ObjId, item: ObjId) -> bool;

    /// `item::loot::pet_arrived(owner, pet, item)`: `do_loot` on behalf of the owner, then (on success) the
    /// multiple picker around the pet. The result of `do_loot` decides whether the loot move goes on.
    fn arrived(
        &mut self,
        ctx: &mut FrameCtx<'_>,
        owner: ObjId,
        pet: ObjId,
        item: ObjId,
    ) -> LootResult;

    /// `owner.InactivateEatPet()`: the pet must go (its owner died or is out of range). The implementation
    /// removes the pet's effects from the owner and deletes the pet mover.
    fn inactivate(&mut self, ctx: &mut FrameCtx<'_>, owner: ObjId, pet: ObjId, why: Dismiss);
}

/// One call a [`StubPets`] saw.
#[derive(Clone, Debug, PartialEq)]
pub enum PetCall {
    /// `can_loot(owner, item)` and the answer.
    CanLoot {
        /// The owner.
        owner: ObjId,
        /// The ground item.
        item: ObjId,
        /// The answer given.
        allowed: bool,
    },
    /// `arrived(owner, pet, item)` and the answer.
    Arrived {
        /// The owner.
        owner: ObjId,
        /// The pet that arrived.
        pet: ObjId,
        /// The ground item.
        item: ObjId,
        /// The answer given.
        result: LootResult,
    },
    /// `inactivate(owner, pet, why)`.
    Inactivate {
        /// The owner.
        owner: ObjId,
        /// The pet.
        pet: ObjId,
        /// Why.
        why: Dismiss,
    },
}

/// What a [`StubPets`] answers (the mocked item system of the AI tests).
#[derive(Clone, Debug, Default)]
pub struct StubOptions {
    /// Item ids (props) `can_loot` refuses: the mocked filter of the acceptance test.
    pub refused_items: Vec<u32>,
    /// The answer of `arrived` (`None` = `Looted` and the ground item is deleted, as the real one does).
    pub arrive_result: Option<LootResult>,
    /// `inactivate` leaves the pet standing (the default deletes it and clears the owner's handle, like the
    /// Tele16 stand-in of `npc::services`).
    pub keep_pet_on_inactivate: bool,
}

/// The default [`PetWorld`] of a [`MovementHooks`] without doc 19's: it allows
/// every item, loots nothing for real (`arrived` deletes the item and answers `Looted`, or the configured
/// result), and reports every call on its log channel (the dismissals the AI asks for among them).
#[derive(Clone, Debug)]
pub struct StubPets {
    opts: StubOptions,
    log: flume::Sender<PetCall>,
}

impl Default for StubPets {
    fn default() -> Self {
        StubPets::new()
    }
}

impl StubPets {
    /// A stub whose calls are not read by anybody.
    pub fn new() -> StubPets {
        StubPets::with_log(StubOptions::default()).0
    }

    /// A stub with `opts` and the receiving end of its call log.
    pub fn with_log(opts: StubOptions) -> (StubPets, flume::Receiver<PetCall>) {
        let (log, rx) = flume::unbounded();
        (StubPets { opts, log }, rx)
    }

    fn note(&self, call: PetCall) {
        // nobody listening is fine: the log is for tests
        let _ = self.log.send(call);
    }
}

impl PetWorld for StubPets {
    fn can_loot(&mut self, ctx: &FrameCtx<'_>, owner: ObjId, item: ObjId) -> bool {
        let allowed = item_alive(ctx, item)
            && ctx
                .store
                .item(item)
                .is_some_and(|d| !self.opts.refused_items.contains(&d.item.item_id));
        self.note(PetCall::CanLoot {
            owner,
            item,
            allowed,
        });
        allowed
    }

    fn arrived(
        &mut self,
        ctx: &mut FrameCtx<'_>,
        owner: ObjId,
        pet: ObjId,
        item: ObjId,
    ) -> LootResult {
        let result = self.opts.arrive_result.unwrap_or(LootResult::Looted);
        if result == LootResult::Looted || result == LootResult::Gone {
            ctx.delete(item);
        }
        self.note(PetCall::Arrived {
            owner,
            pet,
            item,
            result,
        });
        result
    }

    fn inactivate(&mut self, ctx: &mut FrameCtx<'_>, owner: ObjId, pet: ObjId, why: Dismiss) {
        self.note(PetCall::Inactivate { owner, pet, why });
        if self.opts.keep_pet_on_inactivate {
            return;
        }
        if let Some(p) = ctx.store.mover_mut(owner).and_then(Mover::player_mut) {
            p.items.summoned_eat_pet = None;
        }
        ctx.delete(pet);
    }
}

/// A [`PetWorld`] that does nothing: it stands in while the real one is taken out of the pipeline for a pass.
#[derive(Clone, Copy, Debug, Default)]
pub struct Detached;

impl PetWorld for Detached {
    fn can_loot(&mut self, _: &FrameCtx<'_>, _: ObjId, _: ObjId) -> bool {
        false
    }
    fn arrived(&mut self, _: &mut FrameCtx<'_>, _: ObjId, _: ObjId, _: ObjId) -> LootResult {
        LootResult::Gone
    }
    fn inactivate(&mut self, _: &mut FrameCtx<'_>, _: ObjId, _: ObjId, _: Dismiss) {}
}

// ---- brain access ---------------------------------------------------------------------------------------------

impl AiSlot {
    /// The eat-pet brain, if this is a pet.
    pub fn pet(&self) -> Option<&PetAi> {
        match &self.brain {
            AiBrain::Pet(b) => Some(b),
            _ => None,
        }
    }

    /// The eat-pet brain, mutably.
    pub fn pet_mut(&mut self) -> Option<&mut PetAi> {
        match &mut self.brain {
            AiBrain::Pet(b) => Some(b),
            _ => None,
        }
    }
}

/// `true` for an `AII_PET` mover (the movers the sleep rule exempts and the generic driver skips).
pub fn is_pet(m: &Mover) -> bool {
    m.monster().is_some_and(|x| x.ai.kind == AII_PET)
}

/// The owner of the eat pet `m` (`None` for an ownerless pet or a mover that is not one).
pub fn owner_of(m: &Mover) -> Option<ObjId> {
    m.monster()?.ai.pet()?.owner
}

/// The effect of a vis pet (`m_dwMoverSfxId`) as the spawn snapshot and `MoverChangeSfx` carry it (0 = none).
pub fn sfx_of(m: &Mover) -> u32 {
    m.monster().and_then(|x| x.ai.pet()).map_or(0, |p| p.sfx)
}

// ---- creation -------------------------------------------------------------------------------------------------

/// `CreateMover(world, prop.link_kind, owner pos, layer)` + `SetOwner` (`ActivateEatPet`, ref/13 §19.8): the
/// pet mover for `owner`, standing at his feet, with `sfx` already set (a vis pet's effect travels in the spawn
/// snapshot). `None` when the mover prop is unknown.
pub fn build(
    data: &GameData,
    rng: &mut dyn GameRng,
    owner: &Mover,
    mover: MoverId,
    sfx: u32,
) -> Option<Mover> {
    let prop = data.mover(mover)?;
    let (core, mut sim) = monster_parts(data, prop, owner.core.pos, 0.0);
    sim.params.pet_owner = owner.core.id;
    let mut m = Mover::new_monster(core, sim, prop, rng);
    let ext = m.monster_mut()?;
    ext.owner = Some(owner.core.id);
    if let Some(b) = ext.ai.pet_mut() {
        b.owner = Some(owner.core.id);
        b.sfx = sfx;
    }
    Some(m)
}

/// `ai::pet::spawn(owner, item)` (18 §12): creates the pet of `owner` in his world and layer and queues it for
/// the `_add` of the layer. The id of the pet mover, `None` when the owner is gone, the prop is unknown or the
/// world refused the position.
pub fn spawn(
    worlds: &mut Worlds,
    data: &GameData,
    rng: &mut dyn GameRng,
    owner: ObjId,
    mover: MoverId,
    sfx: u32,
) -> Option<ObjId> {
    let o = worlds.mover(owner)?;
    let (world, layer) = (o.core.world, o.core.layer);
    let pet = build(data, rng, o, mover, sfx)?;
    match worlds.spawn_mover(world, layer, pet) {
        Ok(id) => Some(id),
        Err(e) => {
            tracing::warn!(mover, ?e, "an eat pet was refused");
            None
        }
    }
}

// ---- the pass -------------------------------------------------------------------------------------------------

/// What the pass reads of the owner (`prj.GetMover(m_idOwner)`, live).
#[derive(Clone, Copy, Debug)]
struct Owner {
    id: ObjId,
    pos: Vec3,
    die: bool,
    fly: bool,
    radius: f32,
    world: WorldId,
    layer: LayerId,
}

/// What the pass reads of the pet itself.
#[derive(Clone, Copy, Debug)]
struct Body {
    id: ObjId,
    pos: Vec3,
    radius: f32,
    world: WorldId,
    layer: LayerId,
    dead: bool,
    dmg_fly: bool,
    empty_dest: bool,
    count: u32,
}

fn owner_of_id(ctx: &FrameCtx<'_>, id: ObjId) -> Option<Owner> {
    let m = ctx.store.mover(id)?;
    if m.core.deleted {
        return None;
    }
    Some(Owner {
        id,
        pos: m.sim.pos,
        die: m.sim.act.state.is_die() || m.mv.hp == 0,
        fly: m.sim.act.flags.fly(),
        radius: radius(&m.sim.params.bounds, m.sim.params.scale),
        world: m.core.world,
        layer: m.core.layer,
    })
}

fn body_of(m: &Mover) -> Body {
    Body {
        id: m.core.id,
        pos: m.sim.pos,
        radius: radius(&m.sim.params.bounds, m.sim.params.scale),
        world: m.core.world,
        layer: m.core.layer,
        dead: m.sim.act.state.is_die() || m.mv.hp == 0,
        dmg_fly: m.sim.act.state.bits() & OBJSTA_DMG_FLY_ALL != 0,
        empty_dest: m.sim.is_empty_dest(),
        count: m.mv.count,
    }
}

/// `IsValidObj(item)` for a ground item: it exists, is not flagged and was not deleted earlier in this call.
fn item_alive(ctx: &FrameCtx<'_>, id: ObjId) -> bool {
    ctx.store.item(id).is_some_and(|d| !d.core.deleted) && !ctx.is_deleting(id)
}

fn dist_sq(a: Vec3, b: Vec3) -> f32 {
    let d = a - b;
    d.x * d.x + d.y * d.y + d.z * d.z
}

/// `MoveToDst(vDst)` of the pet (`AIPet.cpp:78-87`): unlike the monster helper there is no short-cut for an equal
/// destination; the correction flag is armed but pets are never corrected.
pub fn move_to_pos_pet(m: &mut Mover, v: Vec3) {
    m.sim.set_dest_pos(v, true);
    m.sync.corr_armed = true;
    intents::enqueue(
        m,
        PendingIntent::DestPos {
            pos: v,
            forward: true,
        },
        IntentAudience::Viewers,
    );
}

/// `MoveToDst(idTarget)` of the pet (`AIPet.cpp:90-96`): the destination object, no speed factor, no range.
fn move_to_obj_pet(m: &mut Mover, id: ObjId) {
    intents::set_dest_obj(m, id, 0.0);
}

/// `CAIPet::NotOwnedPetInactivated`: `true` when the pet must not go on this tick.
fn not_owned_pet_inactivated(
    ctx: &mut FrameCtx<'_>,
    b: &mut PetAi,
    me: &Body,
    owner: Option<Owner>,
    pets: &mut dyn PetWorld,
) -> bool {
    let Some(o) = owner else {
        ctx.delete(me.id);
        b.gone = true;
        return true;
    };
    let why = if o.die {
        Dismiss::OwnerDead
    } else if o.world != me.world
        || o.layer != me.layer
        || dist_sq(o.pos, me.pos) >= OWNER_AREA * OWNER_AREA
    {
        Dismiss::OutOfArea
    } else {
        return false;
    };
    pets.inactivate(ctx, o.id, me.id, why);
    b.gone = true;
    true
}

/// `CAIPet::SubItemLoot`: the nearest lootable ground item within [`SCAN_RADIUS`] of the pet becomes the
/// destination. Returns `loot_move` as it is afterwards (see the module docs).
fn sub_item_loot(
    ctx: &mut FrameCtx<'_>,
    key: MoverKey,
    b: &mut PetAi,
    me: &Body,
    owner: Option<Owner>,
    pets: &mut dyn PetWorld,
) -> bool {
    let Some(o) = owner else {
        return false;
    };
    // the owner too far away (the original reads the distance before the owner's flying flag)
    if dist_sq(o.pos, me.pos) > OWNER_AREA * OWNER_AREA || o.fly {
        return false;
    }
    let improved = ctx.movement.shared.flags.improved_pets;
    let mut best: Option<(ObjId, Vec3)> = None;
    let mut best_d = SCAN_START_DIST_SQ;
    let mut candidates: Vec<ObjId> = Vec::new();
    ctx.grid
        .for_each_legacy(me.pos, 0, LinkSet::Others, |id| candidates.push(id));
    for id in candidates {
        let Some(d) = ctx.store.item(id) else {
            continue;
        };
        if d.core.deleted || ctx.is_deleting(id) {
            continue;
        }
        let ipos = d.core.pos;
        if !pets.can_loot(ctx, o.id, id) {
            continue;
        }
        let dsq = dist_sq(ipos, me.pos);
        if dsq < SCAN_RADIUS * SCAN_RADIUS && dsq < best_d {
            best = Some((id, ipos));
            if improved {
                best_d = dsq;
            }
        }
    }
    if let Some((id, pos)) = best
        && item_alive(ctx, id)
        && let Some(m) = ctx.store.movers.get_mut(key)
    {
        move_to_pos_pet(m, pos);
        b.loot_item = Some(id);
        b.loot_move = true;
    }
    b.loot_move
}

/// `AIMSG_ARRIVAL` in `StateIdle`: a looting pet reached its destination.
fn on_arrival(
    ctx: &mut FrameCtx<'_>,
    key: MoverKey,
    b: &mut PetAi,
    me: &Body,
    pets: &mut dyn PetWorld,
) {
    if !b.loot_move {
        return;
    }
    let Some(owner) = b.owner.and_then(|o| owner_of_id(ctx, o)) else {
        return;
    };
    let reached = b.loot_item.filter(|&id| {
        item_alive(ctx, id)
            && ctx
                .store
                .item(id)
                .is_some_and(|d| dist_sq(d.core.pos, me.pos) < ARRIVAL_RADIUS * ARRIVAL_RADIUS)
    });
    let ok = match reached {
        Some(item) => pets.arrived(ctx, owner.id, me.id, item) == LootResult::Looted,
        None => false,
    };
    if ok {
        // the scan runs from where the pet stands now
        let me = ctx.store.movers.get(key).map_or(*me, body_of);
        if !sub_item_loot(ctx, key, b, &me, Some(owner), pets) {
            b.loot_move = false;
            b.loot_item = None;
        }
    } else {
        b.loot_move = false;
        b.loot_item = None;
    }
}

/// `CAIPet::MoveProcessIdle`.
fn move_process(
    hooks: &mut MovementHooks,
    ctx: &mut FrameCtx<'_>,
    key: MoverKey,
    b: &mut PetAi,
    pets: &mut dyn PetWorld,
) {
    let Some(me) = ctx.store.movers.get(key).map(body_of) else {
        return;
    };
    if me.dead || me.dmg_fly {
        return;
    }
    let owner = b.owner.and_then(|o| owner_of_id(ctx, o));
    if not_owned_pet_inactivated(ctx, b, &me, owner, pets) {
        return;
    }
    let Some(owner) = owner else {
        return;
    };
    if !b.loot_move {
        match b.state {
            PetState::Idle => {
                if dist_sq(owner.pos, me.pos) > FOLLOW_DISTANCE * FOLLOW_DISTANCE {
                    if let Some(m) = ctx.store.movers.get_mut(key) {
                        move_to_obj_pet(m, owner.id);
                    }
                    b.state = PetState::Trace;
                }
            }
            PetState::Trace => {
                let mut empty_dest = me.empty_dest;
                if is_range_obj(owner.pos, owner.radius, me.pos, me.radius, 0.0) {
                    b.state = PetState::Idle;
                    let frame = ctx.tick.first_frame();
                    stop(hooks, ctx, key, frame);
                    empty_dest = true;
                }
                if empty_dest {
                    b.loot_move = false;
                    b.loot_item = None;
                    b.state = PetState::Idle;
                }
            }
        }
    }
    // the scan: about once a second, while not looting
    if me.count & SCAN_MASK == 0
        && !b.loot_move
        && sub_item_loot(ctx, key, b, &me, Some(owner), pets)
    {
        b.state = PetState::Idle;
    }
    if b.loot_move {
        let gone = b.loot_item.is_none_or(|id| !item_alive(ctx, id));
        if gone {
            if let Some(m) = ctx.store.movers.get_mut(key) {
                let here = m.sim.pos;
                move_to_pos_pet(m, here);
            }
            b.loot_move = false;
            b.loot_item = None;
            b.state = PetState::Idle;
        } else if ctx
            .store
            .movers
            .get(key)
            .is_some_and(|m| m.sim.is_empty_dest())
        {
            b.loot_move = false;
            b.loot_item = None;
            b.state = PetState::Idle;
        }
    }
}

/// `SendActMsg(OBJMSG_STOP)` + `ClearDest()` (the pet reached its owner).
fn stop(hooks: &mut MovementHooks, ctx: &mut FrameCtx<'_>, key: MoverKey, frame: SimFrame) {
    hooks.with_ai_cx(ctx, frame, key, |_, m, cx| {
        cx.send_act(m, ActMsg::new(OBJMSG_STOP));
        m.sim.clear_dest();
    });
}

/// The AI pass of one pet (see the module docs): the queued arrivals, then `Process`. Called by the movement
/// pipeline in the AI pass of the tick with the real [`PetWorld`] taken out of the hooks.
pub(crate) fn pass(
    hooks: &mut MovementHooks,
    ctx: &mut FrameCtx<'_>,
    key: MoverKey,
    pets: &mut dyn PetWorld,
) {
    let (mut brain, queued) = {
        let Some(m) = ctx.store.movers.get_mut(key) else {
            return;
        };
        let Some(ext) = m.monster_mut() else {
            return;
        };
        let AiBrain::Pet(b) = &ext.ai.brain else {
            return;
        };
        if b.owner.is_none() || b.gone {
            return;
        }
        let brain = (**b).clone();
        let queued: Vec<AiMsg> = ext.ai.queue.drain(..).collect();
        (brain, queued)
    };
    for msg in queued {
        if let AiMsg::Arrival { .. } = msg
            && let Some(me) = ctx.store.movers.get(key).map(body_of)
        {
            on_arrival(ctx, key, &mut brain, &me, pets);
        }
    }
    if !brain.gone {
        move_process(hooks, ctx, key, &mut brain, pets);
    }
    if let Some(ext) = ctx.store.movers.get_mut(key).and_then(Mover::monster_mut)
        && let AiBrain::Pet(b) = &mut ext.ai.brain
    {
        **b = brain;
    }
}

/// A sim event of the pet's own mover (the barrier): an arrival is queued for the next pass (`PostAIMsg`), the
/// rest does not concern a pet (it never collides, is never corrected and has no states).
pub(crate) fn on_event(m: &mut Mover, ev: &flyff_sim::step::SimEvent) {
    use flyff_sim::step::SimEvent;
    let msg = match ev {
        SimEvent::ArrivedAtPos => AiMsg::Arrival {
            obj: None,
            pos_arrival: true,
        },
        SimEvent::ArrivedAtObj { target, .. } => AiMsg::Arrival {
            obj: Some(*target),
            pos_arrival: false,
        },
        _ => return,
    };
    if let Some(ext) = m.monster_mut()
        && matches!(ext.ai.brain, AiBrain::Pet(_))
        && ext.ai.queue.len() < QUEUE_CAP
    {
        ext.ai.queue.push_back(msg);
    }
}
