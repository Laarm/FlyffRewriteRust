//! Target scans (Plan/18 §8.7, ref/12 §8.4, `AIInterface.cpp:126-322`; WP-AI-07).
//!
//! **Window and order (KEEP-QUIRK "first match", KQ-13).** A scan visits the player grid of the monster's layer
//! exactly like the legacy `FOR_LINKMAP` on link level 0 (`World.h:790-863`, [`crate::world::grid::legacy_cells`]):
//! rows `z` ascending, inside a row cells `x` ascending, inside a cell the legacy link-list order (the first
//! player inserted into an empty cell stays at the head, every later one goes directly behind it; the grid of
//! doc 15 keeps that order). The result is the **first match**, not the nearest player, except for
//! [`scan_target_strong`] (highest level, the first of equals). The window argument is passed **as the original
//! passed it**: [`scan_target`] hands its range in metres to a macro that expects heightmap units (so its window
//! is `MPU` times larger than needed, harmless); [`scan_target_strong`] and [`scan_target_over_healer`] convert
//! with `trunc(r / MPU)`, which can miss a player standing near the edge of `r` (Q13).
//!
//! The scans read a [`ScanView`] — a per-tick copy of what they need from the players of the layer, captured at
//! the head of the AI pass — so that a handler can hold the one `&mut Mover` it acts on and still look at every
//! player. Distances are 3-D (`D3DXVec3LengthSq`, f32, `<` as in the original).

use super::ctx::AiCx;
use crate::mover::Mover;
use crate::world::grid::{LinkSet, legacy_cells};
use crate::world::object::{MoverKey, ObjectStore};
use flyff_common::ObjId;
use flyff_common::math::Vec3;
use flyff_data::GameData;
use flyff_rules::consts::TRANSPARENT_MODE;
use flyff_rules::consts::dst::DST_HAWKEYE_RATE;
use flyff_rules::consts::parts::PARTS_RWEAPON;
use smallvec::SmallVec;

/// `RA_SAFETY` of `flyff_common::defs::define`.
const RA_SAFETY: u32 = flyff_common::defs::define::RA_SAFETY;
/// `RANK_GUARD` (`dwClass`).
pub const RANK_GUARD: u32 = flyff_common::defs::attribute::RANK_GUARD;
/// `JOB_ALL` (`= MAX_JOB`, `defineJob.h`): the scan job condition that matches every job.
pub const JOB_ALL: i32 = 32;

/// What a scan or a RAGE tick needs to know about one player (a copy taken at the start of the tick).
#[derive(Clone, Debug, PartialEq)]
pub struct PlayerScan {
    /// The player.
    pub id: ObjId,
    /// `GetPos()`.
    pub pos: Vec3,
    /// `IsLive()`: `!actmover.IsDie() || hp > 0` (the original's `||`: not the opposite of `die`).
    pub live: bool,
    /// `IsDie()`: `actmover.IsDie() || hp == 0`.
    pub die: bool,
    /// `m_pActMover->IsFly()` (the `FLY` state flag).
    pub fly: bool,
    /// `GetJob()`.
    pub job: i32,
    /// `GetLevel()`.
    pub level: i32,
    /// `m_idparty` (0 = none).
    pub party: u32,
    /// `IsChaotic()`: `m_dwPKPropensity > 0`.
    pub chaotic: bool,
    /// `IsMode(TRANSPARENT_MODE)` (a GM in invisible mode).
    pub transparent: bool,
    /// `HasBuffByIk3(IK3_TEXT_DISGUISE)`.
    pub disguised: bool,
    /// `IsRegionAttr(RA_SAFETY)`.
    pub in_safety_region: bool,
    /// `CUser::m_nOverHeal` (`combat.over_heal_ticks`, maintained by the heal code of doc 17; 0 until it does).
    pub over_heal: i32,
    /// The item in the right hand (`GetActiveHandItemProp()` of a player): 0 = bare hands (RUNAWAY reads the
    /// attack range of the target's weapon).
    pub hand_item: u32,
    /// `DST_HAWKEYE_RATE` of the player (the attack range bonus of `GetAttackRange`).
    pub hawkeye: i32,
    /// `(item id, GetItemNum)` for the items some monster scans for (`ScanView::scan_items`).
    pub item_counts: SmallVec<[(u32, u32); 2]>,
    /// The scanned quests (`ScanView::scan_quests`) the player has.
    pub quests: SmallVec<[u32; 2]>,
}

impl PlayerScan {
    /// The copy of player `m` for the scans of one tick; `spec` names the items and quests to look up.
    pub fn of(m: &Mover, spec: &ScanSpec) -> PlayerScan {
        let st = m.sim.act.state;
        let mut item_counts = SmallVec::new();
        let mut quests = SmallVec::new();
        if let Some(p) = m.player() {
            for &item in &spec.items {
                let n: u32 = p.items.inventory.count_of(item);
                item_counts.push((item, n));
            }
            for &q in &spec.quests {
                if p.quests
                    .doc
                    .active
                    .iter()
                    .any(|a| u32::from(a.quest_id) == q)
                {
                    quests.push(q);
                }
            }
        }
        let player = m.player();
        PlayerScan {
            id: m.id(),
            pos: m.core.pos,
            live: !st.is_die() || m.mv.hp > 0,
            die: st.is_die() || m.mv.hp == 0,
            fly: m.sim.act.flags.fly(),
            job: m.mv.job,
            level: m.mv.level,
            party: player
                .and_then(|p| p.social.party)
                .map_or(0, |party| party.0),
            chaotic: player.is_some_and(|p| p.pk.propensity > 0),
            transparent: m.mv.mode & TRANSPARENT_MODE != 0,
            disguised: m.mv.disguise.is_some(),
            in_safety_region: player.is_some_and(|p| p.region_attr & RA_SAFETY != 0),
            over_heal: i32::try_from(m.combat.over_heal_ticks).unwrap_or(i32::MAX),
            hand_item: crate::combat::worn_item(m, PARTS_RWEAPON).map_or(0, |w| w.item_id),
            hawkeye: m.mv.dst.table.param(DST_HAWKEYE_RATE, 0),
            item_counts,
            quests,
        }
    }

    /// `GetItemNum(item)` for an item of the scan spec (0 for any other).
    pub fn item_num(&self, item: u32) -> u32 {
        self.item_counts
            .iter()
            .find(|(i, _)| *i == item)
            .map_or(0, |(_, n)| *n)
    }

    /// `GetQuest(quest) != NULL` for a quest of the scan spec.
    pub fn has_quest(&self, quest: u32) -> bool {
        self.quests.contains(&quest)
    }
}

/// The items and quests any monster prop scans for (`#Scan item` / `#Scan quest`), collected once from the
/// loaded data: only those are looked up in the players' inventories and quest books at capture time (in V15
/// data both lists are empty).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScanSpec {
    /// `m_dwScanItemIdx` values.
    pub items: Vec<u32>,
    /// `m_dwScanQuestId` values.
    pub quests: Vec<u32>,
}

impl ScanSpec {
    /// The spec of `data`'s monster props (sorted, deduplicated).
    pub fn of(data: &GameData) -> ScanSpec {
        let mut spec = ScanSpec::default();
        for p in data.movers.values() {
            if p.ex.ai.scan_item_idx != 0 {
                spec.items.push(p.ex.ai.scan_item_idx);
            }
            if p.ex.ai.scan_quest_id != 0 {
                spec.quests.push(p.ex.ai.scan_quest_id);
            }
        }
        spec.items.sort_unstable();
        spec.items.dedup();
        spec.quests.sort_unstable();
        spec.quests.dedup();
        spec
    }
}

/// The players of a layer as the scans see them: sorted by id, captured at the head of the tick's AI pass.
#[derive(Clone, Debug, Default)]
pub struct ScanView {
    players: Vec<PlayerScan>,
    spec: Option<ScanSpec>,
}

impl ScanView {
    /// An empty view.
    pub fn new() -> ScanView {
        ScanView::default()
    }

    /// A view over `players` (tests build views by hand); they are sorted by id.
    pub fn from_players(mut players: Vec<PlayerScan>) -> ScanView {
        players.sort_by_key(|p| p.id);
        ScanView {
            players,
            spec: Some(ScanSpec::default()),
        }
    }

    /// Replaces the view with the players among `awake` (the sleep rule keeps every player awake). The
    /// [`ScanSpec`] is derived from `data` on the first call.
    pub fn capture(&mut self, store: &ObjectStore, awake: &[MoverKey], data: &GameData) {
        let spec = self.spec.get_or_insert_with(|| ScanSpec::of(data));
        self.players.clear();
        for m in awake.iter().filter_map(|k| store.movers.get(*k)) {
            if m.is_player() && !m.core.deleted {
                self.players.push(PlayerScan::of(m, spec));
            }
        }
        self.players.sort_by_key(|p| p.id);
    }

    /// The player `id`, `None` for anything that is not a player of this layer.
    pub fn get(&self, id: ObjId) -> Option<&PlayerScan> {
        self.players
            .binary_search_by_key(&id, |p| p.id)
            .ok()
            .and_then(|i| self.players.get(i))
    }

    /// Number of players in the view.
    pub fn len(&self) -> usize {
        self.players.len()
    }

    /// `true` when the view holds no player.
    pub fn is_empty(&self) -> bool {
        self.players.is_empty()
    }
}

fn dist_sq(a: Vec3, b: Vec3) -> f32 {
    let d = a - b;
    d.x * d.x + d.y * d.y + d.z * d.z
}

/// `CObj::GetWorld()->m_iMPU` of the layer the scan runs in.
fn mpu(cx: &AiCx<'_>) -> f32 {
    cx.grid.geom().mpu as f32
}

/// `ScanTarget(center, range, job, quest, item, chao)` (18 §8.7). `range_m` is passed to the link-map window
/// as heightmap units and squared as metres, like the original. Returns the first player in grid order that
/// matches all of: alive; the job, item, quest and chaos conditions (all four zero never match — callers pass
/// [`JOB_ALL`]); flight compatible (a guard ignores the flight filter, everybody else needs
/// `is_flying_npc == player.is_fly`); within `range_m` in 3-D; not `TRANSPARENT_MODE`; not disguised.
pub fn scan_target(
    cx: &AiCx<'_>,
    me: &Mover,
    range_m: i32,
    job: i32,
    quest: u32,
    item: u32,
    chao: i32,
) -> Option<ObjId> {
    let cond_scan = job != 0 || quest != 0 || item != 0 || chao != 0;
    if !cond_scan {
        // the original never sets `nAble` without a condition: the scan can never match
        return None;
    }
    let guard = me.monster().is_some_and(|x| x.class == RANK_GUARD);
    let fly_mob = me.sim.params.flying_npc;
    let radius = (range_m * range_m) as f32;
    let pos = me.core.pos;
    let mut found = None;
    cx.grid
        .for_each_legacy(pos, range_m, LinkSet::Players, |id| {
            if found.is_some() {
                return;
            }
            let Some(p) = cx.scan.get(id) else { return };
            if !p.live {
                return;
            }
            let job_ok = job == 0 || job == JOB_ALL || p.job == job;
            let item_ok = item == 0 || p.item_num(item) != 0;
            let quest_ok = quest == 0 || p.has_quest(quest);
            let chao_ok = match chao {
                0 => true,
                100 => p.chaotic,
                101 => !p.chaotic,
                _ => false,
            };
            if !(job_ok && item_ok && quest_ok && chao_ok) {
                return;
            }
            if (guard || fly_mob == p.fly)
                && dist_sq(p.pos, pos) < radius
                && !p.transparent
                && !p.disguised
            {
                found = Some(p.id);
            }
        });
    found
}

/// `ScanTargetNext(center, range, current, party)` (18 §8.7): the first live player other than `cur` in the
/// window of `range`, flight compatible (a ground monster accepts a flying member too, which RAGE drops on its
/// next tick: Q11), within `range` (3-D), in party `party`, not transparent and not disguised. Nothing for
/// `party == 0`, `range <= 0` or a guard.
pub fn scan_target_next(
    cx: &AiCx<'_>,
    me: &Mover,
    range: i32,
    cur: Option<ObjId>,
    party: u32,
) -> Option<ObjId> {
    if party == 0 || range <= 0 || me.monster().is_some_and(|x| x.class == RANK_GUARD) {
        return None;
    }
    let fly_mob = me.sim.params.flying_npc;
    let radius = (range * range) as f32;
    let pos = me.core.pos;
    let mut found = None;
    cx.grid.for_each_legacy(pos, range, LinkSet::Players, |id| {
        if found.is_some() {
            return;
        }
        let Some(p) = cx.scan.get(id) else { return };
        if p.live
            && Some(p.id) != cur
            && (fly_mob == p.fly || !fly_mob)
            && dist_sq(p.pos, pos) < radius
            && p.party == party
            && !p.transparent
            && !p.disguised
        {
            found = Some(p.id);
        }
    });
    found
}

/// `ScanTargetStrong(center, r)` (18 §8.7): the highest-level player within `r_m` metres (3-D), the first of
/// equals; the window is `trunc(r / MPU)` (Q13). No liveness or flight test, only `TRANSPARENT_MODE`.
pub fn scan_target_strong(cx: &AiCx<'_>, me: &Mover, range_m: f32) -> Option<ObjId> {
    let radius = range_m * range_m;
    let pos = me.core.pos;
    let n = (range_m / mpu(cx)) as i32;
    let mut best: Option<(ObjId, i32)> = None;
    cx.grid.for_each_legacy(pos, n, LinkSet::Players, |id| {
        let Some(p) = cx.scan.get(id) else { return };
        if dist_sq(p.pos, pos) < radius && !p.transparent {
            match best {
                Some((_, lvl)) if p.level <= lvl => {}
                _ => best = Some((p.id, p.level)),
            }
        }
    });
    best.map(|(id, _)| id)
}

/// `ScanTargetOverHealer(center, r)` (18 §8.7): the first player with `over_heal > 0` within `r_m` metres, not
/// transparent; the window is `trunc(r / MPU)`.
pub fn scan_target_over_healer(cx: &AiCx<'_>, me: &Mover, range_m: f32) -> Option<ObjId> {
    let radius = range_m * range_m;
    let pos = me.core.pos;
    let n = (range_m / mpu(cx)) as i32;
    let mut found = None;
    cx.grid.for_each_legacy(pos, n, LinkSet::Players, |id| {
        if found.is_some() {
            return;
        }
        let Some(p) = cx.scan.get(id) else { return };
        if dist_sq(p.pos, pos) < radius && !p.transparent && p.over_heal > 0 {
            found = Some(p.id);
        }
    });
    found
}

/// The link-map window a scan with range argument `n_range` visits for a monster at `pos`
/// (`FOR_LINKMAP`, level 0): the inclusive cell rectangle `(xmin, xmax, zmin, zmax)`.
pub fn window(
    geom: &crate::world::catalog::GridGeom,
    pos: Vec3,
    n_range: i32,
) -> (i32, i32, i32, i32) {
    let r = legacy_cells(geom, pos, n_range);
    (r.xmin, r.xmax, r.zmin, r.zmax)
}
