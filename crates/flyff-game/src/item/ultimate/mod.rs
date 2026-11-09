//! Ultimate weapons and gems (Plan/19 §12.7, WP-ITEM-13; ref/13 §18.8, ref/07c §8; src:_Common/UltimateWeapon.cpp,
//! WORLDSERVER/DPSrvr.cpp:9719-9852).
//!
//! The six handlers `UltimateMakeItem`, `UltimateMakeGem`, `UltimateTransWeapon`, `UltimateSetGem`,
//! `UltimateRemoveGem` and `UltimateEnchantWeapon`, the safe enchant branch of `SmeltSafetyReq`
//! ([`safe_enchant`]) and the gem statistics of an ultimate weapon ([`gem_dst_values`]).
//!
//! **One commit block per request.** Every function first runs all its checks (nothing changes), then takes the
//! materials, rolls and applies the result; the messages and audit lines go to the caller's [`Fx`], which the
//! `GameCtx` wrapper flushes once. Client-named object ids are resolved against the sender's own inventory and
//! an id that is missing, reserved by a trade or a stall, or above the bag is a cancel (VR-010).
//!
//! **Replies.** `Ultimate{sub, result[, num]}` with the results `SUCCESS 0, FAILED 1, CANCEL 2, ISULTIMATE 3,
//! INVENTORY 4, ISNOTULTIMATE 5` (`flyff_rules::item::ultimate`). The enchant and the gem removal also send the
//! legacy text, sound and effect before the reply, as `DPSrvr.cpp` does.
//!
//! **Fixes of the original.** VR-024 / PL-074: every handler is refused while the player trades or has a stall
//! open (the reply is `ULTIMATE_CANCEL`, nothing is consumed); PL-076: the enchant and the transmutation share
//! the pacing of the other upgrade attempts ([`UpgradeLimiter`](crate::item::upgrade::UpgradeLimiter), silent
//! when refused); a transmutation whose target item has no prop is cancelled before anything is taken.
//!
//! **Serials (PL-050).** A transmuted weapon keeps its serial: the old entry is removed and the same item with
//! the new id is created (the client has no message that changes an item id; `DeleteItem` + `CreateItem` is what
//! the original sends too).
//!
//! **SMELPROT3** (`II_SYS_SYS_SCR_SMELPROT3`, a buff scroll of WP-ITEM-20) is read through the buff set: a weapon
//! about to be destroyed by a failed enchant (or a failed unique to ultimate transmutation) survives while the
//! buff is held, and the buff is consumed by the attempt, success or not.

use super::container::{NoSerials, SerialSource, StorageId};
use super::env::ItemEnv;
use super::fx::{Fx, item_json};
use super::model::{Item, MAX_PIERCING_ULTIMATE};
use super::ops::{CtxSerials, ItemOrigin, RemoveReason, UiParam, core as core_ops, make_long};
use super::upgrade::UpgradeCx;
use super::usage::UseFacts;
use crate::buff::BuffCx;
use crate::buff::set::{BuffKey, BuffKind};
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::mover::Mover;
use crate::world::fx::{Sound, create_sfx_msg, sound_msg};
use flyff_common::GameMs;
use flyff_common::defs::item::{
    II_GEN_MAT_MOONSTONE, II_GEN_MAT_MOONSTONE_1, II_GEN_MAT_ORICHALCUM01,
    II_GEN_MAT_ORICHALCUM01_1, II_GEN_MAT_ORICHALCUM02, II_SYS_SYS_SCR_SMELPROT3,
};
use flyff_common::defs::itemkind::IK1_WEAPON;
use flyff_common::defs::obj::{XI_INT_FAIL, XI_INT_SUCCESS};
use flyff_common::defs::sound::{SND_INF_UPGRADEFAIL, SND_INF_UPGRADESUCCESS};
use flyff_common::defs::text::{
    TID_GAME_REMOVEGEM_FAILED, TID_GAME_REMOVEGEM_SUCCESS, TID_GAME_ULTIMATE_GEMSPACE,
    TID_GAME_ULTIMATE_ISEQUIP, TID_UPGRADE_FAIL, TID_UPGRADE_MAXOVER, TID_UPGRADE_SUCCEEFUL,
};
use flyff_common::rng::GameRng;
use flyff_data::GameData;
use flyff_data::props::item::ItemProp;
use flyff_db::audit::ev;
use flyff_proto::pb;
use flyff_rules::consts::TRANSPARENT_MODE;
use flyff_rules::item::ultimate::{
    GEM_SOCKET_FROM_AO, MAX_ULTIMATE_AO, ULTIMATE_CANCEL, ULTIMATE_FAILED, ULTIMATE_INVENTORY,
    ULTIMATE_ISNOTULTIMATE, ULTIMATE_PPM, ULTIMATE_SUCCESS, UNIQUE_TO_ULTIMATE_AO, WeaponClass,
    enchant_prob, gem_sockets, make_gem_yield, refer_class, roll_succeeds, safe_roll_fails,
    transmute_prob, weapon_class,
};
use serde_json::{Value, json};

/// `MAX_JEWEL`: the ids `UltimateMakeItem` carries.
pub const MAX_JEWEL: usize = 10;
/// How many pieces of each material a shining Orichalcum takes.
const MAKE_ITEM_PIECES: usize = 5;

/// Which of the six replies (`ULTIMATE_*` sub state).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sub {
    /// `ULTIMATE_MAKEITEM`.
    MakeItem,
    /// `ULTIMATE_TRANSWEAPON`.
    TransWeapon,
    /// `ULTIMATE_MAKEGEM`.
    MakeGem,
    /// `ULTIMATE_SETGEM`.
    SetGem,
    /// `ULTIMATE_REMOVEGEM`.
    RemoveGem,
    /// `ULTIMATE_ENCHANTWEAPON`.
    EnchantWeapon,
}

/// The answer of one request: the `Ultimate` message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reply {
    /// The sub state.
    pub sub: Sub,
    /// `ULTIMATE_*`.
    pub result: i32,
    /// The gems made (`MakeGem` only).
    pub num: i32,
}

impl Reply {
    fn new(sub: Sub, result: i32) -> Reply {
        Reply {
            sub,
            result,
            num: 0,
        }
    }

    fn cancel(sub: Sub) -> Reply {
        Reply::new(sub, ULTIMATE_CANCEL)
    }

    /// The wire message.
    pub fn to_pb(self) -> pb::Ultimate {
        use pb::ultimate::{
            EnchantWeapon, MakeGem, MakeItem, RemoveGem, SetGem, Sub as W, TransWeapon,
        };
        let r = self.result;
        let sub = match self.sub {
            Sub::MakeItem => W::MakeItem(MakeItem { result: r }),
            Sub::TransWeapon => W::TransWeapon(TransWeapon { result: r }),
            Sub::MakeGem => W::MakeGem(MakeGem {
                result: r,
                num: self.num,
            }),
            Sub::SetGem => W::SetGem(SetGem { result: r }),
            Sub::RemoveGem => W::RemoveGem(RemoveGem { result: r }),
            Sub::EnchantWeapon => W::EnchantWeapon(EnchantWeapon { result: r }),
        };
        pb::Ultimate { sub: Some(sub) }
    }
}

/// What an operation needs besides the mover and the [`Fx`].
pub struct UltimateCx<'a> {
    /// The item environment (game data, switches, wall time).
    pub env: &'a ItemEnv<'a>,
    /// The serial source of created items.
    pub serials: &'a mut dyn SerialSource,
    /// The world's random stream.
    pub rng: &'a mut dyn GameRng,
    /// The game clock of the tick (the buff context).
    pub now: GameMs,
}

impl<'a> UltimateCx<'a> {
    fn data(&self) -> &'a GameData {
        self.env.data()
    }
}

// ---- small helpers --------------------------------------------------------------------------------------------

/// The entry named by a client object id: it exists and nothing reserves it (`IsUsableItem`).
fn usable(m: &Mover, obj_id: u32) -> Option<(StorageId, &Item)> {
    let id = StorageId::try_from(obj_id).ok()?;
    let it = m.player()?.items.inventory.get_at_id(id)?;
    it.is_usable().then_some((id, it))
}

fn equipped(m: &Mover, id: StorageId) -> bool {
    m.player().is_some_and(|p| p.items.inventory.is_equip(id))
}

/// VR-024: the player trades or has a stall open.
fn busy(m: &Mover) -> bool {
    let f = UseFacts::of(m);
    f.trading || f.vending
}

fn has_smelprot3(m: &Mover) -> bool {
    m.buffs
        .contains(BuffKey::new(BuffKind::Item, II_SYS_SYS_SCR_SMELPROT3))
}

/// `RemoveBuff(BUFF_ITEM, II_SYS_SYS_SCR_SMELPROT3)`.
fn take_smelprot3(m: &mut Mover, cx: &UltimateCx<'_>, fx: &mut Fx) {
    let bcx = BuffCx::new(*cx.env, cx.now);
    crate::buff::core::remove_fake(
        m,
        &bcx,
        BuffKey::new(BuffKind::Item, II_SYS_SYS_SCR_SMELPROT3),
        fx,
    );
}

/// The sound and, unless the player is an invisible GM, the effect of an attempt.
pub(crate) fn feedback(m: &Mover, fx: &mut Fx, success: bool) {
    let sound = if success {
        SND_INF_UPGRADESUCCESS
    } else {
        SND_INF_UPGRADEFAIL
    };
    fx.out.to_self(sound_msg(&Sound::Id(sound)));
    if m.mv.mode & TRANSPARENT_MODE == 0 {
        let sfx = if success { XI_INT_SUCCESS } else { XI_INT_FAIL };
        fx.out.near(create_sfx_msg(sfx, m.core.pos, false));
    }
}

/// `item.upgrade{kind, result, before, after, materials}` of an ultimate attempt.
fn audit(
    fx: &mut Fx,
    kind: &str,
    result: &str,
    before: &Item,
    after: Option<&Item>,
    materials: Vec<Value>,
) {
    fx.audit(
        ev::ITEM_UPGRADE,
        json!({
            "kind": kind,
            "result": result,
            "before": item_json(before),
            "after": after.map(item_json),
            "materials": materials,
        }),
    );
}

fn material(item_id: u32, count: u32) -> Value {
    json!({ "item": item_id, "count": count })
}

fn weapon_prop<'d>(data: &'d GameData, it: &Item) -> Option<&'d ItemProp> {
    data.item(it.item_id)
}

/// The gem statistics of an ultimate weapon: `(DST, value)` summed per DST in DST order
/// (`CUltimateWeapon::GetDestParamUltimate`; the plan's `item::ultimate::gem_dst_values`). Empty for any other
/// item. Doc 16's equipment engine applies the same rule through `stats::equip::ultimate_dsts`.
pub fn gem_dst_values(item: &Item, data: &GameData) -> Vec<(u32, i32)> {
    let Some(prop) = data.item(item.item_id) else {
        return Vec::new();
    };
    flyff_rules::item::tooltip::gem_dst_values(
        &data.items,
        &data.ultimate.gem_ability,
        prop,
        &item.piercing.ultimate_gems,
    )
}

// ---- UltimateMakeItem -----------------------------------------------------------------------------------------

/// How often each object id occurs in a group of five.
fn occurrences(ids: &[u32]) -> Vec<(u32, u32)> {
    let mut out: Vec<(u32, u32)> = Vec::new();
    for &id in ids {
        match out.iter_mut().find(|(i, _)| *i == id) {
            Some(e) => e.1 += 1,
            None => out.push((id, 1)),
        }
    }
    out.sort_by_key(|(id, _)| *id);
    out
}

/// One group of `MakeItem`: every distinct id is a usable stack of one of `kinds` holding at least as many
/// pieces as the group names it; the entries with their counts.
fn check_group(m: &Mover, ids: &[u32], kinds: [u32; 2]) -> Option<Vec<(StorageId, u32)>> {
    let mut out = Vec::new();
    for (obj, n) in occurrences(ids) {
        let (id, it) = usable(m, obj)?;
        if !kinds.contains(&it.item_id) || n > u32::from(it.count) {
            return None;
        }
        out.push((id, n));
    }
    Some(out)
}

/// `UltimateMakeItem{ids[10]}` (`CUltimateWeapon::MakeItem`): five Orichalcum and five moonstones become one
/// Shining Orichalcum; it always succeeds when the items are there and one slot is free.
pub fn make_item(m: &mut Mover, cx: &mut UltimateCx<'_>, ids: &[u32], fx: &mut Fx) -> Reply {
    let cancel = Reply::cancel(Sub::MakeItem);
    if ids.len() != MAX_JEWEL {
        return cancel;
    }
    let (orichalcum, moonstones) = ids.split_at(MAKE_ITEM_PIECES);
    let Some(g1) = check_group(
        m,
        orichalcum,
        [II_GEN_MAT_ORICHALCUM01, II_GEN_MAT_ORICHALCUM01_1],
    ) else {
        return cancel;
    };
    let Some(g2) = check_group(
        m,
        moonstones,
        [II_GEN_MAT_MOONSTONE, II_GEN_MAT_MOONSTONE_1],
    ) else {
        return cancel;
    };
    let mut product = Item::new(II_GEN_MAT_ORICHALCUM02, 1);
    product.endurance = 0;
    // the original asks for room before it takes anything (a missing prop counts as no room)
    if !core_ops::can_create(m, cx.env, &product) {
        return Reply::new(Sub::MakeItem, ULTIMATE_INVENTORY);
    }
    let mut materials = Vec::new();
    for (id, n) in g1.into_iter().chain(g2) {
        if let Some(it) = m.player().and_then(|p| p.items.inventory.get_at_id(id)) {
            materials.push(material(it.item_id, n));
        }
        core_ops::remove_item(
            m,
            cx.env,
            id,
            n as u16,
            RemoveReason::Other("ultimate_make_item"),
            fx,
        );
    }
    let made = core_ops::create_item(
        m,
        cx.env,
        cx.serials,
        product.clone(),
        ItemOrigin::Ultimate,
        fx,
    );
    audit(
        fx,
        "ultimate_make_item",
        "success",
        &product,
        None,
        materials,
    );
    fx.mark_urgent();
    match made {
        Ok(_) => Reply::new(Sub::MakeItem, ULTIMATE_SUCCESS),
        Err(_) => Reply::cancel(Sub::MakeItem),
    }
}

// ---- UltimateMakeGem ------------------------------------------------------------------------------------------

/// `UltimateMakeGem{weapon}` (`CUltimateWeapon::MakeGem`): the weapon (general or unique, not worn) is
/// destroyed; its `+N` row decides the odds and the number of gems of the kind its level makes.
pub fn make_gem(m: &mut Mover, cx: &mut UltimateCx<'_>, weapon: u32, fx: &mut Fx) -> Reply {
    let cancel = Reply::cancel(Sub::MakeGem);
    let Some((wid, w)) = usable(m, weapon) else {
        return cancel;
    };
    let data = cx.data();
    let Some(prop) = weapon_prop(data, w) else {
        return cancel;
    };
    if prop.item_kind1 != IK1_WEAPON {
        return cancel;
    }
    let class = weapon_class(prop);
    if !matches!(class, WeaponClass::General | WeaponClass::Unique) {
        return cancel;
    }
    if equipped(m, wid) {
        fx.text(TID_GAME_ULTIMATE_ISEQUIP);
        return cancel;
    }
    let Some(gem) = data.ultimate.gem_kind(prop.limit_level1 as i32) else {
        return cancel;
    };
    let Some((prob, num)) = make_gem_yield(&data.ultimate, class, w.upgrade_level) else {
        return cancel;
    };
    let Some(gem_prop) = data.item(gem) else {
        return cancel;
    };
    let Some(count) = u16::try_from(num).ok().filter(|n| *n > 0) else {
        return cancel;
    };
    let mut gems = Item::new(gem, count);
    gems.endurance = 0;
    // the destroyed weapon frees one slot: only a second one is asked for
    let pack_max = gem_prop.pack_max;
    if u32::from(count) > pack_max
        && m.player().is_some_and(|p| {
            p.items
                .inventory
                .is_full(&gems, pack_max, u32::from(count) - pack_max)
        })
    {
        return Reply::new(Sub::MakeGem, ULTIMATE_INVENTORY);
    }
    let before = w.clone();
    core_ops::remove_item(
        m,
        cx.env,
        wid,
        1,
        RemoveReason::Other("ultimate_make_gem"),
        fx,
    );
    fx.mark_urgent();
    let materials = vec![material(before.item_id, 1)];
    if roll_succeeds(cx.rng.random_below(ULTIMATE_PPM), prob) {
        let made = core_ops::create_item(
            m,
            cx.env,
            cx.serials,
            gems.clone(),
            ItemOrigin::Ultimate,
            fx,
        );
        audit(
            fx,
            "ultimate_make_gem",
            "success",
            &before,
            Some(&gems),
            materials,
        );
        if made.is_ok() {
            return Reply {
                sub: Sub::MakeGem,
                result: ULTIMATE_SUCCESS,
                num: i32::from(count),
            };
        }
        return cancel;
    }
    audit(fx, "ultimate_make_gem", "failed", &before, None, materials);
    Reply::new(Sub::MakeGem, ULTIMATE_FAILED)
}

// ---- UltimateTransWeapon --------------------------------------------------------------------------------------

/// `UltimateTransWeapon{weapon, gem, shining}` (`CUltimateWeapon::TransWeapon`): a general weapon becomes its
/// unique form with `GENERAL2UNIQUE`, a `+10` unique weapon its ultimate form with `UNIQUE2ULTIMATE`
/// (`dwReferTarget2` of the prop overrides the odds). The level gem and the Shining Orichalcum are consumed; a
/// failure destroys the weapon unless the SMELPROT3 buff protects a unique weapon.
pub fn trans_weapon(
    m: &mut Mover,
    cx: &mut UltimateCx<'_>,
    weapon: u32,
    gem: u32,
    shining: u32,
    fx: &mut Fx,
) -> Reply {
    let cancel = Reply::cancel(Sub::TransWeapon);
    let (Some((wid, w)), Some((gid, g1)), Some((sid, g2))) =
        (usable(m, weapon), usable(m, gem), usable(m, shining))
    else {
        return cancel;
    };
    if equipped(m, wid) {
        fx.text(TID_GAME_ULTIMATE_ISEQUIP);
        return cancel;
    }
    let data = cx.data();
    let Some(prop) = weapon_prop(data, w) else {
        return cancel;
    };
    if prop.item_kind1 != IK1_WEAPON || prop.refer_target1 == u32::MAX {
        return cancel;
    }
    let class = weapon_class(prop);
    if !matches!(class, WeaponClass::General | WeaponClass::Unique) {
        return cancel;
    }
    if data.ultimate.gem_kind(prop.limit_level1 as i32) != Some(g1.item_id)
        || g2.item_id != II_GEN_MAT_ORICHALCUM02
    {
        return cancel;
    }
    if class == WeaponClass::Unique && w.upgrade_level != UNIQUE_TO_ULTIMATE_AO {
        return cancel;
    }
    let Some(prob) = transmute_prob(&data.ultimate, class, prop.refer_target2) else {
        return cancel;
    };
    // the product must exist before anything is taken
    if data.item(prop.refer_target1).is_none() {
        return cancel;
    }
    let (kind, product_id) = (
        if class == WeaponClass::Unique {
            "ultimate_trans"
        } else {
            "unique_trans"
        },
        prop.refer_target1,
    );
    let before = w.clone();
    let materials = vec![material(g1.item_id, 1), material(g2.item_id, 1)];
    let protected = class == WeaponClass::Unique && has_smelprot3(m);
    let mut materials = materials;
    if protected {
        take_smelprot3(m, cx, fx);
        materials.push(json!({ "buff": II_SYS_SYS_SCR_SMELPROT3 }));
    }
    core_ops::remove_item(m, cx.env, gid, 1, RemoveReason::Other("ultimate_trans"), fx);
    core_ops::remove_item(m, cx.env, sid, 1, RemoveReason::Other("ultimate_trans"), fx);
    fx.mark_urgent();
    if roll_succeeds(cx.rng.random_below(ULTIMATE_PPM), prob) {
        // the same item with the new id and (unique to ultimate) +0: removed and created again, the serial kept
        let mut product = before.clone();
        product.item_id = product_id;
        if class == WeaponClass::Unique {
            product.upgrade_level = 0;
        }
        // the shipped level-120 general weapons convert straight into an ultimate weapon and keep their +N:
        // an ultimate weapon of +N has N - 5 gem sockets (the original left such a weapon without any)
        if data
            .item(product_id)
            .is_some_and(|p| refer_class(p.refer_stat1) == WeaponClass::Ultimate)
        {
            let sockets = gem_sockets(product.upgrade_level).min(MAX_PIERCING_ULTIMATE);
            if product.piercing.ultimate_gems.len() < sockets {
                product.piercing.ultimate_gems.resize(sockets, 0);
            }
        }
        core_ops::remove_item(m, cx.env, wid, 1, RemoveReason::Other("ultimate_trans"), fx);
        let made = core_ops::create_item(
            m,
            cx.env,
            cx.serials,
            product.clone(),
            ItemOrigin::Ultimate,
            fx,
        );
        audit(fx, kind, "success", &before, Some(&product), materials);
        return if made.is_ok() {
            Reply::new(Sub::TransWeapon, ULTIMATE_SUCCESS)
        } else {
            Reply::new(Sub::TransWeapon, ULTIMATE_FAILED)
        };
    }
    if protected {
        audit(fx, kind, "protected", &before, Some(&before), materials);
    } else {
        core_ops::remove_item(m, cx.env, wid, 1, RemoveReason::UpgradeFail, fx);
        audit(fx, kind, "failed", &before, None, materials);
    }
    Reply::new(Sub::TransWeapon, ULTIMATE_FAILED)
}

// ---- UltimateSetGem -------------------------------------------------------------------------------------------

/// `GetGemAbilityKindRandom`: a random `ABILITY` block (the draw happens even when the gem is in none), the
/// ability item that block lists for `gem`.
fn gem_ability_item(cx: &mut UltimateCx<'_>, gem: u32) -> Option<u32> {
    let blocks = &cx.env.data().ultimate.gem_ability;
    if blocks.is_empty() {
        return None;
    }
    let pick = cx
        .rng
        .random_below(u32::try_from(blocks.len()).unwrap_or(u32::MAX)) as usize;
    blocks
        .get(pick)?
        .rows
        .iter()
        .find(|r| r.gem == gem)
        .map(|r| r.gem06)
}

/// `UltimateSetGem{weapon, gem}` (`CUltimateWeapon::SetGem`): the gem goes into the first empty socket of an
/// ultimate weapon as the ability item of a randomly chosen `ABILITY` block.
pub fn set_gem(
    m: &mut Mover,
    cx: &mut UltimateCx<'_>,
    weapon: u32,
    gem: u32,
    fx: &mut Fx,
) -> Reply {
    let cancel = Reply::cancel(Sub::SetGem);
    let (Some((wid, w)), Some((gid, g))) = (usable(m, weapon), usable(m, gem)) else {
        return cancel;
    };
    let Some(prop) = weapon_prop(cx.data(), w) else {
        return cancel;
    };
    if refer_class(prop.refer_stat1) != WeaponClass::Ultimate {
        return Reply::new(Sub::SetGem, ULTIMATE_ISNOTULTIMATE);
    }
    if equipped(m, wid) {
        fx.text(TID_GAME_ULTIMATE_ISEQUIP);
        return cancel;
    }
    let Some(slot) = w.piercing.ultimate_gems.iter().position(|&s| s == 0) else {
        fx.text(TID_GAME_ULTIMATE_GEMSPACE);
        return cancel;
    };
    let gem_item = g.item_id;
    let Some(ability) = gem_ability_item(cx, gem_item) else {
        return cancel;
    };
    let before = w.clone();
    core_ops::remove_item(
        m,
        cx.env,
        gid,
        1,
        RemoveReason::Other("ultimate_set_gem"),
        fx,
    );
    fx.mark_urgent();
    let materials = vec![material(gem_item, 1)];
    if roll_succeeds(
        cx.rng.random_below(ULTIMATE_PPM),
        cx.data().ultimate.set_gem,
    ) {
        core_ops::update_item(
            m,
            cx.env,
            wid,
            UiParam::UltimatePiercing,
            make_long(slot as u32, ability),
            0,
            fx,
        );
        let after = m
            .player()
            .and_then(|p| p.items.inventory.get_at_id(wid))
            .cloned();
        audit(
            fx,
            "ultimate_set_gem",
            "success",
            &before,
            after.as_ref(),
            materials,
        );
        return Reply::new(Sub::SetGem, ULTIMATE_SUCCESS);
    }
    audit(
        fx,
        "ultimate_set_gem",
        "failed",
        &before,
        Some(&before),
        materials,
    );
    Reply::new(Sub::SetGem, ULTIMATE_FAILED)
}

// ---- UltimateRemoveGem ----------------------------------------------------------------------------------------

/// `UltimateRemoveGem{weapon, moonstone}` (`CUltimateWeapon::RemoveGem`): the moonstone is consumed and the
/// last filled socket is cleared. The text, sound and effect of the handler follow.
pub fn remove_gem(
    m: &mut Mover,
    cx: &mut UltimateCx<'_>,
    weapon: u32,
    moonstone: u32,
    fx: &mut Fx,
) -> Reply {
    let cancel = Reply::cancel(Sub::RemoveGem);
    let (Some((wid, w)), Some((mid, ms))) = (usable(m, weapon), usable(m, moonstone)) else {
        return cancel;
    };
    let Some(prop) = weapon_prop(cx.data(), w) else {
        return cancel;
    };
    if refer_class(prop.refer_stat1) != WeaponClass::Ultimate {
        return Reply::new(Sub::RemoveGem, ULTIMATE_ISNOTULTIMATE);
    }
    if w.piercing.ultimate_gems.first().is_none_or(|&s| s == 0) {
        return cancel;
    }
    if ms.item_id != II_GEN_MAT_MOONSTONE && ms.item_id != II_GEN_MAT_MOONSTONE_1 {
        return cancel;
    }
    if equipped(m, wid) {
        fx.text(TID_GAME_ULTIMATE_ISEQUIP);
        return cancel;
    }
    let before = w.clone();
    let materials = vec![material(ms.item_id, 1)];
    core_ops::remove_item(
        m,
        cx.env,
        mid,
        1,
        RemoveReason::Other("ultimate_remove_gem"),
        fx,
    );
    fx.mark_urgent();
    if roll_succeeds(
        cx.rng.random_below(ULTIMATE_PPM),
        cx.data().ultimate.remove_gem,
    ) {
        if let Some(last) = before.piercing.ultimate_gems.iter().rposition(|&s| s != 0) {
            core_ops::update_item(
                m,
                cx.env,
                wid,
                UiParam::UltimatePiercing,
                make_long(last as u32, 0),
                0,
                fx,
            );
        }
        let after = m
            .player()
            .and_then(|p| p.items.inventory.get_at_id(wid))
            .cloned();
        audit(
            fx,
            "ultimate_remove_gem",
            "success",
            &before,
            after.as_ref(),
            materials,
        );
        fx.text(TID_GAME_REMOVEGEM_SUCCESS);
        feedback(m, fx, true);
        return Reply::new(Sub::RemoveGem, ULTIMATE_SUCCESS);
    }
    audit(
        fx,
        "ultimate_remove_gem",
        "failed",
        &before,
        Some(&before),
        materials,
    );
    fx.text(TID_GAME_REMOVEGEM_FAILED);
    feedback(m, fx, false);
    Reply::new(Sub::RemoveGem, ULTIMATE_FAILED)
}

// ---- UltimateEnchantWeapon ------------------------------------------------------------------------------------

/// `AO + 1` of an ultimate weapon: the `+N` and, above `+5`, the gem sockets (`AO - 5`).
fn apply_enchant(m: &mut Mover, env: &ItemEnv<'_>, wid: StorageId, new_ao: i32, fx: &mut Fx) {
    core_ops::update_item(m, env, wid, UiParam::Ao, new_ao as u32, 0, fx);
    if new_ao > GEM_SOCKET_FROM_AO {
        let sockets = gem_sockets(new_ao).min(MAX_PIERCING_ULTIMATE);
        core_ops::update_item(
            m,
            env,
            wid,
            UiParam::UltimatePiercingSize,
            sockets as u32,
            0,
            fx,
        );
    }
}

/// `UltimateEnchantWeapon{weapon, shining}` (`CUltimateWeapon::EnchantWeapon`): the Shining Orichalcum is
/// consumed and `+N+1` is rolled with `ULTIMATE_ENCHANT`; a failure destroys the weapon unless SMELPROT3 is
/// held.
pub fn enchant_weapon(
    m: &mut Mover,
    cx: &mut UltimateCx<'_>,
    weapon: u32,
    shining: u32,
    fx: &mut Fx,
) -> Reply {
    let cancel = Reply::cancel(Sub::EnchantWeapon);
    let (Some((wid, w)), Some((sid, s))) = (usable(m, weapon), usable(m, shining)) else {
        return cancel;
    };
    let Some(prop) = weapon_prop(cx.data(), w) else {
        return cancel;
    };
    if refer_class(prop.refer_stat1) != WeaponClass::Ultimate
        || s.item_id != II_GEN_MAT_ORICHALCUM02
    {
        return Reply::new(Sub::EnchantWeapon, ULTIMATE_ISNOTULTIMATE);
    }
    if equipped(m, wid) {
        fx.text(TID_GAME_ULTIMATE_ISEQUIP);
        return cancel;
    }
    let ao = w.upgrade_level;
    if ao + 1 > MAX_ULTIMATE_AO {
        fx.text(TID_UPGRADE_MAXOVER);
        return cancel;
    }
    let before = w.clone();
    let mut materials = vec![material(s.item_id, 1)];
    let protected = has_smelprot3(m);
    if protected {
        take_smelprot3(m, cx, fx);
        materials.push(json!({ "buff": II_SYS_SYS_SCR_SMELPROT3 }));
    }
    core_ops::remove_item(
        m,
        cx.env,
        sid,
        1,
        RemoveReason::Other("ultimate_enchant"),
        fx,
    );
    fx.mark_urgent();
    let roll = cx.rng.random_below(ULTIMATE_PPM);
    // the row is missing in a corrupt table: the original returns CANCEL after it took the material
    let Some(prob) = enchant_prob(&cx.env.data().ultimate, ao + 1) else {
        return cancel;
    };
    if roll_succeeds(roll, prob) {
        apply_enchant(m, cx.env, wid, ao + 1, fx);
        let after = m
            .player()
            .and_then(|p| p.items.inventory.get_at_id(wid))
            .cloned();
        audit(
            fx,
            "ultimate_enchant",
            "success",
            &before,
            after.as_ref(),
            materials,
        );
        fx.text(TID_UPGRADE_SUCCEEFUL);
        feedback(m, fx, true);
        return Reply::new(Sub::EnchantWeapon, ULTIMATE_SUCCESS);
    }
    if protected {
        audit(
            fx,
            "ultimate_enchant",
            "protected",
            &before,
            Some(&before),
            materials,
        );
    } else {
        core_ops::remove_item(m, cx.env, wid, 1, RemoveReason::UpgradeFail, fx);
        audit(fx, "ultimate_enchant", "failed", &before, None, materials);
    }
    fx.text(TID_UPGRADE_FAIL);
    feedback(m, fx, false);
    Reply::new(Sub::EnchantWeapon, ULTIMATE_FAILED)
}

// ---- the safe enchant -----------------------------------------------------------------------------------------

/// `CUltimateWeapon::SmeltSafetyUltimate`: the `SmeltSafetyReq` branch with a Shining Orichalcum. The caller
/// (the `SmeltSafetyReq` dispatcher of Plan/19 §12.1) has already refused a trade, a stall, a worn target and
/// unusable entries; the result is the `SmeltSafetyNtf` byte: `0` invalid (nothing consumed), `1` success,
/// `2` failed (the weapon is kept, the material and the protection scroll are consumed), `3` already `+10`.
///
/// The fail test is the safe one: `xRandom(1_000_000) > p`. The protection scroll must be the SMELPROT3 *item*
/// (not the buff) and is consumed together with one Shining Orichalcum, success or not.
pub fn safe_enchant(
    m: &mut Mover,
    cx: &mut UltimateCx<'_>,
    weapon: StorageId,
    material_id: StorageId,
    protection: StorageId,
    fx: &mut Fx,
) -> u8 {
    let (Some(w), Some(mat), Some(prot)) = (
        m.player().and_then(|p| p.items.inventory.get_at_id(weapon)),
        m.player()
            .and_then(|p| p.items.inventory.get_at_id(material_id)),
        m.player()
            .and_then(|p| p.items.inventory.get_at_id(protection)),
    ) else {
        return 0;
    };
    let Some(prop) = weapon_prop(cx.data(), w) else {
        return 0;
    };
    if refer_class(prop.refer_stat1) != WeaponClass::Ultimate
        || prot.item_id != II_SYS_SYS_SCR_SMELPROT3
        || mat.item_id != II_GEN_MAT_ORICHALCUM02
        || weapon == material_id
        || weapon == protection
        || material_id == protection
    {
        return 0;
    }
    let ao = w.upgrade_level;
    if ao >= MAX_ULTIMATE_AO {
        return 3;
    }
    let before = w.clone();
    let materials = vec![material(mat.item_id, 1), material(prot.item_id, 1)];
    core_ops::remove_item(
        m,
        cx.env,
        material_id,
        1,
        RemoveReason::Other("ultimate_safe_enchant"),
        fx,
    );
    core_ops::remove_item(
        m,
        cx.env,
        protection,
        1,
        RemoveReason::Other("ultimate_safe_enchant"),
        fx,
    );
    fx.mark_urgent();
    let Some(prob) = enchant_prob(&cx.env.data().ultimate, ao + 1) else {
        return 0;
    };
    if safe_roll_fails(cx.rng.random_below(ULTIMATE_PPM), prob) {
        feedback(m, fx, false);
        audit(
            fx,
            "ultimate_safe_enchant",
            "failed",
            &before,
            Some(&before),
            materials,
        );
        return 2;
    }
    feedback(m, fx, true);
    apply_enchant(m, cx.env, weapon, ao + 1, fx);
    let after = m
        .player()
        .and_then(|p| p.items.inventory.get_at_id(weapon))
        .cloned();
    audit(
        fx,
        "ultimate_safe_enchant",
        "success",
        &before,
        after.as_ref(),
        materials,
    );
    1
}

// ---- the handlers ---------------------------------------------------------------------------------------------

/// Runs `f` on the player's mover with the item environment, the game's serial source, the world's random stream
/// and a fresh [`Fx`], then flushes the effects (the `GameCtx` wrapper of the item operations). `None` when `who`
/// is not a live player.
pub(crate) fn with_actor<R>(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    f: impl FnOnce(
        &mut Mover,
        &ItemEnv<'_>,
        &mut dyn SerialSource,
        &mut dyn GameRng,
        GameMs,
        &mut Fx,
    ) -> R,
) -> Option<R> {
    let core = svc.items.handle();
    let env = core.env(ctx.clock.unix_ms.to_unix_time());
    let now = ctx.clock.game_ms;
    let world = ctx.player(who)?.core.world;
    let mut fx = Fx::new();
    let r = {
        let mut serials = CtxSerials::new(&mut svc.serials, &*ctx.persist, &mut *ctx.tickets);
        let rng = ctx.rng.world(world);
        let m = ctx.worlds.player_mut(who.0)?;
        f(m, &env, &mut serials, rng, now, &mut fx)
    };
    fx.flush(svc, ctx, who);
    Some(r)
}

/// `UpgradeLimiter::accept` of the player (PL-076): `false` = too soon, nothing happens.
pub(crate) fn pace_ok(m: &mut Mover, env: &ItemEnv<'_>, now: GameMs) -> bool {
    let min = env.cfg().upgrade_min_interval_ms;
    m.player_mut()
        .is_none_or(|p| p.items.upgrade_limiter.accept(now, min))
}

/// The shared frame of the six handlers: VR-024, the pacing (`paced`), the operation, the reply.
fn run(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    sub: Sub,
    paced: bool,
    op: impl FnOnce(&mut Mover, &mut UltimateCx<'_>, &mut Fx) -> Reply,
) -> Option<Reply> {
    with_actor(svc, ctx, who, |m, env, serials, rng, now, fx| {
        if busy(m) {
            // VR-024: nothing is consumed; the window is told to stand down
            let reply = Reply::cancel(sub);
            fx.out.to_self(reply.to_pb());
            return Some(reply);
        }
        if paced && !pace_ok(m, env, now) {
            return None;
        }
        let mut cx = UltimateCx {
            env,
            serials,
            rng,
            now,
        };
        let reply = op(m, &mut cx, fx);
        fx.out.to_self(reply.to_pb());
        Some(reply)
    })
    .flatten()
}

/// `UltimateMakeItem`.
pub fn on_make_item(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    msg: pb::UltimateMakeItem,
) -> Option<Reply> {
    run(svc, ctx, who, Sub::MakeItem, false, |m, cx, fx| {
        make_item(m, cx, &msg.item_obj_ids, fx)
    })
}

/// `UltimateMakeGem`.
pub fn on_make_gem(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    msg: pb::UltimateMakeGem,
) -> Option<Reply> {
    run(svc, ctx, who, Sub::MakeGem, false, |m, cx, fx| {
        make_gem(m, cx, msg.weapon_obj_id, fx)
    })
}

/// `UltimateTransWeapon` (paced).
pub fn on_trans_weapon(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    msg: pb::UltimateTransWeapon,
) -> Option<Reply> {
    run(svc, ctx, who, Sub::TransWeapon, true, |m, cx, fx| {
        trans_weapon(
            m,
            cx,
            msg.weapon_obj_id,
            msg.gem_obj_id,
            msg.shining_obj_id,
            fx,
        )
    })
}

/// `UltimateSetGem`.
pub fn on_set_gem(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    msg: pb::UltimateSetGem,
) -> Option<Reply> {
    run(svc, ctx, who, Sub::SetGem, false, |m, cx, fx| {
        set_gem(m, cx, msg.weapon_obj_id, msg.gem_obj_id, fx)
    })
}

/// `UltimateRemoveGem`.
pub fn on_remove_gem(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    msg: pb::UltimateRemoveGem,
) -> Option<Reply> {
    run(svc, ctx, who, Sub::RemoveGem, false, |m, cx, fx| {
        remove_gem(m, cx, msg.weapon_obj_id, msg.moonstone_obj_id, fx)
    })
}

/// `UltimateEnchantWeapon` (paced).
pub fn on_enchant_weapon(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    msg: pb::UltimateEnchantWeapon,
) -> Option<Reply> {
    run(svc, ctx, who, Sub::EnchantWeapon, true, |m, cx, fx| {
        enchant_weapon(m, cx, msg.weapon_obj_id, msg.shining_obj_id, fx)
    })
}

/// [`safe_enchant`] as the branch of the `SmeltSafetyReq` dispatcher of WP-ITEM-12
/// ([`UpgradeHooks::ultimate_safe`](crate::item::upgrade::UpgradeHooks), installed by
/// [`ItemService::new`](crate::item::ItemService::new)): the dispatcher has already paced the attempt, refused a
/// trade, a stall and a worn target and resolved the three entries.
pub fn safe_enchant_hook(
    m: &mut Mover,
    cx: &UpgradeCx<'_>,
    weapon: StorageId,
    material_id: StorageId,
    protection: StorageId,
    rng: &mut dyn GameRng,
    fx: &mut Fx,
) -> u8 {
    let mut serials = NoSerials;
    let mut ucx = UltimateCx {
        env: cx.env,
        serials: &mut serials,
        rng,
        now: cx.now,
    };
    safe_enchant(m, &mut ucx, weapon, material_id, protection, fx)
}
