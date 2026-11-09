//! Combat formulas (Plan/17 §5; WP-COMBAT-01 .. 03): the pure part of the `CAttackArbiter` pipeline.
//!
//! Nothing here owns state. Both sides of an attack are seen through the [`CombatantView`] trait (the server
//! implements it over its `Mover`, the browser over its replicas; [`Combatant`] is the ready-made implementation
//! over the stat inputs of `crate::stats`). Every function states the C++ it ports (`src:_Common/MoverAttack.cpp`,
//! `src:WORLDSERVER/AttackArbiter.cpp`); `float` is `f32`, `(int)` is `as i32`, integer arithmetic on
//! DST-derived values wraps like the C `int`, and random draws come from the injected
//! [`GameRng`](flyff_common::rng::GameRng) in the order of the original (the order is part of the contract, 17
//! §13: for a generic hit it is critical roll, knock-up roll, damage roll, then the defense and block rolls).
//!
//! | module | content |
//! |---|---|
//! | [`attackable`] | `client_attackable`: the client branch of `IsAttackAble` (WP-MOVER-23) |
//! | [`flags`] | `AtkFlags`, `AtkType`, `HitType`, `Hand`, `JobFactor`, `objmsg`, constants, `attack_range` |
//! | [`param`] | `DamageParam` (the packed `nParam`) |
//! | [`rng`] | `x_random`, `x_random_range`, `mul_div`, the charge tables, `FakeRng` (feature `testkit`) |
//! | [`generic`] | `hit_min_max`, `hit_power`, `post_calc_generic`, `can_fly_by_attack` |
//! | [`defense`] | `calc_defense` |
//! | [`hit`] | `attack_result`, `critical_prob`, `is_critical_attack`, `block_factor` |
//! | [`skillpower`] | `magic_hit_power`, `apply_dpc`, `melee_skill_power`, `magic_skill_power`, `post_calc_magic_skill`, `magic_skill_factor`, `post_asalraalaikum` |
//! | [`multiplier`] | `atk_multiplier`, `damage_multiplier`, `link_attack_bonus`, `level_gap_factor` |
//! | [`element`] | the element relation table, `AttributeTable`, `damage_property_factor` |

pub mod attackable;
pub mod defense;
pub mod element;
pub mod flags;
pub mod generic;
pub mod hit;
pub mod multiplier;
pub mod param;
pub mod rng;
pub mod skillpower;

pub use defense::calc_defense;
pub use element::{AttributeTable, damage_property_factor};
pub use flags::{
    AtkFlags, AtkType, Hand, HitType, JobFactor, MAX_ACTMSG, MAX_CHARGE_LEVEL, attack_range, objmsg,
};
pub use generic::{can_fly_by_attack, hit_min_max, hit_power, post_calc_generic};
pub use hit::{attack_result, block_factor, critical_prob, is_critical_attack};
pub use multiplier::{
    LinkAttackState, atk_multiplier, damage_multiplier, level_gap_factor, link_attack_bonus,
};
pub use param::DamageParam;

pub use rng::{BOW_CHARGE, WAND_CHARGE, mul_div, x_random, x_random_range};
pub use skillpower::{
    apply_dpc, magic_hit_power, magic_skill_factor, magic_skill_power, melee_skill_power,
    post_asalraalaikum, post_calc_magic_skill,
};

use crate::RulesConfig;
use crate::consts::dst::DST_CHRSTATE;
use crate::data::RulesData;
use crate::stats::{self, StatInputs};
use flyff_data::props::{item::ItemProp, mover::MoverProp, skill_add::AddSkillProp};

/// An item's element and its level (`CItemElem::m_bItemResist` and `m_nResistAbilityOption`); `element` 0 is
/// `NO_PROP`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ElementItem {
    /// `SAI79::ePropType`: 0 none, 1 fire, 2 water, 3 electricity, 4 wind, 5 earth.
    pub element: u32,
    /// The element upgrade level.
    pub level: i32,
}

/// The combat switches (Plan/17 §12, §0.5): the `[features]`, `[gameplay]` and `[fidelity]` keys the formulas
/// read. [`CombatConfig::default`] equals the registry default (`CombatConfig::from_config` of the default
/// registry, pinned by `combat_config_default_follows_the_registry` in `flyff-game`): the repack, every
/// `features.*` switch on except `splash_autoattacks`, which is off by owner decision #007 (BFIX1);
/// `fidelity.player_melee_can_miss` stays false (KQ-03). Both values of each switch are implemented and tested.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CombatConfig {
    /// The switches shared with the stat formulas (`wand_autoattacks`, `bowdmg_propjob`, the caps).
    pub rules: RulesConfig,
    /// `features.improved_block_calculation`.
    pub improved_block_calculation: bool,
    /// `features.dmgcorrect`.
    pub dmgcorrect: bool,
    /// `features.better_link_attack`.
    pub better_link_attack: bool,
    /// `features.af_magic_fix`.
    pub af_magic_fix: bool,
    /// `features.element_fix`.
    pub element_fix: bool,
    /// `features.improved_spiritbomb`.
    pub improved_spiritbomb: bool,
    /// `features.hop_scaling_fix`.
    pub hop_scaling_fix: bool,
    /// `features.no_hop_cost`.
    pub no_hop_cost: bool,
    /// `features.no_asal_mp`.
    pub no_asal_mp: bool,
    /// `features.new_skill_attribute`.
    pub new_skill_attribute: bool,
    /// `features.better_holycross`.
    pub better_holycross: bool,
    /// `features.holycross_support_gw`.
    pub holycross_support_gw: bool,
    /// `features.magic_steal_hp`.
    pub magic_steal_hp: bool,
    /// `features.splash_autoattacks`.
    pub splash_autoattacks: bool,
    /// `fidelity.player_melee_can_miss` (KQ-03; the repack ignores the `AF_MISS` roll of player autoattacks).
    pub player_melee_can_miss: bool,
}

impl Default for CombatConfig {
    fn default() -> Self {
        CombatConfig {
            rules: RulesConfig::default(),
            improved_block_calculation: true,
            dmgcorrect: true,
            better_link_attack: true,
            af_magic_fix: true,
            element_fix: true,
            improved_spiritbomb: true,
            hop_scaling_fix: true,
            no_hop_cost: true,
            no_asal_mp: true,
            new_skill_attribute: true,
            better_holycross: true,
            holycross_support_gw: true,
            magic_steal_hp: true,
            splash_autoattacks: false,
            player_melee_can_miss: false,
        }
    }
}

impl CombatConfig {
    /// The projection of the configuration registry.
    pub fn from_config(cfg: &flyff_common::config::RulesConfig) -> CombatConfig {
        let f = &cfg.features;
        CombatConfig {
            rules: RulesConfig::from_config(cfg),
            improved_block_calculation: f.improved_block_calculation,
            dmgcorrect: f.dmgcorrect,
            better_link_attack: f.better_link_attack,
            af_magic_fix: f.af_magic_fix,
            element_fix: f.element_fix,
            improved_spiritbomb: f.improved_spiritbomb,
            hop_scaling_fix: f.hop_scaling_fix,
            no_hop_cost: f.no_hop_cost,
            no_asal_mp: f.no_asal_mp,
            new_skill_attribute: f.new_skill_attribute,
            better_holycross: f.better_holycross,
            holycross_support_gw: f.holycross_support_gw,
            magic_steal_hp: f.magic_steal_hp,
            splash_autoattacks: f.splash_autoattacks,
            player_melee_can_miss: cfg.fidelity.player_melee_can_miss,
        }
    }
}

/// What the formulas need besides the two sides: the switches, the element table and the values the running
/// events and the GM back end contribute.
#[derive(Clone, Copy, Debug)]
pub struct CombatCx<'a> {
    /// The switches.
    pub cfg: &'a CombatConfig,
    /// `CItemUpgrade` attribute factors (`ItemUpgrade.lua` `AddAttribute`).
    pub attr: &'a AttributeTable,
    /// `EventLua.GetAttackPower()`: added to a **player** attacker's ATK (0 without Event.lua).
    pub event_attack_power: i32,
    /// `EventLua.GetDefensePower()`: added to a **player** victim's defense (0 without Event.lua).
    pub event_defense_power: i32,
    /// `prj.m_fMonsterHitRate`: the monster damage multiplier (`rates.monster_hit`, 1.0 when unset).
    pub monster_hit_rate: f32,
}

impl<'a> CombatCx<'a> {
    /// A context with neutral event values.
    pub fn new(cfg: &'a CombatConfig, attr: &'a AttributeTable) -> CombatCx<'a> {
        CombatCx {
            cfg,
            attr,
            event_attack_power: 0,
            event_defense_power: 0,
            monster_hit_rate: 1.0,
        }
    }
}

/// The skill an attack names (`prj.GetSkillProp(skill)` and `prj.GetAddSkillProp(sub_define, level)`); both are
/// resolved by the caller. A missing row is `None` and the formulas answer as the original does for a NULL prop.
#[derive(Clone, Copy, Debug, Default)]
pub struct SkillRef<'a> {
    /// The skill row (`ItemProp` of `propSkill.txt`).
    pub prop: Option<&'a ItemProp>,
    /// The level row (`AddSkillProp`).
    pub add: Option<&'a AddSkillProp>,
}

/// `ATTACK_INFO` (Plan/17 §5): the flags, the packed parameter, the hand and the defense factor of one hit, plus
/// the resolved skill and NPC attack item.
#[derive(Clone, Copy, Debug)]
pub struct AttackInfo<'a> {
    /// `dwAtkFlags`; the formulas add `AF_CRITICAL`, `AF_FLYING`, `AF_BLOCKING` and clear them again.
    pub flags: AtkFlags,
    /// `nParam`.
    pub param: DamageParam,
    /// `nParts`.
    pub hand: Hand,
    /// `nDEFFactor`: the element factor applied to the defense, 10000 when neutral.
    pub def_factor: i32,
    /// The skill rows of `param.skill(flags)` / `param.skill_level()`.
    pub skill: SkillRef<'a>,
    /// The attack item of an NPC attacker: the prop of `param.item_id()` when it names one, else `dwAtk1`
    /// (`None` when the NPC has no attack item).
    pub npc_item: Option<&'a ItemProp>,
    /// Output: a one-shot party critical bonus (`MVRF_CRITICAL`, Sphere Circle) was spent by a critical roll;
    /// the caller clears the flag.
    pub party_crit_used: bool,
}

impl<'a> AttackInfo<'a> {
    /// A fresh `ATTACK_INFO` for the right hand with the neutral defense factor.
    pub fn new(flags: AtkFlags, param: DamageParam) -> AttackInfo<'a> {
        AttackInfo {
            flags,
            param,
            hand: Hand::Right,
            def_factor: 10_000,
            skill: SkillRef::default(),
            npc_item: None,
            party_crit_used: false,
        }
    }

    /// `GetSkill()`: the skill id of a skill attack, else 0.
    pub fn skill_id(&self) -> u32 {
        self.param.skill(self.flags)
    }

    /// `GetAtkType()`.
    pub fn atk_type(&self) -> AtkType {
        self.flags.atk_type()
    }

    /// `GetChargeLevel()`.
    pub fn charge_level(&self) -> u32 {
        self.param.charge_level(self.flags)
    }

    /// `GetSkillLevel()`.
    pub fn skill_level(&self) -> u32 {
        self.param.skill_level()
    }
}

/// One side of an attack as the formulas see it (Plan/17 §5). The required methods are the facts about the
/// mover; everything else is derived from them with the formulas of `crate::stats`.
///
/// The view is built for **one hand**: [`StatInputs::weapon`] is the item in the hand that strikes and
/// [`StatInputs::left_weapon`] the item in the other hand (`IsDualWeapon()` needs both). A dual-wield attacker
/// is therefore viewed twice, once per hand.
pub trait CombatantView {
    /// The inputs of the stat formulas (level, job, base stats, DST aggregate, equipment cache, weapons, mover
    /// prop, NPC rates, `SM_ATTACK_UP`).
    fn stat(&self) -> &StatInputs<'_>;
    /// The rule tables (job rows, `expUpitem`, the bare-hand item).
    fn rules(&self) -> &RulesData;
    /// `m_pActMover->IsFly() || IsFlyingNPC()`.
    fn is_flying(&self) -> bool;
    /// The action state has a `OBJSTA_DMG_FLY_ALL` bit (already knocked up).
    fn in_dmg_fly(&self) -> bool;
    /// The action state has `OBJSTA_ATK4` (the fourth combo hit).
    fn in_atk4_state(&self) -> bool;
    /// `GetHitPoint()`.
    fn hp(&self) -> i32;
    /// `GetMaxHitPoint()`.
    fn max_hp(&self) -> i32;
    /// `GetManaPoint()`.
    fn mp(&self) -> i32;
    /// `GetMaxManaPoint()`.
    fn max_mp(&self) -> i32;
    /// `m_dwMode` (`ONEKILL_MODE`, `MATCHLESS_MODE`, ...).
    fn mode(&self) -> u32;
    /// `HasBuff(kind, id)`.
    fn has_buff(&self, kind: u16, id: u32) -> bool;
    /// The element of the weapon **item** in the striking hand (`m_bItemResist` / `m_nResistAbilityOption`);
    /// element 0 when the item has none or there is no item.
    fn weapon_element(&self) -> ElementItem;
    /// The element of the suit item (`PARTS_UPPER_BODY`) of a player.
    fn suit_element(&self) -> ElementItem;
    /// The one-shot Sphere Circle bonus: `Some(party_size / 2)` while `MVRF_CRITICAL` is armed for a party
    /// member, else `None`.
    fn party_crit(&self) -> Option<i32>;
    /// The mover stands in `WI_WORLD_GUILDWAR` (the block cap of PvP there).
    fn in_guild_war_world(&self) -> bool;

    /// `IsPlayer()`.
    fn is_player(&self) -> bool {
        self.stat().is_player
    }
    /// `GetLevel()`.
    fn level(&self) -> i32 {
        self.stat().level
    }
    /// `GetStr()`.
    fn str_(&self) -> i32 {
        stats::get_str(self.stat())
    }
    /// `GetSta()`.
    fn sta(&self) -> i32 {
        stats::get_sta(self.stat())
    }
    /// `GetDex()`.
    fn dex(&self) -> i32 {
        stats::get_dex(self.stat())
    }
    /// `GetInt()`.
    fn int(&self) -> i32 {
        stats::get_int(self.stat())
    }
    /// `GetParam(dst, base)`: the override when set, else `base + adj`.
    fn param(&self, dst: u32, base: i32) -> i32 {
        self.stat().dst.param(dst, base)
    }
    /// `GetAdjParam(dst)`.
    fn adj(&self, dst: u32) -> i32 {
        self.stat().dst.adj(dst)
    }
    /// `GetChgParam(dst)`: the override, `None` when there is none.
    fn chg(&self, dst: u32) -> Option<i32> {
        let v = self.stat().dst.chg(dst);
        (v != crate::consts::NULL_CHGPARAM).then_some(v)
    }
    /// The mover prop.
    fn mover_prop(&self) -> &MoverProp {
        self.stat().mover_prop
    }
    /// `GetProp()->dwClass` (`RANK_*`).
    fn rank(&self) -> u32 {
        self.mover_prop().class
    }
    /// `IsDualWeapon()`.
    fn is_dual_weapon(&self) -> bool {
        self.stat().is_dual_weapon()
    }
    /// `GetAdjParam(DST_CHRSTATE)`: the `CHS_*` bits.
    fn chr_state(&self) -> u32 {
        self.adj(DST_CHRSTATE) as u32
    }
    /// `SM_ATTACK_UP` or `SM_ATTACK_UP1` is active.
    fn sm_attack_up(&self) -> bool {
        self.stat().sm_attack_up
    }
    /// `(m_fAttackPower_Rate, m_fDefence_Rate)` of an NPC (1.0 unless the GM back end overrides them).
    fn npc_rates(&self) -> (f32, f32) {
        let r = self.stat().rates;
        (r.attack, r.defence)
    }
    /// `GetHR()`.
    fn hit_rate(&self) -> i32 {
        stats::hit_rating(self.stat())
    }
    /// `GetParrying()`.
    fn parrying(&self) -> i32 {
        stats::parry(self.stat())
    }
    /// `GetAdjHitRate()`.
    fn adj_hit_rate(&self) -> i32 {
        stats::adj_hit_rate(self.stat())
    }
    /// `GetActiveHandItemProp()`: the item prop of the striking hand; a player without a weapon strikes with
    /// the bare-hand item; an NPC with the attack item the caller put into [`StatInputs::weapon`].
    fn hand_prop(&self) -> Option<&ItemProp> {
        match &self.stat().weapon {
            Some(w) => Some(w.prop),
            None if self.is_player() => self.rules().hand.as_ref(),
            None => None,
        }
    }
    /// `GetHitPointPercent(100)`: `MulDiv(hp, 100, max)`, 0 for a zero maximum.
    fn hp_percent(&self) -> i32 {
        let max = self.max_hp();
        if max == 0 {
            0
        } else {
            mul_div(self.hp(), 100, max)
        }
    }
    /// `GetManaPointPercent(100)`.
    fn mp_percent(&self) -> i32 {
        let max = self.max_mp();
        if max == 0 {
            0
        } else {
            mul_div(self.mp(), 100, max)
        }
    }
    /// `GetJobPropFactor(type)`: the `propJob.inc` column of the mover's job; 1.0 for an NPC.
    fn job_factor(&self, f: JobFactor) -> f32 {
        if !self.is_player() {
            return 1.0;
        }
        let Some(row) = self
            .rules()
            .job(self.stat().job)
            .or_else(|| self.rules().job(0))
        else {
            return 0.0;
        };
        let p = &row.prop;
        match f {
            JobFactor::Swd => p.melee_swd,
            JobFactor::Axe => p.melee_axe,
            JobFactor::Staff => p.melee_staff,
            JobFactor::Stick => p.melee_stick,
            JobFactor::Knuckle => p.melee_knuckle,
            JobFactor::Wand => p.magic_wand,
            JobFactor::Yoyo => p.melee_yoyo,
            JobFactor::Bow => p.melee_bow,
            JobFactor::Blocking => p.blocking,
            JobFactor::Critical => p.critical,
        }
    }
}

/// `CBuffMgr::HasBuff(kind, id)` as the formulas ask it. The server implements it for its buff set; a closure
/// `|kind, id| ...` works too.
pub trait BuffQuery {
    /// `true` when the mover holds the buff `(kind, id)` (`BUFF_SKILL` = 1).
    fn has_buff(&self, kind: u16, id: u32) -> bool;
}

impl<F: Fn(u16, u32) -> bool> BuffQuery for F {
    fn has_buff(&self, kind: u16, id: u32) -> bool {
        self(kind, id)
    }
}

/// The ready-made [`CombatantView`] over the stat inputs: the server fills it from a `Mover` for one hand, the
/// tests from fixtures.
pub struct Combatant<'a> {
    /// The stat-formula inputs (the hand in question as `weapon`).
    pub stat: StatInputs<'a>,
    /// The rule tables.
    pub rules: &'a RulesData,
    /// `IsFly() || IsFlyingNPC()`.
    pub flying: bool,
    /// In a `DMG_FLY` state.
    pub in_dmg_fly: bool,
    /// In `OBJSTA_ATK4`.
    pub in_atk4: bool,
    /// `GetHitPoint()` and `GetMaxHitPoint()`.
    pub hp: (i32, i32),
    /// `GetManaPoint()` and `GetMaxManaPoint()`.
    pub mp: (i32, i32),
    /// `m_dwMode`.
    pub mode: u32,
    /// Element of the weapon item of the striking hand.
    pub weapon_element: ElementItem,
    /// Element of the suit item.
    pub suit_element: ElementItem,
    /// `Some(party_size / 2)` while the one-shot party critical is armed.
    pub party_crit: Option<i32>,
    /// Standing in the guild-war world.
    pub guild_war_world: bool,
    /// `HasBuff(kind, id)`; `None` = no buffs.
    pub buffs: Option<&'a dyn BuffQuery>,
}

impl<'a> Combatant<'a> {
    /// A view over `stat` with full pools, no flags and no buffs (fixtures set the fields they care about).
    pub fn new(stat: StatInputs<'a>, rules: &'a RulesData) -> Combatant<'a> {
        Combatant {
            stat,
            rules,
            flying: false,
            in_dmg_fly: false,
            in_atk4: false,
            hp: (1000, 1000),
            mp: (100, 100),
            mode: 0,
            weapon_element: ElementItem::default(),
            suit_element: ElementItem::default(),
            party_crit: None,
            guild_war_world: false,
            buffs: None,
        }
    }
}

impl CombatantView for Combatant<'_> {
    fn stat(&self) -> &StatInputs<'_> {
        &self.stat
    }
    fn rules(&self) -> &RulesData {
        self.rules
    }
    fn is_flying(&self) -> bool {
        self.flying
    }
    fn in_dmg_fly(&self) -> bool {
        self.in_dmg_fly
    }
    fn in_atk4_state(&self) -> bool {
        self.in_atk4
    }
    fn hp(&self) -> i32 {
        self.hp.0
    }
    fn max_hp(&self) -> i32 {
        self.hp.1
    }
    fn mp(&self) -> i32 {
        self.mp.0
    }
    fn max_mp(&self) -> i32 {
        self.mp.1
    }
    fn mode(&self) -> u32 {
        self.mode
    }
    fn has_buff(&self, kind: u16, id: u32) -> bool {
        self.buffs.is_some_and(|b| b.has_buff(kind, id))
    }
    fn weapon_element(&self) -> ElementItem {
        self.weapon_element
    }
    fn suit_element(&self) -> ElementItem {
        self.suit_element
    }
    fn party_crit(&self) -> Option<i32> {
        self.party_crit
    }
    fn in_guild_war_world(&self) -> bool {
        self.guild_war_world
    }
}

/// `BUFF_SKILL` (a skill buff in `CBuffMgr`); the only buff kind the formulas query.
pub const BUFF_SKILL: u16 = 1;
