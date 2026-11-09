//! Monster drop generation (Plan/19 §6.1-§6.4, §6.9, WP-ITEM-06; src:_Common/Mover.cpp:7368-8244 `CMover::DropItem
//! (CMover*)`, src:_Common/Project.cpp:175-206 `CDropItemGenerator::GetAt`, 4559-4665 `CRandomOptItemGen`; F-014, F-235,
//! F-281, KQ-12, PL-071).
//!
//! [`on_monster_died`] is called once per monster death by the death pipeline (`combat::death::DeathHooks::drop`,
//! installed as [`drop_hook`]) with the credited player `a` (the enemy with the most damage while it is still
//! valid, else the killer); the function is also called directly by the tests. It applies only when `a` is a
//! player and the dead mover is not.
//!
//! ```text
//! 1. wormon bookkeeping                                  QuestDropHook::on_wormon_killed (docs 20/21)
//! 2. event items, iff a.level - monster.level < 10       EventDropHooks::{generic_event_items, lua_event_items}
//! 3. quest items                                         QuestDropHook::on_monster_drop (doc 20)
//! 4. the number of passes (gift box, buffs, DST_GIFTBOX), the shuffled index order
//! 5. per pass: level gate -> item roll -> table (items; gold on pass 0) -> DropKind entries
//! 6. looter monsters drop what they carry               DropSeams::looter_items (doc 18)
//! ```
//!
//! **Probabilities** are rolled on the scale `0..3 000 000 000` (the comment of the data file is wrong: 1e8 is
//! 3.33 %; the roll is uniform, see [`roll_3e9`] for the deviation from the original's biased `xRandom`). The
//! piece factor divides the roll (`r / piece`), the item factor multiplies the per-pass chance
//! (`prob_pct x factors.item` against `random_below(100)`), the level gate lowers both the item chance and the gold
//! (`d = a.level - monster.level`: `<= 1` 100/100, `<= 2` 80/100, `<= 4` 60/80, `<= 7` 30/65, else 10/50 %).
//!
//! **What a drop becomes.** A normal monster: a ground item at `monster.pos +- 1 m` (x and z, `random_f32(2) - 1`)
//! with `GroundItemMeta{owner: a, drop_time: now, from_monster: true}`; `RANK_SUPER` monsters and the event
//! movers `MI_DEMIAN5` / `MI_KEAKOON5` / `MI_MUFFRIN5` leave the owner empty; a registered event monster
//! (`GameData.lua.event_monsters`) sets `owner = a`, `event_monster = id` and spreads by its `item_drop_range`.
//! A **flying** monster (`prop.flying != 0`) puts items into `a`'s inventory (`TID_GAME_REAPITEM`) and adds gold
//! directly when it fits; a full inventory loses the item as in the original unless
//! `fidelity.flying_drop_overflow_to_ground` (PL-071, default on) spawns it on the ground at the monster. A
//! flying table item carries **no** random stat option and no awakening (KEEP-QUIRK); a failed `DropKind` create
//! always falls through to the ground drop (KEEP).
//!
//! **Quirks kept** (19 §6.2): the stat-option index uses the monster level without `- 1` (`index_by_level[level]`);
//! a ground pass stops when `number == max_item` (equality, so `max_item == 0` never stops it unless a
//! `number == NULL_ID` entry drops first) while a flying pass stops at `number >= max_item`; `DropKind` stops after
//! the first drop of a `RANK_SUPER` monster; with the piece factor `<= 0` no roll is drawn and every entry hits.
//! **Fixes**: a `DropKind` of an item with `item_lv == 0` is skipped (the original read index -1); the roll is
//! uniform ([`roll_3e9`]).
//!
//! **Seams** (all default to "nothing"): [`EventDropHooks`] (doc 22: generic and Lua event items, event-spawn
//! monsters), [`QuestDropHook`] (doc 20), [`DropSeams`] (doc 21's party drop modes, ITEM-11's awakening generator,
//! doc 18's looter inventory). The Lua event items of `Event.lua` are wired in [`on_monster_died`].
//!
//! The random draws of one pass are, in order: the item roll `random_below(100)`; per table entry the hit roll
//! (one `next_u32` mapped by [`roll_3e9`], none when `piece <= 0`), then for a hit the count (`random_below(number)`),
//! the stat option (`random_below(i + 1)`, `random_below(100)` for weapons and armor), the awakening generator,
//! the serial, and the two spread draws (`random_f32(2)` for x and z; an event monster draws two more).

use super::container::SerialSource;
use super::env::{ItemCore, ItemEnv};
use super::fx::{Fx, item_json};
use super::gold::{self, core as gold_core};
use super::loot::GroundItemMeta;
use super::model::Item;
use super::ops::{ItemError, ItemOrigin, core as ops};
use crate::buff::BUFF_ITEM;
use crate::combat::hooks::KillCtx;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::event::EventFactors;
use crate::mover::Mover;
use crate::world::item_drop::ItemDrop;
use crate::world::object::ObjectStore;
use flyff_common::config::{Rates, RulesConfig};
use flyff_common::defs::attribute::{DST_GIFTBOX, RANK_SUPER};
use flyff_common::defs::item::{
    II_COUPLE_BUFF_MIRACLE_01, II_GOLD_SEED1, II_GOLD_SEED2, II_GOLD_SEED3, II_GOLD_SEED4,
    II_SYS_SYS_LS_CHEERING, II_SYS_SYS_SCR_GET01, II_SYS_SYS_SCR_GET02, II_SYS_SYS_VOTE_THANKS,
};
use flyff_common::defs::itemkind::{IK1_ARMOR, IK1_GENERAL, IK1_WEAPON, IK2_JEWELRY};
use flyff_common::defs::obj::{
    MI_CLOCKWORK1, MI_DEMIAN5, MI_KEAKOON5, MI_LOADCLOCKWORK, MI_MUFFRIN5,
};
use flyff_common::defs::text::TID_GAME_REAPITEM;
use flyff_common::math::Vec3;
use flyff_common::rng::GameRng;
use flyff_common::{GameMs, LayerId, ObjId, WorldId};
use flyff_data::GameData;
use flyff_data::items_ext::item_etc::{ItemEtcTables, RandomOptItemDef};
use flyff_data::mover_ex::{DropItem, DropType};
use flyff_data::props::item::ItemProp;
use flyff_data::props::mover::MoverProp;
use flyff_data::types::{ItemId, MoverId};
use flyff_db::ItemRecord;
use flyff_db::audit::ev;
use flyff_rules::consts::MAX_MONSTER_LEVEL;
use flyff_rules::item::random_option::{RandomOptKind, RandomOptWord, random_option_kind};
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::{Arc, OnceLock};

/// The scale of every drop probability: a roll is uniform in `0..3_000_000_000` (see [`roll_3e9`]).
pub const DROP_ROLL_SCALE: u32 = 3_000_000_000;

/// One roll on the scale of the drop tables: a uniform value in `0..3_000_000_000`, drawn as
/// `(next_u32 * 3e9) >> 32` (one draw).
///
/// **Deviation from `xRandom(3000000000)`** (= `xRand() % 3000000000`, see 19 §6.1): taking the 32-bit generator
/// output modulo 3e9 is biased, values below 1 294 967 296 come up twice as often, so the original dropped every
/// entry with a probability below that 1.4 times more often than the data says (a 10 000 000 entry hit 4.66 %
/// instead of 3.33 %). The plan states the nominal rate (19 §6.1; acceptance test of WP-ITEM-06: 3.33 % +- 3 sigma),
/// so the roll is uniform. Quest drops and ctrl drops (doc 20, WP-WORLD-18) use the same function so all
/// `3e9` rolls agree.
pub fn roll_3e9(rng: &mut dyn GameRng) -> u32 {
    ((u64::from(rng.next_u32()) * u64::from(DROP_ROLL_SCALE)) >> 32) as u32
}
/// `NULL_ID` in the `number` column: one piece, and the entry does not count against `max_item`.
pub const NULL_NUMBER: u32 = u32::MAX;
/// The most passes one kill can make. The original loops `1 + gift box + buffs + DST_GIFTBOX` times without a
/// bound; this guards a corrupt `DST_GIFTBOX` from freezing the tick.
pub const MAX_PASSES: i32 = 64;

// ---- configuration and factors -----------------------------------------------------------------------------

/// The switches the drop code reads (all startup class).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DropConfig {
    /// `features.shuffle_drop`: the table is walked in a random order.
    pub shuffle_drop: bool,
    /// `features.drop_awaked_items`: gear is pre-awakened.
    pub drop_awaked_items: bool,
    /// `features.better_clockwork_alpha`: `MI_LOADCLOCKWORK` is exempt from the level gate as well.
    pub better_clockwork_alpha: bool,
    /// `features.better_couplebuffs`: the hard-coded 1.05 of the Miracle couple buff is gone.
    pub better_couplebuffs: bool,
    /// `features.only_armor_awakes`.
    pub only_armor_awakes: bool,
    /// `fidelity.flying_drop_overflow_to_ground` (PL-071).
    pub flying_overflow_to_ground: bool,
}

impl DropConfig {
    /// The projection of the configuration.
    pub fn from_rules(r: &RulesConfig) -> DropConfig {
        DropConfig {
            shuffle_drop: r.features.shuffle_drop,
            drop_awaked_items: r.features.drop_awaked_items,
            better_clockwork_alpha: r.features.better_clockwork_alpha,
            better_couplebuffs: r.features.better_couplebuffs,
            only_armor_awakes: r.features.only_armor_awakes,
            flying_overflow_to_ground: r.fidelity.flying_drop_overflow_to_ground,
        }
    }
}

impl Default for DropConfig {
    /// The repack defaults.
    fn default() -> Self {
        DropConfig::from_rules(&RulesConfig::default())
    }
}

/// The multipliers of one kill (all `f32`, computed per kill from the credited player, 19 §6.1,
/// src:_Common/MoverParam.cpp:4143-4189). With the shipped `Event.lua`: item x10, piece x10, gold x250.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DropFactors {
    /// `GetItemDropRateFactor`: `rates.item_drop x monster.item_drop_rate x generic x Event.lua`.
    pub item: f32,
    /// `GetPieceItemDropRateFactor`: `Event.lua x lord x 1.1 (cheering) x 1.05 (thanks) x 1.05 (Miracle couple
    /// buff, off with `better_couplebuffs`)`. `<= 0` makes every entry hit without a roll.
    pub piece: f32,
    /// `prj.m_fGoldDropRate`.
    pub gold: f32,
    /// `MoverProp::m_fPenya_Rate` of the monster (back end, 1.0).
    pub penya: f32,
    /// `Event.lua GetGoldDropFactor`.
    pub gold_event: f32,
}

impl DropFactors {
    /// Neutral factors: every multiplier 1.0.
    pub const NEUTRAL: DropFactors = DropFactors {
        item: 1.0,
        piece: 1.0,
        gold: 1.0,
        penya: 1.0,
        gold_event: 1.0,
    };

    /// The factors of a kill by `attacker` of `monster` under the given rates and event factors.
    pub fn compute(
        rates: &Rates,
        events: &dyn EventFactors,
        monster: &Mover,
        attacker: &Mover,
        better_couplebuffs: bool,
    ) -> DropFactors {
        let mut item = 1.0_f32;
        item *= rates.item_drop_or_default();
        item *= monster.mv.rates.item_drop;
        item *= events.generic_item_drop();
        item *= events.lua_item_drop();
        let mut piece = 1.0_f32;
        piece *= events.lua_piece_drop();
        piece *= events.lord_item();
        if attacker.buffs.has(BUFF_ITEM, II_SYS_SYS_LS_CHEERING) {
            piece *= 1.1;
        }
        if attacker.buffs.has(BUFF_ITEM, II_SYS_SYS_VOTE_THANKS) {
            piece *= 1.05;
        }
        if !better_couplebuffs && attacker.buffs.has(BUFF_ITEM, II_COUPLE_BUFF_MIRACLE_01) {
            piece *= 1.05;
        }
        DropFactors {
            item,
            piece,
            gold: rates.gold_drop_or_default(),
            penya: monster.mv.rates.penya,
            gold_event: events.lua_gold(),
        }
    }
}

/// The level gate of one pass (19 §6.2 step 5.1): the item chance and the penya share in percent.
pub fn level_gate(attacker_level: i32, monster_level: i32) -> (i32, i32) {
    let d = attacker_level.wrapping_sub(monster_level);
    if d <= 1 {
        (100, 100)
    } else if d <= 2 {
        (80, 100)
    } else if d <= 4 {
        (60, 80)
    } else if d <= 7 {
        (30, 65)
    } else {
        (10, 50)
    }
}

// ---- the stat option table ---------------------------------------------------------------------------------

/// `CRandomOptItemGen` (src:_Common/Project.cpp:4559-4665): the random-option items sorted by level and the
/// per-level index, derived at startup from `ItemEtcTables.random_opt_items`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RandomOptItemGen {
    items: Vec<RandomOptItemDef>,
    /// `m_anIndex`: entry `L - 1` is the index of the last item with level `<= L`, `-1` below the first level.
    index_by_level: [i32; MAX_MONSTER_LEVEL as usize],
}

impl RandomOptItemGen {
    /// `Arrange`: the exchange sort by level and the index construction, as the original builds them.
    pub fn new(mut items: Vec<RandomOptItemDef>) -> RandomOptItemGen {
        for i in 0..items.len().saturating_sub(1) {
            for j in i + 1..items.len() {
                if items.get(i).map(|x| x.level) > items.get(j).map(|x| x.level) {
                    items.swap(i, j);
                }
            }
        }
        let mut index = [0_i32; MAX_MONSTER_LEVEL as usize];
        let mut level = 1_i32;
        let mut previous = -1_i32;
        for (i, it) in items.iter().enumerate() {
            if it.level > level {
                for j in level..it.level {
                    if let Some(slot) = usize::try_from(j - 1).ok().and_then(|k| index.get_mut(k)) {
                        *slot = previous;
                    }
                }
                level = it.level;
            }
            previous = i as i32;
        }
        for l in level..=MAX_MONSTER_LEVEL {
            if let Some(slot) = usize::try_from(l - 1).ok().and_then(|k| index.get_mut(k)) {
                *slot = previous;
            }
        }
        RandomOptItemGen {
            items,
            index_by_level: index,
        }
    }

    /// The generator of the loaded data.
    pub fn from_tables(t: &ItemEtcTables) -> RandomOptItemGen {
        RandomOptItemGen::new(t.random_opt_items.clone())
    }

    /// The sorted items.
    pub fn items(&self) -> &[RandomOptItemDef] {
        &self.items
    }

    /// `m_anIndex[level - 1]`: the last item index with level `<= level` (`-1` = none); `None` outside `1..=160`.
    pub fn index_for_level(&self, level: i32) -> Option<i32> {
        let k = usize::try_from(level.checked_sub(1)?).ok()?;
        self.index_by_level.get(k).copied()
    }

    /// `GenRandomOptItem(level, penalty, prop)`: the stat-option id (0 = none). Only weapons and armor roll.
    /// **KEEP-QUIRK:** the index is read at `level`, not `level - 1`.
    pub fn gen_random_opt_item(
        &self,
        level: i32,
        penalty: f32,
        prop: &ItemProp,
        rng: &mut dyn GameRng,
    ) -> i32 {
        if prop.item_kind1 != IK1_WEAPON && prop.item_kind1 != IK1_ARMOR {
            return 0;
        }
        let level = level.clamp(0, MAX_MONSTER_LEVEL - 1);
        let i = usize::try_from(level)
            .ok()
            .and_then(|k| self.index_by_level.get(k))
            .copied()
            .unwrap_or(-1);
        if i < 0 {
            return 0;
        }
        let pick = rng.random_below(i as u32 + 1) as usize;
        let roll = rng.random_below(100);
        let Some(def) = self.items.get(pick) else {
            return 0;
        };
        let p = (def.prob as f32 * penalty) as u32;
        if roll <= p { def.id } else { 0 }
    }
}

impl Default for RandomOptItemGen {
    fn default() -> Self {
        RandomOptItemGen::new(Vec::new())
    }
}

// ---- seams -------------------------------------------------------------------------------------------------

/// What the drop code needs to know of the dead monster (read once, before any mutation).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MonsterFacts {
    /// The object id.
    pub id: ObjId,
    /// The prop id (`m_dwIndex`).
    pub prop_id: MoverId,
    /// `prop.level`.
    pub level: i32,
    /// `prop.class` (`RANK_*`).
    pub class: u32,
    /// `prop.flying != 0`.
    pub flying: bool,
    /// Where it died.
    pub pos: Vec3,
    /// Its world.
    pub world: WorldId,
    /// Its layer.
    pub layer: LayerId,
}

/// Doc 22's event drops (19 §6.2 step 2) and the event-spawn monster query. Every method defaults to "none".
pub trait EventDropHooks {
    /// `CEventGeneric::GetItem`: the generic (`propEvent.inc`) event items now due, as `(item, count)`
    /// (WP-EVENT-07).
    fn generic_event_items(&mut self) -> Vec<(ItemId, i32)> {
        Vec::new()
    }

    /// `prj.m_EventLua.GetItem(monster level)` (WP-EVENT-05). [`on_monster_died`] additionally asks the real
    /// `Event.lua` family.
    fn lua_event_items(&mut self, _monster_level: u32) -> Vec<(ItemId, i32)> {
        Vec::new()
    }

    /// `prj.m_EventLua.IsEventSpawnMonster(id)`: such a monster is exempt from the level gate.
    fn is_event_spawn_monster(&self, _mover: MoverId) -> bool {
        false
    }
}

/// Doc 20's quest drops and the wormon bookkeeping (19 §6.2 steps 1 and 3). The default does nothing.
pub trait QuestDropHook {
    /// Step 1: the guild / party quest "wormon" of the monster was killed (`quest::on_wormon_killed`; no item
    /// effect here).
    fn on_wormon_killed(
        &mut self,
        _cx: &mut DropCx<'_>,
        _monster: &MonsterFacts,
        _attacker: ObjId,
    ) {
    }

    /// Step 3: `quest::on_monster_drop`, called exactly once per kill, after the event items and before the passes.
    /// Use [`DropCx::create_item`] / [`DropCx::spawn_ground`] to hand the items out.
    fn on_monster_drop(&mut self, _cx: &mut DropCx<'_>, _monster: &MonsterFacts, _attacker: ObjId) {
    }
}

/// The party modes that change the number and the luck of the passes (doc 21 `drop_party_modes`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PartyDropModes {
    /// The gift-box mode: two passes.
    pub giftbox: bool,
    /// The fortune-circle mode: unique mode (halves low `DropKind` rolls).
    pub fortune: bool,
}

/// The remaining seams (default: nothing).
pub trait DropSeams {
    /// Step 4: `social::party::drop_party_modes(a)`.
    fn party_drop_modes(&mut self, _attacker: &Mover) -> PartyDropModes {
        PartyDropModes::default()
    }

    /// `CRandomOptionProperty::GenRandomOption(word, kind, parts)` (ITEM-11): fills the lines of a freshly
    /// initialised word. The default is the real generator over the tables of `randomoption.inc`
    /// (`item::upgrade::random_option::gen_random_option`).
    fn gen_random_option(
        &mut self,
        data: &GameData,
        kind: RandomOptKind,
        parts: u32,
        word: &mut RandomOptWord,
        rng: &mut dyn GameRng,
    ) {
        super::upgrade::random_option::gen_random_option(
            &data.random_option,
            kind,
            parts,
            word,
            false,
            rng,
        );
    }

    /// Step 6: what a looter monster (`MoverAi.loot == 2`) carries (doc 18 owns its inventory).
    fn looter_items(&mut self, _monster: ObjId) -> Vec<ItemRecord> {
        Vec::new()
    }
}

/// No seam installed.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoDropHooks;

impl EventDropHooks for NoDropHooks {}
impl QuestDropHook for NoDropHooks {}
impl DropSeams for NoDropHooks {}

/// The drop service (`Services.drops`): the installed seams and the lazily built stat-option table.
pub struct DropService {
    /// Doc 22's event drops.
    pub events: Box<dyn EventDropHooks + Send>,
    /// Doc 20's quest drops.
    pub quest: Box<dyn QuestDropHook + Send>,
    /// Party modes, awakening generator, looter inventory.
    pub seams: Box<dyn DropSeams + Send>,
    optgen: OnceLock<Arc<RandomOptItemGen>>,
}

impl Default for DropService {
    fn default() -> Self {
        DropService {
            events: Box::new(NoDropHooks),
            quest: Box::new(NoDropHooks),
            seams: Box::new(NoDropHooks),
            optgen: OnceLock::new(),
        }
    }
}

impl std::fmt::Debug for DropService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DropService(..)")
    }
}

impl DropService {
    /// The stat-option generator of `data`, built on first use.
    pub fn generator(&self, data: &GameData) -> &Arc<RandomOptItemGen> {
        self.optgen
            .get_or_init(|| Arc::new(RandomOptItemGen::from_tables(&data.item_etc)))
    }

    /// A service holding `g` as its generator (tests with a hand-made table).
    pub fn with_generator(g: RandomOptItemGen) -> DropService {
        let s = DropService::default();
        let _ = s.optgen.set(Arc::new(g));
        s
    }
}

// ---- the run -----------------------------------------------------------------------------------------------

/// A ground item to spawn in a layer.
#[derive(Clone, Debug)]
pub struct GroundDrop {
    /// The world.
    pub world: WorldId,
    /// The layer.
    pub layer: LayerId,
    /// The item.
    pub drop: ItemDrop,
}

/// What a kill produced: the ground items to spawn (in creation order) and the messages / audit lines of every
/// player that received something.
#[derive(Debug, Default)]
pub struct DropOutcome {
    /// The ground items, in creation order.
    pub ground: Vec<GroundDrop>,
    /// The effects per receiving player (ascending id).
    pub fx: BTreeMap<ObjId, Fx>,
    /// How many items went to a flying monster's killer.
    pub direct_items: u32,
    /// Gold added directly to the killer (flying monsters).
    pub direct_gold: i64,
}

impl DropOutcome {
    /// Spawns the ground items (in order) into the worlds; returns how many the worlds refused.
    pub fn spawn_ground(&mut self, worlds: &mut crate::world::Worlds) -> usize {
        let mut refused = 0;
        for g in self.ground.drain(..) {
            if let Err(e) = worlds.spawn_item(g.world, g.layer, g.drop) {
                tracing::warn!(error = %e, "monster drop: the ground item was refused");
                refused += 1;
            }
        }
        refused
    }
}

/// Everything one kill reads and writes: the object store (the receivers' inventories), the world's random
/// stream, the serial source, the game data and the seams. Built by [`on_monster_died`]; the quest hook gets it
/// to hand items to other players.
pub struct DropCx<'a> {
    /// The game data.
    pub data: &'a GameData,
    /// The item environment (`create_item`).
    pub env: ItemEnv<'a>,
    /// The switches.
    pub cfg: DropConfig,
    /// All objects (movers receive items; the monster and the credited player are read here).
    pub store: &'a mut ObjectStore,
    /// The world's random stream.
    pub rng: &'a mut dyn GameRng,
    /// Fresh serials (best effort: `0` is numbered at pickup).
    pub serials: &'a mut dyn SerialSource,
    /// The game clock (`drop_time`).
    pub now: GameMs,
    /// The stat-option generator.
    pub optgen: &'a RandomOptItemGen,
    /// Doc 22's event drops.
    pub events: &'a mut dyn EventDropHooks,
    /// Doc 20's quest drops (taken out while it runs, so it can borrow the context).
    pub quest: Option<&'a mut dyn QuestDropHook>,
    /// Party modes, awakening, looters.
    pub seams: &'a mut dyn DropSeams,
    /// The result.
    pub out: DropOutcome,
}

impl DropCx<'_> {
    /// The effects collected for `player`.
    pub fn fx(&mut self, player: ObjId) -> &mut Fx {
        self.out.fx.entry(player).or_default()
    }

    /// `pPlayer->CreateItem(item)`: puts `item` into `player`'s inventory (messages and audit lines go to
    /// `fx(player)`). `LackSpace` changes nothing.
    pub fn create_item(
        &mut self,
        player: ObjId,
        item: Item,
        origin: ItemOrigin,
    ) -> Result<(), ItemError> {
        let Some(m) = self.store.mover_mut(player) else {
            return Err(ItemError::NotFound);
        };
        let fx = self.out.fx.entry(player).or_default();
        ops::create_item(m, &self.env, &mut *self.serials, item, origin, fx).map(|_| ())
    }

    /// Queues a ground item for the layer `(world, layer)`.
    pub fn spawn_ground(
        &mut self,
        world: WorldId,
        layer: LayerId,
        pos: Vec3,
        angle: f32,
        item: ItemRecord,
        meta: GroundItemMeta,
    ) {
        self.out.ground.push(GroundDrop {
            world,
            layer,
            drop: ItemDrop::new(item, pos, angle, meta),
        });
    }

    fn next_serial(&mut self) -> u64 {
        self.serials.next_serial().unwrap_or(0)
    }

    /// `xRandomF(2.0f) - 1.0f` on x and z.
    fn spread(&mut self, base: Vec3) -> Vec3 {
        let x = base.x + (self.rng.random_f32(2.0) - 1.0);
        let z = base.z + (self.rng.random_f32(2.0) - 1.0);
        Vec3::new(x, base.y, z)
    }
}

/// Counters of one kill (tests and the debug log).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DropReport {
    /// The passes made.
    pub passes: i32,
    /// Ground items queued.
    pub ground_items: usize,
    /// Items put into the killer's inventory.
    pub direct_items: u32,
    /// Gold added to the killer's wallet directly.
    pub direct_gold: i64,
}

fn monster_facts(store: &ObjectStore, data: &GameData, id: ObjId) -> Option<MonsterFacts> {
    let m = store.mover(id)?;
    if m.is_player() {
        return None;
    }
    let prop = data.mover(m.core.index)?;
    Some(MonsterFacts {
        id,
        prop_id: prop.id,
        level: i32::try_from(prop.level).unwrap_or(i32::MAX),
        class: prop.class,
        flying: prop.flying != 0,
        pos: m.core.pos,
        world: m.core.world,
        layer: m.core.layer,
    })
}

/// Who may take a drop first: the killer, except for `RANK_SUPER` monsters and the event movers
/// (`bJJim == FALSE`).
fn owner_of(mf: &MonsterFacts, attacker: ObjId, event: bool) -> Option<ObjId> {
    let public =
        mf.class == RANK_SUPER || matches!(mf.prop_id, MI_DEMIAN5 | MI_KEAKOON5 | MI_MUFFRIN5);
    (!public || event).then_some(attacker)
}

/// `CMover::DropItem(CMover* pAttacker)` (19 §6.2): generates the drop of the monster `victim` for the credited
/// player `credited` into `cx`. Nothing happens when `credited` is not a player or `victim` is not a monster
/// with a prop.
pub fn generate(
    cx: &mut DropCx<'_>,
    factors: DropFactors,
    victim: ObjId,
    credited: ObjId,
) -> DropReport {
    let mut report = DropReport::default();
    let Some(mf) = monster_facts(&*cx.store, cx.data, victim) else {
        return report;
    };
    let Some(prop) = cx.data.mover(mf.prop_id) else {
        return report;
    };
    let attacker_level = match cx.store.mover(credited) {
        Some(a) if a.is_player() => a.mv.level,
        _ => return report,
    };
    // step 1
    if let Some(q) = cx.quest.take() {
        q.on_wormon_killed(cx, &mf, credited);
        cx.quest = Some(q);
    }
    // step 2
    if attacker_level.wrapping_sub(mf.level) < 10 {
        event_items(cx, &mf, credited, &mut report);
    }
    // step 3
    if let Some(q) = cx.quest.take() {
        q.on_monster_drop(cx, &mf, credited);
        cx.quest = Some(q);
    }
    // step 4
    let event = event_monster(cx, &mf);
    let passes = passes_of(cx, event.as_ref(), credited);
    // step 5
    let drops = &prop.ex.drops;
    let order = index_order(cx, drops.items.len());
    for k in 0..passes.count {
        report.passes += 1;
        let exempt = mf.prop_id == MI_CLOCKWORK1
            || (cx.cfg.better_clockwork_alpha && mf.prop_id == MI_LOADCLOCKWORK)
            || matches!(mf.prop_id, MI_DEMIAN5 | MI_KEAKOON5 | MI_MUFFRIN5)
            || event.is_some()
            || cx.events.is_event_spawn_monster(mf.prop_id);
        let (prob_pct, penya_pct) = if exempt {
            (100, 100)
        } else {
            level_gate(attacker_level, mf.level)
        };
        let item_rate = prob_pct as f32 * factors.item;
        if (cx.rng.random_below(100) as f32) >= item_rate {
            continue; // nothing at all this pass, gold included
        }
        table_pass(
            cx,
            &mf,
            prop,
            credited,
            &factors,
            &order,
            (k, prob_pct, penya_pct),
            event.as_ref(),
            &mut report,
        );
        kind_pass(
            cx,
            &mf,
            prop,
            credited,
            prob_pct,
            passes.unique,
            event.as_ref(),
            &mut report,
        );
    }
    // step 6
    let carried = cx.seams.looter_items(victim);
    for item in carried {
        let pos = cx.spread(mf.pos);
        let angle = cx.rng.random_below(360) as f32;
        let meta = GroundItemMeta {
            owner: Some(credited),
            holder: None,
            drop_time: cx.now,
            from_monster: false,
            event_monster: None,
            gold: None,
            respawned: false,
        };
        cx.spawn_ground(mf.world, mf.layer, pos, angle, item, meta);
        report.ground_items += 1;
    }
    report
}

/// The registered event monster of `mf` (`CEventMonster::SetEventMonster`): its drop range, loot rules.
#[derive(Clone, Copy, Debug)]
struct EventMonster {
    id: MoverId,
    range: f32,
    gift_box: bool,
}

fn event_monster(cx: &DropCx<'_>, mf: &MonsterFacts) -> Option<EventMonster> {
    // the single lookup of the event monsters (the first entry of EventMonster.lua wins; WP-ITEM-25)
    crate::event::monsters::find(&cx.data.lua.event_monsters, mf.prop_id).map(|e| EventMonster {
        id: e.mover,
        range: e.item_drop_range,
        gift_box: e.gift_box,
    })
}

/// Step 2: the generic and Lua event items.
fn event_items(cx: &mut DropCx<'_>, mf: &MonsterFacts, a: ObjId, report: &mut DropReport) {
    let mut items = cx.events.generic_event_items();
    items.extend(
        cx.events
            .lua_event_items(u32::try_from(mf.level).unwrap_or(0)),
    );
    for (id, n) in items {
        let count = u16::try_from(n.clamp(1, i32::from(u16::MAX))).unwrap_or(1);
        if mf.flying {
            let mut item = Item::new(id, count);
            item.serial = flyff_common::ItemSerial(cx.next_serial());
            give_or_drop(cx, mf, a, item, None, report);
        } else {
            let mut item = Item::new(id, count);
            item.endurance = -1;
            item.serial = flyff_common::ItemSerial(cx.next_serial());
            let pos = cx.spread(mf.pos);
            let meta = GroundItemMeta {
                owner: Some(a),
                holder: None,
                drop_time: cx.now,
                from_monster: true,
                event_monster: None,
                gold: None,
                respawned: false,
            };
            cx.spawn_ground(mf.world, mf.layer, pos, 0.0, item.to_record(0), meta);
            report.ground_items += 1;
        }
    }
}

/// A flying monster's item for the killer: into the inventory (`TID_GAME_REAPITEM`), or, when it does not fit and
/// PL-071 is on, onto the ground at the monster. Returns `true` when the item went anywhere.
fn give_or_drop(
    cx: &mut DropCx<'_>,
    mf: &MonsterFacts,
    a: ObjId,
    item: Item,
    event: Option<&EventMonster>,
    report: &mut DropReport,
) -> bool {
    let name = cx
        .data
        .item(item.item_id)
        .map(|p| format!("\"{}\"", p.name));
    match cx.create_item(a, item.clone(), ItemOrigin::Drop) {
        Ok(()) => {
            if let Some(n) = name {
                cx.fx(a).defined(TID_GAME_REAPITEM, vec![n]);
            }
            report.direct_items += 1;
            cx.out.direct_items += 1;
            true
        }
        Err(ItemError::LackSpace) if cx.cfg.flying_overflow_to_ground => {
            let pos = ground_position(cx, mf, event);
            let meta = GroundItemMeta {
                owner: owner_of(mf, a, event.is_some()),
                holder: None,
                drop_time: cx.now,
                from_monster: true,
                event_monster: event.map(|e| e.id),
                gold: None,
                respawned: false,
            };
            let mut rec = item.to_record(0);
            if rec.serial == 0 {
                rec.serial = cx.next_serial();
            }
            cx.spawn_ground(mf.world, mf.layer, pos, 0.0, rec, meta);
            report.ground_items += 1;
            true
        }
        Err(_) => false,
    }
}

/// The spread of a table drop: `+-1 m` (two draws), and for an event monster two more draws within
/// `+-range / 2` that replace it (the original draws both).
fn ground_position(cx: &mut DropCx<'_>, mf: &MonsterFacts, event: Option<&EventMonster>) -> Vec3 {
    let default = cx.spread(mf.pos);
    let Some(e) = event else {
        return default;
    };
    let half = e.range / 2.0;
    let x = mf.pos.x + (cx.rng.random_f32(e.range) - half);
    let z = mf.pos.z + (cx.rng.random_f32(e.range) - half);
    Vec3::new(x, mf.pos.y, z)
}

struct Passes {
    count: i32,
    unique: bool,
}

/// Step 4: the number of passes.
fn passes_of(cx: &mut DropCx<'_>, event: Option<&EventMonster>, a: ObjId) -> Passes {
    let mut count = 1_i32;
    let mut unique = false;
    let (giftbox, fortune, buff1, buff2, dst) = match cx.store.mover(a) {
        Some(m) => {
            // doc 21 `drop_party_modes`: the killer's own party (WP-ITEM-23), or what a test installs
            let seam = cx.seams.party_drop_modes(m);
            let (g, f) = crate::social::party::skills::drop_party_modes_in(cx.store, a);
            (
                seam.giftbox || g,
                seam.fortune || f,
                m.buffs.has(BUFF_ITEM, II_SYS_SYS_SCR_GET01),
                m.buffs.has(BUFF_ITEM, II_SYS_SYS_SCR_GET02),
                m.mv.dst.table.adj(DST_GIFTBOX),
            )
        }
        None => (false, false, false, false, 0),
    };
    if giftbox {
        count = 2;
    }
    if fortune {
        unique = true;
    }
    if buff1 {
        count += 1;
    }
    if buff2 {
        count += 2;
    }
    count = count.saturating_add(dst);
    // an event monster without gift-box rights always makes one pass
    if event.is_some_and(|e| !e.gift_box) {
        count = 1;
    }
    Passes {
        count: count.clamp(0, MAX_PASSES),
        unique,
    }
}

/// `random_shuffle` of the index order (`__SHUFFLE_DROP`), built once per kill: Fisher-Yates on the world stream.
fn index_order(cx: &mut DropCx<'_>, n: usize) -> Vec<usize> {
    let mut v: Vec<usize> = (0..n).collect();
    if cx.cfg.shuffle_drop {
        for i in (1..v.len()).rev() {
            let j = cx.rng.random_below(i as u32 + 1) as usize;
            v.swap(i, j);
        }
    }
    v
}

/// `CDropItemGenerator::GetAt`: does table entry `e` hit? With `piece > 0` one roll is drawn, divided by the
/// piece factor and compared with the probability; seed rows always hit; with `piece <= 0` nothing is drawn
/// and every entry hits.
pub fn entry_hits(rng: &mut dyn GameRng, e: &DropItem, piece: f32) -> bool {
    if piece > 0.0 {
        let r = roll_3e9(rng);
        let r = (r as f32 / piece) as u32;
        e.dtype == DropType::Seed || r < e.probability
    } else {
        true
    }
}

/// Step 5.3: the item and gold entries of pass `pass.0` (`pass = (k, prob_pct, penya_pct)`).
#[allow(clippy::too_many_arguments)]
fn table_pass(
    cx: &mut DropCx<'_>,
    mf: &MonsterFacts,
    prop: &MoverProp,
    a: ObjId,
    factors: &DropFactors,
    order: &[usize],
    pass: (i32, i32, i32),
    event: Option<&EventMonster>,
    report: &mut DropReport,
) {
    let (k, prob_pct, penya_pct) = pass;
    let table = &prop.ex.drops;
    let mut number: u32 = 0;
    for &idx in order {
        let Some(e) = table.items.get(idx) else {
            continue;
        };
        if !entry_hits(cx.rng, e, factors.piece) {
            continue;
        }
        match e.dtype {
            DropType::Normal => {
                let num = if e.number == NULL_NUMBER { 1 } else { e.number };
                let count = u16::try_from(cx.rng.random_below(num).saturating_add(1)).unwrap_or(1);
                let Some(iprop) = cx.data.item(e.item) else {
                    continue;
                };
                if mf.flying {
                    let mut item = Item::new(e.item, count);
                    item.upgrade_level = i32::try_from(e.level).unwrap_or(i32::MAX);
                    if give_or_drop(cx, mf, a, item.clone(), event, report) {
                        unique_log(cx, a, mf, iprop, &item);
                        if e.number != NULL_NUMBER {
                            number += 1;
                        }
                        if number >= table.max_item {
                            break;
                        }
                    }
                    continue;
                }
                let mut item = Item::new(e.item, count);
                item.endurance = i32::try_from(iprop.endurance).unwrap_or(i32::MAX);
                let stat =
                    cx.optgen
                        .gen_random_opt_item(mf.level, prob_pct as f32 / 100.0, iprop, cx.rng);
                item.random_option.set_stat_id(stat as u8);
                item.upgrade_level = i32::try_from(e.level).unwrap_or(i32::MAX);
                item.serial = flyff_common::ItemSerial(cx.next_serial());
                if cx.cfg.drop_awaked_items
                    && let Some(kind) = random_option_kind(
                        iprop.parts,
                        iprop.item_kind3,
                        false,
                        cx.cfg.only_armor_awakes,
                    )
                {
                    item.random_option.initialize();
                    let mut word = item.random_option;
                    cx.seams
                        .gen_random_option(cx.data, kind, iprop.parts, &mut word, cx.rng);
                    item.random_option = word;
                }
                let pos = ground_position(cx, mf, event);
                let meta = GroundItemMeta {
                    owner: owner_of(mf, a, event.is_some()),
                    holder: None,
                    drop_time: cx.now,
                    from_monster: true,
                    event_monster: event.map(|e| e.id),
                    gold: None,
                    respawned: false,
                };
                unique_log(cx, a, mf, iprop, &item);
                cx.spawn_ground(mf.world, mf.layer, pos, 0.0, item.to_record(0), meta);
                report.ground_items += 1;
                if e.number != NULL_NUMBER {
                    number += 1;
                }
                if number == table.max_item {
                    break;
                }
            }
            DropType::Seed if k == 0 => seed_drop(cx, mf, a, e, factors, penya_pct, event, report),
            DropType::Seed => {}
        }
    }
}

/// A gold row: `number + random_below(number2 - number)`, the penya share, the rates, and then straight into
/// the killer's wallet (flying monsters) or a gold seed on the ground.
#[allow(clippy::too_many_arguments)]
fn seed_drop(
    cx: &mut DropCx<'_>,
    mf: &MonsterFacts,
    a: ObjId,
    e: &DropItem,
    factors: &DropFactors,
    penya_pct: i32,
    event: Option<&EventMonster>,
    report: &mut DropReport,
) {
    let span = e.number2.wrapping_sub(e.number);
    let mut gold = e.number.wrapping_add(cx.rng.random_below(span)) as i32;
    gold = gold.wrapping_mul(penya_pct) / 100;
    gold = ((gold as f32 * factors.gold) * factors.penya) as i32;
    if gold == 0 {
        return;
    }
    gold = (gold as f32 * factors.gold_event) as i32;
    if mf.flying {
        let wallet = cx.store.mover(a).map_or(0, gold_core::gold);
        if gold::can_add(wallet, i64::from(gold))
            && let Some(m) = cx.store.mover_mut(a)
        {
            let fx = cx.out.fx.entry(a).or_default();
            gold_core::add_gold(m, &cx.env, i64::from(gold), true, fx);
            gold_core::gold_text(m, &cx.env, &mut *cx.serials, i64::from(gold), fx);
            report.direct_gold += i64::from(gold);
            cx.out.direct_gold += i64::from(gold);
        }
        return;
    }
    let seed_of = |id: u32| cx.data.item(id).map_or(0, |p| i64::from(p.ability_max));
    let amount = i64::from(gold);
    let seed = if amount <= seed_of(II_GOLD_SEED1) {
        II_GOLD_SEED1
    } else if amount <= seed_of(II_GOLD_SEED2) {
        II_GOLD_SEED2
    } else if amount <= seed_of(II_GOLD_SEED3) {
        II_GOLD_SEED3
    } else {
        II_GOLD_SEED4
    };
    let pos = ground_position(cx, mf, event);
    let meta = GroundItemMeta {
        owner: owner_of(mf, a, event.is_some()),
        holder: None,
        drop_time: cx.now,
        from_monster: true,
        event_monster: event.map(|e| e.id),
        gold: None,
        respawned: false,
    };
    let d = ItemDrop::gold_seed(seed, gold, pos, 0.0, meta);
    cx.out.ground.push(GroundDrop {
        world: mf.world,
        layer: mf.layer,
        drop: d,
    });
    report.ground_items += 1;
}

/// The `item.unique` audit line of a unique drop (weapons, armor and jewelry whose prop has a `log` class).
fn unique_log(cx: &mut DropCx<'_>, a: ObjId, mf: &MonsterFacts, iprop: &ItemProp, item: &Item) {
    let logged = iprop.item_kind1 == IK1_WEAPON
        || iprop.item_kind1 == IK1_ARMOR
        || (iprop.item_kind1 == IK1_GENERAL && iprop.item_kind2 == IK2_JEWELRY);
    if !logged {
        return;
    }
    let ao = match iprop.log {
        1 => item.upgrade_level,
        2 => 200,
        3 => 100,
        _ => return,
    };
    cx.fx(a).audit(
        ev::ITEM_UNIQUE,
        json!({ "item": item_json(item), "ao": ao, "monster": mf.prop_id }),
    );
}

/// Step 5.4: the `DropKind` entries of one pass (19 §6.3).
#[allow(clippy::too_many_arguments)]
fn kind_pass(
    cx: &mut DropCx<'_>,
    mf: &MonsterFacts,
    prop: &MoverProp,
    a: ObjId,
    prob_pct: i32,
    unique: bool,
    event: Option<&EventMonster>,
    report: &mut DropReport,
) {
    for kind in &prop.ex.drops.kinds {
        let (lo, hi) = (i32::from(kind.min_uniq), i32::from(kind.max_uniq));
        let rare = |j: i32| u32::try_from(j).unwrap_or(u32::MAX);
        let min_idx = (lo..=hi)
            .map(|j| cx.data.item_index.min_idx(kind.ik3, rare(j)))
            .find(|&i| i != -1)
            .unwrap_or(-1);
        let max_idx = (lo..=hi)
            .rev()
            .map(|j| cx.data.item_index.max_idx(kind.ik3, rare(j)))
            .find(|&i| i != -1)
            .unwrap_or(-1);
        if min_idx < 0 || max_idx < 0 {
            continue;
        }
        let span = (max_idx - min_idx + 1).max(0) as u32;
        let pick = (min_idx as u32).wrapping_add(cx.rng.random_below(span)) as usize;
        let Some(item_id) = cx
            .data
            .item_index
            .by_kind3
            .get(kind.ik3 as usize)
            .and_then(|l| l.get(pick))
            .copied()
        else {
            continue;
        };
        let Some(iprop) = cx.data.item(item_id) else {
            continue;
        };
        let ability = cx.rng.random_below(11) as i32;
        if iprop.item_lv == 0 {
            continue; // FIX: the original read expDropLuck[-1]
        }
        let row = if iprop.item_lv > 120 {
            119
        } else {
            iprop.item_lv as usize - 1
        };
        let mut dropped = false;
        for k in (0..=ability).rev() {
            let luck = cx.data.exp.drop_luck(row, k as usize);
            let p = (luck as f32 * (prop.correction_value as f32 / 100.0)) as u32;
            let mut r = roll_3e9(cx.rng);
            if unique && p <= 10_000_000 {
                r /= 2;
            }
            if r >= p {
                continue;
            }
            let stat =
                cx.optgen
                    .gen_random_opt_item(mf.level, prob_pct as f32 / 100.0, iprop, cx.rng);
            let mut item = Item::new(item_id, 1);
            item.upgrade_level = k;
            item.random_option.set_stat_id(stat as u8);
            if mf.flying {
                // the item is created without a serial: the container numbers it
                if cx.create_item(a, item.clone(), ItemOrigin::Drop).is_ok() {
                    let name = format!("\"{}\"", iprop.name);
                    cx.fx(a).defined(TID_GAME_REAPITEM, vec![name]);
                    unique_log(cx, a, mf, iprop, &item);
                    report.direct_items += 1;
                    cx.out.direct_items += 1;
                    break;
                }
            }
            item.endurance = i32::try_from(iprop.endurance).unwrap_or(i32::MAX);
            item.serial = flyff_common::ItemSerial(cx.next_serial());
            let meta = GroundItemMeta {
                owner: owner_of(mf, a, event.is_some()),
                holder: None,
                drop_time: cx.now,
                from_monster: true,
                event_monster: event.map(|e| e.id),
                gold: None,
                respawned: false,
            };
            unique_log(cx, a, mf, iprop, &item);
            cx.spawn_ground(mf.world, mf.layer, mf.pos, 0.0, item.to_record(0), meta);
            report.ground_items += 1;
            dropped = true;
            break;
        }
        if mf.class == RANK_SUPER && dropped {
            break;
        }
    }
}

// ---- the entry points --------------------------------------------------------------------------------------

/// `drop::on_monster_died` (19 §6.2): the drop of the dead monster `victim` for the credited player `credited`,
/// in a service context. Builds the context, runs [`generate`], spawns the ground items in their layer and
/// flushes the messages and audit lines of every player that received something.
pub fn on_monster_died(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    victim: ObjId,
    credited: ObjId,
) -> DropReport {
    let Some(world) = ctx.worlds.core(victim).map(|c| c.world) else {
        return DropReport::default();
    };
    let core: Arc<ItemCore> = svc.items.handle();
    let env = core.env(ctx.clock.unix_ms.to_unix_time());
    let cfg = DropConfig::from_rules(&ctx.cfg.rules);
    let now = ctx.clock.game_ms;
    let factors = {
        let (Some(v), Some(a)) = (ctx.worlds.mover(victim), ctx.worlds.mover(credited)) else {
            return DropReport::default();
        };
        DropFactors::compute(
            &ctx.live.rates,
            svc.event.factors(),
            v,
            a,
            cfg.better_couplebuffs,
        )
    };
    // doc 22 §6.3 (task event-propevent): the generic event items draw from a copy of the `event` stream, written
    // back after the block (the drop context holds the world stream)
    let hour = ctx.clock.local.hour;
    let mut ev = ctx.rng.event().clone();
    let (report, mut outcome) = {
        let Services {
            drops,
            serials,
            event,
            ..
        } = svc;
        let DropService {
            events,
            quest,
            seams,
            optgen,
        } = drops;
        let optgen =
            optgen.get_or_init(|| Arc::new(RandomOptItemGen::from_tables(&ctx.data.item_etc)));
        let mut live = crate::event::drops::EventDrops::new(
            &mut **events,
            &*ctx.lua,
            &event.lua_events,
            ctx.data,
            now,
        )
        .with_spawns(&event.spawns)
        .with_generic(crate::event::drops::GenericDrops {
            service: &mut event.generic,
            flags: &event.flags,
            hour,
            rng: &mut ev,
        });
        let mut serials = super::ops::CtxSerials::new(serials, ctx.persist, ctx.tickets);
        let mut cx = DropCx {
            data: ctx.data,
            env,
            cfg,
            store: ctx.worlds.store_mut(),
            rng: ctx.rng.world(world),
            serials: &mut serials,
            now,
            optgen,
            events: &mut live,
            quest: Some(&mut **quest),
            seams: &mut **seams,
            out: DropOutcome::default(),
        };
        let report = generate(&mut cx, factors, victim, credited);
        (report, cx.out)
    };
    *ctx.rng.event() = ev;
    let refused = outcome.spawn_ground(ctx.worlds);
    if refused > 0 {
        tracing::warn!(refused, "monster drop: ground items were refused");
    }
    for (id, fx) in std::mem::take(&mut outcome.fx) {
        fx.flush(svc, ctx, PlayerRef(id));
    }
    report
}

/// The `DeathHooks::drop` entry of the death pipeline (`OnDied` step 11): the loot belongs to the enemy with the
/// most damage while it is still valid, else to the killer ([`crate::combat::death::loot_owner`]).
pub fn drop_hook(svc: &mut Services, ctx: &mut GameCtx<'_>, k: &KillCtx) {
    let owner = crate::combat::death::loot_owner(ctx, k);
    on_monster_died(svc, ctx, k.victim, owner);
}
