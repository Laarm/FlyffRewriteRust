//! The map subscription: its frame types and the collector (Plan/25 §3.7, §3.15; types: WP-ADMIN-06, collector:
//! **WP-ADMIN-07**).
//!
//! The collector runs in the `post` phase, after the flush, for each subscription that is due, round-robin from the
//! registry's cursor ([`super::views`]). It reads cells and member lists only: it is not a link-map viewer and never
//! calls wake or visibility code (sleep-rule neutrality, 25 §3.15, 15 §5.6), and it never takes `&mut Game`.
//!
//! # Algorithm (25 §3.15)
//!
//! 1. **Players** are the avatars of the session table that are members of the subscription's layer. They are never
//!    taken from the cell walk, so a player is in a frame exactly once and in every tier, whatever the cell walk
//!    managed (`Layer.members` lists every linked object, ~25 000 in Madrigal; the session table is the same set of
//!    players at the cost of the online count). They are read when the frame completes, so they are the freshest part
//!    of it.
//! 2. **Other kinds** are read from `cell.others`, row by row from the subscription's `(row, col)` cursor over the
//!    cell rectangle of the view. Every id goes into the subscription's `seen` set, which is cleared at the start of a
//!    frame: an id already in it is skipped, so **a frame never contains an entity twice**. A mover that crosses from
//!    an unvisited row into a visited one during a multi-tick frame can be missing from that one frame; it appears
//!    in the next.
//! 3. **Budget.** Each entity read costs 1 and each cell visited 1/16 of the registry's per-tick budget
//!    (`admin.map_visit_budget`). When it runs out the cursor stays and the frame continues on the next tick (at least
//!    one cell is read per call, so a frame always advances).
//! 4. **Tier C** walks the rectangle in 8 row bands over 8 ticks (Madrigal: 480 rows, 60 per band). Monsters other
//!    than bosses are aggregated into 128 m bins (normal, captain, asleep). The bins are *approximate* (a monster
//!    that changes band during the scan is counted 0 or 2 times: the bins skip the `seen` set, so the errors cancel on
//!    average instead of only losing monsters); players and bosses are exact.
//! 5. A completed frame is returned to the registry, which moves it into the connection's sink (a latest-wins slot).
//!
//! # Tiers (25 §3.7)
//!
//! The tier comes from the rectangle's size ([`tier_for`]: the larger side against `admin.map_tier_a_m` /
//! `map_tier_b_m`) and the client's `max_tier` (the coarser of the two wins: the client can ask for a coarser tier,
//! never a finer one). A tier A or B frame with more than `admin.map_max_entities` entities is sent as tier C instead,
//! flagged `degraded`; the subscription then stays at C for [`DEGRADE_HOLD_TICKS`] so a crowded view does not rebuild
//! and discard a finer frame every few ticks.
//!
//! | tier | kinds | cadence | per entity |
//! |---|---|---|---|
//! | A | all enabled | 3 ticks | everything, monster targets (pets: the owner) |
//! | B | players, monsters, NPCs | 8 ticks | no target |
//! | C | players, bosses individually, other monsters as bins | 30 ticks | no target |
//!
//! # Conventions
//!
//! * A position is inside the rectangle when `x0 <= x <= x1` and `z0 <= z <= z1`.
//! * `yaw` is the object's angle in degrees as the server keeps it (D3D left-handed), `0..=255` for `0..360`.
//! * `sub` of a monster is its rank code ([`rank_code`]: 0 normal, 1 captain, 2 mid-boss, 3 boss (`RANK_SUPER`:
//!   Clockworks, Rangda, Asmodan, the instance bosses), 4 giant (`RANK_BOSS`: the "Giant X" field monsters, Great White
//!   Bolo, the dungeon giants and leaders), 5 guard; the zero is not on the wire, which saves two bytes of every normal
//!   monster); of a player its job id; of a ctrl its `ctrl_kind1`.
//! * `level` of a ground item is its stack count.
//! * `hp_pct` is `ceil(hp * 100 / max_hp)` (at least 1 while alive, 0 when dead); NPCs report 100, items and ctrls 0.
//!   A monster's `max_hp` is its prop's times the GM multiplier (buffs on the maximum are not looked at); a player's is
//!   the full `GetMaxHitPoint`.
//! * `ASLEEP` is a monster's flag (the sleep rule's own state): NPCs, pets, items and ctrls never carry it.

use super::events::{SubError, SubErrorCode};
use super::views::VisitBudget;
use crate::mover::Mover;
use crate::tick::Game;
use crate::world::{CellRect, Ctrl, GridGeom, ItemDrop, Layer, LinkState, ObjRef, ObjectStore};
use flyff_common::defs::attribute::{RANK_BOSS, RANK_CAPTAIN, RANK_MIDBOSS, RANK_SUPER};
use flyff_common::det::{DetMap, DetSet};
use flyff_common::{LayerId, ObjId, WorldId};
use flyff_rules::consts::defs::neuz::AII_PET;

/// The edge of a tier-C bin in metres (25 §3.7).
pub const BIN_M: u32 = 128;
/// Row bands of a tier-C frame, one per tick (25 §3.15 step 4).
pub const C_BANDS: u32 = 8;
/// Ticks between two frames of tier A (5 Hz).
pub const CADENCE_A_TICKS: u32 = 3;
/// Ticks between two frames of tier B (about 1.9 Hz).
pub const CADENCE_B_TICKS: u32 = 8;
/// Ticks between two frames of tier C (0.5 Hz).
pub const CADENCE_C_TICKS: u32 = 30;
/// How long a subscription whose tier A or B frame overflowed `admin.map_max_entities` stays at tier C (10 s).
pub const DEGRADE_HOLD_TICKS: u64 = 150;
/// The `kinds` bits a subscription may carry (`1 << EntityKind`, 1..=6).
pub const KINDS_MASK: u32 = 0b111_1110;
/// Ids of `cell.others` that are turned into records together (see `read_batch`).
const READ_BATCH: usize = 64;
/// The loot priority of a ground item's owner lasts this long after the drop (19 §6.5).
const LOOT_PRIORITY_MS: u64 = 7_000;

/// What a map entity is. The numbers are the `EntityKind` values of the protocol; `1 << kind` is the bit of a
/// `MapSubscribe.kinds` set.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum EntityKind {
    /// A player character.
    Player = 1,
    /// A monster.
    Monster = 2,
    /// A `character.inc` NPC.
    Npc = 3,
    /// A pet.
    Pet = 4,
    /// A ground item.
    Item = 5,
    /// A chest, door, trap or trigger.
    Ctrl = 6,
}

impl EntityKind {
    /// The bit of this kind in a `kinds` set.
    pub const fn bit(self) -> u32 {
        1 << (self as u32)
    }

    /// The kind with this protocol number.
    pub const fn from_u8(v: u8) -> Option<EntityKind> {
        Some(match v {
            1 => EntityKind::Player,
            2 => EntityKind::Monster,
            3 => EntityKind::Npc,
            4 => EntityKind::Pet,
            5 => EntityKind::Item,
            6 => EntityKind::Ctrl,
            _ => return None,
        })
    }
}

/// The detail tier of a frame (25 §3.7).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum Tier {
    /// Full: every enabled entity in the rectangle (view width up to `admin.map_tier_a_m`).
    A = 1,
    /// Movers: players, monsters and NPCs.
    B = 2,
    /// Overview: every player, bosses, aggregated bins.
    C = 3,
}

impl Tier {
    /// The tier with this protocol number.
    pub const fn from_u8(v: u8) -> Option<Tier> {
        Some(match v {
            1 => Tier::A,
            2 => Tier::B,
            3 => Tier::C,
            _ => return None,
        })
    }

    /// Ticks between two frames of this tier.
    pub const fn cadence_ticks(self) -> u32 {
        match self {
            Tier::A => CADENCE_A_TICKS,
            Tier::B => CADENCE_B_TICKS,
            Tier::C => CADENCE_C_TICKS,
        }
    }

    /// The kinds a frame of this tier may carry, of the client's enabled `kinds` (25 §3.7 table).
    pub const fn kinds(self, enabled: u32) -> u32 {
        let allowed = match self {
            Tier::A => KINDS_MASK,
            Tier::B => EntityKind::Player.bit() | EntityKind::Monster.bit() | EntityKind::Npc.bit(),
            Tier::C => EntityKind::Player.bit() | EntityKind::Monster.bit(),
        };
        enabled & allowed
    }
}

/// The numbers of `[admin]` the collector reads (live: sent by the gateway with [`super::views::ViewLimits`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MapTuning {
    /// `admin.map_tier_a_m`: a rectangle up to this size is tier A.
    pub tier_a_m: u32,
    /// `admin.map_tier_b_m`: a rectangle up to this size is tier B, a larger one tier C.
    pub tier_b_m: u32,
    /// `admin.map_max_entities`: a tier A or B frame with more entities is sent as tier C.
    pub max_entities: u32,
}

impl Default for MapTuning {
    fn default() -> Self {
        Self {
            tier_a_m: 1200,
            tier_b_m: 6000,
            max_entities: 4000,
        }
    }
}

/// The tier of a view: by the larger side of its rectangle, never finer than the client's `max_tier` (25 §3.7).
pub fn tier_for(rect: [f32; 4], max_tier: Tier, t: &MapTuning) -> Tier {
    let side = (rect[2] - rect[0]).max(rect[3] - rect[1]);
    let by_size = if side <= t.tier_a_m as f32 {
        Tier::A
    } else if side <= t.tier_b_m as f32 {
        Tier::B
    } else {
        Tier::C
    };
    by_size.max(max_tier)
}

/// The bits of `MapEntity.flags` (25 §3.15).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct MapFlags(pub u16);

impl MapFlags {
    /// A monster that is not in the tick's awake set (the sleep rule's own state).
    pub const ASLEEP: u16 = 1 << 0;
    /// Dead.
    pub const DEAD: u16 = 1 << 1;
    /// In a fight (a monster with a target, a player attacking).
    pub const FIGHTING: u16 = 1 << 2;
    /// An aggressive monster.
    pub const AGGRESSIVE: u16 = 1 << 3;
    /// A player with PK status.
    pub const PK: u16 = 1 << 4;
    /// A player with GM authority.
    pub const GM: u16 = 1 << 5;
    /// Invisible (the transparent mode).
    pub const INVISIBLE: u16 = 1 << 6;
    /// A ground item that still has loot priority.
    pub const OWNER_ONLY: u16 = 1 << 7;
    /// A boss.
    pub const BOSS: u16 = 1 << 8;
    /// Flying.
    pub const FLYING: u16 = 1 << 9;

    /// `true` when `bit` is set.
    pub const fn has(self, bit: u16) -> bool {
        self.0 & bit != 0
    }

    /// Sets `bit` when `on`.
    #[must_use]
    pub const fn with(self, bit: u16, on: bool) -> MapFlags {
        if on { MapFlags(self.0 | bit) } else { self }
    }

    /// The flags of a mover as the console shows them: everything this module can tell from the mover alone
    /// (`ASLEEP`, `DEAD`, `FIGHTING`, `AGGRESSIVE`, `PK`, `GM`, `INVISIBLE`, `BOSS`, `FLYING`). `asleep` is the
    /// monster's own state under the sleep rule: no viewers and not a pet (15 §5.6); the caller reads it, nothing
    /// here wakes anything.
    pub fn of_mover(m: &Mover) -> MapFlags {
        let monster = m.monster();
        let asleep = !m.is_player()
            && m.core.viewers.is_empty()
            && !monster.is_some_and(|e| e.ai.kind == AII_PET);
        let target = monster
            .and_then(|e| e.ai.monster())
            .is_some_and(|b| b.target.is_some());
        let invisible = m.mv.mode & flyff_common::gm::ModeFlags::TRANSPARENT.0 != 0;
        let class = monster.map_or(0, |e| e.class);
        MapFlags::default()
            .with(Self::ASLEEP, asleep)
            .with(Self::DEAD, crate::combat::is_die(m))
            .with(Self::FIGHTING, target)
            .with(Self::AGGRESSIVE, monster.is_some_and(|e| e.active_attack))
            .with(Self::PK, m.player().is_some_and(|p| p.pk.propensity > 0))
            .with(
                Self::GM,
                m.player().is_some_and(|p| p.ident.authority.is_gm()),
            )
            .with(Self::INVISIBLE, invisible)
            .with(
                Self::BOSS,
                matches!(class, RANK_BOSS | RANK_MIDBOSS | RANK_SUPER),
            )
            .with(
                Self::FLYING,
                m.mv.type_flags & flyff_rules::consts::OBJTYPE_FLYING != 0,
            )
    }
}

/// One entity of a frame (about 32 bytes; 25 §3.15).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MapEntity {
    /// The object id.
    pub id: u32,
    /// What it is.
    pub kind: EntityKind,
    /// Players: job id; monsters: rank code ([`rank_code`]); pets: 0; ctrls: ctrl kind; NPCs and items: 0.
    pub sub: u8,
    /// Prop id (items: the item id).
    pub prop: u32,
    /// Level (ground items: the stack count).
    pub level: u16,
    /// Position x (metres).
    pub x: f32,
    /// Position z (metres).
    pub z: f32,
    /// Yaw in 256 steps.
    pub yaw: u8,
    /// Hit points in percent.
    pub hp_pct: u8,
    /// State bits.
    pub flags: MapFlags,
    /// The monster's target, `0` = none (tier A only; a pet: its owner).
    pub target: u32,
}

/// One 128 m bin of a tier-C frame: the monsters of the bin by rank and sleep state (approximate, 25 §3.15).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MapBin {
    /// Bin column.
    pub bx: u16,
    /// Bin row.
    pub bz: u16,
    /// Normal monsters.
    pub normal: u16,
    /// Captains.
    pub captain: u16,
    /// Monsters asleep (of the above).
    pub asleep: u16,
}

/// A completed frame of one subscription, moved into the sink.
#[derive(Clone, Debug, PartialEq)]
pub struct MapFrame {
    /// The subscription.
    pub sub_id: u32,
    /// The tick the frame was completed in.
    pub tick: u64,
    /// The tier of the content.
    pub tier: Tier,
    /// The requested tier A or B was replaced by C (too many entities).
    pub degraded: bool,
    /// The frame was complete within one tick (exact content of the rectangle).
    pub complete_in_one_tick: bool,
    /// The entities.
    pub entities: Vec<MapEntity>,
    /// The bins (tier C).
    pub bins: Vec<MapBin>,
    /// The name of every player in the frame.
    pub player_names: Vec<(u32, String)>,
}

/// The rectangle, world, layer, kinds and ceiling of a `MapSubscribe` / `MapUpdate`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MapSpec {
    /// World.
    pub world: WorldId,
    /// Layer.
    pub layer: LayerId,
    /// The viewport rectangle with its margin: `[x0, z0, x1, z1]` in metres.
    pub rect: [f32; 4],
    /// The enabled kinds as a bitset of [`EntityKind::bit`].
    pub kinds: u32,
    /// The finest tier the client accepts: the server picks the tier from the rectangle's size and never goes finer
    /// than this (the client can ask for a coarser tier, never a finer one, 25 §3.7).
    pub max_tier: Tier,
    /// The viewer may see invisible GMs (an authority of `N` or higher, 25 §3.5). Without it a player with the
    /// transparent mode never appears in a frame, nor does their name.
    pub see_invisible: bool,
}

impl MapSpec {
    /// A spec over `(world, layer)` for tests and tools: every kind, tier A at most, invisible GMs hidden.
    pub fn new(world: WorldId, layer: LayerId, rect: [f32; 4]) -> MapSpec {
        MapSpec {
            world,
            layer,
            rect,
            kinds: KINDS_MASK,
            max_tier: Tier::A,
            see_invisible: false,
        }
    }

    /// Checks the rectangle and the kinds (the gateway validated them too; the game does not trust it).
    fn check(&self) -> Result<(), SubError> {
        let finite = self.rect.iter().all(|v| v.is_finite());
        if !finite || self.rect[2] <= self.rect[0] || self.rect[3] <= self.rect[1] {
            return Err(SubError::new(
                SubErrorCode::BadSpec,
                "the rectangle is not valid",
            ));
        }
        if self.kinds & !KINDS_MASK != 0 || self.kinds & KINDS_MASK == 0 {
            return Err(SubError::new(
                SubErrorCode::BadSpec,
                "the entity kinds are not valid",
            ));
        }
        Ok(())
    }
}

/// The bins of a tier-C frame under construction: a dense grid over the bin range of the view
/// (`[normal, captain, asleep]` per bin).
#[derive(Clone, Debug, Default)]
struct BinGrid {
    bx0: i32,
    bz0: i32,
    nx: i32,
    nz: i32,
    counts: Vec<[u16; 3]>,
}

impl BinGrid {
    /// The grid over the part of `rect` inside the world (`extent` metres per side).
    fn over(rect: [f32; 4], extent: (f32, f32), reuse: Vec<[u16; 3]>) -> BinGrid {
        let bin = BIN_M as f32;
        let lo = |v: f32| (v.max(0.0) / bin).floor() as i32;
        let hi = |v: f32, ext: f32| (v.min((ext - 0.01).max(0.0)) / bin).floor() as i32;
        let (bx0, bx1) = (lo(rect[0]), hi(rect[2], extent.0));
        let (bz0, bz1) = (lo(rect[1]), hi(rect[3], extent.1));
        let (nx, nz) = (bx1 - bx0 + 1, bz1 - bz0 + 1);
        let mut counts = reuse;
        counts.clear();
        if nx > 0 && nz > 0 {
            counts.resize((nx as usize).saturating_mul(nz as usize), [0; 3]);
        }
        BinGrid {
            bx0,
            bz0,
            nx: nx.max(0),
            nz: nz.max(0),
            counts,
        }
    }

    /// Counts one monster at `(x, z)`.
    fn add(&mut self, x: f32, z: f32, captain: bool, asleep: bool) {
        let bin = BIN_M as f32;
        let (bx, bz) = ((x / bin).floor() as i32, (z / bin).floor() as i32);
        let (ix, iz) = (bx - self.bx0, bz - self.bz0);
        if ix < 0 || iz < 0 || ix >= self.nx || iz >= self.nz {
            return;
        }
        let idx = (iz as usize) * (self.nx as usize) + ix as usize;
        if let Some(c) = self.counts.get_mut(idx) {
            let slot = usize::from(captain);
            if let Some(n) = c.get_mut(slot) {
                *n = n.saturating_add(1);
            }
            if asleep && let Some(n) = c.get_mut(2) {
                *n = n.saturating_add(1);
            }
        }
    }

    /// The non-empty bins in row-major order.
    fn bins(&self) -> Vec<MapBin> {
        let mut out = Vec::new();
        if self.nx <= 0 {
            return out;
        }
        for (i, c) in self.counts.iter().enumerate() {
            if c.iter().all(|n| *n == 0) {
                continue;
            }
            let (ix, iz) = (i % self.nx as usize, i / self.nx as usize);
            out.push(MapBin {
                bx: u16::try_from(self.bx0 + ix as i32).unwrap_or(u16::MAX),
                bz: u16::try_from(self.bz0 + iz as i32).unwrap_or(u16::MAX),
                normal: c[0],
                captain: c[1],
                asleep: c[2],
            });
        }
        out
    }
}

/// The walk of the frame under construction.
#[derive(Clone, Debug)]
struct Walk {
    /// The tick the frame started.
    started: u64,
    /// The tier that was asked for (before a degrade).
    tier: Tier,
    /// Replaced by tier C.
    degraded: bool,
    /// The kinds this frame carries.
    kinds: u32,
    /// The clamped cell rectangle of the view (empty when the layer is gone or the view is outside the world).
    cells: CellRect,
    /// Tier C: the band being walked and the row after its last.
    band: u32,
    bands: u32,
    band_end: i32,
    /// Tier C: the bins.
    bins: BinGrid,
    /// Entities go into `seen` only once the frame spans a tick boundary: within one tick nothing moves, so an id
    /// cannot come up twice, and the common one-tick frame skips the set.
    track_seen: bool,
    /// The props seen so far (kept from frame to frame in [`MapSub`]).
    props: DetMap<u32, PropInfo>,
}

/// The state of one map subscription on the game thread (25 §3.15 `MapSub`).
#[derive(Clone, Debug)]
pub struct MapSub {
    /// The client's subscription id (stamped on every frame).
    pub sub_id: u32,
    /// What the client asked for.
    pub spec: MapSpec,
    /// Ticks between two frames of the current tier.
    pub cadence: u32,
    /// The tick the next frame (or the next step of this one) is due.
    pub due_tick: u64,
    /// The `(row, col)` the multi-tick walk continues from.
    pub cursor: (u32, u32),
    /// Frames completed so far.
    pub generation: u32,
    /// The ids already in the frame (a frame never contains an entity twice).
    pub seen: DetSet<ObjId>,
    /// The frame under construction.
    pub frame: Option<MapFrame>,
    /// The walk of that frame.
    walk: Option<Walk>,
    /// Until this tick new frames start at tier C (a tier A or B frame overflowed the cap).
    degrade_until: u64,
    /// The bin buffer of the previous tier-C frame (kept for its capacity).
    spare_bins: Vec<[u16; 3]>,
    /// The props of the previous frame's monsters.
    spare_props: DetMap<u32, PropInfo>,
}

impl MapSub {
    /// The tier of the frame under construction (`None` between frames).
    pub fn walking_tier(&self) -> Option<Tier> {
        self.walk
            .as_ref()
            .map(|w| if w.degraded { Tier::C } else { w.tier })
    }
}

/// Checks that `(world, layer)` exists.
fn check_target(g: &Game, spec: &MapSpec) -> Result<(), SubError> {
    spec.check()?;
    let Some(w) = g.worlds.world(spec.world) else {
        return Err(SubError::new(
            SubErrorCode::NotFound,
            format!("world {} is not loaded", spec.world.0),
        ));
    };
    if !w.layers.contains_key(&spec.layer) {
        return Err(SubError::new(
            SubErrorCode::NotFound,
            format!("world {} has no layer {}", spec.world.0, spec.layer.0),
        ));
    }
    Ok(())
}

/// Creates the state of a new map subscription (hook of `AdminViewOp::Open`). The first frame is due at once.
pub fn open(
    g: &Game,
    sub_id: u32,
    spec: MapSpec,
    tick: u64,
    t: &MapTuning,
) -> Result<MapSub, SubError> {
    check_target(g, &spec)?;
    Ok(MapSub {
        sub_id,
        cadence: tier_for(spec.rect, spec.max_tier, t).cadence_ticks(),
        spec,
        due_tick: tick,
        cursor: (0, 0),
        generation: 0,
        seen: DetSet::default(),
        frame: None,
        walk: None,
        degrade_until: 0,
        spare_bins: Vec::new(),
        spare_props: DetMap::default(),
    })
}

/// Applies a `MapUpdate` (hook of `AdminViewOp::Update`): the frame under construction is dropped and a new one
/// starts at once. A refused update (the world or layer does not exist) leaves the subscription as it was.
pub fn update(
    g: &Game,
    sub: &mut MapSub,
    spec: MapSpec,
    tick: u64,
    t: &MapTuning,
) -> Result<(), SubError> {
    check_target(g, &spec)?;
    sub.cadence = tier_for(spec.rect, spec.max_tier, t).cadence_ticks();
    sub.spec = spec;
    sub.due_tick = tick;
    sub.cursor = (0, 0);
    sub.frame = None;
    sub.walk = None;
    sub.seen.clear();
    sub.degrade_until = 0;
    Ok(())
}

/// What one step of the walk ended with.
enum Advance {
    /// The budget is out or a band is done: continue next tick.
    Pending,
    /// Every cell of the frame was read.
    Complete,
    /// A tier A or B frame has more entities than `admin.map_max_entities`.
    Overflow,
}

/// Advances one subscription in the `post` phase when it is due (25 §3.15 steps 1-5). Charges every entity read with
/// [`VisitBudget::entities`] and every cell with [`VisitBudget::cells`]; when the budget is out
/// ([`VisitBudget::exhausted`]) the cursor stays and the walk continues next tick. Returns the frame when it
/// completed.
pub fn collect(
    sub: &mut MapSub,
    g: &Game,
    tick: u64,
    budget: &mut VisitBudget,
    t: &MapTuning,
) -> Option<MapFrame> {
    let layer = g.worlds.layer(sub.spec.world, sub.spec.layer);
    if sub.walk.is_none() {
        begin(sub, layer, tick, t, false);
    }
    loop {
        match advance(sub, g, layer, budget, t) {
            Advance::Pending => {
                // the frame goes on in the next tick, where a mover can have crossed into a row already read
                if let (Some(w), Some(f)) = (sub.walk.as_mut(), sub.frame.as_ref())
                    && !w.track_seen
                {
                    sub.seen.extend(f.entities.iter().map(|e| ObjId(e.id)));
                    w.track_seen = true;
                }
                sub.due_tick = tick + 1;
                return None;
            }
            Advance::Overflow => {
                // too many entities: this frame becomes a tier-C frame, and so do the next ones for a while
                sub.degrade_until = tick + DEGRADE_HOLD_TICKS;
                begin(sub, layer, tick, t, true);
            }
            Advance::Complete => match finish(sub, g, layer, tick, budget, t) {
                Some(frame) => return Some(frame),
                None => begin(sub, layer, tick, t, true),
            },
        }
    }
}

/// Starts a frame: tier, kinds, cell rectangle, bands, the cleared `seen` set. `force_c` starts a degraded tier-C
/// frame.
fn begin(sub: &mut MapSub, layer: Option<&Layer>, tick: u64, t: &MapTuning, force_c: bool) {
    let requested = tier_for(sub.spec.rect, sub.spec.max_tier, t);
    let degraded = requested != Tier::C && (force_c || tick < sub.degrade_until);
    let tier = if degraded { Tier::C } else { requested };
    let kinds = tier.kinds(sub.spec.kinds);
    let geom = layer.map(|l| *l.grid.geom());
    let cells = geom
        .map(|geom| cell_rect(&geom, sub.spec.rect))
        .filter(|c| !c.is_empty())
        .unwrap_or(EMPTY_CELLS);
    // tier C: 8 bands of rows (fewer when the view has fewer rows)
    let rows = u32::try_from((cells.zmax - cells.zmin + 1).max(0)).unwrap_or(0);
    let bands = if tier == Tier::C {
        rows.min(C_BANDS)
    } else {
        1
    };
    let band_end = if tier == Tier::C {
        band_end_row(&cells, bands, 0)
    } else {
        cells.zmax + 1
    };
    let bins = if tier == Tier::C && kinds & EntityKind::Monster.bit() != 0 {
        let extent = geom.map_or((0.0, 0.0), |gm| {
            (gm.cells.0 as f32 * gm.cell_m, gm.cells.1 as f32 * gm.cell_m)
        });
        BinGrid::over(sub.spec.rect, extent, std::mem::take(&mut sub.spare_bins))
    } else {
        BinGrid::default()
    };
    sub.cadence = tier.cadence_ticks();
    sub.cursor = (cells.zmin.max(0) as u32, cells.xmin.max(0) as u32);
    sub.seen.clear();
    sub.frame = Some(MapFrame {
        sub_id: sub.sub_id,
        tick: 0,
        tier,
        degraded,
        complete_in_one_tick: false,
        entities: Vec::new(),
        bins: Vec::new(),
        player_names: Vec::new(),
    });
    sub.walk = Some(Walk {
        started: tick,
        tier: requested,
        degraded,
        kinds,
        cells,
        band: 0,
        bands,
        band_end,
        bins,
        track_seen: false,
        props: std::mem::take(&mut sub.spare_props),
    });
}

/// A cell rectangle without cells.
const EMPTY_CELLS: CellRect = CellRect {
    xmin: 0,
    xmax: -1,
    zmin: 0,
    zmax: -1,
};

/// The row after the last row of band `band` of `bands` over `cells` (25 §3.15 step 4: `ceil(rows / bands)` rows
/// per band; Madrigal: 480 rows, 60 per band).
fn band_end_row(cells: &CellRect, bands: u32, band: u32) -> i32 {
    if bands == 0 {
        return cells.zmin;
    }
    let rows = u32::try_from((cells.zmax - cells.zmin + 1).max(0)).unwrap_or(0);
    let per_band = i64::from(rows.div_ceil(bands));
    let end = i64::from(cells.zmin) + per_band * (i64::from(band) + 1);
    i32::try_from(end.min(i64::from(cells.zmax) + 1)).unwrap_or(cells.zmax + 1)
}

/// The cells that cover `rect`, clamped to the map (empty when the view is outside the world).
fn cell_rect(geom: &GridGeom, rect: [f32; 4]) -> CellRect {
    let m = geom.cell_m.max(1.0);
    let cell = |v: f32| (v / m).floor() as i32;
    CellRect {
        xmin: cell(rect[0]).max(0),
        xmax: cell(rect[2]).min(geom.cells.0 - 1),
        zmin: cell(rect[1]).max(0),
        zmax: cell(rect[3]).min(geom.cells.1 - 1),
    }
}

fn inside(rect: &[f32; 4], x: f32, z: f32) -> bool {
    x >= rect[0] && x <= rect[2] && z >= rect[1] && z <= rect[3]
}

/// An angle in degrees as 256 steps (a negative angle wraps in two's complement: `-90` is 192).
fn yaw_of(degrees: f32) -> u8 {
    if !degrees.is_finite() {
        return 0;
    }
    ((degrees * (256.0 / 360.0)).round() as i64 & 0xFF) as u8
}

/// `ceil(hp * 100 / max)`: at least 1 while alive.
fn hp_pct_of(hp: i32, max: i32) -> u8 {
    if hp <= 0 || max <= 0 {
        return 0;
    }
    if hp >= max {
        return 100;
    }
    let pct = (i64::from(hp) * 100 + i64::from(max) - 1) / i64::from(max);
    u8::try_from(pct.clamp(1, 100)).unwrap_or(100)
}

/// What a mover is on the map (`None` for a player: players come from the session table).
fn kind_of(m: &Mover) -> Option<EntityKind> {
    if m.npc().is_some() {
        return Some(EntityKind::Npc);
    }
    m.monster().map(|e| {
        if e.ai.kind == AII_PET {
            EntityKind::Pet
        } else {
            EntityKind::Monster
        }
    })
}

/// The entity record of a player (see the module documentation for the conventions).
fn player_entity(g: &Game, m: &Mover) -> MapEntity {
    let core = &m.core;
    MapEntity {
        id: core.id.0,
        kind: EntityKind::Player,
        sub: u8::try_from(m.mv.job.max(0)).unwrap_or(0),
        prop: core.index,
        level: u16::try_from(m.mv.level.max(0)).unwrap_or(u16::MAX),
        x: core.pos.x,
        z: core.pos.z,
        yaw: yaw_of(core.angle),
        hp_pct: hp_pct_of(
            m.mv.hp,
            crate::stats::env::max_hp(m, g.services.stats.core()),
        ),
        flags: MapFlags::of_mover(m),
        target: 0,
    }
}

/// The flags of a monster, pet or NPC: what [`MapFlags::of_mover`] says for them, from fewer cache lines (a crowded
/// view reads thousands of these; `of_mover` also looks at the mode flags, the type flags and the action state, which
/// only players need: a dead monster has no HP).
fn creature_flags(m: &Mover, e: Option<&crate::mover::MonsterExt>, flying: bool) -> MapFlags {
    let Some(e) = e else {
        return MapFlags::default().with(MapFlags::DEAD, m.mv.hp == 0);
    };
    let pet = e.ai.kind == AII_PET;
    MapFlags::default()
        .with(MapFlags::ASLEEP, !pet && m.core.viewers.is_empty())
        .with(MapFlags::DEAD, m.mv.hp == 0)
        .with(
            MapFlags::FIGHTING,
            e.ai.monster().is_some_and(|b| b.target.is_some()),
        )
        .with(MapFlags::AGGRESSIVE, e.active_attack)
        .with(
            MapFlags::BOSS,
            matches!(e.class, RANK_BOSS | RANK_MIDBOSS | RANK_SUPER),
        )
        .with(MapFlags::FLYING, flying)
}

/// The rank of a monster as `MapEntity.sub` carries it: 0 normal (also low and material), 1 captain, 2 mid-boss,
/// 3 boss (`RANK_SUPER`: Clockworks, Rangda, Asmodan, the instance bosses), 4 giant (`RANK_BOSS`: the "Giant X" field
/// monsters, Great White Bolo, the dungeon giants and leaders), 5 guard.
pub fn rank_code(class: u32) -> u8 {
    match class {
        RANK_CAPTAIN => 1,
        RANK_MIDBOSS => 2,
        RANK_SUPER => 3,
        RANK_BOSS => 4,
        flyff_common::defs::attribute::RANK_GUARD => 5,
        _ => 0,
    }
}

/// What a monster's record reads of its prop: the maximum HP base and whether it flies. Cached per subscription
/// (a few hundred props are in a view; the prop table itself is a megabyte of cold lines).
#[derive(Clone, Copy, Debug, Default)]
struct PropInfo {
    add_hp: u32,
    flying: bool,
}

/// The entity record of a monster, pet or NPC (see the module documentation for the conventions).
fn creature_entity(
    g: &Game,
    m: &Mover,
    kind: EntityKind,
    tier: Tier,
    props: &mut DetMap<u32, PropInfo>,
) -> MapEntity {
    let core = &m.core;
    let monster = m.monster();
    let mut flying = false;
    let (sub, target, hp_pct) = match kind {
        EntityKind::Npc => (0, 0, 100),
        _ => {
            // a monster's maximum is its prop's (times the GM multiplier): reading the DST table of every monster
            // of a crowded view costs more than the rest of the record; a buff on the maximum is not seen (the bar
            // is clamped at 100 %)
            let info = *props.entry(m.prop_id()).or_insert_with(|| {
                g.data
                    .mover(m.prop_id())
                    .map_or(PropInfo::default(), |p| PropInfo {
                        add_hp: p.add_hp,
                        flying: p.flying == 1,
                    })
            });
            flying = info.flying;
            let hp = hp_pct_of(
                m.mv.hp,
                flyff_rules::stats::npc_max_hp_origin(info.add_hp, 1.0, m.mv.rates.hp),
            );
            let (sub, target) = if kind == EntityKind::Pet {
                (0, monster.and_then(|e| e.owner).map_or(0, |o| o.0))
            } else {
                (
                    monster.map_or(0, |e| rank_code(e.class)),
                    monster
                        .and_then(|e| e.ai.monster())
                        .and_then(|b| b.target)
                        .map_or(0, |o| o.0),
                )
            };
            (sub, if tier == Tier::A { target } else { 0 }, hp)
        }
    };
    let mut flags = creature_flags(m, monster, flying);
    if kind != EntityKind::Monster {
        // the sleep rule is about monsters
        flags = MapFlags(flags.0 & !MapFlags::ASLEEP);
    }
    MapEntity {
        id: core.id.0,
        kind,
        sub,
        prop: core.index,
        level: u16::try_from(m.mv.level.max(0)).unwrap_or(u16::MAX),
        x: core.pos.x,
        z: core.pos.z,
        yaw: yaw_of(core.angle),
        hp_pct,
        flags,
        target,
    }
}

/// An object of a cell, resolved from its id.
enum Obj<'a> {
    Mover(&'a Mover),
    Item(&'a ItemDrop),
    Ctrl(&'a Ctrl),
}

fn resolve(store: &ObjectStore, id: ObjId) -> Option<Obj<'_>> {
    match store.lookup(id)? {
        ObjRef::Mover(k) => store.movers.get(k).map(Obj::Mover),
        ObjRef::Item(k) => store.items.get(k).map(Obj::Item),
        ObjRef::Ctrl(k) => store.ctrls.get(k).map(Obj::Ctrl),
    }
}

/// Loads what the record of `m` reads. A crowded view is mostly cache misses (a mover is 2.6 KB and the record
/// reads a handful of its lines): a batch whose lines are all touched in one tight loop has its misses overlap, and
/// the record code that follows finds them in cache (about 1.5 times faster on a cold cache; `res_admin_views`).
fn touch_mover(m: &Mover) -> u64 {
    let c = &m.core;
    let base = u64::from(c.deleted)
        ^ u64::from(c.pos.x.to_bits())
        ^ u64::from(c.angle.to_bits())
        ^ (c.viewers.len() as u64)
        ^ (m.mv.hp as u64)
        ^ u64::from(m.mv.rates.hp.to_bits());
    match m.monster() {
        Some(e) => base ^ u64::from(e.class) ^ u64::from(e.ai.state) ^ u64::from(e.active_attack),
        // a player: the mode and type flags and the action state too
        None => {
            base ^ u64::from(m.mv.mode)
                ^ u64::from(m.mv.type_flags)
                ^ u64::from(crate::combat::is_die(m))
        }
    }
}

fn touch(o: &Obj<'_>) -> u64 {
    match o {
        Obj::Mover(m) => touch_mover(m),
        Obj::Item(i) => u64::from(i.core.pos.x.to_bits()) ^ u64::from(i.item.count),
        Obj::Ctrl(c) => u64::from(c.core.pos.x.to_bits()),
    }
}

/// Reads a batch of ids of `cell.others` for the frame under construction, in order: the ids are resolved, their
/// memory is touched together, then every record is built.
fn read_batch(
    g: &Game,
    ids: &[ObjId],
    w: &mut Walk,
    rect: &[f32; 4],
    seen: &mut DetSet<ObjId>,
    out: &mut Vec<MapEntity>,
) {
    let store = g.worlds.store();
    let objs: Vec<(ObjId, Obj<'_>)> = ids
        .iter()
        .filter_map(|&id| resolve(store, id).map(|o| (id, o)))
        .collect();
    let mut acc = 0_u64;
    for (_, o) in &objs {
        acc ^= touch(o);
    }
    std::hint::black_box(acc);
    for (id, o) in objs {
        if let Read::Entity(e) = read_object(g, id, o, w, rect, seen) {
            out.push(e);
        }
    }
}

/// What reading one object of a cell produced.
enum Read {
    /// Nothing for this frame (gone, filtered, outside the rectangle).
    Skip,
    /// An entity.
    Entity(MapEntity),
    /// A monster counted into a bin.
    Binned,
}

/// Reads one object of a cell for the frame under construction.
fn read_object(
    g: &Game,
    id: ObjId,
    obj: Obj<'_>,
    w: &mut Walk,
    rect: &[f32; 4],
    seen: &mut DetSet<ObjId>,
) -> Read {
    let tier = if w.degraded { Tier::C } else { w.tier };
    match obj {
        Obj::Mover(m) => {
            if m.core.deleted {
                return Read::Skip;
            }
            let Some(kind) = kind_of(m) else {
                return Read::Skip;
            };
            if w.kinds & kind.bit() == 0 || !inside(rect, m.core.pos.x, m.core.pos.z) {
                return Read::Skip;
            }
            if tier == Tier::C && kind == EntityKind::Monster {
                let class = m.monster().map_or(0, |e| e.class);
                if !matches!(class, RANK_BOSS | RANK_MIDBOSS | RANK_SUPER) {
                    // counted where it is read: no `seen` entry (a mover that changes band during the scan is
                    // counted 0 or 2 times, which the approximate bins allow; the errors cancel on average)
                    let asleep = m.core.viewers.is_empty();
                    w.bins
                        .add(m.core.pos.x, m.core.pos.z, class == RANK_CAPTAIN, asleep);
                    return Read::Binned;
                }
            }
            if w.track_seen && !seen.insert(id) {
                return Read::Skip;
            }
            Read::Entity(creature_entity(g, m, kind, tier, &mut w.props))
        }
        Obj::Item(it) => {
            if it.core.deleted
                || w.kinds & EntityKind::Item.bit() == 0
                || !inside(rect, it.core.pos.x, it.core.pos.z)
                || (w.track_seen && !seen.insert(id))
            {
                return Read::Skip;
            }
            let now = g.clock.game_ms.0;
            let priority = it.meta.owner.is_some()
                && now.saturating_sub(it.meta.drop_time.0) < LOOT_PRIORITY_MS;
            Read::Entity(MapEntity {
                id: id.0,
                kind: EntityKind::Item,
                sub: 0,
                prop: it.core.index,
                level: it.item.count,
                x: it.core.pos.x,
                z: it.core.pos.z,
                yaw: yaw_of(it.core.angle),
                hp_pct: 0,
                flags: MapFlags::default()
                    .with(MapFlags::OWNER_ONLY, priority || it.meta.holder.is_some()),
                target: 0,
            })
        }
        Obj::Ctrl(c) => {
            if c.core.deleted
                || w.kinds & EntityKind::Ctrl.bit() == 0
                || !inside(rect, c.core.pos.x, c.core.pos.z)
                || (w.track_seen && !seen.insert(id))
            {
                return Read::Skip;
            }
            let kind = g
                .data
                .ctrls
                .get(c.core.index)
                .map_or(0, |p| u8::try_from(p.ctrl_kind1).unwrap_or(u8::MAX));
            Read::Entity(MapEntity {
                id: id.0,
                kind: EntityKind::Ctrl,
                sub: kind,
                prop: c.core.index,
                level: 0,
                x: c.core.pos.x,
                z: c.core.pos.z,
                yaw: yaw_of(c.core.angle),
                hp_pct: 0,
                flags: MapFlags::default(),
                target: 0,
            })
        }
    }
}

/// Walks cells from the cursor until the frame (tier A, B) or the band (tier C) is done, the budget runs out, or a
/// tier A or B frame overflows the cap.
fn advance(
    sub: &mut MapSub,
    g: &Game,
    layer: Option<&Layer>,
    budget: &mut VisitBudget,
    t: &MapTuning,
) -> Advance {
    let MapSub {
        spec,
        cursor,
        seen,
        frame,
        walk,
        ..
    } = sub;
    let (Some(w), Some(frame)) = (walk.as_mut(), frame.as_mut()) else {
        return Advance::Complete;
    };
    let Some(layer) = layer else {
        return Advance::Complete;
    };
    let tier_c = w.degraded || w.tier == Tier::C;
    let rect = spec.rect;
    let cap = t.max_entities as usize;
    let row_end = if tier_c { w.band_end } else { w.cells.zmax + 1 };
    let (xmin, xmax) = (w.cells.xmin.max(0), w.cells.xmax);
    let mut stepped = false;
    // the ids of the cells read so far, turned into records in batches (see `read_batch`)
    let mut batch: Vec<ObjId> = Vec::with_capacity(READ_BATCH + 64);
    macro_rules! flush {
        () => {
            if !batch.is_empty() {
                read_batch(g, &batch, w, &rect, seen, &mut frame.entities);
                batch.clear();
            }
        };
    }
    while (cursor.0 as i32) < row_end {
        let row = cursor.0 as i32;
        while (cursor.1 as i32) <= xmax {
            // a call always reads at least one chunk, so a frame advances even on an exhausted budget
            if stepped && budget.exhausted() {
                flush!();
                return Advance::Pending;
            }
            stepped = true;
            // the chunk is as long as the budget pays for in empty cells (at least one cell)
            let first = cursor.1 as i32;
            let afford = i32::try_from(budget.cells_left().max(1)).unwrap_or(i32::MAX);
            let last = xmax.min(first.saturating_add(afford - 1));
            let mut next = first;
            for (x, cell) in layer.grid.row_cells(row, first, last) {
                // the empty cells before this one and the cell itself
                budget.cells(u32::try_from(x - next + 1).unwrap_or(1));
                next = x + 1;
                cursor.1 = u32::try_from(next).unwrap_or(0);
                budget.entities(u32::try_from(cell.others.len()).unwrap_or(u32::MAX));
                batch.extend_from_slice(&cell.others);
                if batch.len() >= READ_BATCH || budget.exhausted() {
                    flush!();
                    if !tier_c && frame.entities.len() > cap {
                        return Advance::Overflow;
                    }
                    if budget.exhausted() {
                        return Advance::Pending;
                    }
                }
            }
            // the empty cells at the end of the chunk
            budget.cells(u32::try_from(last - next + 1).unwrap_or(0));
            cursor.1 = u32::try_from(last + 1).unwrap_or(0);
        }
        cursor.1 = u32::try_from(xmin).unwrap_or(0);
        cursor.0 += 1;
    }
    flush!();
    if !tier_c && frame.entities.len() > cap {
        return Advance::Overflow;
    }
    if tier_c && w.band + 1 < w.bands {
        // the next band is read on the next tick
        w.band += 1;
        w.band_end = band_end_row(&w.cells, w.bands, w.band);
        return Advance::Pending;
    }
    Advance::Complete
}

/// Completes a frame: the players of the layer, the bins, the names, the bookkeeping. `None` when a tier A or B
/// frame has too many entities with the players counted (the caller restarts it as tier C).
fn finish(
    sub: &mut MapSub,
    g: &Game,
    layer: Option<&Layer>,
    tick: u64,
    budget: &mut VisitBudget,
    t: &MapTuning,
) -> Option<MapFrame> {
    let (Some(w), Some(mut frame)) = (sub.walk.take(), sub.frame.take()) else {
        return None;
    };
    let tier = frame.tier;
    if let Some(layer) = layer
        && w.kinds & EntityKind::Player.bit() != 0
    {
        let reads = players(g, &sub.spec, layer, &mut sub.seen, &mut frame);
        budget.entities(reads);
    }
    if tier != Tier::C && frame.entities.len() > t.max_entities as usize {
        sub.degrade_until = tick + DEGRADE_HOLD_TICKS;
        return None;
    }
    if tier == Tier::C {
        frame.bins = w.bins.bins();
        sub.spare_bins = w.bins.counts;
    }
    sub.spare_props = w.props;
    frame.tick = tick;
    frame.complete_in_one_tick = w.started == tick;
    sub.generation = sub.generation.wrapping_add(1);
    sub.due_tick = (w.started + u64::from(tier.cadence_ticks())).max(tick + 1);
    Some(frame)
}

/// Appends the players of `layer` inside the rectangle to `frame`; returns the number of session entries read.
fn players(
    g: &Game,
    spec: &MapSpec,
    layer: &Layer,
    seen: &mut DetSet<ObjId>,
    frame: &mut MapFrame,
) -> u32 {
    let store = g.worlds.store();
    // the avatars, resolved together and touched together (see `touch_mover`), then filtered in id order
    let movers: Vec<&Mover> = g
        .sessions
        .iter()
        .filter_map(|e| store.mover(e.player))
        .collect();
    let mut acc = 0_u64;
    for m in &movers {
        acc ^= touch_mover(m);
    }
    std::hint::black_box(acc);
    for m in movers {
        let core = &m.core;
        if core.deleted
            || core.world != spec.world
            || core.layer != spec.layer
            || !matches!(core.link, LinkState::Linked { .. })
            || !inside(&spec.rect, core.pos.x, core.pos.z)
            || !layer.members.contains(&core.id)
        {
            continue;
        }
        let Some(p) = m.player() else { continue };
        let ent = player_entity(g, m);
        if ent.flags.has(MapFlags::INVISIBLE) && !spec.see_invisible {
            continue;
        }
        if !seen.insert(core.id) {
            continue;
        }
        frame.player_names.push((core.id.0, p.ident.name.clone()));
        frame.entities.push(ent);
    }
    u32::try_from(g.sessions.len()).unwrap_or(u32::MAX)
}
