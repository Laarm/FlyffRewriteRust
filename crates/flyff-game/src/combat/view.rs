//! The adapter from a [`Mover`] to the formula view (`flyff_rules::combat::Combatant`, Plan/17 §5;
//! WP-COMBAT-04): the stat inputs of `stats::env`, the weapons of the striking hand, the elements of the worn
//! weapon and suit, the pools, the action-state facts and the buffs.
//!
//! [`worn_item`] is the **one** place the combat code reads the item model of doc 19; every weapon, element and
//! expiry fact comes through it.

use crate::buff::BuffSet;
use crate::mover::Mover;
use crate::stats::env::{StatsCore, inputs, max_hp, max_mp, point_param};
use flyff_data::GameData;
use flyff_data::props::item::ItemProp;
use flyff_rules::combat::{
    AtkFlags, AttackInfo, BuffQuery, Combatant, DamageParam, ElementItem, Hand, SkillRef,
};
use flyff_rules::consts::dst::{DST_HP, DST_MP};
use flyff_rules::consts::parts::{PARTS_LWEAPON, PARTS_RWEAPON, PARTS_UPPER_BODY};
use flyff_rules::consts::{NULL_ID, OBJTYPE_FLYING};
use flyff_rules::stats::WeaponView;
use flyff_sim::types::OBJSTA_ATK4;

impl BuffQuery for BuffSet {
    fn has_buff(&self, kind: u16, id: u32) -> bool {
        self.has(kind, id)
    }
}

/// An equipped item as the formulas need it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WornItem {
    /// The item id.
    pub item_id: u32,
    /// `GetAbilityOption()`: the upgrade level.
    pub ability_option: i32,
    /// `CItemElem::expired`.
    pub expired: bool,
    /// `m_bItemResist`.
    pub element: u32,
    /// `m_nResistAbilityOption`.
    pub element_level: i32,
}

/// The item worn in equipment part `part` (`m_Inventory.GetEquip(part)`) of a player; `None` for an empty part
/// and for NPCs.
pub fn worn_item(m: &Mover, part: u32) -> Option<WornItem> {
    let it = m.player()?.items.inventory.get_equip(part)?;
    Some(WornItem {
        item_id: it.item_id,
        ability_option: it.upgrade_level,
        expired: it.is_expired(),
        element: it.element as u32,
        element_level: it.element_level,
    })
}

fn element_of(w: Option<WornItem>) -> ElementItem {
    w.map_or(ElementItem::default(), |w| ElementItem {
        element: w.element,
        level: w.element_level,
    })
}

/// The `WeaponView` of a worn item whose prop is in `data`.
pub fn weapon_view<'a>(data: &'a GameData, w: WornItem) -> Option<WeaponView<'a>> {
    Some(WeaponView::new(
        data.item(w.item_id)?,
        w.ability_option,
        w.expired,
    ))
}

/// The attack item of an NPC hit (`GetHitMinMax`): `info.param >> 16` when that names an item (not 0, not
/// `NULL_ID`), else the prop's `dwAtk1`; `None` when neither resolves.
pub fn npc_attack_item<'a>(
    m: &Mover,
    core: &'a StatsCore,
    param: DamageParam,
) -> Option<&'a ItemProp> {
    let prop = core.data.mover(m.prop_id())?;
    let id = param.item_id().unwrap_or(prop.atk1);
    if id == NULL_ID {
        return None;
    }
    core.data.item(id)
}

/// The skill rows an attack names (`GetSkillProp(skill)`, `GetAddSkillProp(sub_define, level)`), resolved
/// through the game data; empty for an attack that is not a skill attack.
pub fn skill_ref<'a>(core: &'a StatsCore, flags: AtkFlags, param: DamageParam) -> SkillRef<'a> {
    let id = param.skill(flags);
    if id == 0 {
        return SkillRef::default();
    }
    let prop = core.data.skill(id);
    let add = prop.and_then(|p| core.data.add_skill(p.sub_define, param.skill_level()));
    SkillRef { prop, add }
}

/// A fresh `ATTACK_INFO` for `flags` / `param` with the skill rows and the NPC attack item of `att` resolved.
pub fn attack_info<'a>(
    att: &Mover,
    core: &'a StatsCore,
    flags: AtkFlags,
    param: DamageParam,
) -> AttackInfo<'a> {
    let mut info = AttackInfo::new(flags, param);
    info.skill = skill_ref(core, flags, param);
    if !att.is_player() {
        info.npc_item = npc_attack_item(att, core, param);
    }
    info
}

/// The formula view of `m` for the striking `hand` (see the trait docs: the hand's item is the stat inputs'
/// `weapon`, the other hand's item `left_weapon`). NPCs strike with their virtual item (`m_dwVirtItem`).
/// `party_crit` is the armed one-shot Sphere Circle bonus, from the party service.
pub fn combatant<'a>(
    m: &'a Mover,
    core: &'a StatsCore,
    hand: Hand,
    party_crit: Option<i32>,
) -> Combatant<'a> {
    let data: &GameData = &core.data;
    let right = worn_item(m, PARTS_RWEAPON);
    let left = worn_item(m, PARTS_LWEAPON);
    let (strike, other) = match hand {
        Hand::Right => (right, left),
        Hand::Left => (left, right),
    };
    let (weapon, left_weapon) = if m.is_player() {
        (
            strike.and_then(|w| weapon_view(data, w)),
            other.and_then(|w| weapon_view(data, w)),
        )
    } else {
        (
            data.item(m.mv.virt_item)
                .map(|p| WeaponView::new(p, 0, false)),
            None,
        )
    };
    let stat = inputs(m, core, weapon, left_weapon);
    let mut c = Combatant::new(stat, &core.rules);
    c.flying = m.sim.act.flags.fly() || m.mv.type_flags & OBJTYPE_FLYING != 0;
    c.in_dmg_fly = m.sim.act.state.dmg_fly_all();
    c.in_atk4 = m.sim.act.state.bits() & OBJSTA_ATK4 != 0;
    c.hp = (point_param(m, core, DST_HP), max_hp(m, core));
    c.mp = (point_param(m, core, DST_MP), max_mp(m, core));
    c.mode = m.mv.mode;
    c.weapon_element = element_of(strike);
    c.suit_element = element_of(worn_item(m, PARTS_UPPER_BODY));
    c.party_crit = party_crit;
    c.guild_war_world = m.core.world.0 == flyff_common::defs::world::WI_WORLD_GUILDWAR;
    c.buffs = Some(&m.buffs);
    c
}

/// `GetActiveHandItemProp()` of `m` (the right hand): a player's worn weapon or the bare-hand item, an NPC's
/// virtual item.
pub fn active_hand_prop<'a>(m: &Mover, core: &'a StatsCore) -> Option<&'a ItemProp> {
    if m.is_player() {
        match worn_item(m, PARTS_RWEAPON) {
            Some(w) => core.data.item(w.item_id),
            None => core.rules.hand.as_ref(),
        }
    } else {
        core.data.item(m.mv.virt_item)
    }
}
