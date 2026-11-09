//! The bank handlers (Plan/19 §10.2): `OpenBankWnd`, `ConfirmBank`, `ChangeBankPassReq`, `CloseBankWnd`,
//! `PutItemBack`, `GetItemBack`, `PutGoldBack`, `GetGoldBack` and `PutBackToBank`.
//!
//! Every handler validates on the immutable state first and mutates last; the moves are single-entity changes
//! saved by the urgent snapshot of the player (13 §17.1; the snapshot carries every bank of the account), so the
//! in-memory change and the persistence request are atomic by construction. Messages go to the owner only.

use super::{BankAccess, BankSession, Pin, kind};
use crate::combat::state::is_die;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::item::container::StorageId;
use crate::item::env::ItemEnv;
use crate::item::fx::{Fx, item_json};
use crate::item::gold::{can_add, core::add_gold};
use crate::item::inventory::{ACCOUNT_SLOTS, BANK_SLOTS};
use crate::item::model::Item;
use crate::item::ops::{CtxSerials, ItemOrigin, RemoveReason, core as ops};
use crate::mover::Mover;
use flyff_common::ItemSerial;
use flyff_common::defs::item::II_SYS_SYS_SCR_CUSTODY;
use flyff_common::defs::itemkind::{IK3_EVENTMAIN, IK3_LINK, IK3_QUEST};
use flyff_common::defs::neuz::MMI_BANKING;
use flyff_common::defs::text::{
    TID_GAME_CANNOT_DO_USINGITEM, TID_GAME_EQUIPTRADE, TID_GAME_STILLNOTUSE,
};
use flyff_data::GameData;
use flyff_db::DbOp;
use flyff_proto::pb;
use flyff_rules::consts::NULL_ID;
use serde_json::json;

/// The PIN of a new character that was never changed: the client then asks for a new one (`BankWindow.mode` 0).
const UNSET_PIN: [u8; 4] = *b"0000";

/// `NUM` of `UpdateBankItem` (the only parameter that is sent).
const UI_NUM: i32 = 0;

/// A bank request, resolved: the confirmed session and the compartment it addresses.
struct Access {
    slot: usize,
}

/// `IsDie()` of a living-only request.
fn is_dead(m: &Mover) -> bool {
    is_die(m)
}

/// `slot < 3`.
fn slot_of(raw: u32) -> Option<usize> {
    let s = usize::try_from(raw).ok()?;
    (s < ACCOUNT_SLOTS).then_some(s)
}

/// A banker NPC within the NPC-menu range (VR-012; the original asked `IsCloseNpc(MMI_BANKING)` for the NPC path only).
fn near_banker(ctx: &GameCtx<'_>, m: &Mover) -> bool {
    ctx.worlds
        .close_npc(MMI_BANKING, m.core.world, m.core.layer, m.core.pos)
        .is_some()
}

/// The confirmed bank session of a living player (VR-014).
fn confirmed(m: &Mover) -> Option<BankSession> {
    if is_dead(m) {
        return None;
    }
    let s = m.player()?.econ.bank?;
    s.pin_confirmed.then_some(s)
}

/// May the player use compartment `slot`: its own, or another character's with a remote-bank mode (`IsCommBank`).
fn may_use_slot(m: &Mover, slot: usize) -> bool {
    let Some(p) = m.player() else { return false };
    if p.items.bank(slot).is_none() {
        return false;
    }
    slot == usize::from(p.ident.slot) || crate::stats::sm::is_comm_bank(m)
}

/// The shared prefix of put / get: a confirmed session, `slot < 3`, a banker in range unless the session is
/// instant, and the compartment rule.
fn access(ctx: &GameCtx<'_>, who: PlayerRef, raw_slot: u32) -> Option<Access> {
    let m = ctx.player(who)?;
    let sess = confirmed(m)?;
    let slot = slot_of(raw_slot)?;
    if !sess.instant && !near_banker(ctx, m) {
        return None;
    }
    may_use_slot(m, slot).then_some(Access { slot })
}

/// A chaotic player may use the bank when the propensity-penalty row of their value has `bank != 0` (the row of
/// the range, else step 8).
fn chaotic_may_bank(data: &GameData, propensity: u32) -> bool {
    let p = i64::from(propensity);
    let row = data
        .pk
        .penalties
        .iter()
        .find(|r| i64::from(r.min) <= p && p <= i64::from(r.max))
        .or_else(|| data.pk.penalties.iter().find(|r| r.step == 8));
    row.is_some_and(|r| r.bank != 0)
}

/// `NumberClamp`: the request count in `1..=count`.
fn clamp_num(num: i32, count: u16) -> u16 {
    let n = num.clamp(1, i32::from(count.max(1)));
    u16::try_from(n).unwrap_or(1)
}

/// The item that moves into a bank or the bag: `n` pieces, no reservation, a fresh serial for a partial stack
/// (19 §2.7: the moved part of a split gets one when it creates an entry, the rest keeps the original).
fn moved_part(item: &Item, n: u16) -> Item {
    let mut it = item.clone();
    it.count = n;
    it.reserved = 0;
    it.vendor_price = 0;
    if n < item.count {
        it.serial = ItemSerial(0);
    }
    it
}

// ---- the session ----------------------------------------------------------------------------------------

/// `OpenBankWnd{obj_id, item_id}` (Plan/19 §10.2; ref/07a §14.1). `obj_id == NULL_ID`: a banker NPC within range;
/// otherwise the inventory item must be usable and be `II_SYS_SYS_SCR_CUSTODY` (**FIX** VR-014: the original
/// accepted any matching pair). A chaotic player needs `bank != 0` in the penalty row. Anything else is ignored.
/// Answer: `BankWindow{mode, obj_id, item_id}` (mode 0 while the PIN is still `0000`); the session starts
/// unconfirmed. A new open replaces a running session.
pub fn on_open(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, msg: pb::OpenBankWnd) {
    let core = svc.items.handle();
    let Some(m) = ctx.player(who) else { return };
    let Some(p) = m.player() else { return };
    if is_dead(m) {
        return;
    }
    let via = if msg.obj_id == NULL_ID {
        if !near_banker(ctx, m) {
            return;
        }
        BankAccess::Npc
    } else {
        let Ok(id) = StorageId::try_from(msg.obj_id) else {
            return;
        };
        let ok = p.items.inventory.get_at_id(id).is_some_and(|it| {
            it.is_usable()
                && it.item_id == II_SYS_SYS_SCR_CUSTODY
                && msg.item_id == II_SYS_SYS_SCR_CUSTODY
        });
        if !ok {
            return;
        }
        BankAccess::InstantItem { obj_id: id }
    };
    if p.pk.propensity > 0 && !chaotic_may_bank(&core.data, p.pk.propensity) {
        return;
    }
    let Some(own) = p.items.bank(usize::from(p.ident.slot)) else {
        return;
    };
    let mode = if own.pin == UNSET_PIN { 0 } else { 1 };
    let mut fx = Fx::new();
    fx.out.to_self(pb::BankWindow {
        mode,
        obj_id: msg.obj_id,
        item_id: msg.item_id,
    });
    if let Some(e) = ctx.player_mut(who).and_then(|m| m.player_mut()) {
        e.econ.bank = Some(BankSession::opened(via));
        e.econ.bank_open = false;
    }
    fx.flush(svc, ctx, who);
}

/// `CloseBankWnd`: the session is dropped. No answer.
pub fn on_close(
    _svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    _msg: pb::CloseBankWnd,
) {
    if let Some(e) = ctx.player_mut(who).and_then(|m| m.player_mut()) {
        e.econ.end_bank_session();
    }
}

/// `ConfirmBank{pass, obj_id, item_id}` (Plan/19 §10.2; ref/07a §14.2; VR-014, VR-015, PL-061, PL-156).
///
/// Needs an opened, not yet confirmed session that was opened with these `obj_id` / `item_id`, no trade partner,
/// no personal shop, a closed guild bank and, for the NPC path, a banker in range. While the bank is locked the
/// answer is `ConfirmBankPass{false}` plus `TID_GAME_STILLNOTUSE` (the right PIN is refused too). A wrong PIN
/// counts (`gameplay.bank_pin_max_failures`, 0 = unlimited) and locks for `gameplay.bank_pin_lock_s`. The right
/// PIN resets the count; the instant-bank item (still usable and still `II_SYS_SYS_SCR_CUSTODY`) is consumed and
/// the session becomes `instant`; then the session is confirmed and the answer is `ConfirmBankPass{true}`.
pub fn on_confirm(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, msg: pb::ConfirmBank) {
    let core = svc.items.handle();
    let now = ctx.clock.unix_ms.to_unix_time();
    let (character, stored, sess) = {
        let Some(m) = ctx.player(who) else { return };
        let Some(p) = m.player() else { return };
        let Some(sess) = p.econ.bank else { return };
        if sess.pin_confirmed
            || is_dead(m)
            || p.econ.has_partner()
            || p.econ.vending
            || p.social.guild_bank_open
        {
            return;
        }
        // the request must be the one the session was opened with
        let same = match sess.via {
            BankAccess::Npc => msg.obj_id == NULL_ID,
            BankAccess::InstantItem { obj_id } => {
                msg.obj_id == u32::from(obj_id) && msg.item_id == II_SYS_SYS_SCR_CUSTODY
            }
        };
        if !same {
            return;
        }
        if sess.via == BankAccess::Npc && !near_banker(ctx, m) {
            return;
        }
        let Some(own) = p.items.bank(usize::from(p.ident.slot)) else {
            return;
        };
        (p.ident.character_id, own.pin, sess)
    };
    let mut fx = Fx::new();
    let reply = |ok: bool| pb::ConfirmBankPass {
        ok,
        obj_id: msg.obj_id,
        item_id: msg.item_id,
    };
    let gp = &ctx.cfg.rules.gameplay;
    let (max_failures, lock_s) = (gp.bank_pin_max_failures, gp.bank_pin_lock_s);
    if svc.econ.bank.is_locked(character, now) {
        fx.out.to_self(reply(false));
        fx.text(TID_GAME_STILLNOTUSE);
        fx.flush(svc, ctx, who);
        return;
    }
    let right = svc
        .econ
        .bank
        .verify(ctx.rng.service(), &stored, &Pin::typed(&msg.pass));
    if !right {
        let locked = svc
            .econ
            .bank
            .record_failure(character, now, max_failures, lock_s);
        if locked {
            tracing::info!(character = character.0, lock_s, "bank PIN locked");
        }
        fx.out.to_self(reply(false));
        fx.flush(svc, ctx, who);
        return;
    }
    svc.econ.bank.record_success(character);
    // the instant-bank item is consumed by the confirmation
    let mut instant = false;
    if let BankAccess::InstantItem { obj_id } = sess.via {
        let env = core.env(now);
        let still = ctx
            .player(who)
            .and_then(|m| m.player())
            .and_then(|p| p.items.inventory.get_at_id(obj_id))
            .is_some_and(|it| it.is_usable() && it.item_id == II_SYS_SYS_SCR_CUSTODY);
        if !still {
            fx.out.to_self(reply(false));
            fx.flush(svc, ctx, who);
            return;
        }
        if let Some(m) = ctx.worlds.player_mut(who.0) {
            ops::remove_item(m, &env, obj_id, 1, RemoveReason::Consume, &mut fx);
        }
        // a system item is used up: urgent snapshot (19 §20.2)
        fx.mark_urgent();
        instant = true;
    }
    if let Some(e) = ctx.player_mut(who).and_then(|m| m.player_mut())
        && let Some(s) = e.econ.bank.as_mut()
    {
        s.pin_confirmed = true;
        s.instant = instant;
        e.econ.bank_open = true;
    }
    fx.out.to_self(reply(true));
    fx.flush(svc, ctx, who);
}

/// `ChangeBankPassReq{old_pass, new_pass, obj_id, item_id}` (Plan/19 §10.2; ref/07a §14.3; PL-156). Needs an
/// opened session (the PIN dialog of a fresh character runs before the confirmation). Both strings at most 4
/// characters (else ignored); while the bank is locked the answer is `ChangeBankPassNtf{false}` plus
/// `TID_GAME_STILLNOTUSE`; the new PIN must be exactly 4 ASCII digits (**FIX**; else `false`); the old PIN is
/// checked like the confirmation's (a wrong one counts towards the lock). On success `banks.pin` changes in
/// memory and one `SetBankPin` op is queued.
pub fn on_change_pin(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    msg: pb::ChangeBankPassReq,
) {
    if msg.old_pass.len() > 4 || msg.new_pass.len() > 4 {
        return;
    }
    let now = ctx.clock.unix_ms.to_unix_time();
    let (character, slot, stored) = {
        let Some(m) = ctx.player(who) else { return };
        let Some(p) = m.player() else { return };
        if p.econ.bank.is_none() || is_dead(m) {
            return;
        }
        let slot = usize::from(p.ident.slot);
        let Some(own) = p.items.bank(slot) else {
            return;
        };
        (p.ident.character_id, slot, own.pin)
    };
    let mut fx = Fx::new();
    let reply = |ok: bool| pb::ChangeBankPassNtf {
        ok,
        obj_id: msg.obj_id,
        item_id: msg.item_id,
    };
    let gp = &ctx.cfg.rules.gameplay;
    let (max_failures, lock_s) = (gp.bank_pin_max_failures, gp.bank_pin_lock_s);
    if svc.econ.bank.is_locked(character, now) {
        fx.out.to_self(reply(false));
        fx.text(TID_GAME_STILLNOTUSE);
        fx.flush(svc, ctx, who);
        return;
    }
    let Some(new_pin) = Pin::typed(&msg.new_pass).digits() else {
        fx.out.to_self(reply(false));
        fx.flush(svc, ctx, who);
        return;
    };
    let right = svc
        .econ
        .bank
        .verify(ctx.rng.service(), &stored, &Pin::typed(&msg.old_pass));
    if !right {
        svc.econ
            .bank
            .record_failure(character, now, max_failures, lock_s);
        fx.out.to_self(reply(false));
        fx.flush(svc, ctx, who);
        return;
    }
    svc.econ.bank.record_success(character);
    if let Some(b) = ctx
        .player_mut(who)
        .and_then(|m| m.player_mut())
        .and_then(|p| p.items.banks.get_mut(slot))
        .and_then(Option::as_mut)
    {
        b.pin = new_pin;
    }
    ctx.persist.op(DbOp::SetBankPin {
        character_id: character,
        pin: new_pin,
    });
    fx.out.to_self(reply(true));
    fx.flush(svc, ctx, who);
}

// ---- items ----------------------------------------------------------------------------------------------

/// `PutItemBack{slot, obj_id, num}` (Plan/19 §10.2; ref/07a §14.5): the bag item into a compartment.
///
/// Refused silently: no confirmed session (VR-014), `slot >= 3`, no banker in range (unless the session is
/// instant), a compartment the player may not use, a reserved item, a quest item for a foreign compartment, a
/// bound item, an item without prop, `IK3_EVENTMAIN`, `IK3_LINK`, a guild cloak, a vagrant ride; with a text: an
/// item in use (`TID_GAME_CANNOT_DO_USINGITEM`) and a worn item (`TID_GAME_EQUIPTRADE`). A full compartment
/// answers `BankIsFull`. Effect: `PutItemBank{slot, item(count = n)}`, then the bag entry shrinks (`UpdateItem`),
/// audit `bank.put`, urgent snapshot.
pub fn on_put_item(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    msg: pb::PutItemBack,
) {
    let core = svc.items.handle();
    let env = core.env(ctx.clock.unix_ms.to_unix_time());
    let Some(acc) = access(ctx, who, msg.slot) else {
        return;
    };
    let Ok(id) = StorageId::try_from(msg.obj_id) else {
        return;
    };
    let mut fx = Fx::new();
    let (incoming, n, owner) = {
        let Some(m) = ctx.player(who) else { return };
        let Some(p) = m.player() else { return };
        let Some(item) = p.items.inventory.get_at_id(id) else {
            return;
        };
        if !item.is_usable() {
            return;
        }
        let Some(prop) = env.data().item(item.item_id) else {
            return;
        };
        let own_slot = acc.slot == usize::from(p.ident.slot);
        if prop.item_kind3 == IK3_QUEST && !own_slot {
            return;
        }
        if item.is_binds(prop, env.cfg().only_armor_awakes) {
            return;
        }
        if p.items.is_using(id) {
            fx.text(TID_GAME_CANNOT_DO_USINGITEM);
            fx.flush(svc, ctx, who);
            return;
        }
        if prop.item_kind3 == IK3_EVENTMAIN
            || prop.item_kind3 == IK3_LINK
            || item.is_guild_cloak(prop)
            || Item::is_vagrant_ride(prop)
        {
            return;
        }
        if p.items.inventory.is_equip(id) {
            fx.text(TID_GAME_EQUIPTRADE);
            fx.flush(svc, ctx, who);
            return;
        }
        let n = clamp_num(msg.num, item.count);
        let Some(bank) = p.items.bank(acc.slot) else {
            return;
        };
        let incoming = moved_part(item, n);
        if bank
            .items
            .is_full(&incoming, prop.pack_max.max(1), u32::from(n))
        {
            fx.out.to_self(pb::BankIsFull {});
            fx.flush(svc, ctx, who);
            return;
        }
        (incoming, n, bank.character_id)
    };
    let Services { serials, .. } = &mut *svc;
    let mut src = CtxSerials::new(serials, ctx.persist, ctx.tickets);
    let Some(m) = ctx.worlds.player_mut(who.0) else {
        return;
    };
    let Some(p) = m.player_mut() else { return };
    let Some(bank) = p.items.banks.get_mut(acc.slot).and_then(Option::as_mut) else {
        return;
    };
    let plan = match bank.items.add(&incoming, env.data(), &mut src) {
        Ok(plan) => plan,
        Err(e) => {
            tracing::warn!(error = %e, "bank put: the item was not added");
            return;
        }
    };
    let mut sent = incoming;
    if sent.serial.0 == 0 {
        sent.serial = plan.template_serial;
    }
    let first = plan.touched.first().map_or(0, |t| t.0);
    fx.out.to_self(pb::PutItemBank {
        slot: acc.slot as u32,
        item: Some(sent.to_pb(first)),
    });
    ops::remove_item(m, &env, id, n, RemoveReason::Moved, &mut fx);
    fx.audit(
        flyff_db::audit::ev::BANK_PUT,
        json!({
            "kind": kind::PUT_ITEM,
            "bank_owner": owner.0,
            "slot": acc.slot,
            "item": item_json(&sent),
            "count": n,
            "gold": 0,
        }),
    );
    fx.mark_urgent();
    fx.flush(svc, ctx, who);
}

/// `GetItemBack{slot, obj_id, num}` (Plan/19 §10.2; ref/07a §14.6): a compartment item into the bag.
///
/// The same prefix as [`on_put_item`]; the bank item must exist; the bag needs at least one **empty** slot even
/// when the item would stack (`count() < 168`, KEEP) else `BankIsFull`. Effect: `CreateItem` (the bag side),
/// `GetItemBank{item}`, `UpdateBankItem{slot, obj_id, NUM, rest}` (0 removes the entry), audit `bank.take`,
/// urgent snapshot.
pub fn on_get_item(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    msg: pb::GetItemBack,
) {
    let core = svc.items.handle();
    let env = core.env(ctx.clock.unix_ms.to_unix_time());
    let Some(acc) = access(ctx, who, msg.slot) else {
        return;
    };
    let Ok(bid) = StorageId::try_from(msg.obj_id) else {
        return;
    };
    let mut fx = Fx::new();
    let (incoming, n, owner, held) = {
        let Some(m) = ctx.player(who) else { return };
        let Some(p) = m.player() else { return };
        let Some(bank) = p.items.bank(acc.slot) else {
            return;
        };
        let Some(item) = bank.items.get_at_id(bid) else {
            return;
        };
        let n = clamp_num(msg.num, item.count);
        if p.items.inventory.empty_count() < 1 {
            fx.out.to_self(pb::BankIsFull {});
            fx.flush(svc, ctx, who);
            return;
        }
        (moved_part(item, n), n, bank.character_id, item.count)
    };
    let Services { serials, .. } = &mut *svc;
    let mut src = CtxSerials::new(serials, ctx.persist, ctx.tickets);
    let Some(m) = ctx.worlds.player_mut(who.0) else {
        return;
    };
    // the bag first: it is the only step that can fail (a serial fault), nothing has changed then
    let plan = match ops::create_item(
        m,
        &env,
        &mut src,
        incoming.clone(),
        ItemOrigin::BankGet,
        &mut fx,
    ) {
        Ok(plan) => plan,
        Err(e) => {
            tracing::warn!(error = %e, "bank get: the item was not created");
            return;
        }
    };
    let mut sent = incoming;
    if sent.serial.0 == 0 {
        sent.serial = plan.template_serial;
    }
    let first = plan.touched.first().map_or(0, |t| t.0);
    fx.out.to_self(pb::GetItemBank {
        item: Some(sent.to_pb(first)),
    });
    let rest = held - n;
    if let Some(bank) = m
        .player_mut()
        .and_then(|p| p.items.banks.get_mut(acc.slot))
        .and_then(Option::as_mut)
    {
        if rest == 0 {
            bank.items.remove_at_id(bid);
        } else if let Some(it) = bank.items.get_at_id_mut(bid) {
            it.count = rest;
        }
    }
    fx.out.to_self(pb::UpdateBankItem {
        slot: acc.slot as u32,
        obj_id: u32::from(bid),
        param: UI_NUM,
        value: u32::from(rest),
    });
    fx.audit(
        flyff_db::audit::ev::BANK_TAKE,
        json!({
            "kind": kind::GET_ITEM,
            "bank_owner": owner.0,
            "slot": acc.slot,
            "item": item_json(&sent),
            "count": n,
            "gold": 0,
        }),
    );
    fx.mark_urgent();
    fx.flush(svc, ctx, who);
}

// ---- penya ----------------------------------------------------------------------------------------------

/// The `PutGoldBank` message of compartment `slot`.
fn gold_msg(m: &Mover, slot: usize) -> Option<pb::PutGoldBank> {
    let p = m.player()?;
    Some(pb::PutGoldBank {
        slot: slot as u32,
        gold: u32::try_from(p.items.gold.max(0)).unwrap_or(0),
        bank_gold: u32::try_from(p.items.bank(slot)?.gold.max(0)).unwrap_or(u32::MAX),
    })
}

/// `PutGoldBack{slot, gold}` (Plan/19 §10.2; ref/07a §14.7): wallet penya into a compartment. `gold` read as
/// `i32` must be positive; `n = min(gold, wallet)`; the balance must be able to take `n` (`CanAdd`). Answer:
/// `PutGoldBank{slot, wallet, balance}` (the wallet change itself is silent, `AddGold(.., FALSE)`).
pub fn on_put_gold(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    msg: pb::PutGoldBack,
) {
    move_gold(svc, ctx, who, msg.slot, msg.gold, true);
}

/// `GetGoldBack{slot, gold}` (Plan/19 §10.2; ref/07a §14.8): the mirror of [`on_put_gold`]: `n = min(gold,
/// balance)`, the wallet must be able to take `n`.
pub fn on_get_gold(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    msg: pb::GetGoldBack,
) {
    move_gold(svc, ctx, who, msg.slot, msg.gold, false);
}

/// The common body of both gold moves; `deposit` = wallet to bank.
fn move_gold(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    raw_slot: u32,
    raw_gold: u32,
    deposit: bool,
) {
    let core = svc.items.handle();
    let env = core.env(ctx.clock.unix_ms.to_unix_time());
    if (raw_gold as i32) <= 0 {
        return;
    }
    let Some(acc) = access(ctx, who, raw_slot) else {
        return;
    };
    let (n, owner) = {
        let Some(m) = ctx.player(who) else { return };
        let Some(p) = m.player() else { return };
        let Some(bank) = p.items.bank(acc.slot) else {
            return;
        };
        let wallet = i64::from(p.items.gold);
        let want = i64::from(raw_gold);
        let (n, target) = if deposit {
            (want.min(wallet), bank.gold)
        } else {
            (want.min(bank.gold), wallet)
        };
        if !can_add(target, n) {
            return;
        }
        (n, bank.character_id)
    };
    let mut fx = Fx::new();
    let Some(m) = ctx.worlds.player_mut(who.0) else {
        return;
    };
    let signed = if deposit { n } else { -n };
    if let Some(b) = m
        .player_mut()
        .and_then(|p| p.items.banks.get_mut(acc.slot))
        .and_then(Option::as_mut)
    {
        b.gold += signed;
    }
    add_gold(m, &env, -signed, false, &mut fx);
    if let Some(msg) = gold_msg(m, acc.slot) {
        fx.out.to_self(msg);
    }
    let (ev, k) = if deposit {
        (flyff_db::audit::ev::BANK_PUT, kind::PUT_GOLD)
    } else {
        (flyff_db::audit::ev::BANK_TAKE, kind::GET_GOLD)
    };
    fx.audit(
        ev,
        json!({
            "kind": k,
            "bank_owner": owner.0,
            "slot": acc.slot,
            "item": null,
            "count": 0,
            "gold": n,
        }),
    );
    fx.mark_urgent();
    fx.flush(svc, ctx, who);
}

// ---- bank to bank ---------------------------------------------------------------------------------------

/// `PutBackToBank{flag, src_slot, dst_slot, obj_id, num, gold}` (Plan/19 §10.2; ref/07a §14.9; VR-014, VR-016):
/// between two compartments, which needs the remote-bank mode. A confirmed session; `src != dst`; both slots
/// `< 3` and occupied by an active character; no banker is needed (as the original). `flag == 1` moves an item
/// (the source entry must exist, `num` is clamped, the destination must hold fewer than 42 entries, else
/// `BankIsFull`; no quest or bind restriction, KEEP): `PutItemBank{dst, item}`, `UpdateBankItem{src, ..}`.
/// Any other flag moves penya: `n = min(gold, balance)`, the destination must be able to take it: one
/// `PutGoldBank` per compartment.
pub fn on_bank_to_bank(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    msg: pb::PutBackToBank,
) {
    let core = svc.items.handle();
    let env = core.env(ctx.clock.unix_ms.to_unix_time());
    let (Some(from), Some(to)) = (slot_of(msg.src_slot), slot_of(msg.dst_slot)) else {
        return;
    };
    if from == to {
        return;
    }
    {
        let Some(m) = ctx.player(who) else { return };
        if confirmed(m).is_none() {
            return;
        }
        let Some(p) = m.player() else { return };
        if p.items.bank(from).is_none()
            || p.items.bank(to).is_none()
            || !crate::stats::sm::is_comm_bank(m)
        {
            return;
        }
    }
    if msg.flag == 1 {
        bank_item_to_bank(svc, ctx, who, &env, (from, to), &msg);
    } else {
        bank_gold_to_bank(svc, ctx, who, (from, to), msg.gold);
    }
}

/// The item branch of [`on_bank_to_bank`].
fn bank_item_to_bank(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    env: &ItemEnv<'_>,
    (from, to): (usize, usize),
    msg: &pb::PutBackToBank,
) {
    let Ok(id) = StorageId::try_from(msg.obj_id) else {
        return;
    };
    let mut fx = Fx::new();
    let (incoming, n, held, owner) = {
        let Some(p) = ctx.player(who).and_then(Mover::player) else {
            return;
        };
        let (Some(src), Some(dst)) = (p.items.bank(from), p.items.bank(to)) else {
            return;
        };
        let Some(item) = src.items.get_at_id(id) else {
            return;
        };
        let n = clamp_num(msg.num, item.count);
        if dst.items.count() >= usize::from(BANK_SLOTS) {
            fx.out.to_self(pb::BankIsFull {});
            fx.flush(svc, ctx, who);
            return;
        }
        (moved_part(item, n), n, item.count, dst.character_id)
    };
    let Services { serials, .. } = &mut *svc;
    let mut src = CtxSerials::new(serials, ctx.persist, ctx.tickets);
    let Some(p) = ctx.worlds.player_mut(who.0).and_then(Mover::player_mut) else {
        return;
    };
    let Some(dst) = p.items.banks.get_mut(to).and_then(Option::as_mut) else {
        return;
    };
    let plan = match dst.items.add(&incoming, env.data(), &mut src) {
        Ok(plan) => plan,
        Err(e) => {
            tracing::warn!(error = %e, "bank to bank: the item was not added");
            return;
        }
    };
    let mut sent = incoming;
    if sent.serial.0 == 0 {
        sent.serial = plan.template_serial;
    }
    let first = plan.touched.first().map_or(0, |t| t.0);
    fx.out.to_self(pb::PutItemBank {
        slot: to as u32,
        item: Some(sent.to_pb(first)),
    });
    let rest = held - n;
    if let Some(b) = p.items.banks.get_mut(from).and_then(Option::as_mut) {
        if rest == 0 {
            b.items.remove_at_id(id);
        } else if let Some(it) = b.items.get_at_id_mut(id) {
            it.count = rest;
        }
    }
    fx.out.to_self(pb::UpdateBankItem {
        slot: from as u32,
        obj_id: u32::from(id),
        param: UI_NUM,
        value: u32::from(rest),
    });
    fx.audit(
        flyff_db::audit::ev::BANK_PUT,
        json!({
            "kind": kind::BANK_TO_BANK,
            "bank_owner": owner.0,
            "from_slot": from,
            "slot": to,
            "item": item_json(&sent),
            "count": n,
            "gold": 0,
        }),
    );
    fx.mark_urgent();
    fx.flush(svc, ctx, who);
}

/// The penya branch of [`on_bank_to_bank`].
fn bank_gold_to_bank(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    (from, to): (usize, usize),
    raw_gold: u32,
) {
    let (n, owner) = {
        let Some(p) = ctx.player(who).and_then(Mover::player) else {
            return;
        };
        let (Some(src), Some(dst)) = (p.items.bank(from), p.items.bank(to)) else {
            return;
        };
        let n = i64::from(raw_gold).min(src.gold);
        if !can_add(dst.gold, n) {
            return;
        }
        (n, dst.character_id)
    };
    let mut fx = Fx::new();
    let Some(m) = ctx.worlds.player_mut(who.0) else {
        return;
    };
    if let Some(p) = m.player_mut() {
        if let Some(b) = p.items.banks.get_mut(from).and_then(Option::as_mut) {
            b.gold -= n;
        }
        if let Some(b) = p.items.banks.get_mut(to).and_then(Option::as_mut) {
            b.gold += n;
        }
    }
    for slot in [from, to] {
        if let Some(msg) = gold_msg(m, slot) {
            fx.out.to_self(msg);
        }
    }
    fx.audit(
        flyff_db::audit::ev::BANK_PUT,
        json!({
            "kind": kind::BANK_TO_BANK,
            "bank_owner": owner.0,
            "from_slot": from,
            "slot": to,
            "item": null,
            "count": 0,
            "gold": n,
        }),
    );
    fx.mark_urgent();
    fx.flush(svc, ctx, who);
}
