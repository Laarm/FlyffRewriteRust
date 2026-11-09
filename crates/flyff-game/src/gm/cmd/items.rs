//! The item and upgrade GM commands (Plan/23 §4.6, §4.7, §6.0; WP-GM-14): `/ResistItem` (row 81), `/createitem2`
//! (85), `/Piercing` (90), `/RefineAccessory` (99), `/RefineCollector` (100), `/GenRandomOption` (101),
//! `/InitializeRandomOption` (102), `/SetRandomOption` (103), `/EscapeReset` (133), `/ItemLevel` (162),
//! `/RemoveTotalGold` (186) and `/SetTutorialState` (187).
//!
//! They act on the issuer's own character ("self"). The ones without an object id take the item shown in
//! **inventory slot 0** (`m_Inventory.GetAt( 0 )`); the browser fills the id of the upgrade window's slot 0 into
//! `/ritem` and `/pier` (23 §3.10), the server accepts the explicit forms from any issuer. Every change goes
//! through the lifecycle of doc 19 (`item::ops`: `UpdateItem` / `UpdateItemEx`), so the container, the worn-item
//! statistics and the messages stay in step, and each one writes an `item.upgrade` audit line (`kind =
//! "gm_<command>"`, `path = "gm"`) next to the `gm.command` record. Bad arguments are silent like the legacy
//! handlers.
//!
//! Decisions (also in the NOTES): a negative level or count counts as 0 (the legacy passed the negative `int`
//! on as an unsigned value, which the item layer clamps to the maximum); an item that a trade or a stall
//! reserves is left alone (`txn::usable`); `/rtg` with a negative amount takes nothing (the legacy added the
//! amount); `/sts` outside `0..=guide_max_seq` answers a line and keeps the step (VR-004).

use crate::ctx::PlayerRef;
use crate::gm::lookup;
use crate::gm::{CmdArgs, CmdCtx, CmdDone, CmdError, CmdResult};
use crate::item::container::StorageId;
use crate::item::fx::Fx;
use crate::item::gold::total::{remove_total_gold, total_gold};
use crate::item::model::{Item, MAX_PIERCING_WEAPON};
use crate::item::ops::{self, ItemOrigin, UiParam, core as core_ops};
use crate::item::upgrade::random_option::{awakening_kind, gen_random_option};
use crate::item::upgrade::txn::{UpgradeCx, audit, equipped, feedback, item_at, usable};
use crate::item::upgrade::{UpgradeHooks, safe::can_take_socket};
use crate::mover::Mover;
use crate::stats::Out;
use flyff_common::CharacterId;
use flyff_common::defs::itemkind::IK3_VIRTUAL;
use flyff_common::defs::text::{TID_GAME_EQUIPPUT, TID_UPGRADE_SUCCEEFUL};
use flyff_common::rng::GameRng;
use flyff_rules::consts::{NULL_ID, SM_ESCAPE};
use flyff_rules::item::random_option::{LevelDown, RandomOptKind, RandomOptLine};
use flyff_rules::item::refine::UpgradeTables;

/// The highest destination parameter `/sro` accepts (`nDst > 96` stops the list).
pub const SRO_MAX_DST: i32 = 96;
/// The destination `/sro` stops at (`nDst == 48`).
pub const SRO_STOP_DST: i32 = 48;
/// The largest value `/sro` accepts (`nAdj > 512 || nAdj < -512` stops the list).
pub const SRO_MAX_ADJ: i32 = 512;
/// The highest level `/rc` sets.
pub const COLLECTOR_MAX_LEVEL: i32 = 5;

/// The issuer's character.
fn me(cx: &CmdCtx<'_, '_>) -> Result<PlayerRef, CmdError> {
    cx.self_player.ok_or(CmdError::NeedsSelf)
}

/// Runs `f` for the player `who` with the upgrade context (item environment, odds tables), the item hooks, the
/// world's random stream and a fresh [`Fx`], then flushes the effects (messages, audit lines, urgent save).
/// `None` when `who` is not a live player.
fn run<R>(
    cx: &mut CmdCtx<'_, '_>,
    who: PlayerRef,
    f: impl FnOnce(&mut Mover, &UpgradeCx<'_>, &UpgradeHooks, &mut dyn GameRng, &mut Fx) -> R,
) -> Option<R> {
    let core = cx.svc.items.handle();
    let hooks = cx.svc.items.upgrade_hooks;
    let env = core.env(cx.ctx.clock.unix_ms.to_unix_time());
    let data = &*core.data;
    let tables = UpgradeTables::from_data(
        data.lua.item_upgrade.as_ref(),
        &data.accessory,
        &data.collecting,
    );
    let ucx = UpgradeCx::new(&env, cx.ctx.clock.game_ms, &tables);
    let world = cx.ctx.worlds.core(who.0)?.world;
    let mut fx = Fx::new();
    let r = {
        let m = cx.ctx.worlds.player_mut(who.0)?;
        f(m, &ucx, &hooks, cx.ctx.rng.world(world), &mut fx)
    };
    fx.flush(cx.svc, cx.ctx, who);
    Some(r)
}

/// The item shown in inventory slot 0 and its storage id (`m_Inventory.GetAt( 0 )`).
fn slot0(m: &Mover) -> Option<(StorageId, &Item)> {
    let inv = &m.player()?.items.inventory;
    let id = inv.obj_at_slot(0)?;
    Some((id, inv.get_at_id(id)?))
}

/// A number argument as a count: negative values count as 0.
fn non_negative(n: i32) -> u32 {
    u32::try_from(n).unwrap_or(0)
}

/// The `item.upgrade` audit line of a GM change.
fn audit_gm(fx: &mut Fx, kind: &str, before: &Item, after: Option<&Item>) {
    audit(fx, kind, "gm", "success", before, after, Vec::new());
}

// ---- /ResistItem (81) -------------------------------------------------------------------------------------------

/// Row 81 `/ResistItem (ritem) <objId> <elem 0..5> <elemLv> <upgr>` (`TextCmd_ResistItem`): the element, its
/// level and the `+N` of an inventory item. `elem` is a byte (`> 5` stops), `elemLv` must not exceed the number
/// of element levels of `ItemUpgrade.lua` and `upgr` not the general `+N` maximum; `elem 0` forces the level 0.
/// A worn item answers `TID_GAME_EQUIPPUT` and keeps its values. An ultimate weapon with `upgr > 5` also gets
/// the ultimate piercing size `upgr - 5`.
pub fn resist_item(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let who = me(cx)?;
    let obj_id = args.number();
    // `BYTE bItemResist`: the number is truncated to a byte
    let elem = (args.number() & 0xFF) as u8;
    let element_level = args.number();
    let upgrade = args.number();
    if elem > 5 {
        return Ok(CmdDone::default());
    }
    let _ = run(cx, who, |m, ucx, _, _, fx| {
        if element_level < 0
            || element_level > ucx.tables.max_attribute()
            || upgrade < 0
            || upgrade > ucx.tables.max_general()
        {
            return;
        }
        let element_level = if elem == 0 { 0 } else { element_level };
        let Some(id) = u32::try_from(obj_id).ok().and_then(|o| usable(m, o)) else {
            return;
        };
        if equipped(m, id) {
            fx.text(TID_GAME_EQUIPPUT);
            return;
        }
        let Some(before) = item_at(m, id).cloned() else {
            return;
        };
        let env = ucx.env;
        core_ops::update_item(m, env, id, UiParam::Ir, u32::from(elem), 0, fx);
        core_ops::update_item(m, env, id, UiParam::Rao, non_negative(element_level), 0, fx);
        core_ops::update_item(m, env, id, UiParam::Ao, non_negative(upgrade), 0, fx);
        if upgrade > 5
            && ucx
                .prop(before.item_id)
                .is_some_and(flyff_rules::item::smelt::is_ultimate_weapon)
        {
            core_ops::update_item(
                m,
                env,
                id,
                UiParam::UltimatePiercingSize,
                non_negative(upgrade - 5),
                0,
                fx,
            );
        }
        fx.mark_urgent();
        audit_gm(fx, "gm_ritem", &before, item_at(m, id));
    });
    Ok(CmdDone::default())
}

// ---- /createitem2 (85) ------------------------------------------------------------------------------------------

/// Row 85 `/createitem2 (ci2) <itemName> <randomOptItemId>` (`TextCmd_CreateItem2`): one item whose stat
/// bonus is the `RandomOptItem` row `randomOptItemId` of `propItemEtc.inc`. The item comes by id, name or
/// `II_*` define (PL-099); `IK3_VIRTUAL` and an unknown stat row create nothing; a full inventory is silent.
pub fn create_item2(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let who = me(cx)?;
    let token = args.token().to_owned();
    let is_number = args.token_is_number();
    let Some(id) = lookup::item(cx.ctx.data, &token, is_number) else {
        return Ok(CmdDone::default());
    };
    let Some(prop) = cx.ctx.data.item(id) else {
        return Ok(CmdDone::default());
    };
    if prop.item_kind3 == IK3_VIRTUAL {
        return Ok(CmdDone::default());
    }
    let wanted = args.number();
    let stat = cx
        .ctx
        .data
        .item_etc
        .random_opt_items
        .iter()
        .find(|d| d.id == wanted)
        .and_then(|d| u8::try_from(d.id).ok());
    let Some(stat) = stat else {
        return Ok(CmdDone::default());
    };
    let mut item = Item::new(id, 1);
    item.random_option.set_stat_id(stat);
    let _ = ops::create_item(cx.svc, cx.ctx, who, item, ItemOrigin::Gm);
    Ok(CmdDone::on(token))
}

// ---- /Piercing (90) ---------------------------------------------------------------------------------------------

/// Row 90 `/Piercing (pier) <objId> <n 0..10>` (`TextCmd_Piercing`): the socket count of an inventory item.
/// `n` is a byte and must not exceed 10; a worn item answers `TID_GAME_EQUIPPUT`. A smaller `n` than the
/// current size sets it at once (the cards beyond go with the sockets); otherwise the size grows one socket at
/// a time while the item can take another (`IsPierceAble( NULL_ID, TRUE )`, which is never true for gear under
/// `features.no_piercing`, the repack default).
pub fn piercing(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let who = me(cx)?;
    let obj_id = args.number();
    let wanted = (args.number() & 0xFF) as usize;
    let _ = run(cx, who, |m, ucx, _, _, fx| {
        let Some(id) = u32::try_from(obj_id).ok().and_then(|o| usable(m, o)) else {
            return;
        };
        if wanted > MAX_PIERCING_WEAPON {
            return;
        }
        if equipped(m, id) {
            fx.text(TID_GAME_EQUIPPUT);
            return;
        }
        let Some(before) = item_at(m, id).cloned() else {
            return;
        };
        let env = ucx.env;
        let current = before.piercing.cards.len();
        if wanted < current {
            core_ops::update_item(m, env, id, UiParam::PiercingSize, wanted as u32, 0, fx);
        } else if let Some(prop) = ucx.prop(before.item_id) {
            // `for i in 1..=n: if IsPierceAble( NULL_ID, TRUE ) SetPiercingSize( i )`: the size the loop ends at
            // (the legacy kept the cards of the array, so the whole walk is one resize here)
            let no_piercing = env.cfg().no_piercing;
            let mut probe = before.clone();
            for i in 1..=wanted {
                if can_take_socket(&probe, prop, no_piercing) {
                    probe.piercing.cards.resize(i, 0);
                }
            }
            let size = probe.piercing.cards.len();
            if size != current {
                core_ops::update_item(m, env, id, UiParam::PiercingSize, size as u32, 0, fx);
            }
        }
        fx.mark_urgent();
        audit_gm(fx, "gm_pier", &before, item_at(m, id));
    });
    Ok(CmdDone::default())
}

// ---- /RefineAccessory (99), /RefineCollector (100) --------------------------------------------------------------

/// Row 99 `/RefineAccessory (ra) [n]` (`TextCmd_RefineAccessory`): the accessory in inventory slot 0 gets the
/// upgrade level `n` (default 0; the item layer caps it at 20). Anything else in slot 0 is left alone.
pub fn refine_accessory(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let who = me(cx)?;
    let n = non_negative(args.number());
    let _ = run(cx, who, |m, ucx, _, _, fx| {
        let Some((id, item)) = slot0(m) else { return };
        if !item.is_accessory(ucx.env.data()) {
            return;
        }
        let before = item.clone();
        core_ops::update_item(m, ucx.env, id, UiParam::Ao, n, 0, fx);
        fx.mark_urgent();
        audit_gm(fx, "gm_ra", &before, item_at(m, id));
    });
    Ok(CmdDone::default())
}

/// Row 100 `/RefineCollector (rc) [n <= 5]` (`TextCmd_RefineCollector`): the refinable collector in inventory
/// slot 0 gets the level `n` (default 0, at most 5) with the success text, sound and effect (no effect for a
/// transparent GM).
pub fn refine_collector(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let who = me(cx)?;
    let n = args.number().clamp(0, COLLECTOR_MAX_LEVEL) as u32;
    let _ = run(cx, who, |m, ucx, _, _, fx| {
        let Some((id, item)) = slot0(m) else { return };
        if !item.is_collector(true) {
            return;
        }
        let before = item.clone();
        fx.text(TID_UPGRADE_SUCCEEFUL);
        feedback(m, fx, true);
        core_ops::update_item(m, ucx.env, id, UiParam::Ao, n, 0, fx);
        fx.mark_urgent();
        audit_gm(fx, "gm_rc", &before, item_at(m, id));
    });
    Ok(CmdDone::default())
}

// ---- /GenRandomOption (101), /InitializeRandomOption (102), /SetRandomOption (103) ------------------------------

/// The slot-0 item of an awakening kind (`GetRandomOptionKind( item ) >= 0`): `f` gets the storage id, the kind,
/// the item's `parts` and the word; a new word is written back with `UpdateItemEx`. The summoned pet's stat
/// bonus is taken off before and applied after, like in the upgrade paths (`UpgradeHooks`).
fn rewrite_word(
    cx: &mut CmdCtx<'_, '_>,
    who: PlayerRef,
    audit_kind: &'static str,
    f: impl FnOnce(
        &UpgradeCx<'_>,
        RandomOptKind,
        u32,
        &mut flyff_rules::item::random_option::RandomOptWord,
        &mut dyn GameRng,
    ),
) {
    let _ = run(cx, who, |m, ucx, hooks, rng, fx| {
        let Some((id, item)) = slot0(m) else { return };
        let Some(kind) = awakening_kind(ucx, item) else {
            return;
        };
        let Some(parts) = ucx.prop(item.item_id).map(|p| p.parts) else {
            return;
        };
        let before = item.clone();
        let using = crate::item::upgrade::random_option::is_using(m, id);
        let pet_kind = matches!(kind, RandomOptKind::SystemPet | RandomOptKind::EatPet);
        if using
            && pet_kind
            && let Some(reset) = hooks.pet_options_reset
        {
            reset(m, ucx, id, fx);
        }
        let mut word = before.random_option;
        f(ucx, kind, parts, &mut word, rng);
        core_ops::update_item_ex(m, ucx.env, id, word, fx);
        if using
            && pet_kind
            && let Some(set) = hooks.pet_options_set
        {
            set(m, ucx, id, fx);
        }
        fx.mark_urgent();
        audit_gm(fx, audit_kind, &before, item_at(m, id));
    });
}

/// Row 101 `/GenRandomOption (gro)`: the awakening / blessing of the item in slot 0 is rolled anew (the old
/// lines are cleared first, the stat id and the level-down bits stay). Items without a random-option kind are
/// left alone.
pub fn gen_random_option_cmd(cx: &mut CmdCtx<'_, '_>, _args: &mut CmdArgs<'_>) -> CmdResult {
    let who = me(cx)?;
    rewrite_word(cx, who, "gm_gro", |ucx, kind, parts, word, rng| {
        word.initialize();
        gen_random_option(&ucx.env.data().random_option, kind, parts, word, false, rng);
    });
    Ok(CmdDone::default())
}

/// Row 102 `/InitializeRandomOption (iro)`: the lines of the item in slot 0 are cleared.
pub fn init_random_option(cx: &mut CmdCtx<'_, '_>, _args: &mut CmdArgs<'_>) -> CmdResult {
    let who = me(cx)?;
    rewrite_word(cx, who, "gm_iro", |_, _, _, word, _| word.initialize());
    Ok(CmdDone::default())
}

/// Row 103 `/SetRandomOption (sro) <dst> <adj> [<dst> <adj> ...]` (`TextCmd_SetRandomOption`): the item in slot 0
/// loses its lines and gets up to three `(dst, adj)` pairs. The list stops at `dst 48`, at a `dst` outside
/// `1..=96` and at an `adj` outside `-512..=512` (the pair that breaks the rule is not stored); a dangling `dst`
/// without a value counts as `adj 0`. The legacy encoding cannot hold `|adj| = 512` (the bit is the sign
/// bit): such a line is stored with the value 0. With no pair at all the item is simply cleared.
pub fn set_random_option(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let who = me(cx)?;
    let mut pairs: Vec<(u8, i16)> = Vec::new();
    let mut dst = args.number();
    while !args.finished() {
        let adj = args.number();
        if dst == SRO_STOP_DST
            || !(1..=SRO_MAX_DST).contains(&dst)
            || !(-SRO_MAX_ADJ..=SRO_MAX_ADJ).contains(&adj)
        {
            break;
        }
        let adj = if adj.abs() == SRO_MAX_ADJ { 0 } else { adj };
        if let (Ok(d), Ok(a)) = (u8::try_from(dst), i16::try_from(adj)) {
            pairs.push((d, a));
        }
        if pairs.len() >= flyff_rules::item::random_option::RandomOptWord::MAX_LINES {
            break;
        }
        dst = args.number();
    }
    rewrite_word(cx, who, "gm_sro", |_, _, _, word, _| {
        word.initialize();
        for (dst, adj) in pairs {
            let _ = word.push_line(RandomOptLine { dst, adj });
        }
    });
    Ok(CmdDone::default())
}

// ---- /ItemLevel (162) -------------------------------------------------------------------------------------------

/// Row 162 `/ItemLevel (il) [n]` (`TextCmd_ItemLevel`): the level-down of the equipment in slot 0 (an item with
/// parts and a level limit): `1` = -5 levels, `2` = -10, anything else (and no argument) clears it, exactly
/// like `SetLevelDown( eNoLevelDown / e5LevelDown / e10LevelDown )`.
pub fn item_level(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let who = me(cx)?;
    let n = args.number();
    let _ = run(cx, who, |m, ucx, _, _, fx| {
        let Some((id, item)) = slot0(m) else { return };
        let Some(prop) = ucx.prop(item.item_id) else {
            return;
        };
        if prop.parts == NULL_ID || prop.limit_level1 == NULL_ID {
            return;
        }
        let before = item.clone();
        let mut word = before.random_option;
        word.set_level_down(match n {
            1 => LevelDown::Minus5,
            2 => LevelDown::Minus10,
            _ => LevelDown::None,
        });
        core_ops::update_item_ex(m, ucx.env, id, word, fx);
        fx.mark_urgent();
        audit_gm(fx, "gm_il", &before, item_at(m, id));
    });
    Ok(CmdDone::default())
}

// ---- /RemoveTotalGold (186) -------------------------------------------------------------------------------------

/// Row 186 `/RemoveTotalGold (rtg) <i64>` (`TextCmd_RemoveTotalGold`): an amount above the total (Perins of the
/// inventory x 100 000 000 + penya) answers `TextCmd_RemoveTotalGold: <total>`; otherwise the amount is taken,
/// Perins first (`item::gold::total`). A negative amount takes nothing.
pub fn remove_total_gold_cmd(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let who = me(cx)?;
    let amount = args.number_i64();
    let total = cx.ctx.player(who).map_or(0, total_gold);
    if amount > total {
        cx.out.text(format!("TextCmd_RemoveTotalGold: {total}"));
        return Ok(CmdDone::default());
    }
    let core = cx.svc.items.handle();
    let env = core.env(cx.ctx.clock.unix_ms.to_unix_time());
    let mut fx = Fx::new();
    let taken = match cx.ctx.worlds.player_mut(who.0) {
        Some(m) => remove_total_gold(m, &env, amount, &mut fx),
        None => return Ok(CmdDone::default()),
    };
    if amount > 0 {
        fx.audit(
            flyff_db::audit::ev::ITEM_DESTROY,
            serde_json::json!({"reason": "gm", "gold": amount, "perins": taken}),
        );
    }
    fx.flush(cx.svc, cx.ctx, who);
    Ok(CmdDone::default())
}

// ---- /EscapeReset (133) -----------------------------------------------------------------------------------------

/// Row 133 `/EscapeReset [idPlayer]` (`TextCmd_EscapeReset`): the escape cooldown (`SM_ESCAPE`) of the online
/// character with that character id, else of the issuer, is reset (`SmMode{SM_ESCAPE, 0}` to its owner).
pub fn escape_reset(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let who = me(cx)?;
    let id = args.number();
    let target = u32::try_from(id)
        .ok()
        .and_then(|id| cx.svc.presence.get(CharacterId(id)))
        .and_then(|e| e.online)
        .map(|o| PlayerRef(o.obj))
        .filter(|p| cx.ctx.player(*p).is_some())
        .unwrap_or(who);
    let now = cx.ctx.clock.unix_ms.to_unix_time().0;
    let mut out = Out::new();
    if let Some(m) = cx.ctx.worlds.player_mut(target.0) {
        crate::stats::sm::set(m, SM_ESCAPE, 0, now, &mut out);
    }
    out.flush(cx.ctx, target.0);
    Ok(CmdDone::default())
}

// ---- /SetTutorialState (187) ------------------------------------------------------------------------------------

/// Row 187 `/SetTutorialState (sts) <n>` (`TextCmd_SetTutorialState`): the issuer's tutorial step becomes `n`
/// and `SetTutorialState` echoes it (`stats::modes::set_tutorial_state`, VR-004: the step must be in
/// `0..=guide_max_seq`; anything else answers a line and changes nothing).
pub fn set_tutorial_state(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let who = me(cx)?;
    let n = args.number();
    let core = cx.svc.stats.handle();
    let mut out = Out::new();
    let stored = cx
        .ctx
        .worlds
        .player_mut(who.0)
        .is_some_and(|m| crate::stats::modes::set_tutorial_state(m, &core, n, &mut out));
    out.flush(cx.ctx, who.0);
    if !stored {
        cx.out
            .text(format!("tutorial state must be 0..={}", core.guide_max_seq));
    }
    Ok(CmdDone::default())
}
