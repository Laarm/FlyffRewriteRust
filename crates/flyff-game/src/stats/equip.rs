//! The equipment rule engine and the transactional slot planner (Plan/16 §13, WP-MOVER-11; F-131, VR-088,
//! PL-077; ref/10 §14; src:_Common/MoverEquip.cpp:459-1046, 1478-1856, 2135-2334, 2591-2645).
//!
//! Doc 19's `DoEquipReq` handler is the only caller: after its own checks it asks [`plan_equip`] (item in the
//! bag) or [`plan_unequip`] (item worn); `Err(EquipDenied)` changes nothing; `Ok(plan)` goes to [`commit`]
//! exactly once. There is no toggle function. The container moves are `ItemContainer::{do_equip, un_equip}`
//! (19 §3.2); this module owns the order of the moves, the DST sources (`Equip{part}`, `Ultimate{part}`),
//! `equip_info`, the cached defense numbers and the `DoEquipNtf` messages.
//!
//! The planner is a FIX over the original (VR-088, PL-077): the number of distinct parts an equip displaces
//! must fit into the free bag slots **before** anything changes, and a two-handed weapon also removes the
//! left-hand weapon.
//!
//! The commercial element stones of worn items start and shorten their SM timers here
//! (`stats::sm::{stone_equip, stone_unequip}`, WP-ITEM-21), a worn item with `DST_GIFTBOX` in a prop slot holds a
//! `BUFF_EQUIP` buff for as long as it is worn (WP-MOVER-15), and the gems of an ultimate right-hand weapon are
//! the `Ultimate{part}` source ([`ultimate_dsts`]).
//!
//! Deliberate gaps (hooks for later work packages, each answers "nothing"): the card DSTs of normal piercing
//! (WP-ITEM-13/14), the sim messages of a ride (`OBJMSG_MODE_FLY` / `MODE_GROUND`) and of unequipping a weapon in
//! combat (`MODE_PEACE`): those need the sim environment of the movement pipeline, so [`CommitReport`] tells the
//! caller what to do.

use super::dst::DstSource;
use super::out::Out;
use super::source::{apply_source, remove_source};
use crate::item::container::StorageId;
use crate::item::env::ItemEnv;
use crate::item::model::Item;
use crate::mover::{EquipInfo, Mover};
use crate::world::terrain::HeightAttr;
use flyff_common::defs::item::{II_ARM_S_CLO_CLO_BLANK, II_RID_RID_STI_MAGIC01};
use flyff_common::defs::itemkind::{IK1_ARMOR, IK2_ARMOR, IK2_ARMORETC, WEAPON_ULTIMATE};
use flyff_common::defs::neuz::{PARTS_LWEAPON, PARTS_RIDE, PARTS_RWEAPON};
use flyff_common::defs::text::{
    TID_ERROR_NOFLY, TID_GAME_CANNOT_FLY_WITH_PET, TID_GAME_CHAOTIC_NOT_FLY, TID_GAME_LACKSPACE,
    TID_GAME_USEAIRCRAFT, TID_QUEST_DISQUISE_NOTFLY,
};
use flyff_data::GameData;
use flyff_data::props::item::ItemProp;
use flyff_proto::pb;
use flyff_rules::consts::{MAX_HUMAN_PARTS, NULL_CHGPARAM, NULL_ID, dst::*};
use flyff_rules::equip_rules::{
    EquipDenied, Wearer, Worn, check_equip, displaced_parts, equip_target, rweapon_is_bow,
    swaps_left_to_right,
};
use flyff_rules::item::ITEM_FLAG_EXPIRED;
use flyff_rules::sets::element_resist_dsts;
use flyff_rules::stats::WeaponView;
use flyff_sim::types::{OBJSTA_SIT, OBJSTA_STAND};
use smallvec::SmallVec;

/// What the planner needs to know about the world and the player that is not on the mover (the ride rules of
/// `IsEquipAble` step 1 and `IsUnEquipAble`). The handler fills it from the world; the default allows
/// everything.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EquipFacts {
    /// The world allows flying (`.wld` `fly`).
    pub fly_allowed: bool,
    /// The height attribute at the player's position.
    pub terrain: HeightAttr,
    /// `m_nFlightLv` (rides need `flight_limit`).
    pub flight_level: i32,
    /// A chaotic player whose propensity penalty forbids flying (doc 17).
    pub chaotic_nofly: bool,
}

impl Default for EquipFacts {
    fn default() -> Self {
        EquipFacts {
            fly_allowed: true,
            terrain: HeightAttr::None,
            flight_level: 0,
            chaotic_nofly: false,
        }
    }
}

/// The result of planning: which item moves where (16 §13.3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EquipPlan {
    /// The storage id of the item being equipped or unequipped.
    pub item: StorageId,
    /// `true` = equip, `false` = unequip.
    pub equip: bool,
    /// The part the item goes to (equip) or comes from (unequip).
    pub target: u32,
    /// The parts that are unequipped first, in the legacy order (distinct).
    pub displaced: SmallVec<[u32; 4]>,
    /// Unequipping the right weapon while the left hand holds a weapon: the left weapon moves to the right.
    pub swap_left_to_right: bool,
}

/// What a commit did that the caller must finish (see the module docs).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CommitReport {
    /// A ride was equipped: send `OBJMSG_MODE_FLY(item id)` and clear the destination.
    pub ride_equipped: bool,
    /// A ride was unequipped: send `OBJMSG_MODE_GROUND` and clear the destination.
    pub ride_unequipped: bool,
    /// A weapon was unequipped (the caller sends `OBJMSG_MODE_PEACE` when the player is in combat mode).
    pub weapon_unequipped: bool,
}

/// The props of the 31 worn items.
pub fn worn<'a>(m: &Mover, data: &'a GameData) -> Worn<'a> {
    let mut w = Worn::empty();
    if let Some(p) = m.player() {
        for (part, slot) in w.parts.iter_mut().enumerate() {
            *slot = p
                .items
                .inventory
                .get_equip(part as u32)
                .and_then(|it| data.item(it.item_id));
        }
    }
    w
}

/// The weapon views of the right and left hand for the attack formulas (`GetWeaponItem`).
pub fn weapon_views<'a>(
    m: &Mover,
    data: &'a GameData,
) -> (Option<WeaponView<'a>>, Option<WeaponView<'a>>) {
    let view = |part: u32| {
        let it = m.player()?.items.inventory.get_equip(part)?;
        let prop = data.item(it.item_id)?;
        Some(WeaponView::new(prop, it.upgrade_level, it.is_expired()))
    };
    (view(PARTS_RWEAPON), view(PARTS_LWEAPON))
}

fn eq_info(it: &Item) -> EquipInfo {
    EquipInfo {
        item_id: it.item_id,
        option: it.attr_option(),
        flag: it.flags.0,
    }
}

/// `IsEquipAble(item, bIgnoreLevel)` (`MoverEquip.cpp:1478-1702`): the ride rules, then the pure checks of
/// `flyff_rules::equip_rules::check_equip` (sex, job, level, arrows).
pub fn is_equip_able(
    m: &Mover,
    env: &ItemEnv<'_>,
    item: &Item,
    prop: &ItemProp,
    ignore_level: bool,
    facts: &EquipFacts,
) -> Result<(), EquipDenied> {
    if prop.parts == PARTS_RIDE {
        let limit = if prop.id == II_RID_RID_STI_MAGIC01 {
            0
        } else if prop.flight_limit == NULL_ID {
            1
        } else {
            i32::try_from(prop.flight_limit).unwrap_or(i32::MAX)
        };
        if facts.flight_level < limit {
            return Err(EquipDenied::text(TID_GAME_USEAIRCRAFT));
        }
        if !facts.fly_allowed {
            return Err(EquipDenied::text(TID_ERROR_NOFLY));
        }
        if m.mv.disguise.is_some() {
            return Err(EquipDenied::text(TID_QUEST_DISQUISE_NOTFLY));
        }
        if facts.terrain == HeightAttr::NoFly {
            return Err(EquipDenied::silent());
        }
        if m.sim.act.state.bits() != OBJSTA_STAND {
            return Err(EquipDenied::silent());
        }
        if facts.chaotic_nofly {
            return Err(EquipDenied::text(TID_GAME_CHAOTIC_NOT_FLY));
        }
        let pets = m
            .player()
            .is_some_and(|p| p.items.pet_obj_id.is_some() || p.items.summoned_eat_pet.is_some());
        if pets && !env.cfg().improved_pets {
            // with improved pets the handler dismisses them instead (19 §5.13.2)
            return Err(EquipDenied::text(TID_GAME_CANNOT_FLY_WITH_PET));
        }
    }
    let wearer = Wearer {
        sex: m.player().map_or(0, |p| u32::from(p.appearance.sex)),
        job: m.mv.job,
        level: m.mv.level,
    };
    let w = worn(m, env.data());
    check_equip(
        &env.stats().rules,
        &wearer,
        prop,
        item.random_option,
        &prop.name,
        rweapon_is_bow(&w),
        ignore_level,
    )
}

/// `IsUnEquipAble`: a ride cannot be taken off while sitting or on no-fly / no-walk / no-move terrain.
pub fn is_unequip_able(m: &Mover, prop: &ItemProp, facts: &EquipFacts) -> bool {
    if prop.parts == PARTS_RIDE {
        let st = m.sim.act.state;
        if st.move_() == OBJSTA_SIT {
            return false;
        }
        if matches!(
            facts.terrain,
            HeightAttr::NoFly | HeightAttr::NoWalk | HeightAttr::NoMove
        ) {
            return false;
        }
    }
    true
}

/// Plans the equip of the bag item `id` (16 §13.2-§13.3): the dead and attacking player is refused silently, a
/// second or expired ride is refused, then `is_equip_able`, the target and displaced parts, and the bag-space
/// transaction (VR-088): more displaced parts than free bag slots gives `TID_GAME_LACKSPACE` and nothing changes.
pub fn plan_equip(
    m: &Mover,
    env: &ItemEnv<'_>,
    id: StorageId,
    facts: &EquipFacts,
) -> Result<EquipPlan, EquipDenied> {
    let p = m.player().ok_or_else(EquipDenied::silent)?;
    if m.sim.act.state.is_die() || m.sim.act.state.is_act_attack() || m.mv.hp <= 0 {
        return Err(EquipDenied::silent());
    }
    let item = p
        .items
        .inventory
        .get_at_id(id)
        .ok_or_else(EquipDenied::silent)?;
    if p.items.inventory.is_equip(id) {
        return Err(EquipDenied::silent());
    }
    let prop = env
        .data()
        .item(item.item_id)
        .ok_or_else(EquipDenied::silent)?;
    if prop.parts == PARTS_RIDE
        && (p.items.inventory.get_equip(PARTS_RIDE).is_some() || item.is_expired())
    {
        return Err(EquipDenied::silent());
    }
    is_equip_able(m, env, item, prop, false, facts)?;
    let w = worn(m, env.data());
    let target = equip_target(m.mv.job, prop, &w).ok_or_else(EquipDenied::silent)?;
    let displaced = displaced_parts(prop, target, &w);
    if displaced.len() > p.items.inventory.empty_count() {
        return Err(EquipDenied::text(TID_GAME_LACKSPACE));
    }
    Ok(EquipPlan {
        item: id,
        equip: true,
        target,
        displaced,
        swap_left_to_right: false,
    })
}

/// Plans the unequip of the item worn in `part` (16 §13.3): `is_unequip_able`, a free bag slot (a full bag
/// gives `TID_GAME_LACKSPACE`, KQ-15), the left-to-right swap flag.
pub fn plan_unequip(
    m: &Mover,
    env: &ItemEnv<'_>,
    part: u32,
    facts: &EquipFacts,
) -> Result<EquipPlan, EquipDenied> {
    let p = m.player().ok_or_else(EquipDenied::silent)?;
    if m.sim.act.state.is_die() || m.sim.act.state.is_act_attack() || m.mv.hp <= 0 {
        return Err(EquipDenied::silent());
    }
    let id = p
        .items
        .inventory
        .equip_id(part)
        .ok_or_else(EquipDenied::silent)?;
    let item = p
        .items
        .inventory
        .get_at_id(id)
        .ok_or_else(EquipDenied::silent)?;
    if let Some(prop) = env.data().item(item.item_id)
        && !is_unequip_able(m, prop, facts)
    {
        return Err(EquipDenied::silent());
    }
    if p.items.inventory.empty_count() < 1 {
        return Err(EquipDenied::text(TID_GAME_LACKSPACE));
    }
    let w = worn(m, env.data());
    Ok(EquipPlan {
        item: id,
        equip: false,
        target: part,
        displaced: SmallVec::new(),
        swap_left_to_right: swaps_left_to_right(part, &w),
    })
}

/// `equip_dsts(item)` (`SetDestParamEquip`, 16 §13.4): empty for an expired item, else the prop's slots, the
/// armour element or the prop's resist columns, the accessory bonus of the upgrade level and the random-option
/// lines. The socketed cards are [`equip_dsts_with`]'s (this one reads the repack default
/// `features.no_piercing = true`: gear takes no cards), the ultimate gems [`ultimate_dsts`]'s.
pub fn equip_dsts(item: &Item, prop: &ItemProp, data: &GameData) -> Vec<(u32, i32, i32)> {
    equip_dsts_with(item, prop, data, true)
}

/// [`equip_dsts`] with the switch `features.no_piercing`: when the item takes cards (a suit, a shield or a weapon
/// with the switch off, a vis pet always) the DSTs of its socketed cards are added after the accessory
/// bonus (`DestParamPiercingAvail`, step 5 of `SetDestParamEquip`).
pub fn equip_dsts_with(
    item: &Item,
    prop: &ItemProp,
    data: &GameData,
    no_piercing: bool,
) -> Vec<(u32, i32, i32)> {
    let mut out: Vec<(u32, i32, i32)> = Vec::new();
    if item.is_expired() {
        return out;
    }
    // 2. the prop slots
    for i in 0..3 {
        let dst = prop.dest_param.get(i).copied().unwrap_or(NULL_ID);
        if dst != NULL_ID {
            out.push((
                dst,
                prop.adj_param_val.get(i).copied().unwrap_or(0),
                prop.chg_param_val.get(i).copied().unwrap_or(0) as i32,
            ));
        }
    }
    // 3. armour element, else the prop's resist columns
    if item.element_level != 0 && prop.item_kind1 == IK1_ARMOR {
        if item.element as u8 != 0 {
            for (dst, adj) in element_resist_dsts(item.element as u8, item.element_level) {
                out.push((dst, adj, NULL_CHGPARAM));
            }
        }
    } else {
        for (dst, v) in [
            (DST_RESIST_ELECTRICITY, prop.item_resist_electricity),
            (DST_RESIST_FIRE, prop.item_resist_fire),
            (DST_RESIST_WIND, prop.item_resist_wind),
            (DST_RESIST_WATER, prop.item_resist_water),
            (DST_RESIST_EARTH, prop.item_resist_earth),
        ] {
            if v != 0 {
                out.push((dst, v, NULL_CHGPARAM));
            }
        }
    }
    // 4. accessory
    out.extend(flyff_rules::sets::accessory_dsts(
        &data.accessory,
        item.item_id,
        item.upgrade_level,
    ));
    // 5. the socketed cards
    for (dst, adj) in crate::item::piercing::card_dsts(item, prop, data, no_piercing) {
        out.push((dst, adj, NULL_CHGPARAM));
    }
    // 7. random options: the stat-scroll id, then the awakening lines
    let w = item.random_option;
    if w.stat_id() != 0
        && let Some(def) = data
            .item_etc
            .random_opt_items
            .iter()
            .find(|d| d.id == i32::from(w.stat_id()))
    {
        for pair in &def.params {
            out.push((pair[0] as u32, pair[1], NULL_CHGPARAM));
        }
    }
    for line in w.lines() {
        out.push((u32::from(line.dst), i32::from(line.adj), NULL_CHGPARAM));
    }
    out
}

/// `CUltimateWeapon::GetDestParamUltimate` (`UltimateWeapon.cpp:837`): the DSTs the gems of an ultimate weapon
/// (`refer_stat1 == WEAPON_ULTIMATE`) add. Each distinct gem counts once per socket (up to the first empty
/// one), the gem's `refer_stat2` names the DST and the ability table gives the value for the number of equal
/// gems (`values[n - 1]`); gems of the same DST add up. In DST order; zero values are dropped (an expired
/// weapon gives nothing, like `equip_dsts`: a deviation, the original kept the gem stats of an expired weapon).
pub fn ultimate_dsts(item: &Item, prop: &ItemProp, data: &GameData) -> Vec<(u32, i32, i32)> {
    if prop.refer_stat1 != WEAPON_ULTIMATE || item.is_expired() {
        return Vec::new();
    }
    let mut count: std::collections::BTreeMap<u32, usize> = std::collections::BTreeMap::new();
    for &gem in &item.piercing.ultimate_gems {
        if gem == 0 {
            break;
        }
        *count.entry(gem).or_insert(0) += 1;
    }
    let mut dsts: std::collections::BTreeMap<u32, i32> = std::collections::BTreeMap::new();
    for (gem, n) in count {
        let Some(gem_prop) = data.item(gem) else {
            continue;
        };
        let dst = gem_prop.refer_stat2;
        let value = data
            .ultimate
            .gem_ability
            .iter()
            .find(|a| a.dst == dst)
            .and_then(|a| a.rows.iter().find(|r| r.gem06 == gem))
            .and_then(|r| r.values.get(n - 1).copied())
            .unwrap_or(0);
        *dsts.entry(dst).or_insert(0) += value;
    }
    dsts.into_iter()
        .filter(|&(_, v)| v != 0)
        .map(|(dst, v)| (dst, v, NULL_CHGPARAM))
        .collect()
}

/// `true` when a prop slot of the item is `DST_GIFTBOX`: wearing it holds a `BUFF_EQUIP` buff.
fn holds_giftbox_buff(prop: &ItemProp) -> bool {
    prop.dest_param.iter().take(3).any(|&d| d == DST_GIFTBOX)
}

/// `AddBuff(BUFF_EQUIP, item id, 1, 999999999)` of a worn `DST_GIFTBOX` item (`SetDestParamEquip`); an expired
/// item adds nothing.
fn equip_buff_on(m: &mut Mover, env: &ItemEnv<'_>, item: &Item, prop: &ItemProp, out: &mut Out) {
    if !holds_giftbox_buff(prop) || item.is_expired() {
        return;
    }
    let cx = crate::buff::BuffCx::new(*env, flyff_common::GameMs(0));
    let mut fx = crate::item::fx::Fx::new();
    crate::buff::core::add(
        m,
        &cx,
        crate::buff::set::BuffKind::Equip,
        prop.id,
        1,
        flyff_rules::buff::ITEM_BUFF_PERMANENT,
        None,
        &mut fx,
    );
    for o in fx.out.take() {
        out.push(o.aud, o.body);
    }
}

/// `RemoveBuff(BUFF_EQUIP, item id)` of an item that leaves the body (`ResetDestParamEquip`).
fn equip_buff_off(m: &mut Mover, env: &ItemEnv<'_>, prop: &ItemProp, out: &mut Out) {
    if !holds_giftbox_buff(prop) {
        return;
    }
    let cx = crate::buff::BuffCx::new(*env, flyff_common::GameMs(0));
    let mut fx = crate::item::fx::Fx::new();
    crate::buff::core::remove_fake(
        m,
        &cx,
        crate::buff::set::BuffKey::new(crate::buff::set::BuffKind::Equip, prop.id),
        &mut fx,
    );
    for o in fx.out.take() {
        out.push(o.aud, o.body);
    }
}

/// `UpdateParam()` of a player (16 §13.4): `defense_min/max` from the worn armour, `adj_hit_rate` and
/// `adj_parry` from every part, then the sim's copy of the stat-derived inputs (the weapon may have changed).
pub fn update_param(m: &mut Mover, env: &ItemEnv<'_>) {
    let data = env.data();
    let Some(p) = m.player() else { return };
    let mut armour: Vec<WeaponView<'_>> = Vec::new();
    let (mut hit, mut parry) = (0_i32, 0_i32);
    for part in 0..MAX_HUMAN_PARTS as u32 {
        let Some(it) = p.items.inventory.get_equip(part) else {
            continue;
        };
        let Some(prop) = data.item(it.item_id) else {
            continue;
        };
        if prop.item_kind2 == IK2_ARMOR || prop.item_kind2 == IK2_ARMORETC {
            armour.push(WeaponView::new(prop, it.upgrade_level, it.is_expired()));
        }
        hit = hit.wrapping_add(prop.adj_hit_rate.max(0));
        parry = parry.wrapping_add(if (prop.parry as i32) > 0 {
            prop.parry as i32
        } else {
            0
        });
    }
    let (dmin, dmax) =
        flyff_rules::stats::equip_defense_sum(&env.stats().rules, &m.mv.dst.table, armour);
    m.mv.equip_cache.defense_min = dmin;
    m.mv.equip_cache.defense_max = dmax;
    m.mv.equip_cache.adj_hit_rate = hit;
    m.mv.equip_cache.adj_parry = parry;
    super::env::refresh_sim_params(m, env.stats());
}

fn notify(out: &mut Out, id: StorageId, info: EquipInfo, equip: bool, part: u32) {
    out.near(pb::DoEquipNtf {
        obj_id: u32::from(id),
        id_guild: 0,
        equip,
        info: Some(pb::EquipInfo {
            item_id: info.item_id,
            option: info.option,
            flag: u32::from(info.flag),
        }),
        part: part as i32,
    });
}

/// Takes the item in `part` off for real: the bag move, the sources, `equip_info`, the notification. `false`
/// (nothing changed) when the bag has no free slot or nothing is worn there.
fn unequip_one(m: &mut Mover, env: &ItemEnv<'_>, part: u32, out: &mut Out) -> bool {
    let Some(p) = m.player_mut() else {
        return false;
    };
    let Some(id) = p.items.inventory.equip_id(part) else {
        return false;
    };
    let Some(info) = p.items.inventory.get_at_id(id).map(eq_info) else {
        return false;
    };
    if !p.items.inventory.un_equip(part) {
        return false;
    }
    drop_part_state(m, env, part, id, out);
    notify(out, id, info, false, part);
    true
}

/// The state that follows a part being emptied: its DST sources, the `DST_GIFTBOX` buff, the resist stone of the
/// item (`DoSMItemUnEquip`) and `equip_info`. `id` is the item that was worn (in the bag already or about to be
/// destroyed).
fn drop_part_state(m: &mut Mover, env: &ItemEnv<'_>, part: u32, id: StorageId, out: &mut Out) {
    let stats = env.stats();
    let part8 = part as u8;
    remove_source(m, stats, DstSource::Equip { part: part8 }, true, out);
    let prop_id = m
        .player()
        .and_then(|p| p.items.inventory.get_at_id(id))
        .map(|it| it.item_id);
    if let Some(prop) = prop_id.and_then(|i| env.data().item(i)) {
        equip_buff_off(m, env, prop, out);
    }
    super::sm::stone_unequip(m, id, part, out);
    if part == PARTS_RWEAPON || part == PARTS_LWEAPON {
        remove_source(m, stats, DstSource::Ultimate { part: part8 }, true, out);
    }
    if let Some(slot) = m.mv.equip_info.get_mut(part as usize) {
        *slot = EquipInfo::EMPTY;
    }
}

/// The left weapon becomes the right one (`swap_left_to_right`): the slots exchange their items, the sources
/// are re-keyed (same values), `equip_info` follows.
fn swap_left_over(m: &mut Mover) {
    let Some(p) = m.player_mut() else { return };
    let inv = &mut p.items.inventory;
    // after a real unequip the right slot is empty (the left item moves over); on the removal path the doomed
    // item still sits there (the two exchange and the caller deletes the one in the left slot)
    let moved = if inv.equip_id(PARTS_RWEAPON).is_none() {
        inv.move_equip(PARTS_LWEAPON, PARTS_RWEAPON)
    } else {
        inv.swap_equip(PARTS_RWEAPON, PARTS_LWEAPON)
    };
    if moved {
        m.mv.equip_info
            .swap(PARTS_RWEAPON as usize, PARTS_LWEAPON as usize);
        m.mv.dst.rekey(
            DstSource::Equip {
                part: PARTS_LWEAPON as u8,
            },
            DstSource::Equip {
                part: PARTS_RWEAPON as u8,
            },
        );
        m.mv.dst.rekey(
            DstSource::Ultimate {
                part: PARTS_LWEAPON as u8,
            },
            DstSource::Ultimate {
                part: PARTS_RWEAPON as u8,
            },
        );
    }
}

/// Executes a plan (16 §13.3 commit, in the normative order): the displaced parts come off first, then the
/// item goes on (or comes off), then the sets, `update_param` and the notification. Panic-free: a container
/// move that cannot happen (the planner excluded it) skips its step.
pub fn commit(m: &mut Mover, env: &ItemEnv<'_>, plan: &EquipPlan, out: &mut Out) -> CommitReport {
    let mut report = CommitReport::default();
    let data = env.data();
    let stats = env.stats();
    if plan.equip {
        for &part in &plan.displaced {
            let was_weapon = m
                .player()
                .and_then(|p| p.items.inventory.get_equip(part))
                .and_then(|it| data.item(it.item_id))
                .is_some_and(|pr| pr.item_kind1 == flyff_common::defs::itemkind::IK1_WEAPON);
            let was_ride = part == PARTS_RIDE;
            if unequip_one(m, env, part, out) {
                report.weapon_unequipped |= was_weapon;
                report.ride_unequipped |= was_ride;
            }
        }
        let Some(p) = m.player_mut() else {
            return report;
        };
        let Some(slot) = p.items.inventory.slot_of(plan.item) else {
            return report;
        };
        if !p.items.inventory.do_equip(slot, plan.target) {
            tracing::error!(
                item = plan.item,
                part = plan.target,
                "equip: the container refused a planned equip"
            );
            return report;
        }
        let Some(item) = p.items.inventory.get_at_id(plan.item).cloned() else {
            return report;
        };
        let part8 = plan.target as u8;
        if let Some(e) = m.mv.equip_info.get_mut(plan.target as usize) {
            *e = eq_info(&item);
        }
        if let Some(prop) = data.item(item.item_id) {
            let dsts = equip_dsts_with(&item, prop, data, env.cfg().no_piercing);
            apply_source(m, stats, DstSource::Equip { part: part8 }, &dsts, true, out);
            equip_buff_on(m, env, &item, prop, out);
            if prop.parts == PARTS_RIDE {
                report.ride_equipped = true;
            }
            super::sm::stone_equip(m, env, plan.item, plan.target, out);
            if plan.target == PARTS_RWEAPON {
                let ult = ultimate_dsts(&item, prop, data);
                if !ult.is_empty() {
                    apply_source(
                        m,
                        stats,
                        DstSource::Ultimate { part: part8 },
                        &ult,
                        true,
                        out,
                    );
                }
            }
        }
        if item.item_id == II_ARM_S_CLO_CLO_BLANK
            && let Some(p) = m.player_mut()
        {
            p.social.guild_cloak = item.guild_id;
        }
        refresh_sets(m, env, out);
        update_param(m, env);
        super::points::stats_changed(m, stats);
        notify(out, plan.item, eq_info(&item), true, plan.target);
    } else {
        let Some(info) = m
            .player()
            .and_then(|p| p.items.inventory.get_at_id(plan.item))
            .map(eq_info)
        else {
            return report;
        };
        let prop = data.item(info.item_id);
        let was_weapon =
            prop.is_some_and(|pr| pr.item_kind1 == flyff_common::defs::itemkind::IK1_WEAPON);
        if !unequip_one(m, env, plan.target, out) {
            return report;
        }
        report.weapon_unequipped = was_weapon;
        report.ride_unequipped = plan.target == PARTS_RIDE;
        if plan.swap_left_to_right {
            swap_left_over(m);
        }
        refresh_sets(m, env, out);
        update_param(m, env);
        super::points::stats_changed(m, stats);
    }
    report
}

/// `unequip_for_removal(part)` (16 §13.3): the item worn in `part` is about to be destroyed by
/// `remove_item`. Never fails and needs no bag space: the sources go, `equip_info` empties, a left weapon
/// takes over the right hand, one `DoEquipNtf` (unequip) is queued; the caller deletes the item afterwards.
pub fn unequip_for_removal(m: &mut Mover, env: &ItemEnv<'_>, part: u32, out: &mut Out) {
    let Some(p) = m.player() else { return };
    let Some(id) = p.items.inventory.equip_id(part) else {
        return;
    };
    let Some(info) = p.items.inventory.get_at_id(id).map(eq_info) else {
        return;
    };
    let w = worn(m, env.data());
    let swap = swaps_left_to_right(part, &w);
    drop_part_state(m, env, part, id, out);
    notify(out, id, info, false, part);
    if swap {
        swap_left_over(m);
    }
    refresh_sets(m, env, out);
    update_param(m, env);
    super::points::stats_changed(m, env.stats());
}

/// `reset_dest_param_equip(part)` (19 §5.15 expiry): the stats of the worn item go, the item stays equipped
/// (KQ-15: expired gear stays on without stats); no `DoEquipNtf`.
pub fn reset_dest_param_equip(m: &mut Mover, env: &ItemEnv<'_>, part: u32, out: &mut Out) {
    let stats = env.stats();
    remove_source(m, stats, DstSource::Equip { part: part as u8 }, true, out);
    let prop_id = m
        .player()
        .and_then(|p| p.items.inventory.get_equip(part))
        .map(|it| it.item_id);
    if let Some(prop) = prop_id.and_then(|i| env.data().item(i)) {
        equip_buff_off(m, env, prop, out);
    }
    remove_source(
        m,
        stats,
        DstSource::Ultimate { part: part as u8 },
        true,
        out,
    );
    refresh_sets(m, env, out);
    update_param(m, env);
    super::points::stats_changed(m, stats);
}

/// Re-applies the stats of the worn item in `part` (after an upgrade changed it, 19 §4.1 `UI_AO` etc.).
pub fn reapply_part(m: &mut Mover, env: &ItemEnv<'_>, part: u32, out: &mut Out) {
    let stats = env.stats();
    let Some(item) = m
        .player()
        .and_then(|p| p.items.inventory.get_equip(part))
        .cloned()
    else {
        return;
    };
    let Some(prop) = env.data().item(item.item_id) else {
        return;
    };
    let dsts = equip_dsts_with(&item, prop, env.data(), env.cfg().no_piercing);
    super::source::replace_source(
        m,
        stats,
        DstSource::Equip { part: part as u8 },
        &dsts,
        true,
        out,
    );
    if part == PARTS_RWEAPON {
        let ult = ultimate_dsts(&item, prop, env.data());
        super::source::replace_source(
            m,
            stats,
            DstSource::Ultimate { part: part as u8 },
            &ult,
            true,
            out,
        );
    }
    if item.is_expired() {
        equip_buff_off(m, env, prop, out);
    }
    if let Some(e) = m.mv.equip_info.get_mut(part as usize) {
        *e = eq_info(&item);
    }
    refresh_sets(m, env, out);
    update_param(m, env);
}

/// `refresh_sets`: the set bonuses follow the worn items (armour +N and the named sets of `propItemEtc.inc`,
/// 16 §13.5). Sources whose membership did not change keep their ledger.
pub fn refresh_sets(m: &mut Mover, env: &ItemEnv<'_>, out: &mut Out) {
    super::equip_sets::refresh(m, env, out);
}

/// `redo_equip(send)` at join (16 §13.6): validates every worn part against its prop, rebuilds `equip_info`
/// and the sources. `send = false` suppresses the DST messages (the spawn snapshot carries the result).
pub fn redo_equip(m: &mut Mover, env: &ItemEnv<'_>, send: bool, out: &mut Out) {
    let data = env.data();
    let stats = env.stats();
    for part in 0..MAX_HUMAN_PARTS as u32 {
        let Some(item) = m
            .player()
            .and_then(|p| p.items.inventory.get_equip(part))
            .cloned()
        else {
            if let Some(e) = m.mv.equip_info.get_mut(part as usize) {
                *e = EquipInfo::EMPTY;
            }
            continue;
        };
        let Some(prop) = data.item(item.item_id) else {
            continue;
        };
        let fits = prop.parts == part
            || prop.partsub == part
            || (prop.parts == PARTS_RWEAPON && part == PARTS_LWEAPON);
        if !fits {
            if let Some(e) = m.mv.equip_info.get_mut(part as usize) {
                *e = EquipInfo::EMPTY;
            }
            continue;
        }
        if let Some(e) = m.mv.equip_info.get_mut(part as usize) {
            *e = eq_info(&item);
        }
        if prop.parts == PARTS_RIDE {
            // the fly mode is the join driver's (doc 31); the item stays
        }
        if item.item_id == II_ARM_S_CLO_CLO_BLANK
            && let Some(p) = m.player_mut()
        {
            p.social.guild_cloak = item.guild_id;
        }
        let dsts = equip_dsts_with(&item, prop, data, env.cfg().no_piercing);
        apply_source(
            m,
            stats,
            DstSource::Equip { part: part as u8 },
            &dsts,
            send,
            out,
        );
        equip_buff_on(m, env, &item, prop, out);
        if part == PARTS_RWEAPON {
            let ult = ultimate_dsts(&item, prop, data);
            if !ult.is_empty() {
                apply_source(
                    m,
                    stats,
                    DstSource::Ultimate { part: part as u8 },
                    &ult,
                    send,
                    out,
                );
            }
        }
    }
    refresh_sets(m, env, out);
    update_param(m, env);
}

/// `invalid_equip_off` (16 §13.6): unequips every worn item `is_equip_able(item, ignore_level = true)` rejects
/// (a sex change, a job reset); a full bag leaves the item on (KEEP). Returns the parts that came off.
pub fn invalid_equip_off(
    m: &mut Mover,
    env: &ItemEnv<'_>,
    facts: &EquipFacts,
    out: &mut Out,
) -> Vec<u32> {
    let mut off = Vec::new();
    for part in 0..MAX_HUMAN_PARTS as u32 {
        let Some(item) = m
            .player()
            .and_then(|p| p.items.inventory.get_equip(part))
            .cloned()
        else {
            continue;
        };
        let Some(prop) = env.data().item(item.item_id) else {
            continue;
        };
        if is_equip_able(m, env, &item, prop, true, facts).is_err()
            && unequip_one(m, env, part, out)
        {
            off.push(part);
        }
    }
    if !off.is_empty() {
        refresh_sets(m, env, out);
        update_param(m, env);
    }
    off
}

/// `true` when the item is worn (or equipped item flags matter): the expiry flag value, exported for the
/// expiry code.
pub const EXPIRED_FLAG: u8 = ITEM_FLAG_EXPIRED;
