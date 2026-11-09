//! The mover-level buff operations (Plan/17 §7.2-§7.6; `src:_Common/buff.cpp`, `moverbuff.cpp`):
//! add / refresh / replace with the 28 cap, the processing pass, the release with the exact DST revert, the
//! removal helpers and the weapon invalidation.
//!
//! Everything here works on one `&mut Mover`, the [`BuffCx`] (game data, stats environment, clocks) and the
//! [`Fx`] it fills with messages and [`Followup`]s; the service-level API of `buff::api` resolves the movers and
//! flushes. The effects of a buff are DST operations under the source `DstSource::Buff { kind, id }`, so the
//! release reverts exactly what the buff applied (PL-080).

use super::set::{AddPlan, AddRequest, BuffEntry, BuffKey, BuffKind};
use crate::combat::SimFx;
use crate::follow::Followup;
use crate::item::env::{ItemConfig, ItemEnv};
use crate::item::fx::Fx;
use crate::mover::Mover;
use crate::skill::apply::{CasterFacts, apply_param};
use crate::stats::dst::DstSource;
use crate::stats::env::StatsCore;
use crate::stats::source::{apply_dst, remove_source};
use flyff_common::defs::item::{
    II_SYS_SYS_SCR_PET_FEED_POCKET02, II_SYS_SYS_SCR_PET_TONIC_A, II_SYS_SYS_SCR_PET_TONIC_B,
};
use flyff_common::defs::itemkind::{IK1_HOUSING, IK2_BUFF_TOGIFT};
use flyff_common::defs::neuz::{PARTS_LWEAPON, PARTS_RWEAPON, PARTS_SHIELD};
use flyff_common::defs::obj::XI_SKILL_ELE_FIRE_BURINGFIELD02;
use flyff_common::defs::skill::{SI_ACR_YOYO_COUNTER, SI_GEN_BURN, SI_RIG_MASTER_BLESSING};
use flyff_common::{GameMs, ObjId};
use flyff_data::GameData;
use flyff_data::props::item::ItemProp;
use flyff_data::props::skill_add::AddSkillProp;
use flyff_proto::pb;
use flyff_rules::buff::{BUFF_LOAD_CAP_MS, HandView, ITEM_BUFF_PERMANENT, skill_buff_invalid};
use flyff_rules::consts::dst::*;
use flyff_rules::consts::{NULL_CHGPARAM, NULL_ID};

/// What the buff functions read besides the mover: the item environment (game data, stats environment,
/// switches, the wall clock) and the tick's game clock.
#[derive(Clone, Copy, Debug)]
pub struct BuffCx<'a> {
    /// The item environment (its `now` is the wall clock in unix seconds).
    pub env: ItemEnv<'a>,
    /// The game clock of the tick (`timeGetTime()` of the original).
    pub now: GameMs,
}

impl<'a> BuffCx<'a> {
    /// A context.
    pub fn new(env: ItemEnv<'a>, now: GameMs) -> BuffCx<'a> {
        BuffCx { env, now }
    }

    /// The loaded game data.
    pub fn data(&self) -> &'a GameData {
        self.env.data()
    }

    /// The stat functions' environment.
    pub fn stats(&self) -> &'a StatsCore {
        self.env.stats()
    }

    /// The switches.
    pub fn cfg(&self) -> &'a ItemConfig {
        self.env.cfg()
    }

    /// The wall clock in unix seconds.
    pub fn unix(&self) -> i64 {
        self.env.now.0
    }
}

/// `GetSkillProp(&prop, &add, skill, level)`: the skill row and the add-skill row of the level; `None` when
/// either is missing.
pub fn skill_props(data: &GameData, skill: u32, level: u32) -> Option<(&ItemProp, &AddSkillProp)> {
    let prop = data.skill(skill)?;
    let add = data.add_skill(prop.sub_define, level)?;
    Some((prop, add))
}

/// The prop that describes buff `key`: the skill row for a skill buff, the item prop for the item kinds.
pub fn buff_prop(data: &GameData, key: BuffKey) -> Option<&ItemProp> {
    match key.kind {
        BuffKind::Skill => data.skill(u32::from(key.id)),
        BuffKind::Item | BuffKind::Item2 => data.item(u32::from(key.id)),
        BuffKind::Pet | BuffKind::Equip => None,
    }
}

/// `IsGood` of a skill buff (and the `GetFirstBS` test): the skill prop exists and `evildoing >= 0`.
fn is_good_skill(data: &GameData, key: BuffKey) -> bool {
    key.kind == BuffKind::Skill && buff_prop(data, key).is_some_and(|p| p.evildoing >= 0)
}

/// `IsCommon()` (death removal, the 60-minute load cap): skill buffs and the item buffs whose prop is not
/// charged; pet and equip buffs are not.
pub fn is_common(data: &GameData, key: BuffKey) -> bool {
    match key.kind {
        BuffKind::Skill => true,
        BuffKind::Item | BuffKind::Item2 => !buff_prop(data, key).is_some_and(|p| p.charged != 0),
        BuffKind::Pet | BuffKind::Equip => false,
    }
}

/// The two DST slots (`dest_param[0..2]`) a buff's prop drives, for the `CHRSTATE` / `DSTPARAM` matching.
fn skill_slots(data: &GameData, key: BuffKey, level: u32) -> [(u32, i32); 2] {
    match skill_props(data, u32::from(key.id), level) {
        Some((_, add)) => [
            (add.dest_param[0], add.adj_param_val[0] as i32),
            (add.dest_param[1], add.adj_param_val[1] as i32),
        ],
        None => [(NULL_ID, 0); 2],
    }
}

/// `HasChrState(state)`.
fn has_chr_state(data: &GameData, key: BuffKey, entry: &BuffEntry, state: u32) -> bool {
    match key.kind {
        BuffKind::Skill => skill_slots(data, key, entry.level)
            .iter()
            .any(|&(dst, adj)| dst == DST_CHRSTATE && adj as u32 & state != 0),
        BuffKind::Item | BuffKind::Item2 => buff_prop(data, key).is_some_and(|p| {
            (0..2)
                .any(|i| p.dest_param[i] == DST_CHRSTATE && p.adj_param_val[i] as u32 & state != 0)
        }),
        _ => false,
    }
}

/// `IsDebuf()`: hostile skill and item buffs.
fn is_debuf(data: &GameData, key: BuffKey) -> bool {
    matches!(key.kind, BuffKind::Skill | BuffKind::Item | BuffKind::Item2)
        && buff_prop(data, key).is_some_and(|p| p.evildoing < 0)
}

/// The `RBF_*` flags of `RemoveBuffs` (`buff.h:11-19`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RemoveFlags(pub u32);

impl RemoveFlags {
    /// `RBF_COMMON`: every common buff (death).
    pub const COMMON: RemoveFlags = RemoveFlags(0x0000_0001);
    /// `RBF_IK3`: item buffs of the kind in the parameter.
    pub const IK3: RemoveFlags = RemoveFlags(0x0000_0002);
    /// `RBF_GOODSKILL`: beneficial skill buffs.
    pub const GOODSKILL: RemoveFlags = RemoveFlags(0x0000_0004);
    /// `RBF_CHRSTATE`: buffs whose slots carry the state bits of the parameter.
    pub const CHRSTATE: RemoveFlags = RemoveFlags(0x0000_0008);
    /// `RBF_DSTPARAM`: hostile skill buffs with the DST of the parameter.
    pub const DSTPARAM: RemoveFlags = RemoveFlags(0x0000_0010);
    /// `RBF_DEBUF`: hostile buffs.
    pub const DEBUF: RemoveFlags = RemoveFlags(0x0000_0020);
    /// `RBF_ATTACKER`: hostile skill buffs cast by the attacker of the parameter.
    pub const ATTACKER: RemoveFlags = RemoveFlags(0x0000_0040);
    /// `RBF_UNCONDITIONAL`: everything except housing buffs.
    pub const UNCONDITIONAL: RemoveFlags = RemoveFlags(0x0100_0000);
    /// `RBF_ONCE`: stop after the first match.
    pub const ONCE: RemoveFlags = RemoveFlags(0x1000_0000);

    /// `true` when every bit of `other` is set.
    pub fn has(self, other: RemoveFlags) -> bool {
        self.0 & other.0 != 0
    }
}

impl std::ops::BitOr for RemoveFlags {
    type Output = RemoveFlags;
    fn bitor(self, rhs: RemoveFlags) -> RemoveFlags {
        RemoveFlags(self.0 | rhs.0)
    }
}

// ---- the DST source of a buff ---------------------------------------------------------------------------------

/// The ledger source of a buff.
pub fn source_of(key: BuffKey) -> DstSource {
    DstSource::Buff {
        kind: key.kind.as_u8(),
        id: u32::from(key.id),
    }
}

/// `SetSkillState` for `key` with the given values (the audience is everybody around, the buffed mover
/// included).
fn send_state(
    m: &mut Mover,
    cx: &BuffCx<'_>,
    key: BuffKey,
    level: u32,
    total_ms: u32,
    fx: &mut Fx,
) {
    let id = m.id();
    let level = if key.kind == BuffKind::Item2 {
        // the client reads the seconds left, not the absolute time
        i32::try_from(i64::from(level) - cx.unix()).unwrap_or(0)
    } else {
        level as i32
    };
    fx.out.near(pb::SetSkillState {
        target: id.0,
        buff_type: u32::from(key.kind.as_u8()),
        buff_id: u32::from(key.id),
        level,
        total_ms,
    });
}

/// `AddTotal` adds the remaining time for these ids instead of taking the maximum (`CBuffItem::AddTotal`).
fn accumulates(key: BuffKey) -> bool {
    key.kind == BuffKind::Item
        && matches!(
            u32::from(key.id),
            II_SYS_SYS_SCR_PET_FEED_POCKET02
                | II_SYS_SYS_SCR_PET_TONIC_A
                | II_SYS_SYS_SCR_PET_TONIC_B
        )
}

/// `IsAddable` of the skill kind: a hostile skill buff is refused on a player in its invulnerability window.
fn addable(m: &Mover, cx: &BuffCx<'_>, key: BuffKey) -> bool {
    if key.kind != BuffKind::Skill {
        return true;
    }
    !(m.is_player()
        && m.combat.dead_ticks > 0
        && buff_prop(cx.data(), key).is_some_and(|p| p.evildoing < 0))
}

/// `CMover::AddBuff(type, id, level, total, attacker)` (`buff.cpp:662-810`): returns `true` for a **new**
/// buff (the caller applies its effects), `false` for a refusal, a refresh or an ignored lower level. The
/// effects of a replaced or evicted buff are reverted and `RemoveSkillInfluence` goes out for each.
#[allow(clippy::too_many_arguments)]
pub fn add(
    m: &mut Mover,
    cx: &BuffCx<'_>,
    kind: BuffKind,
    id: u32,
    level: u32,
    total_ms: u32,
    attacker: Option<ObjId>,
    fx: &mut Fx,
) -> bool {
    let key = BuffKey::new(kind, id);
    let housing = kind == BuffKind::Item
        && cx
            .data()
            .item(id)
            .is_some_and(|p| p.item_kind1 == IK1_HOUSING);
    let req = AddRequest {
        key,
        level,
        total_ms,
        attacker,
        volatile: housing,
    };
    let plan = m
        .buffs
        .plan_add(&req, addable(m, cx, key), |k| is_good_skill(cx.data(), k));
    match plan {
        AddPlan::Refused => false,
        AddPlan::Refresh => {
            m.buffs.refresh(key, total_ms, accumulates(key), cx.now);
            send_state(m, cx, key, level, total_ms, fx);
            false
        }
        AddPlan::IgnoreLower => {
            send_state(m, cx, key, level, total_ms, fx);
            false
        }
        AddPlan::Insert { replace, evict } => {
            if let Some(k) = replace {
                remove_real(m, cx, k, fx);
            }
            if let Some(k) = evict {
                remove_real(m, cx, k, fx);
            }
            m.buffs.insert(&req, cx.now);
            send_state(m, cx, key, level, total_ms, fx);
            true
        }
    }
}

// ---- applying and releasing the effects -------------------------------------------------------------------------

/// What the first process of a loaded buff (and a freshly added one, through `skill::apply`) applies:
/// `CBuffSkill::Apply`, `CBuffItem::Apply`, `CBuffPet::Apply`. `send = false` is the join / load path (the
/// spawn snapshot carries the result).
pub fn apply_effects(
    m: &mut Mover,
    cx: &BuffCx<'_>,
    key: BuffKey,
    level: u32,
    send: bool,
    fx: &mut Fx,
) {
    let src = source_of(key);
    let data = cx.data();
    match key.kind {
        BuffKind::Skill => {
            if let Some((prop, add)) = skill_props(data, u32::from(key.id), level) {
                let me = CasterFacts::of(m, cx);
                apply_param(m, cx, src, &me, prop, Some(add), send, 0, fx);
            }
        }
        BuffKind::Item => {
            if let Some(prop) = data.item(u32::from(key.id)) {
                let me = CasterFacts::of(m, cx);
                apply_param(m, cx, src, &me, prop, None, send, 0, fx);
            }
        }
        BuffKind::Pet => {
            apply_dst(
                m,
                cx.stats(),
                src,
                (level >> 16, (level & 0xFFFF) as i32, NULL_CHGPARAM),
                false,
                &mut fx.out,
            );
        }
        BuffKind::Item2 | BuffKind::Equip => {}
    }
}

/// `IBuff::Release` (`buff.cpp:94-114, 467-480`): the effects revert, with the extra actions of the kind.
fn release(m: &mut Mover, cx: &BuffCx<'_>, key: BuffKey, level: u32, fx: &mut Fx) {
    let src = source_of(key);
    let stats = cx.stats();
    match key.kind {
        BuffKind::Skill => {
            let skill = u32::from(key.id);
            if skill == SI_ACR_YOYO_COUNTER {
                fx.then(Followup::Sim {
                    mover: m.id(),
                    fx: SimFx::ResetAttack,
                });
            }
            if cx.cfg().new_skill_attribute && skill == SI_RIG_MASTER_BLESSING {
                // the shield shrinks as it absorbs: give back what is left, not what was granted
                let left = m.mv.dst.table.adj(DST_IGNORE_DMG);
                m.mv.dst.set_recorded_total(src, DST_IGNORE_DMG, left);
            }
            remove_source(m, stats, src, true, &mut fx.out);
        }
        BuffKind::Item | BuffKind::Item2 => {
            remove_source(m, stats, src, true, &mut fx.out);
            let Some(prop) = cx.data().item(u32::from(key.id)) else {
                return;
            };
            if let Some(command) = crate::gm::item_text::release_text(prop) {
                if prop.active_skill != NULL_ID {
                    remove_fake(m, cx, BuffKey::new(BuffKind::Skill, prop.active_skill), fx);
                }
                // the registry decides: the whitelist bypasses the authority gate with
                // `fidelity.item_gm_commands`, else the player's own authority applies (23 §5.2 step 3)
                if m.is_player() {
                    fx.then(Followup::ItemCommand {
                        item_id: u32::from(key.id),
                        player: m.id(),
                        text: command,
                    });
                }
            }
            if prop.item_kind2 == IK2_BUFF_TOGIFT && m.is_player() {
                fx.then(Followup::ToGift {
                    player: m.id(),
                    item_id: prop.refer_target1,
                });
            }
        }
        BuffKind::Pet => remove_source(m, stats, src, true, &mut fx.out),
        BuffKind::Equip => {}
    }
    let _ = level;
}

/// `RemoveBuff(pBuff, FALSE)`: release, `RemoveSkillInfluence` to everybody around, erase.
pub fn remove_real(m: &mut Mover, cx: &BuffCx<'_>, key: BuffKey, fx: &mut Fx) {
    let Some(e) = m.buffs.get(key).copied() else {
        return;
    };
    if !e.removed {
        release(m, cx, key, e.level, fx);
    }
    fx.out.near(pb::RemoveSkillInfluence {
        buff_type: u32::from(key.kind.as_u8()),
        buff_id: u32::from(key.id),
    });
    m.buffs.erase(key);
}

/// `RemoveBuff(type, id)` (the default `bFake = TRUE`): the effects revert at once, the entry is erased - and
/// `RemoveSkillInfluence` sent - by the next process.
pub fn remove_fake(m: &mut Mover, cx: &BuffCx<'_>, key: BuffKey, fx: &mut Fx) {
    let Some(e) = m.buffs.get(key).copied() else {
        return;
    };
    if !e.removed {
        release(m, cx, key, e.level, fx);
    }
    m.buffs.mark_removed(key);
}

/// `CBuffMgr::RemoveBuffs(flags, param)` (`buff.cpp:829-860`): the matches are collected in key order and
/// removed (fake removal) from the back.
pub fn remove_buffs(m: &mut Mover, cx: &BuffCx<'_>, flags: RemoveFlags, param: u32, fx: &mut Fx) {
    let data = cx.data();
    let mut hits: Vec<BuffKey> = Vec::new();
    for (key, e) in m.buffs.iter() {
        let housing = key.kind == BuffKind::Item
            && data
                .item(u32::from(key.id))
                .is_some_and(|p| p.item_kind1 == IK1_HOUSING);
        let ik3 = key.kind == BuffKind::Item
            && data
                .item(u32::from(key.id))
                .is_some_and(|p| p.item_kind3 == param);
        let hit = (flags.has(RemoveFlags::UNCONDITIONAL) && !housing)
            || (flags.has(RemoveFlags::COMMON) && is_common(data, key))
            || (flags.has(RemoveFlags::IK3) && ik3)
            || (flags.has(RemoveFlags::GOODSKILL) && is_good_skill(data, key))
            || (flags.has(RemoveFlags::CHRSTATE) && has_chr_state(data, key, e, param))
            || (flags.has(RemoveFlags::DSTPARAM)
                && key.kind == BuffKind::Skill
                && buff_prop(data, key).is_some_and(|p| p.evildoing < 0)
                && skill_slots(data, key, e.level)
                    .iter()
                    .any(|&(dst, _)| dst == param))
            || (flags.has(RemoveFlags::DEBUF) && is_debuf(data, key))
            || (flags.has(RemoveFlags::ATTACKER)
                && key.kind == BuffKind::Skill
                && buff_prop(data, key).is_some_and(|p| p.evildoing < 0)
                && e.attacker.is_some_and(|a| a.0 == param));
        if hit {
            hits.push(key);
            if flags.has(RemoveFlags::ONCE) {
                break;
            }
        }
    }
    for key in hits.into_iter().rev() {
        remove_fake(m, cx, key, fx);
    }
}

// ---- queries ----------------------------------------------------------------------------------------------------

/// `HasBuffByIk3` / `GetBuffByIk3`: the first `BUFF_ITEM` entry (key order) whose prop has `ik3`
/// (`CBuffItem::IsIk3`; `BUFF_ITEM2` entries never match).
pub fn find_ik3(m: &Mover, data: &GameData, ik3: u32) -> Option<BuffKey> {
    m.buffs
        .iter()
        .find(|(key, _)| {
            key.kind == BuffKind::Item
                && data
                    .item(u32::from(key.id))
                    .is_some_and(|p| p.item_kind3 == ik3)
        })
        .map(|(k, _)| k)
}

// ---- processing -------------------------------------------------------------------------------------------------

/// `GetActiveHandItemProp()` for the weapon-bound buffs: the right hand, the shield slot, the dual-wield test
/// and the elements of the weapons. Only players have worn items; a non-player is never invalidated.
fn hand_view(m: &Mover, data: &GameData) -> Option<HandView> {
    let p = m.player()?;
    let inv = &p.items.inventory;
    let right = inv.get_equip(PARTS_RWEAPON);
    let left = inv.get_equip(PARTS_LWEAPON);
    let rp = right.and_then(|it| data.item(it.item_id));
    let shield = inv
        .get_equip(PARTS_SHIELD)
        .and_then(|it| data.item(it.item_id));
    let bare = data.item(flyff_common::defs::item::II_WEA_HAN_HAND);
    let hand = rp.or(bare);
    Some(HandView {
        weapon_type: hand.map_or(NULL_ID, |h| h.weapon_type),
        handed: hand.map_or(NULL_ID, |h| h.handed),
        shield_kind3: shield.map_or(bare.map_or(NULL_ID, |b| b.item_kind3), |s| s.item_kind3),
        dual: right.is_some() && left.is_some(),
        right_element: right.map(|it| it.element != crate::item::model::Element::None),
        left_element: left.map(|it| it.element != crate::item::model::Element::None),
    })
}

/// `IsRemovable`: whether the buff may end by time.
fn removable(cx: &BuffCx<'_>, key: BuffKey, e: &BuffEntry) -> bool {
    let data = cx.data();
    match key.kind {
        BuffKind::Skill => {
            skill_props(data, u32::from(key.id), e.level).is_some_and(|(_, add)| add.skill_time > 0)
        }
        BuffKind::Item => !data
            .item(u32::from(key.id))
            .is_some_and(|p| p.skill_time == ITEM_BUFF_PERMANENT),
        BuffKind::Item2 => true,
        BuffKind::Pet | BuffKind::Equip => false,
    }
}

/// `Timeover`: the time is up (`BUFF_ITEM2` by the wall clock).
fn timeover(cx: &BuffCx<'_>, key: BuffKey, e: &BuffEntry, inst: GameMs) -> bool {
    if key.kind == BuffKind::Item2 {
        return cx.unix() >= i64::from(e.level);
    }
    cx.now.0.saturating_sub(inst.0) >= u64::from(e.total_ms)
}

/// `CBuffSkill::OnExpire`: `SI_GEN_BURN` burns once more when it runs out.
fn on_expire(m: &mut Mover, cx: &BuffCx<'_>, key: BuffKey, e: &BuffEntry, fx: &mut Fx) {
    if key.kind == BuffKind::Skill && u32::from(key.id) == SI_GEN_BURN {
        let (min, max) = cx
            .data()
            .skill(SI_GEN_BURN)
            .map_or((0, 0), |p| (p.ability_min as i32, p.ability_max as i32));
        fx.then(Followup::BurnDamage {
            victim: m.id(),
            attacker: e.attacker,
            min,
            max,
        });
        fx.out.near(pb::CreateSfxObj {
            sfx_id: XI_SKILL_ELE_FIRE_BURINGFIELD02,
            x: 0.0,
            y: 0.0,
            z: 0.0,
            flag: false,
        });
    }
}

/// `CBuffMgr::Process()` (`buff.cpp:811-828`): in key order a removed buff, an invalid one and one whose time
/// is up are collected (a loaded buff applies its effects first and starts its clock); the collected are
/// removed for real from the back.
pub fn process(m: &mut Mover, cx: &BuffCx<'_>, fx: &mut Fx) {
    if m.buffs.is_empty() {
        return;
    }
    let data = cx.data();
    let hand = if m.buffs.iter().any(|(k, _)| k.kind == BuffKind::Skill) {
        hand_view(m, data)
    } else {
        None
    };
    let better_impower = cx.cfg().better_impowerweapon;
    let mut expired: Vec<BuffKey> = Vec::new();
    for key in m.buffs.keys() {
        let Some(e) = m.buffs.get(key).copied() else {
            continue;
        };
        if e.removed {
            expired.push(key);
            continue;
        }
        if key.kind == BuffKind::Skill
            && let Some(h) = &hand
            && skill_buff_invalid(u32::from(key.id), h, better_impower)
        {
            expired.push(key);
            continue;
        }
        let inst = match e.inst {
            Some(i) => i,
            None => {
                apply_effects(m, cx, key, e.level, false, fx);
                if let Some(e) = m.buffs.get_mut(key) {
                    e.inst = Some(cx.now);
                }
                cx.now
            }
        };
        if removable(cx, key, &e) && timeover(cx, key, &e, inst) {
            on_expire(m, cx, key, &e, fx);
            expired.push(key);
        }
    }
    for key in expired.into_iter().rev() {
        remove_real(m, cx, key, fx);
    }
}

/// The load cap of a persisted buff: `IsCommon` and a remaining time of an hour or more (`MIN(60)`).
pub fn capped_remaining(data: &GameData, key: BuffKey, remaining_ms: u32) -> u32 {
    if is_common(data, key) && remaining_ms >= BUFF_LOAD_CAP_MS {
        BUFF_LOAD_CAP_MS
    } else {
        remaining_ms
    }
}
