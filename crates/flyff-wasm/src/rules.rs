//! Area 3 of the module: the rules exports (Plan/16 §18.4, WP-MOVER-13; D7 area 3).
//!
//! [`RulesContext`] is the one stateful export of this area. It holds (a) immutable data decoded once from the
//! JSON bundles (the rule tables, the item and skill props) and (b) a **DST replica** per mover the client knows.
//! Everything else in the browser stays plain functions of its arguments; the replica cache is the one deviation
//! from D7's "pure functions taking plain data" (needs an ADR, 16 §21 Q16), so its contract is part of the type:
//!
//! **Contract of the replica cache.** The replicas are a *cache of server-sent state*, not game state.
//! TypeScript owns the lifecycle of the entities: it passes a mover's spawn snapshot when the mover spawns, its
//! `SetDestParam` / `ResetDestParam` / `ResetDestParamSync` messages as ops, and the id in `forgets` when the mover
//! despawns; it calls `dst_reset_all` on a session re-attach and on a world change (before the new spawns arrive),
//! after which the cache is rebuilt only from spawn snapshots and later deltas. A snapshot is **loaded**
//! (assigned, `DstTable::replica_load`), never applied as a delta, so a root counter above 1 survives; ops are the
//! legacy deltas (`replica_set` / `replica_reset` / `replica_reset_sync`). No rule reads anything TypeScript did
//! not feed; every export is a pure function of (replica, arguments). An object without a replica is evaluated on
//! the default (all zero) table.
//!
//! **Budget** (Plan/40 §7.3, §7.10): per network drain exactly one [`RulesContext::drain_sync`] call; the
//! per-object path (`dst_snapshot`, `dst_apply_ops`, `dst_forget`, `mover_params`) is for tools and the fallback;
//! `dst_view` is debug-only and never on the drain path.
//!
//! The Rust core ([`RulesCore`]) has no `JsValue`, so it is tested natively; the `#[wasm_bindgen]` shell only
//! converts. Every function is panic-free: malformed arrays are `Err` (a `JsError` in the browser) and change
//! nothing. Exports that read a record array validate the whole array before they apply anything.
//!
//! Integer slots returned as `f32` by `drain_sync` are exact below 2^24 (the CHS bits stop at 0x0020_0000); callers
//! that need a wider range use `mover_params`, which returns `i32`.

use flyff_common::defs::attribute::*;
use flyff_common::defs::item::II_WEA_HAN_HAND;
use flyff_common::defs::itemkind::{IK1_WEAPON, IK2_ARMOR, IK2_ARMORETC, IK3_BOW};
use flyff_common::defs::neuz::{PARTS_LWEAPON, PARTS_RWEAPON};
use flyff_data::bundle::types::{Envelope, ItemExtBundle, ItemsBundle, SkillsBundle};
use flyff_data::props::item::ItemProp;
use flyff_data::props::mover::MoverProp;
use flyff_data::types::IdTable;
use flyff_rules::RulesConfig;
use flyff_rules::consts::{MAX_SKILL_JOB, NULL_CHGPARAM};
use flyff_rules::data::{RulesData, RulesError};
use flyff_rules::dst::DstTable;
use flyff_rules::equip_rules::{Wearer, check_equip};
use flyff_rules::item::random_option::RandomOptWord;
use flyff_rules::jobs::{EMPTY_SKILLS, JobSkill, SkillTable};
use flyff_rules::progression::exp_percent_x100;
use flyff_rules::skill::plan::validate_skill_plan;
use flyff_rules::stats::{
    self, BaseStats, DerivedStats, EquipCache, MonsterRates, StatInputs, Statistics, WeaponView,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use wasm_bindgen::prelude::{JsError, JsValue, wasm_bindgen};

mod attack;
mod gems;
mod items;
mod upgrade;

pub use attack::{AttackCtxJs, AttackViewJs};
pub use gems::{ANY_KIND, ultimate_sockets};
pub use items::{
    HairCostInputJs, ItemTooltipInputJs, ItemTooltipNumbersJs, PriceInputJs, TradeableInputJs,
};
pub use upgrade::{ConstantsJs, OddsInputJs, OddsJs, RouteInputJs, RouteJs};

/// The most bytes of one bundle the context accepts (the real `items` bundle is 5.4 MB).
pub const MAX_BUNDLE_BYTES: usize = 64 << 20;
/// `f32` words per `drain_sync` result row.
pub const DRAIN_ROW_WORDS: usize = 7;
/// `i32` words per `mover_params` result row.
pub const PARAMS_ROW_WORDS: usize = 8;
/// What `equip_check` answers for a refusal without a message (an arrow without a bow).
pub const EQUIP_SILENT_REFUSAL: u32 = u32::MAX;

fn err(msg: impl Into<String>) -> RulesError {
    RulesError(msg.into())
}

// ---- configuration and inputs from JavaScript ---------------------------------------------------------------

/// The `cfg` argument of the constructor: the `RulesConfig` switches (camelCase). Missing fields (and `undefined`
/// / `null` as a whole) take the repack defaults of Plan/03 §4.
#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RulesCfgJs {
    /// `features.bowdmg_propjob`.
    pub bowdmg_propjob: Option<bool>,
    /// `features.wand_autoattacks`.
    pub wand_autoattacks: Option<bool>,
    /// `features.better_armor_bonus`.
    pub better_armor_bonus: Option<bool>,
    /// `features.auto_skill_max`.
    pub auto_skill_max: Option<bool>,
    /// `features.fix_get_hp_mp_fp_point`.
    pub fix_get_hp_mp_fp_point: Option<bool>,
    /// `features.remove_max_recover_rate`.
    pub remove_max_recover_rate: Option<bool>,
    /// `features.heal_over_time`.
    pub heal_over_time: Option<bool>,
    /// `gameplay.set_max_speed`.
    pub set_max_speed: Option<i32>,
    /// `gameplay.set_max_cast`.
    pub set_max_cast: Option<i32>,
    /// `gameplay.set_max_crit`.
    pub set_max_crit: Option<i32>,
    /// `gameplay.max_block_value`.
    pub max_block_value: Option<i32>,
    /// `gameplay.max_block_value_gw`.
    pub max_block_value_gw: Option<i32>,
    /// `features.only_armor_awakes` (default true): which items carry a random-option kind in the tooltip
    /// numbers (WP-MOVER-22). Not part of `RulesConfig`.
    pub only_armor_awakes: Option<bool>,
}

impl RulesCfgJs {
    /// The rules configuration with the repack defaults for every missing field.
    pub fn into_config(self) -> RulesConfig {
        let d = RulesConfig::default();
        RulesConfig {
            bowdmg_propjob: self.bowdmg_propjob.unwrap_or(d.bowdmg_propjob),
            wand_autoattacks: self.wand_autoattacks.unwrap_or(d.wand_autoattacks),
            better_armor_bonus: self.better_armor_bonus.unwrap_or(d.better_armor_bonus),
            auto_skill_max: self.auto_skill_max.unwrap_or(d.auto_skill_max),
            fix_get_hp_mp_fp_point: self
                .fix_get_hp_mp_fp_point
                .unwrap_or(d.fix_get_hp_mp_fp_point),
            remove_max_recover_rate: self
                .remove_max_recover_rate
                .unwrap_or(d.remove_max_recover_rate),
            heal_over_time: self.heal_over_time.unwrap_or(d.heal_over_time),
            set_max_speed: self.set_max_speed.unwrap_or(d.set_max_speed),
            set_max_cast: self.set_max_cast.unwrap_or(d.set_max_cast),
            set_max_crit: self.set_max_crit.unwrap_or(d.set_max_crit),
            max_block_value: self.max_block_value.unwrap_or(d.max_block_value),
            max_block_value_gw: self.max_block_value_gw.unwrap_or(d.max_block_value_gw),
        }
    }
}

/// One worn item of [`StatInputJs`].
#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EquipJs {
    /// `PARTS_*` (0..31).
    pub part: u32,
    /// The item id; an id the item table does not have is ignored.
    pub item_id: u32,
    /// `GetAbilityOption()`: the upgrade level.
    pub ability_option: i32,
    /// `CItemElem::expired`: contributes nothing.
    pub expired: bool,
}

/// The character-window input (Plan/16 §18.4 `StatInputJs`); the DST part is the replica of the object.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct StatInputJs {
    /// `IsPlayer()` (an NPC is evaluated on a neutral mover prop: only players are shown in the windows of M3).
    pub is_player: bool,
    /// `GetLevel()`.
    pub level: i32,
    /// `GetJob()`, `-1` for none.
    pub job: i32,
    /// `[str, sta, dex, int]`.
    pub base: [i32; 4],
    /// The worn items. Random options, pierced cards and sets reach the numbers through the DST replica; only the
    /// cached defense, hit rate and parry (`update_param`) are derived from this list.
    pub equip: Vec<EquipJs>,
    /// The part of the active weapon (`PARTS_RWEAPON` by default).
    pub weapon_part: u32,
}

impl Default for StatInputJs {
    fn default() -> Self {
        StatInputJs {
            is_player: true,
            level: 1,
            job: 0,
            base: [15; 4],
            equip: Vec::new(),
            weapon_part: PARTS_RWEAPON,
        }
    }
}

/// [`Statistics`] for JavaScript (camelCase).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatisticsJs {
    /// `GetSpeedRun`.
    pub speed_run: i32,
    /// `GetCriticalRate`.
    pub critical_rate: i32,
    /// `GetSpellCastSpeed`.
    pub spell_cast_speed: i32,
    /// `GetHitRate`.
    pub hit_rate: i32,
    /// `GetBlockMelee`.
    pub block_melee: i32,
    /// `GetBlockRange`.
    pub block_range: i32,
    /// `GetPvMDmg`.
    pub pvm_dmg: i32,
    /// `GetPvPDmg`.
    pub pvp_dmg: i32,
}

impl From<Statistics> for StatisticsJs {
    fn from(s: Statistics) -> Self {
        StatisticsJs {
            speed_run: s.speed_run,
            critical_rate: s.critical_rate,
            spell_cast_speed: s.spell_cast_speed,
            hit_rate: s.hit_rate,
            block_melee: s.block_melee,
            block_range: s.block_range,
            pvm_dmg: s.pvm_dmg,
            pvp_dmg: s.pvp_dmg,
        }
    }
}

/// [`DerivedStats`] for JavaScript (camelCase): every number of the character window.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DerivedStatsJs {
    /// `GetStr()`.
    pub str: i32,
    /// `GetSta()`.
    pub sta: i32,
    /// `GetDex()`.
    pub dex: i32,
    /// `GetInt()`.
    pub int: i32,
    /// `GetMaxHitPoint()`.
    pub max_hp: i32,
    /// `GetMaxManaPoint()`.
    pub max_mp: i32,
    /// `GetMaxFatiguePoint()`.
    pub max_fp: i32,
    /// `GetHPRecovery()`.
    pub hp_recovery: i32,
    /// `GetMPRecovery()`.
    pub mp_recovery: i32,
    /// `GetFPRecovery()`.
    pub fp_recovery: i32,
    /// Minimum attack of the active hand.
    pub attack_min: i32,
    /// Maximum attack of the active hand.
    pub attack_max: i32,
    /// `CalcDefense(AF_GENERIC)` without the Event.lua term.
    pub defense_shown: i32,
    /// `GetHR()`.
    pub hit_rating: i32,
    /// `GetParrying()`.
    pub parry: i32,
    /// `GetCriticalProb()` without the party bonus.
    pub critical_prob: i32,
    /// `GetAttackSpeed()`.
    pub attack_speed: f32,
    /// `GetCastingAniSpeed()`.
    pub casting_ani_speed: f32,
    /// `GetSpeed(base)` for the player's base speed.
    pub move_speed: f32,
    /// The `__STATISTICS` getters.
    pub statistics: StatisticsJs,
}

impl From<DerivedStats> for DerivedStatsJs {
    fn from(d: DerivedStats) -> Self {
        DerivedStatsJs {
            str: d.str,
            sta: d.sta,
            dex: d.dex,
            int: d.int,
            max_hp: d.max_hp,
            max_mp: d.max_mp,
            max_fp: d.max_fp,
            hp_recovery: d.hp_recovery,
            mp_recovery: d.mp_recovery,
            fp_recovery: d.fp_recovery,
            attack_min: d.attack_min,
            attack_max: d.attack_max,
            defense_shown: d.defense_shown,
            hit_rating: d.hit_rating,
            parry: d.parry,
            critical_prob: d.critical_prob,
            attack_speed: d.attack_speed,
            casting_ani_speed: d.casting_ani_speed,
            move_speed: d.move_speed,
            statistics: d.statistics.into(),
        }
    }
}

// ---- record arrays ------------------------------------------------------------------------------------------

/// One spawn snapshot: the object and its `(dst, adj, chg)` triples.
#[derive(Clone, Copy, Debug)]
struct Snapshot<'a> {
    obj: u32,
    triples: &'a [i32],
}

/// One DST op `[obj, kind, dst, a, b]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Op {
    obj: u32,
    kind: OpKind,
    dst: u32,
    a: i32,
    b: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OpKind {
    /// `SetDestParam{dst, adj = a, chg = b}`.
    Set,
    /// `ResetDestParam{dst, adj = a}`.
    Reset,
    /// `ResetDestParamSync{dst, adj = a, total = b}`.
    Sync,
}

/// One `drain_sync` param row `[obj, job, level, base_dex, weapon_item]`.
#[derive(Clone, Copy, Debug)]
struct ParamIn {
    obj: u32,
    job: i32,
    level: i32,
    dex: i32,
    weapon_item: u32,
}

/// `obj` words travel in `i32` arrays: the bit pattern is the `u32` id.
fn id_of(word: i32) -> u32 {
    word as u32
}

/// The `n` triples that follow position `at + 1` of a counted record; returns `(triples, next position)`.
fn counted<'a>(a: &'a [i32], at: usize, what: &str) -> Result<(&'a [i32], usize), RulesError> {
    let n = a
        .get(at)
        .copied()
        .ok_or_else(|| err(format!("{what}: truncated before the count at word {at}")))?;
    let n = usize::try_from(n).map_err(|_| err(format!("{what}: negative count at word {at}")))?;
    let words = n
        .checked_mul(3)
        .ok_or_else(|| err(format!("{what}: count overflow at word {at}")))?;
    let start = at + 1;
    let end = start
        .checked_add(words)
        .ok_or_else(|| err(format!("{what}: count overflow at word {at}")))?;
    let triples = a
        .get(start..end)
        .ok_or_else(|| err(format!("{what}: {n} triples at word {at} run past the end")))?;
    Ok((triples, end))
}

/// `[obj, n, (dst, adj, chg) x n]*` (the `drain_sync` snapshot format).
fn parse_interleaved(a: &[i32]) -> Result<Vec<Snapshot<'_>>, RulesError> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < a.len() {
        let obj = id_of(
            a.get(at)
                .copied()
                .ok_or_else(|| err("snapshots: truncated"))?,
        );
        let (triples, next) = counted(a, at + 1, "snapshots")?;
        out.push(Snapshot { obj, triples });
        at = next;
    }
    Ok(out)
}

/// `objs` plus `[n, (dst, adj, chg) x n]*` (the `dst_snapshot` format).
fn parse_split<'a>(objs: &[u32], a: &'a [i32]) -> Result<Vec<Snapshot<'a>>, RulesError> {
    let mut out = Vec::with_capacity(objs.len());
    let mut at = 0;
    for &obj in objs {
        let (triples, next) = counted(a, at, "dst_snapshot")?;
        out.push(Snapshot { obj, triples });
        at = next;
    }
    if at != a.len() {
        return Err(err(format!(
            "dst_snapshot: {} words left after {} objects",
            a.len() - at,
            objs.len()
        )));
    }
    Ok(out)
}

/// `[obj, kind, dst, a, b]*`.
fn parse_ops(a: &[i32]) -> Result<Vec<Op>, RulesError> {
    let (records, rest) = a.as_chunks::<5>();
    if !rest.is_empty() {
        return Err(err(format!(
            "ops: {} words is not a multiple of 5",
            a.len()
        )));
    }
    records
        .iter()
        .enumerate()
        .map(|(i, &[obj, kind, dst, a, b])| {
            let kind = match kind {
                0 => OpKind::Set,
                1 => OpKind::Reset,
                2 => OpKind::Sync,
                other => return Err(err(format!("ops: record {i}: unknown kind {other}"))),
            };
            Ok(Op {
                obj: id_of(obj),
                kind,
                dst: id_of(dst),
                a,
                b,
            })
        })
        .collect()
}

/// `[obj, job, level, base_dex, weapon_item]*`.
fn parse_param_rows(a: &[i32]) -> Result<Vec<ParamIn>, RulesError> {
    let (records, rest) = a.as_chunks::<5>();
    if !rest.is_empty() {
        return Err(err(format!(
            "param rows: {} words is not a multiple of 5",
            a.len()
        )));
    }
    Ok(records
        .iter()
        .map(|&[obj, job, level, dex, weapon]| ParamIn {
            obj: id_of(obj),
            job,
            level,
            dex,
            weapon_item: id_of(weapon),
        })
        .collect())
}

/// What the sim reads of one mover, from its DST replica and the attack-speed inputs (31 §5.4).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParamRow {
    /// `adj(DST_SPEED)`.
    pub speed_adj_pct: i32,
    /// `chg(DST_SPEED) != NULL_CHGPARAM` (rooted).
    pub speed_chg_set: bool,
    /// `param(DST_JUMPING, 0)`.
    pub jumping: i32,
    /// `adj(DST_CHRSTATE)` (the `CHS_*` bits).
    pub chr_state: i32,
    /// `param(DST_HAWKEYE_RATE, 0)`.
    pub hawkeye_rate: i32,
    /// `GetAttackSpeed()`.
    pub attack_speed: f32,
    /// `GetCastingAniSpeed()`.
    pub casting_ani_speed: f32,
}

// ---- the core -----------------------------------------------------------------------------------------------

/// The state and the logic of [`RulesContext`], without any JavaScript type (natively testable).
#[derive(Debug)]
pub struct RulesCore {
    rules: RulesData,
    items: IdTable<ItemProp>,
    skills: IdTable<ItemProp>,
    cfg: RulesConfig,
    /// A neutral mover prop for the formulas (players read no NPC number from it).
    mover_prop: MoverProp,
    /// The DST replica of every mover the client knows (ascending object id: deterministic).
    replicas: BTreeMap<u32, DstTable>,
    /// What an unknown object is evaluated on.
    neutral: DstTable,
    /// The `item-ext` tables (named sets, piercing materials, gem abilities, random options): the tooltip
    /// numbers read them (WP-MOVER-22).
    ext: ItemExtBundle,
    /// `features.only_armor_awakes`.
    only_armor_awakes: bool,
}

fn check_size(name: &str, bytes: &[u8]) -> Result<(), RulesError> {
    if bytes.len() > MAX_BUNDLE_BYTES {
        return Err(err(format!(
            "{name}: {} bytes is above the limit of {MAX_BUNDLE_BYTES}",
            bytes.len()
        )));
    }
    Ok(())
}

fn item_table(rows: Vec<ItemProp>) -> IdTable<ItemProp> {
    let mut t = IdTable::new();
    for p in rows {
        let id = p.id;
        let _ = t.insert(id, p); // an id above the table limit is dropped
    }
    t
}

impl RulesCore {
    /// Builds the context from the four JSON bundles (each an envelope `{bundle, schema, data}` as served by the
    /// server): `tables` and `item-ext` into [`RulesData`] (`RulesData::from_bundles`), `items` and `skills` into
    /// the prop tables; the bare-hand item (`II_WEA_HAN_HAND`) comes from `items`.
    pub fn new(
        tables: &[u8],
        items: &[u8],
        item_ext: &[u8],
        skills: &[u8],
        cfg: RulesConfig,
    ) -> Result<RulesCore, RulesError> {
        for (n, b) in [
            ("tables", tables),
            ("items", items),
            ("item-ext", item_ext),
            ("skills", skills),
        ] {
            check_size(n, b)?;
        }
        let mut rules = RulesData::from_bundles(tables, item_ext)?;
        let items_env: Envelope<ItemsBundle> =
            serde_json::from_slice(items).map_err(|e| err(format!("items bundle: {e}")))?;
        if items_env.bundle != "items" {
            return Err(err(format!(
                "expected the items bundle, got {:?}",
                items_env.bundle
            )));
        }
        let skills_env: Envelope<SkillsBundle> =
            serde_json::from_slice(skills).map_err(|e| err(format!("skills bundle: {e}")))?;
        if skills_env.bundle != "skills" {
            return Err(err(format!(
                "expected the skills bundle, got {:?}",
                skills_env.bundle
            )));
        }
        let ext_env: Envelope<ItemExtBundle> =
            serde_json::from_slice(item_ext).map_err(|e| err(format!("item-ext bundle: {e}")))?;
        let items = item_table(items_env.data.items.rows);
        rules.hand = items.get(II_WEA_HAN_HAND).cloned();
        Ok(RulesCore {
            rules,
            items,
            skills: item_table(skills_env.data.skills.rows),
            cfg,
            mover_prop: MoverProp::default(),
            replicas: BTreeMap::new(),
            neutral: DstTable::default(),
            ext: ext_env.data,
            only_armor_awakes: true,
        })
    }

    /// Sets `features.only_armor_awakes` (default `true`, the repack): which items have a random-option kind in
    /// the tooltip numbers.
    pub fn with_only_armor_awakes(mut self, on: bool) -> RulesCore {
        self.only_armor_awakes = on;
        self
    }

    /// The `item-ext` tables.
    pub fn item_ext(&self) -> &ItemExtBundle {
        &self.ext
    }

    /// The rule tables.
    pub fn rules(&self) -> &RulesData {
        &self.rules
    }

    /// The item props of the `items` bundle.
    pub fn items(&self) -> &IdTable<ItemProp> {
        &self.items
    }

    /// The skill props of the `skills` bundle (`SI_*`).
    pub fn skills(&self) -> &IdTable<ItemProp> {
        &self.skills
    }

    /// The number of movers with a replica.
    pub fn replica_count(&self) -> usize {
        self.replicas.len()
    }

    fn table(&self, obj: u32) -> &DstTable {
        self.replicas.get(&obj).unwrap_or(&self.neutral)
    }

    // ---- the replica cache ----

    fn load_snapshot(&mut self, s: &Snapshot<'_>) {
        let mut t = DstTable::default();
        for &[dst, adj, chg] in s.triples.as_chunks::<3>().0 {
            t.replica_load(id_of(dst), adj, chg);
        }
        self.replicas.insert(s.obj, t);
    }

    /// Applies the ops in order, creating a default replica for an object without one; returns the objects whose
    /// replica differs from before the call, ascending.
    fn apply_ops(&mut self, ops: &[Op]) -> Vec<u32> {
        let mut before: BTreeMap<u32, Option<DstTable>> = BTreeMap::new();
        for op in ops {
            before
                .entry(op.obj)
                .or_insert_with(|| self.replicas.get(&op.obj).cloned());
            let t = self.replicas.entry(op.obj).or_default();
            match op.kind {
                OpKind::Set => t.replica_set(op.dst, op.a, op.b),
                OpKind::Reset => t.replica_reset(op.dst, op.a),
                OpKind::Sync => t.replica_reset_sync(op.dst, op.a, op.b),
            }
        }
        before
            .into_iter()
            .filter(|(obj, was)| match (was, self.replicas.get(obj)) {
                (Some(w), Some(now)) => w != now,
                (None, Some(now)) => !now.is_default(),
                _ => false,
            })
            .map(|(obj, _)| obj)
            .collect()
    }

    /// `dst_snapshot(objs, triples)`: per object `[n, (dst, adj, chg) x n]`; each replica is replaced by the
    /// snapshot (assignment).
    pub fn dst_snapshot(&mut self, objs: &[u32], triples: &[i32]) -> Result<(), RulesError> {
        let snaps = parse_split(objs, triples)?;
        for s in &snaps {
            self.load_snapshot(s);
        }
        Ok(())
    }

    /// `dst_apply_ops(ops)`: `[obj, kind (0 set | 1 reset | 2 sync), dst, a, b]*`; returns the changed objects,
    /// ascending.
    pub fn dst_apply_ops(&mut self, ops: &[i32]) -> Result<Vec<u32>, RulesError> {
        let ops = parse_ops(ops)?;
        Ok(self.apply_ops(&ops))
    }

    /// `dst_forget(objs)`: drops the replicas.
    pub fn dst_forget(&mut self, objs: &[u32]) {
        for o in objs {
            self.replicas.remove(o);
        }
    }

    /// `dst_reset_all()`: session re-attach and world change.
    pub fn dst_reset_all(&mut self) {
        self.replicas.clear();
    }

    /// `dst_view(obj)`: `adj[95] ++ chg[95]` of the replica (debug only), `None` without one.
    pub fn dst_view(&self, obj: u32) -> Option<Vec<i32>> {
        let t = self.replicas.get(&obj)?;
        Some(t.adj.iter().chain(t.chg.iter()).copied().collect())
    }

    // ---- the mover parameters of the sim ----

    /// A weapon view of an item id (`0` and unknown ids are bare hands).
    fn weapon_view(&self, item: u32) -> Option<WeaponView<'_>> {
        if item == 0 {
            return None;
        }
        self.items.get(item).map(|p| WeaponView::new(p, 0, false))
    }

    fn param_row(&self, dst: &DstTable, p: &ParamIn) -> ParamRow {
        let inputs = StatInputs {
            is_player: p.job >= 0,
            level: p.level,
            job: p.job,
            base: BaseStats {
                str: 0,
                sta: 0,
                dex: p.dex,
                int: 0,
            },
            dst,
            equip_cache: EquipCache::default(),
            weapon: self.weapon_view(p.weapon_item),
            left_weapon: None,
            mover_prop: &self.mover_prop,
            rates: MonsterRates::default(),
            sm_attack_up: false,
        };
        ParamRow {
            speed_adj_pct: dst.adj(DST_SPEED),
            speed_chg_set: dst.chg(DST_SPEED) != NULL_CHGPARAM,
            jumping: dst.param(DST_JUMPING, 0),
            chr_state: dst.adj(DST_CHRSTATE),
            hawkeye_rate: dst.param(DST_HAWKEYE_RATE, 0),
            attack_speed: stats::attack_speed(&inputs, &self.rules),
            casting_ani_speed: stats::casting_ani_speed(dst),
        }
    }

    /// `mover_params(objs, inputs)`: `inputs` are 4 words per object `[job (-1 NPC), level, base_dex, weapon item
    /// (0 = unarmed / NPC)]`; the result is 8 words per object `[speed_adj_pct, speed_chg_set (0/1), jumping,
    /// chr_state, hawkeye_rate, attack_speed (f32 bits), casting_ani_speed (f32 bits), 0]`.
    pub fn mover_params(&self, objs: &[u32], inputs: &[i32]) -> Result<Vec<i32>, RulesError> {
        let want = objs.len().checked_mul(4);
        if want != Some(inputs.len()) {
            return Err(err(format!(
                "mover_params: {} objects need {:?} input words, got {}",
                objs.len(),
                want,
                inputs.len()
            )));
        }
        let mut out = Vec::with_capacity(objs.len() * PARAMS_ROW_WORDS);
        for (obj, &[job, level, dex, weapon]) in objs.iter().zip(inputs.as_chunks::<4>().0) {
            let r = self.param_row(
                self.table(*obj),
                &ParamIn {
                    obj: *obj,
                    job,
                    level,
                    dex,
                    weapon_item: id_of(weapon),
                },
            );
            out.extend_from_slice(&[
                r.speed_adj_pct,
                i32::from(r.speed_chg_set),
                r.jumping,
                r.chr_state,
                r.hawkeye_rate,
                r.attack_speed.to_bits() as i32,
                r.casting_ani_speed.to_bits() as i32,
                0,
            ]);
        }
        Ok(out)
    }

    /// `drain_sync(snapshots, ops, forgets, param_rows)`: **the** boundary call of one network drain. The three
    /// DST arrays are applied in this order (snapshots, ops, forgets), then every param row is evaluated on the
    /// resulting replicas (an object without a replica uses the default table). The arrays are validated first: a
    /// malformed one is an `Err` and nothing changes. Returns 7 words per row, in row order
    /// `[speed_adj_pct, speed_chg_set (0/1), chr_state, hawkeye_rate, jumping, attack_speed, casting_ani_speed]`.
    pub fn drain_sync(
        &mut self,
        snapshots: &[i32],
        ops: &[i32],
        forgets: &[u32],
        param_rows: &[i32],
    ) -> Result<Vec<f32>, RulesError> {
        let snaps = parse_interleaved(snapshots)?;
        let ops = parse_ops(ops)?;
        let rows = parse_param_rows(param_rows)?;
        for s in &snaps {
            self.load_snapshot(s);
        }
        let _ = self.apply_ops(&ops);
        self.dst_forget(forgets);
        let mut out = Vec::with_capacity(rows.len() * DRAIN_ROW_WORDS);
        for p in &rows {
            let r = self.param_row(self.table(p.obj), p);
            out.extend_from_slice(&[
                r.speed_adj_pct as f32,
                if r.speed_chg_set { 1.0 } else { 0.0 },
                r.chr_state as f32,
                r.hawkeye_rate as f32,
                r.jumping as f32,
                r.attack_speed,
                r.casting_ani_speed,
            ]);
        }
        Ok(out)
    }

    // ---- numbers ----

    /// The worn item views, the equipment cache and the weapon views of an input.
    fn derived_with(&self, dst: &DstTable, input: &StatInputJs) -> DerivedStats {
        let worn: Vec<(u32, WeaponView<'_>)> = input
            .equip
            .iter()
            .filter_map(|e| {
                self.items
                    .get(e.item_id)
                    .map(|p| (e.part, WeaponView::new(p, e.ability_option, e.expired)))
            })
            .collect();
        let at = |part: u32| worn.iter().find(|(p, _)| *p == part).map(|(_, v)| *v);
        let weapon = at(input.weapon_part);
        let left_weapon = if input.weapon_part == PARTS_RWEAPON {
            at(PARTS_LWEAPON).filter(|v| v.prop.item_kind1 == IK1_WEAPON)
        } else {
            None
        };
        let armour = worn
            .iter()
            .filter(|(_, v)| matches!(v.prop.item_kind2, IK2_ARMOR | IK2_ARMORETC))
            .map(|(_, v)| *v);
        let (defense_min, defense_max) = stats::equip_defense_sum(&self.rules, dst, armour);
        let mut adj_hit_rate = 0_i32;
        let mut adj_parry = 0_i32;
        for (_, v) in &worn {
            // `SumEquipAdjValue`: only positive values count
            if v.prop.adj_hit_rate > 0 {
                adj_hit_rate = adj_hit_rate.wrapping_add(v.prop.adj_hit_rate);
            }
            if (v.prop.parry as i32) > 0 {
                adj_parry = adj_parry.wrapping_add(v.prop.parry as i32);
            }
        }
        let inputs = StatInputs {
            is_player: input.is_player,
            level: input.level,
            job: input.job,
            base: BaseStats {
                str: input.base[0],
                sta: input.base[1],
                dex: input.base[2],
                int: input.base[3],
            },
            dst,
            equip_cache: EquipCache {
                defense_min,
                defense_max,
                adj_hit_rate,
                adj_parry,
            },
            weapon,
            left_weapon,
            mover_prop: &self.mover_prop,
            rates: MonsterRates::default(),
            sm_attack_up: false,
        };
        stats::derived_stats(&inputs, &self.rules, &self.cfg)
    }

    /// `derived_stats(obj, input)`: every number of the character window for the replica of `obj`.
    pub fn derived_stats(&self, obj: u32, input: &StatInputJs) -> DerivedStatsJs {
        self.derived_with(self.table(obj), input).into()
    }

    /// `stat_preview(obj, input, add)`: the same with `add = [str, sta, dex, int]` pending stat points on top of the
    /// base stats.
    pub fn stat_preview(
        &self,
        obj: u32,
        input: &StatInputJs,
        add: &[i32],
    ) -> Result<DerivedStatsJs, RulesError> {
        let &[s, t, d, i] = add else {
            return Err(err(format!(
                "stat_preview: add has {} words, expected 4",
                add.len()
            )));
        };
        let mut input = input.clone();
        for (b, a) in input.base.iter_mut().zip([s, t, d, i]) {
            *b = b.wrapping_add(a);
        }
        Ok(self.derived_stats(obj, &input))
    }

    /// `move_speed(obj, base, speed_factor)`: `GetSpeed` with the replica of `obj` (no forced speed).
    pub fn move_speed(&self, obj: u32, base: f32, speed_factor: f32) -> f32 {
        stats::move_speed(
            base,
            speed_factor,
            0.0,
            self.table(obj),
            self.cfg.set_max_speed,
        )
    }

    /// `attack_speed(obj, job, level, dex, weapon_item)`.
    pub fn attack_speed(&self, obj: u32, job: i32, level: i32, dex: i32, weapon_item: u32) -> f32 {
        self.param_row(
            self.table(obj),
            &ParamIn {
                obj,
                job,
                level,
                dex,
                weapon_item,
            },
        )
        .attack_speed
    }

    /// `casting_ani_speed(obj)`.
    pub fn casting_ani_speed(&self, obj: u32) -> f32 {
        stats::casting_ani_speed(self.table(obj))
    }

    /// `exp_percent_x100(level, exp)`: `(int)(exp * 10000 / next_exp(level))`.
    pub fn exp_percent_x100(&self, level: i32, exp: f64) -> i32 {
        exp_percent_x100(&self.rules, level, exp as i64)
    }

    /// `equip_check(item, random_opt_hi, sex, job, level, rweapon_item)`: `0` when the character may wear the item,
    /// else the `TID_*` of the refusal ([`EQUIP_SILENT_REFUSAL`] for a refusal without a text); an unknown item
    /// is `0`. `random_opt_hi` is the high word of the random-option word (the level-down bits).
    pub fn equip_check(
        &self,
        item_id: u32,
        random_opt_hi: u32,
        sex: u32,
        job: i32,
        level: i32,
        rweapon_item: u32,
    ) -> u32 {
        let Some(prop) = self.items.get(item_id) else {
            return 0;
        };
        let bow = self
            .items
            .get(rweapon_item)
            .is_some_and(|p| p.item_kind3 == IK3_BOW);
        let wearer = Wearer { sex, job, level };
        let opt = RandomOptWord(u64::from(random_opt_hi) << 32);
        match check_equip(&self.rules, &wearer, prop, opt, "", bow, false) {
            Ok(()) => 0,
            Err(d) => d.tid.unwrap_or(EQUIP_SILENT_REFUSAL),
        }
    }

    /// `item_multiplier(item, ability_option, expired)`: `GetItemMultiplier` (1.0 for an unknown item).
    pub fn item_multiplier(&self, item_id: u32, ability_option: i32, expired: bool) -> f32 {
        let Some(prop) = self.items.get(item_id) else {
            return 1.0;
        };
        let v = WeaponView::new(prop, ability_option, expired);
        stats::item_multiplier(&self.rules, prop, ability_option, expired, v.refer_ultimate)
    }

    /// `armor_set_level(equip_options)`: `GetSetItemClient` over the 31 attribute options (`EquipInfo.option`, 0 =
    /// empty): the lowest `option & 0xFF` of the four set parts (upper body, hand, foot, cap), capped at 10; 0 when
    /// one is missing.
    pub fn armor_set_level(&self, equip_options: &[i32]) -> i32 {
        armor_set_level_client(equip_options)
    }

    // ---- skill points (M4, WP-MOVER-24: the browser half of the skill-plan rule) ----

    /// `skill_point_cost(skill_id)` (WP-MOVER-24): the points one level of the skill costs -
    /// `flyff_rules::skill_points::sp_cost`, 1 / 2 / 3 by the tier of the skill (vagrant, expert, every other).
    /// `Err` for a skill the data does not know.
    pub fn skill_point_cost(&self, skill_id: u32) -> Result<u32, RulesError> {
        let prop = self
            .skills
            .get(skill_id)
            .ok_or_else(|| err(format!("skill_point_cost: unknown skill {skill_id}")))?;
        Ok(flyff_rules::skill_points::sp_cost(prop, &self.rules))
    }

    /// `skill_plan_check(cur, plan, level, job, sp)` (WP-MOVER-24, VR-086): the very function the server validates
    /// `DoUseSkillPointReq` with, `flyff_rules::skill::plan::validate_skill_plan`, on two tables of 45 slots, each as
    /// 90 words `[skill_id, level] x 45` in the server slot order. `>= 0`: the plan is legal and costs that many
    /// points; `< 0`: `-(code + 1)` of the `SkillPlanError` (0 wrong skill, 1 level down, 2 above max, 3 level
    /// requirement, 4 prerequisite, 5 not enough points, 6 nothing to do). `Err` for a table of the wrong length.
    pub fn skill_plan_check(
        &self,
        cur: &[u32],
        plan: &[u32],
        level: i32,
        job: i32,
        sp: i32,
    ) -> Result<i32, RulesError> {
        let cur = skill_table_from_words(cur, "cur")?;
        let plan = skill_table_from_words(plan, "plan")?;
        Ok(
            match validate_skill_plan(&cur, &plan, level, job, sp, &self.skills, &self.rules) {
                Ok(cost) => cost,
                Err(e) => -(i32::from(e.code()) + 1),
            },
        )
    }

    /// `fast_job_targets(level, job)` (M5, WP-MOVER-18/19, Plan/16 §11.4): the jobs an `UpdateJob` may name for a
    /// character of that level and job - `flyff_rules::jobs::fast_job_targets`, the very function the server
    /// validates with. Empty when no change is possible. (`Result` for the uniform export shape.)
    pub fn fast_job_targets(&self, level: i32, job: i32) -> Result<Vec<u32>, RulesError> {
        Ok(flyff_rules::jobs::fast_job_targets(&self.rules, level, job).to_vec())
    }

    /// `fast_job_window(level, job, exp_percent_x100)` (M5, WP-MOVER-19): does the client open the fast job change
    /// window - level 15 vagrant, level 60 expert, level 120 pro or master with a full EXP bar
    /// (`exp_percent_x100 == 9999`) - `flyff_rules::jobs::fast_job_window`.
    pub fn fast_job_window(
        &self,
        level: i32,
        job: i32,
        exp_percent_x100: i32,
    ) -> Result<bool, RulesError> {
        Ok(flyff_rules::jobs::fast_job_window(
            level,
            job,
            exp_percent_x100,
        ))
    }
}

/// A skill table from its wire words: `[skill_id, level]` for each of the 45 slots (90 words).
fn skill_table_from_words(words: &[u32], what: &str) -> Result<SkillTable, RulesError> {
    if words.len() != MAX_SKILL_JOB * 2 {
        return Err(err(format!(
            "skill_plan_check: {what} needs {} words ({MAX_SKILL_JOB} x [skill, level]), got {}",
            MAX_SKILL_JOB * 2,
            words.len()
        )));
    }
    let mut table = EMPTY_SKILLS;
    let (pairs, _) = words.as_chunks::<2>();
    for (slot, &[skill_id, level]) in table.iter_mut().zip(pairs) {
        *slot = JobSkill { skill_id, level };
    }
    Ok(table)
}

/// `CMover::GetSetItemClient` (`Mover.cpp:8896-8911`): the armour +N set level seen from the options of the other
/// players' `m_aEquipInfo` (the low byte is the upgrade level): the minimum over `PARTS_UPPER_BODY`, `PARTS_HAND`,
/// `PARTS_FOOT` and `PARTS_CAP`, starting from 10; any missing part makes it 0. Fewer than 31 words count as
/// empty parts.
pub fn armor_set_level_client(options: &[i32]) -> i32 {
    use flyff_common::defs::neuz::{PARTS_CAP, PARTS_FOOT, PARTS_HAND, PARTS_UPPER_BODY};
    let mut level = 10;
    for part in [PARTS_UPPER_BODY, PARTS_HAND, PARTS_FOOT, PARTS_CAP] {
        let v = options.get(part as usize).copied().unwrap_or(0) & 0xFF;
        if v == 0 {
            return 0;
        }
        level = level.min(v);
    }
    level
}

// ---- the wasm shell -----------------------------------------------------------------------------------------

/// The rules of the browser (Plan/16 §18.4).
///
/// **Contract of the replica cache** (a deviation from D7's "pure functions taking plain data"). Besides the data
/// decoded once from the bundles (rule tables, item and skill props) the context holds one DST replica per mover the
/// client knows. The replicas are a *cache of server-sent state*, not game state: TypeScript owns the lifecycle of
/// the entities. It passes a mover's spawn snapshot (`MoverCommon.dst_params`) when the mover spawns, the
/// `SetDestParam` / `ResetDestParam` / `ResetDestParamSync` messages as ops, and the id in `forgets` when the mover
/// despawns; it calls `dst_reset_all` on a session re-attach and on a world change, before the new spawns arrive.
/// A snapshot is *loaded* (assigned), never applied as a delta, so a root counter above 1 survives; ops are the legacy
/// deltas. No rule reads anything TypeScript did not feed, and every export is a pure function of (replica,
/// arguments); an object without a replica is evaluated on the default (all zero) table. Per network drain exactly
/// one `drain_sync` call; the per-object exports are for tools and the fallback, `dst_view` is debug-only.
/// Arrays that do not parse throw and change nothing.
#[wasm_bindgen]
pub struct RulesContext {
    core: RulesCore,
}

fn js_err(e: impl std::fmt::Display) -> JsError {
    JsError::new(&e.to_string())
}

fn from_js<T: serde::de::DeserializeOwned>(v: JsValue, what: &str) -> Result<T, JsError> {
    serde_wasm_bindgen::from_value(v).map_err(|e| JsError::new(&format!("{what}: {e}")))
}

fn to_js<T: Serialize>(v: &T) -> Result<JsValue, JsError> {
    serde_wasm_bindgen::to_value(v).map_err(js_err)
}

#[wasm_bindgen]
impl RulesContext {
    /// `new RulesContext(tables, items, itemExt, skills, cfg)`: once after the bundles loaded (the four JSON files
    /// as bytes, each the envelope `{bundle, schema, data}`). `cfg` is an object with the optional `RulesConfig`
    /// switches in camelCase (`bowdmgPropjob`, `wandAutoattacks`, `betterArmorBonus`, `autoSkillMax`,
    /// `fixGetHpMpFpPoint`, `removeMaxRecoverRate`, `healOverTime`, `setMaxSpeed`, `setMaxCast`, `setMaxCrit`,
    /// `maxBlockValue`, `maxBlockValueGw`); `undefined` means the repack defaults.
    /// `onlyArmorAwakes` (default `true`) decides which items carry a random-option kind in the tooltip numbers.
    #[wasm_bindgen(constructor)]
    pub fn new(
        tables: &[u8],
        items: &[u8],
        item_ext: &[u8],
        skills: &[u8],
        cfg: JsValue,
    ) -> Result<RulesContext, JsError> {
        let cfg: RulesCfgJs = if cfg.is_undefined() || cfg.is_null() {
            RulesCfgJs::default()
        } else {
            from_js(cfg, "rules cfg")?
        };
        let only_armor_awakes = cfg.only_armor_awakes.unwrap_or(true);
        let core = RulesCore::new(tables, items, item_ext, skills, cfg.into_config())
            .map_err(js_err)?
            .with_only_armor_awakes(only_armor_awakes);
        Ok(RulesContext { core })
    }

    /// **The** boundary call of a network drain (Plan/40 §7.10): `snapshots` `[obj, n, (dst, adj, chg) x n]*`
    /// applied first, `ops` `[obj, kind, dst, a, b]*` second, `forgets` `[obj]*` third, then the `param_rows`
    /// `[obj, job (-1 NPC), level, base_dex, weapon_item (0 = unarmed / NPC)]*` are evaluated; 7 `f32` per row
    /// `[speed_adj_pct, speed_chg_set, chr_state, hawkeye_rate, jumping, attack_speed, casting_ani_speed]`.
    pub fn drain_sync(
        &mut self,
        snapshots: &[i32],
        ops: &[i32],
        forgets: &[u32],
        param_rows: &[i32],
    ) -> Result<Vec<f32>, JsError> {
        self.core
            .drain_sync(snapshots, ops, forgets, param_rows)
            .map_err(js_err)
    }

    /// Replica cache: `objs` and, per object, `[n, (dst, adj, chg) x n]` (`MoverCommon.dst_params`).
    pub fn dst_snapshot(&mut self, objs: &[u32], triples: &[i32]) -> Result<(), JsError> {
        self.core.dst_snapshot(objs, triples).map_err(js_err)
    }

    /// Replica cache: `[obj, kind (0 set | 1 reset | 2 sync), dst, a, b]*`; returns the objects whose replica
    /// changed, ascending.
    pub fn dst_apply_ops(&mut self, ops: &[i32]) -> Result<Vec<u32>, JsError> {
        self.core.dst_apply_ops(ops).map_err(js_err)
    }

    /// Replica cache: the movers despawned.
    pub fn dst_forget(&mut self, objs: &[u32]) {
        self.core.dst_forget(objs);
    }

    /// Replica cache: session re-attach and world change.
    pub fn dst_reset_all(&mut self) {
        self.core.dst_reset_all();
    }

    /// Debug only, never on the drain path: `adj[95]` then `chg[95]` of the replica, `undefined` without one.
    pub fn dst_view(&self, obj: u32) -> Option<Vec<i32>> {
        self.core.dst_view(obj)
    }

    /// The sim inputs of many movers in one call: `inputs` is `[job (-1 NPC), level, base_dex, weapon item (0 =
    /// unarmed / NPC)]` per object, the result 8 words per object `[speed_adj_pct, speed_chg_set (0/1), jumping,
    /// chr_state, hawkeye_rate, attack_speed (f32 bits), casting_ani_speed (f32 bits), 0]`; read slots 5-6 through
    /// a `Float32Array` view of the same buffer.
    pub fn mover_params(&self, objs: &[u32], inputs: &[i32]) -> Result<Vec<i32>, JsError> {
        self.core.mover_params(objs, inputs).map_err(js_err)
    }

    /// Every number of the character window: `input` is `{ isPlayer, level, job, base: [str, sta, dex, int], equip:
    /// [{ part, itemId, abilityOption, expired }], weaponPart }`, the result `DerivedStats` in camelCase.
    pub fn derived_stats(&self, obj: u32, input: JsValue) -> Result<JsValue, JsError> {
        let input: StatInputJs = from_js(input, "derived_stats input")?;
        to_js(&self.core.derived_stats(obj, &input))
    }

    /// `derived_stats` with `add = [str, sta, dex, int]` pending stat points.
    pub fn stat_preview(&self, obj: u32, input: JsValue, add: &[i32]) -> Result<JsValue, JsError> {
        let input: StatInputJs = from_js(input, "stat_preview input")?;
        to_js(&self.core.stat_preview(obj, &input, add).map_err(js_err)?)
    }

    /// `GetSpeed` with the replica of `obj`.
    pub fn move_speed(&self, obj: u32, base: f32, speed_factor: f32) -> f32 {
        self.core.move_speed(obj, base, speed_factor)
    }

    /// `GetAttackSpeed()` for the given job, level, dexterity and weapon item.
    pub fn attack_speed(&self, obj: u32, job: i32, level: i32, dex: i32, weapon_item: u32) -> f32 {
        self.core.attack_speed(obj, job, level, dex, weapon_item)
    }

    /// `GetCastingAniSpeed()`.
    pub fn casting_ani_speed(&self, obj: u32) -> f32 {
        self.core.casting_ani_speed(obj)
    }

    /// `(int)(exp * 10000 / next_exp(level))`; `exp` is below 2^53.
    pub fn exp_percent_x100(&self, level: i32, exp: f64) -> i32 {
        self.core.exp_percent_x100(level, exp)
    }

    /// `0` when the item may be worn, else the `TID_*` (`0xFFFFFFFF`: refused without a text).
    pub fn equip_check(
        &self,
        item_id: u32,
        random_opt_hi: u32,
        sex: u32,
        job: i32,
        level: i32,
        rweapon_item: u32,
    ) -> u32 {
        self.core
            .equip_check(item_id, random_opt_hi, sex, job, level, rweapon_item)
    }

    /// `GetItemMultiplier`.
    pub fn item_multiplier(&self, item_id: u32, ability_option: i32, expired: bool) -> f32 {
        self.core.item_multiplier(item_id, ability_option, expired)
    }

    /// The armour +N set level of 31 attribute options.
    pub fn armor_set_level(&self, equip_options: &[i32]) -> i32 {
        self.core.armor_set_level(equip_options)
    }

    /// M4 (WP-MOVER-24): the points one level of the skill costs; throws for an unknown skill.
    pub fn skill_point_cost(&self, skill_id: u32) -> Result<u32, JsError> {
        self.core.skill_point_cost(skill_id).map_err(js_err)
    }

    /// M4 (WP-MOVER-24, VR-086): `cur` and `plan` are 45 slots as 90 words `[skill_id, level]`; `>= 0` the cost of
    /// the plan, `< 0` the negated error code plus one (see `RulesCore::skill_plan_check`); throws for a wrong length.
    pub fn skill_plan_check(
        &self,
        cur: &[u32],
        plan: &[u32],
        level: i32,
        job: i32,
        sp: i32,
    ) -> Result<i32, JsError> {
        self.core
            .skill_plan_check(cur, plan, level, job, sp)
            .map_err(js_err)
    }

    /// M5 (WP-MOVER-19): the jobs `UpdateJob` may name for a character of `level` and `job` (the strict table of
    /// Plan/16 §11.4; empty when no fast job change is possible).
    pub fn fast_job_targets(&self, level: i32, job: i32) -> Result<Vec<u32>, JsError> {
        self.core.fast_job_targets(level, job).map_err(js_err)
    }

    /// M5 (WP-MOVER-19): the client rule that opens the fast job change window (level 15 vagrant, level 60 expert,
    /// level 120 pro or master with `exp_percent_x100 == 9999`).
    pub fn fast_job_window(
        &self,
        level: i32,
        job: i32,
        exp_percent_x100: i32,
    ) -> Result<bool, JsError> {
        self.core
            .fast_job_window(level, job, exp_percent_x100)
            .map_err(js_err)
    }
}
